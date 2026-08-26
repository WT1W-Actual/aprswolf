use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::connect_info::ConnectInfo;
use axum::extract::State as AxumState;
use axum::http::header;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use local_ip_address::local_ip;
use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::runtime;
use crate::settings::{self, AppSettings};

pub const DEFAULT_WEB_SERVER_PORT: u16 = 18420;

pub struct WebServerHandle {
    port: u16,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

#[derive(Clone, Default)]
pub struct WebServerState {
    pub handle: Arc<Mutex<Option<WebServerHandle>>>,
}

#[derive(Clone, Serialize)]
pub struct WebServerStatus {
    pub enabled: bool,
    pub port: Option<u16>,
    pub url: Option<String>,
}

#[derive(Clone, Serialize)]
struct RemoteStateSnapshot {
    app_version: String,
    settings: AppSettings,
    runtime: runtime::RuntimeSnapshot,
    web_server: WebServerStatus,
    manual_beacon_cooldown_remaining_sec: u64,
    queue: Vec<crate::tnc::QueueSnapshot>,
    received: Vec<serde_json::Value>,
}

#[derive(Clone)]
struct HttpState {
    app: AppHandle,
}

#[derive(serde::Deserialize)]
struct ConnectTncRequest {
    host: String,
    port: u16,
}

#[derive(serde::Deserialize)]
struct ConnectAprsIsRequest {
    host: String,
    port: u16,
    passcode: String,
    filter: String,
}

#[derive(serde::Deserialize)]
struct SendMessageRequest {
    destination: String,
    text: String,
}

#[derive(Serialize)]
struct ApiError {
    error: String,
}

fn local_url(port: u16) -> String {
    match local_ip() {
        Ok(ip) => format!("http://{}:{}", ip, port),
        Err(_) => format!("http://127.0.0.1:{}", port),
    }
}

fn current_status(app: &AppHandle) -> WebServerStatus {
    let state = app.state::<WebServerState>();
    let handle = state.handle.lock().unwrap();
    if let Some(handle) = handle.as_ref() {
        WebServerStatus {
            enabled: true,
            port: Some(handle.port),
            url: Some(local_url(handle.port)),
        }
    } else {
        WebServerStatus {
            enabled: false,
            port: None,
            url: None,
        }
    }
}

fn emit_status(app: &AppHandle) {
    let _ = app.emit("web_server_status", current_status(app));
}

fn json_error(error: String) -> impl IntoResponse {
    (StatusCode::BAD_REQUEST, Json(ApiError { error }))
}

// ── Token auth ──────────────────────────────────────────────

/// Constant-time string comparison to avoid timing oracles on the token.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Auth rule for the remote-control API:
/// - No token configured -> loopback only; remote callers get 403 with a
///   pointer to configure the token.
/// - Token configured -> every caller (including localhost) must present
///   `Authorization: Bearer <token>`.
fn authorize(configured_token: &str, is_loopback: bool, presented: Option<&str>) -> Result<(), StatusCode> {
    let configured = configured_token.trim();
    if configured.is_empty() {
        return if is_loopback {
            Ok(())
        } else {
            Err(StatusCode::FORBIDDEN)
        };
    }
    match presented {
        Some(token) if constant_time_eq(token.trim(), configured) => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

async fn require_token(
    AxumState(state): AxumState<HttpState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let configured = settings::load_settings(state.app.clone()).web_server_token;
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    match authorize(&configured, addr.ip().is_loopback(), presented) {
        Ok(()) => next.run(request).await,
        Err(StatusCode::FORBIDDEN) => (
            StatusCode::FORBIDDEN,
            Json(ApiError {
                error: "web server token not configured; set it in Preferences before using remote access from other devices".to_string(),
            }),
        )
            .into_response(),
        Err(_) => (
            StatusCode::UNAUTHORIZED,
            Json(ApiError {
                error: "missing or invalid Bearer token".to_string(),
            }),
        )
            .into_response(),
    }
}

// ── Handlers ────────────────────────────────────────────────

async fn snapshot(app: &AppHandle) -> Result<RemoteStateSnapshot, String> {
    Ok(RemoteStateSnapshot {
        app_version: app
            .config()
            .version
            .as_deref()
            .unwrap_or_default()
            .to_string(),
        settings: settings::load_settings(app.clone()),
        runtime: app.state::<crate::runtime::RuntimeState>().snapshot(),
        web_server: current_status(app),
        manual_beacon_cooldown_remaining_sec: crate::tnc::get_manual_beacon_cooldown_remaining(app.clone()),
        queue: crate::tnc::inspect_queue_for_app(app.clone()).await?,
        received: settings::load_received_messages(app.clone()),
    })
}

async fn index_handler() -> Html<&'static str> {
    Html(REMOTE_HTML)
}

async fn state_handler(AxumState(state): AxumState<HttpState>) -> impl IntoResponse {
    match snapshot(&state.app).await {
        Ok(data) => (StatusCode::OK, Json(data)).into_response(),
        Err(error) => json_error(error).into_response(),
    }
}

/// The remote page posts a partial settings object using its own key names;
/// remap them to AppSettings fields and merge-save (omitted fields keep
/// their current values).
async fn save_settings_handler(
    AxumState(state): AxumState<HttpState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let mut mapped = serde_json::Map::new();
    if let Some(obj) = payload.as_object() {
        for (key, value) in obj {
            let target = match key.as_str() {
                "callsign" => "source",
                "beacon_latitude" => "beacon_lat",
                "beacon_longitude" => "beacon_lon",
                "tnc_host" => "host",
                "tnc_port" => "port",
                other => other,
            };
            // AppSettings stores ports as strings; the page sends numbers.
            let value = match (target, value) {
                ("port" | "aprsis_port", serde_json::Value::Number(n)) => {
                    serde_json::Value::String(n.to_string())
                }
                (_, v) => v.clone(),
            };
            mapped.insert(target.to_string(), value);
        }
    }

    match settings::save_settings(state.app.clone(), serde_json::Value::Object(mapped)) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => json_error(error).into_response(),
    }
}

async fn connect_tnc_handler(
    AxumState(state): AxumState<HttpState>,
    Json(payload): Json<ConnectTncRequest>,
) -> impl IntoResponse {
    let settings = settings::load_settings(state.app.clone());
    if settings.source.trim().is_empty() {
        return json_error("Configure the station callsign before connecting".to_string())
            .into_response();
    }
    match crate::tnc::connect_tnc_for_app(state.app.clone(), settings.source, payload.host, payload.port).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => json_error(error).into_response(),
    }
}

async fn disconnect_tnc_handler(AxumState(state): AxumState<HttpState>) -> impl IntoResponse {
    match crate::tnc::disconnect_tnc_for_app(state.app.clone()).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => json_error(error).into_response(),
    }
}

async fn connect_aprsis_handler(
    AxumState(state): AxumState<HttpState>,
    Json(payload): Json<ConnectAprsIsRequest>,
) -> impl IntoResponse {
    let settings = settings::load_settings(state.app.clone());
    if settings.source.trim().is_empty() {
        return json_error("Configure the station callsign before connecting".to_string())
            .into_response();
    }
    match crate::aprsis::connect_aprsis_for_app(
        state.app.clone(),
        payload.host,
        payload.port,
        settings.source,
        payload.passcode,
        payload.filter,
    )
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => json_error(error).into_response(),
    }
}

async fn disconnect_aprsis_handler(AxumState(state): AxumState<HttpState>) -> impl IntoResponse {
    match crate::aprsis::disconnect_aprsis_for_app(state.app.clone()).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => json_error(error).into_response(),
    }
}

async fn send_message_handler(
    AxumState(state): AxumState<HttpState>,
    Json(payload): Json<SendMessageRequest>,
) -> impl IntoResponse {
    let settings = settings::load_settings(state.app.clone());
    if settings.source.trim().is_empty() {
        return json_error("Configure the station callsign before sending".to_string())
            .into_response();
    }
    match crate::tnc::send_message_for_app(
        state.app.clone(),
        settings.source,
        payload.destination,
        settings.path,
        payload.text,
    )
    .await
    {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(error) => json_error(error).into_response(),
    }
}

/// Manual beacon using the station/beacon settings (no request body needed).
async fn send_beacon_handler(AxumState(state): AxumState<HttpState>) -> impl IntoResponse {
    let settings = settings::load_settings(state.app.clone());
    if settings.source.trim().is_empty() {
        return json_error("Configure the station callsign before beaconing".to_string())
            .into_response();
    }
    let (lat, lon) = match (
        settings.beacon_lat.trim().parse::<f64>(),
        settings.beacon_lon.trim().parse::<f64>(),
    ) {
        (Ok(lat), Ok(lon)) => (lat, lon),
        _ => return json_error("Beacon latitude/longitude are not configured".to_string())
            .into_response(),
    };
    match crate::tnc::send_beacon_for_app(
        state.app.clone(),
        settings.source,
        settings.path,
        lat,
        lon,
        settings.beacon_symbol,
        String::new(),
        true,
    )
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => json_error(error).into_response(),
    }
}

async fn clear_queue_handler(AxumState(state): AxumState<HttpState>) -> impl IntoResponse {
    match crate::tnc::clear_queue_for_app(state.app.clone()).await {
        Ok(count) => (StatusCode::OK, Json(count)).into_response(),
        Err(error) => json_error(error).into_response(),
    }
}

async fn disable_web_server_handler(AxumState(state): AxumState<HttpState>) -> impl IntoResponse {
    let app = state.app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = disable_web_server_internal(app).await;
    });
    StatusCode::NO_CONTENT
}

// ── Lifecycle ───────────────────────────────────────────────

pub async fn enable_web_server_internal(app: AppHandle, port: u16) -> Result<WebServerStatus, String> {
    if port == 0 {
        return Err("Web server port must be greater than 0".to_string());
    }

    let state = app.state::<WebServerState>();
    if state.handle.lock().unwrap().is_some() {
        return Err("Web server is already enabled".to_string());
    }

    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .map_err(|err| format!("failed to bind web server on port {port}: {err}"))?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let app_for_task = app.clone();

    let api = Router::new()
        .route("/api/state", get(state_handler))
        .route("/api/save-settings", post(save_settings_handler))
        .route("/api/tnc/connect", post(connect_tnc_handler))
        .route("/api/tnc/disconnect", post(disconnect_tnc_handler))
        .route("/api/tnc/clear-queue", post(clear_queue_handler))
        .route("/api/aprsis/connect", post(connect_aprsis_handler))
        .route("/api/aprsis/disconnect", post(disconnect_aprsis_handler))
        .route("/api/send-message", post(send_message_handler))
        .route("/api/send-beacon", post(send_beacon_handler))
        .route("/api/disable", post(disable_web_server_handler))
        .route_layer(middleware::from_fn_with_state(
            HttpState { app: app.clone() },
            require_token,
        ));

    let router = Router::new()
        .route("/", get(index_handler))
        .merge(api)
        .with_state(HttpState { app: app.clone() });

    let task = tauri::async_runtime::spawn(async move {
        let server = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        });
        let _ = server.await;

        let state = app_for_task.state::<WebServerState>();
        let mut handle_lock = state.handle.lock().unwrap();
        if handle_lock.as_ref().map(|handle| handle.port) == Some(port) {
            *handle_lock = None;
        }
        drop(handle_lock);
        emit_status(&app_for_task);
    });

    let mut handle_lock = state.handle.lock().unwrap();
    *handle_lock = Some(WebServerHandle {
        port,
        shutdown_tx: Some(shutdown_tx),
        task,
    });
    drop(handle_lock);

    emit_status(&app);
    Ok(current_status(&app))
}

pub async fn disable_web_server_internal(app: AppHandle) -> Result<(), String> {
    let state = app.state::<WebServerState>();
    let handle = state.handle.lock().unwrap().take();
    let Some(mut handle) = handle else {
        return Err("Web server is not enabled".to_string());
    };

    if let Some(shutdown_tx) = handle.shutdown_tx.take() {
        let _ = shutdown_tx.send(());
    }

    handle.task.abort();
    emit_status(&app);
    Ok(())
}

#[tauri::command]
pub fn get_web_server_status(app: AppHandle) -> WebServerStatus {
    current_status(&app)
}

#[tauri::command]
pub async fn enable_web_server(app: AppHandle, port: u16) -> Result<WebServerStatus, String> {
    enable_web_server_internal(app, port).await
}

#[tauri::command]
pub async fn disable_web_server(app: AppHandle) -> Result<(), String> {
    disable_web_server_internal(app).await
}

const REMOTE_HTML: &str = include_str!("../remote-control.html");

#[cfg(test)]
mod tests {
    use super::{authorize, constant_time_eq};

    #[test]
    fn constant_time_eq_matches_equal_strings_only() {
        assert!(constant_time_eq("s3cret", "s3cret"));
        assert!(!constant_time_eq("s3cret", "s3creT"));
        assert!(!constant_time_eq("s3cret", "s3cre"));
        assert!(!constant_time_eq("", "x"));
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn authorize_loopback_only_when_token_unconfigured() {
        assert!(authorize("", true, None).is_ok());
        assert!(authorize("", false, None).is_err());
        // Even a presented token doesn't help when none is configured remotely.
        assert!(authorize("  ", false, Some("anything")).is_err());
    }

    #[test]
    fn authorize_requires_matching_bearer_when_configured() {
        assert!(authorize("abc123", false, Some("abc123")).is_ok());
        // Localhost is NOT exempt once a token is configured.
        assert!(authorize("abc123", true, None).is_err());
        assert!(authorize("abc123", false, Some("abc124")).is_err());
        assert!(authorize("abc123", false, None).is_err());
        // Surrounding whitespace on the presented token is tolerated.
        assert!(authorize("abc123", false, Some("  abc123 ")).is_ok());
    }
}
