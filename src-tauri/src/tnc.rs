use std::fs;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};

use crate::ax25::{build_aprs_ack_info, build_aprs_message_info, build_ui_frame, decode_mic_e, parse_aprs_message, parse_ui_frame, ParsedAddress, ParsedUiFrame};
use crate::agw::{AgwBinaryDecoder, AgwFrame, AGW_HEADER_LEN, build_startup_command_frames};
use crate::kiss::encode_data_frame;

const MAX_ATTEMPTS: u32 = 5;
const RETRY_BASE_MS: u64 = 15_000;
const MANUAL_BEACON_COOLDOWN_MS: u64 = 180_000;
const MAX_UNCOMPRESSED_POSITION_COMMENT_LEN: usize = 43;
const OVERLAY_CAPABLE_ALTERNATE_SYMBOL_CODES: &str = "!#$%&'(-08;<=>A\\W^_achiknsuvwz";
pub(crate) const APRS_TOCALL: &str = "APWOLF";

#[derive(Clone, Default)]
pub struct AppState {
    pub client: Arc<Mutex<Option<ClientHandle>>>,
    pub retry_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    pub periodic_beacon_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    pub queue: Arc<Mutex<Vec<QueuedMessage>>>,
    pub queue_file: Arc<Mutex<Option<PathBuf>>>,
    pub next_id: Arc<Mutex<u64>>,
    pub connection_epoch: Arc<AtomicU64>,
    pub my_callsign: Arc<Mutex<Option<String>>>,
    pub latest_reply_acks: Arc<Mutex<HashMap<String, String>>>,
}

pub struct ClientHandle {
    tx: mpsc::UnboundedSender<Vec<u8>>,
    task: JoinHandle<()>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum DeliveryState {
    Queued,
    Sent,
    Acked,
    Failed,
    Expired,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct QueuedMessage {
    id: u64,
    destination: String,
    body: String,
    #[serde(default)]
    path: String,
    payload: String,
    #[serde(default)]
    ax25_hex: String,
    #[serde(default)]
    aprsis_payload: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    aprs_id: String,
    created_at_ms: u64,
    attempt_count: u32,
    status: DeliveryState,
    next_retry_at_ms: Option<u64>,
    last_error: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct ConnectionStatus {
    state: String,
    detail: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct ReceivedMessage {
    text: String,
    payload_hex: String,
    bytes: usize,
    source: Option<String>,
    destination: Option<String>,
    path: Option<Vec<String>>,
    info_text: Option<String>,
    protocol_source: Option<String>,
    protocol_destination: Option<String>,
    protocol_path: Option<Vec<String>>,
    protocol_info_text: Option<String>,
    from: String,
    to: Option<String>,
    body: String,
    #[serde(rename = "aprsType")]
    aprs_type: String,
    #[serde(rename = "isBulletin")]
    is_bulletin: bool,
    #[serde(rename = "micEDecoded")]
    mic_e_decoded: bool,
    summary_source: String,
    summary_destination: Option<String>,
    mic_e_text: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct SentMessage {
    id: u64,
    status: DeliveryState,
    payload_hex: String,
    bytes: usize,
}

#[derive(Clone, Serialize)]
pub struct DeliveryEvent {
    id: u64,
    status: DeliveryState,
    attempts: u32,
    detail: Option<String>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn retry_delay_ms(attempt: u32) -> u64 {
    let factor = 2_u64.pow(attempt.saturating_sub(1).min(4));
    RETRY_BASE_MS.saturating_mul(factor)
}

/// Validate an AX.25 source callsign base (without SSID).
///
/// Rules per the AX.25 spec and APRS documentation:
/// - 1 to 6 characters
/// - Each character must be an uppercase ASCII letter or ASCII digit
fn validate_ax25_callsign(call: &str) -> Result<(), String> {
    if call.is_empty() || call.len() > 6 {
        return Err(format!(
            "Callsign \"{}\" must be 1–6 characters",
            call
        ));
    }
    for ch in call.chars() {
        if !ch.is_ascii_uppercase() && !ch.is_ascii_digit() {
            return Err(format!(
                "Callsign \"{}\" contains invalid character '{}'; only uppercase letters and digits are allowed",
                call, ch
            ));
        }
    }
    Ok(())
}

fn parse_ax25_station(input: &str, label: &str) -> Result<(String, u8), String> {
    let normalized = input.trim().to_uppercase();
    if normalized.is_empty() {
        return Err(format!("{label} is required"));
    }

    let (callsign, ssid) = if let Some((call, ssid_text)) = normalized.split_once('-') {
        if ssid_text.is_empty() {
            return Err(format!("{label} SSID is missing"));
        }
        if !ssid_text.chars().all(|ch| ch.is_ascii_digit()) {
            return Err(format!("{label} SSID must be a number from 1 to 15"));
        }

        let parsed_ssid = ssid_text
            .parse::<u8>()
            .map_err(|_| format!("{label} SSID must be a number from 1 to 15"))?;
        if parsed_ssid == 0 {
            return Err(format!("{label} must not include -0; omit the SSID when it is zero"));
        }
        if parsed_ssid > 15 {
            return Err(format!("{label} SSID must be in the range 1 to 15"));
        }

        (call.to_string(), parsed_ssid)
    } else {
        (normalized, 0)
    };

    validate_ax25_callsign(&callsign)?;
    Ok((callsign, ssid))
}

fn format_station_with_ssid(callsign: &str, ssid: u8) -> String {
    if ssid == 0 {
        callsign.to_string()
    } else {
        format!("{}-{}", callsign, ssid)
    }
}

fn parse_digipeater_path(path: &str) -> Result<Vec<String>, String> {
    let digis: Vec<String> = path
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (call, ssid) = parse_ax25_station(entry, "Digipeater path entry")?;
            if matches!(call.as_str(), "RELAY" | "TRACE" | "WIDE") {
                return Err(format!(
                    "Digipeater path entry \"{}\" is obsolete; use WIDE1-1,WIDE2-1 style aliases or specific callsigns",
                    entry.trim().to_uppercase()
                ));
            }
            Ok(format_station_with_ssid(&call, ssid))
        })
        .collect::<Result<Vec<_>, String>>()?;

    if digis.len() > 8 {
        return Err("Digipeater path must contain at most 8 entries".to_string());
    }

    Ok(digis)
}

fn validate_position(lat: f64, lon: f64) -> Result<(), String> {
    if !lat.is_finite() || !(-90.0..=90.0).contains(&lat) {
        return Err("Latitude must be a finite value in the range -90 to 90".to_string());
    }
    if !lon.is_finite() || !(-180.0..=180.0).contains(&lon) {
        return Err("Longitude must be a finite value in the range -180 to 180".to_string());
    }
    Ok(())
}

fn validate_uncompressed_position_comment(comment: &str) -> Result<(), String> {
    if comment.len() > MAX_UNCOMPRESSED_POSITION_COMMENT_LEN {
        return Err(format!(
            "Beacon comment exceeds the APRS {}-character limit for uncompressed position reports",
            MAX_UNCOMPRESSED_POSITION_COMMENT_LEN
        ));
    }
    Ok(())
}

fn resolve_uncompressed_symbol(symbol: &str) -> Result<(char, char), String> {
    let trimmed = symbol.trim();
    if trimmed.is_empty() {
        return Ok(('/', '-'));
    }

    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() != 2 {
        return Err("Beacon symbol must be exactly 2 characters: table/overlay plus symbol code".to_string());
    }

    let symbol_table = chars[0];
    let symbol_code = chars[1];

    let valid_table = symbol_table == '/'
        || symbol_table == '\\'
        || symbol_table.is_ascii_digit()
        || symbol_table.is_ascii_uppercase();
    if !valid_table {
        return Err(
            "Beacon symbol table must be '/', '\\', a digit overlay, or an uppercase overlay letter for uncompressed APRS positions"
                .to_string(),
        );
    }

    if !symbol_code.is_ascii_graphic() {
        return Err("Beacon symbol code must be a printable ASCII character".to_string());
    }

    if (symbol_table.is_ascii_digit() || symbol_table.is_ascii_uppercase())
        && !OVERLAY_CAPABLE_ALTERNATE_SYMBOL_CODES.contains(symbol_code)
    {
        return Err(format!(
            "Beacon symbol \"{}{}\" uses an overlay with symbol code \"{}\", but that alternate-table symbol is not overlay-capable in APRS Appendix 2",
            symbol_table, symbol_code, symbol_code
        ));
    }

    Ok((symbol_table, symbol_code))
}

/// Strip any embedded carriage return or line feed characters from a string.
///
/// CR/LF must not appear in APRS message info fields; their presence would
/// break TNC2 framing and cause interoperability issues with IGate stations.
fn strip_crlf(s: &str) -> String {
    s.chars().filter(|&c| c != '\r' && c != '\n').collect()
}

fn validate_aprs_message_text(text: &str) -> Result<(), String> {
    if text.len() > 67 {
        return Err("Message exceeds the APRS 67-character limit".to_string());
    }
    Ok(())
}

/// Parse ACK ID from ACK or ack{id} format. Validates 1-5 alphanumeric characters.
pub fn parse_ack_id(text: &str) -> Option<String> {
    let trimmed = text.trim();
    // Accept both "ACK:" (with colon, any case) and "ack"/"ACK" (without colon, any case).
    // `get` returns None instead of panicking when the cut would fall inside
    // a multibyte character, which received packets can contain.
    let ack_part = if trimmed.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("ack:")) {
        &trimmed[4..]
    } else if trimmed.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("ack")) {
        &trimmed[3..]
    } else {
        return None;
    };

    let (message_id, _) = split_reply_ack_id(ack_part)?;
    Some(message_id.to_string())
}

pub(crate) fn split_reply_ack_id(value: &str) -> Option<(&str, Option<&str>)> {
    if value.is_empty() {
        return None;
    }

    let (message_id, reply_ack) = if let Some((message_id, reply_ack)) = value.split_once('}') {
        (message_id, Some(reply_ack))
    } else {
        (value, None)
    };

    if message_id.is_empty() || message_id.len() > 5 || !message_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }

    match reply_ack {
        Some("") => Some((message_id, None)),
        Some(ack) if ack.len() <= 5 && ack.chars().all(|c| c.is_ascii_alphanumeric()) => {
            Some((message_id, Some(ack)))
        }
        Some(_) => None,
        None => Some((message_id, None)),
    }
}

pub(crate) async fn remember_latest_reply_ack(state: &AppState, station: &str, message_id: &str) {
    let Some((message_number, _)) = split_reply_ack_id(message_id) else {
        return;
    };

    let key = station.trim().to_uppercase();
    if key.is_empty() {
        return;
    }

    let mut latest = state.latest_reply_acks.lock().await;
    latest.insert(key, message_number.to_string());
}

async fn latest_reply_ack_for_station(state: &AppState, station: &str) -> Option<String> {
    let key = station.trim().to_uppercase();
    if key.is_empty() {
        return None;
    }

    let latest = state.latest_reply_acks.lock().await;
    latest.get(&key).cloned()
}

struct MessagePayloads {
    ax25_frame: Vec<u8>,
    ax25_hex: String,
    aprsis_payload: String,
}

fn build_queued_message_payloads(
    source: &str,
    destination: &str,
    path: &str,
    body: &str,
    aprs_id: &str,
    reply_ack: Option<&str>,
) -> Result<MessagePayloads, String> {
    let source = source.trim().to_uppercase();
    let destination = destination.trim().to_uppercase();
    let (src_call, src_ssid) = parse_ax25_station(&source, "Source callsign")?;
    let digi_strings = parse_digipeater_path(path)?;
    let digis: Vec<&str> = digi_strings.iter().map(String::as_str).collect();

    let encoded_message_id = match reply_ack {
        Some(reply_ack) if !reply_ack.is_empty() => format!("{aprs_id}}}{reply_ack}"),
        _ => format!("{aprs_id}}}"),
    };

    let aprs_info = build_aprs_message_info(&destination, body, Some(&encoded_message_id));
    let aprs_info_str = String::from_utf8_lossy(&aprs_info).to_string();
    let ax25_frame = build_ui_frame(APRS_TOCALL, &src_call, src_ssid, &digis, &aprs_info);
    let source_with_ssid = format_station_with_ssid(&src_call, src_ssid);
    let aprsis_payload = format!("{source_with_ssid}>{APRS_TOCALL}:{aprs_info_str}");

    Ok(MessagePayloads {
        ax25_hex: to_hex(&ax25_frame),
        ax25_frame,
        aprsis_payload,
    })
}

/// Message ids that `info_text` acknowledges on behalf of `station`.
///
/// An APRS ack is a message addressed to the station whose message it
/// confirms, so anything addressed elsewhere is ignored: another station's
/// `ack07` must not clear our own message 07 to the same sender. Two forms
/// count: a plain `ackNN` message, and the reply-ack half of an incoming
/// message id (`{MM}AA` acknowledges our `AA`).
pub(crate) fn ack_ids_for_station(info_text: &str, station: &str, allow_base_callsign: bool) -> Vec<String> {
    let Some(aprs_msg) = parse_aprs_message(info_text.trim()) else {
        return Vec::new();
    };
    if !callsign_matches_station(&aprs_msg.addressee, station, allow_base_callsign) {
        return Vec::new();
    }
    let mut ids = Vec::new();
    match aprs_msg.msgid.as_deref() {
        None => {
            if let Some(id) = parse_ack_id(&aprs_msg.text) {
                ids.push(id);
            }
        }
        Some(msgid) => {
            if let Some((_, Some(reply_ack))) = split_reply_ack_id(msgid) {
                ids.push(reply_ack.to_string());
            }
        }
    }
    ids
}

pub(crate) fn callsign_matches_station(addressee: &str, station: &str, allow_base_callsign: bool) -> bool {
    let add = addressee.trim().to_uppercase();
    let me = station.trim().to_uppercase();
    if add == me {
        return true;
    }
    if !allow_base_callsign {
        return false;
    }
    let add_base = add.split('-').next().unwrap_or("");
    let me_base = me.split('-').next().unwrap_or("");
    !add_base.is_empty() && add_base == me_base
}

fn emit_status(app: &AppHandle, state: &str, detail: Option<String>) {
    app.state::<crate::runtime::RuntimeState>().record_tnc_status(state, detail.clone());
    let _ = app.emit(
        "tnc_status",
        ConnectionStatus {
            state: state.to_string(),
            detail,
        },
    );
}

fn emit_delivery(app: &AppHandle, message: &QueuedMessage, detail: Option<String>) {
    let _ = app.emit(
        "tnc_delivery",
        DeliveryEvent {
            id: message.id,
            status: message.status.clone(),
            attempts: message.attempt_count,
            detail,
        },
    );
}

pub async fn auto_beacon_from_settings(app: &AppHandle, trigger: &str) {
    let settings = crate::settings::load_settings(app.clone());
    let source = settings.source.trim().to_string();
    if source.is_empty() {
        emit_status(app, "tx_debug", Some(format!("auto beacon skipped ({trigger}): no source callsign configured")));
        return;
    }

    let lat = match settings.beacon_lat.trim().parse::<f64>() {
        Ok(value) => value,
        Err(_) => {
            emit_status(app, "tx_debug", Some(format!("auto beacon skipped ({trigger}): invalid latitude")));
            return;
        }
    };

    let lon = match settings.beacon_lon.trim().parse::<f64>() {
        Ok(value) => value,
        Err(_) => {
            emit_status(app, "tx_debug", Some(format!("auto beacon skipped ({trigger}): invalid longitude")));
            return;
        }
    };

    let symbol = if settings.beacon_symbol.trim().is_empty() {
        "/-".to_string()
    } else {
        settings.beacon_symbol.clone()
    };

    match send_beacon_for_app(
        app.clone(),
        source,
        settings.path,
        lat,
        lon,
        symbol,
        String::new(),
        false,
    )
    .await
    {
        Ok(()) => emit_status(app, "tx_debug", Some(format!("auto beacon sent ({trigger})"))),
        Err(err) => emit_status(app, "tx_error", Some(format!("auto beacon failed ({trigger}): {err}"))),
    }
}

/// Start a periodic beacon timer that fires `auto_beacon_from_settings` at the configured interval.
/// If the interval is 0 (disabled), no task is spawned. Any existing periodic task is cancelled first.
pub async fn start_periodic_beacon(app: &AppHandle, state: &AppState) {
    // Cancel any existing periodic beacon task
    stop_periodic_beacon(state).await;

    let settings = crate::settings::load_settings(app.clone());
    let interval_secs = settings.auto_beacon_interval_secs;
    if interval_secs == 0 {
        emit_status(app, "tx_debug", Some("periodic beacon: disabled (interval = 0)".to_string()));
        return;
    }

    let app_clone = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs as u64));
        // Skip the first tick (immediate) — wait one full interval
        interval.tick().await;
        loop {
            interval.tick().await;
            auto_beacon_from_settings(&app_clone, "periodic").await;
        }
    });

    *state.periodic_beacon_task.lock().await = Some(task);
    emit_status(
        app,
        "tx_debug",
        Some(format!("periodic beacon started (every {interval_secs}s)")),
    );
}

/// Cancel the periodic beacon task if one is running.
pub async fn stop_periodic_beacon(state: &AppState) {
    let mut lock = state.periodic_beacon_task.lock().await;
    if let Some(task) = lock.take() {
        task.abort();
    }
}

async fn ensure_queue_storage(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let existing = { state.queue_file.lock().await.clone() };
    if existing.is_some() {
        return Ok(());
    }

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data directory: {err}"))?;
    fs::create_dir_all(&app_data_dir)
        .map_err(|err| format!("failed to create app data directory: {err}"))?;
    let queue_path = app_data_dir.join("outbound_queue.json");

    // Queue is intentionally non-persistent across launches.
    // Always start with an empty queue and reset the queue file.
    let loaded_queue: Vec<QueuedMessage> = Vec::new();
    let next_id = 1;

    {
        let mut queue = state.queue.lock().await;
        *queue = loaded_queue;
    }
    {
        let mut id_lock = state.next_id.lock().await;
        *id_lock = next_id;
    }
    {
        let mut file_lock = state.queue_file.lock().await;
        *file_lock = Some(queue_path);
    }

    persist_queue(state).await?;

    Ok(())
}

async fn persist_queue(state: &AppState) -> Result<(), String> {
    let path = state
        .queue_file
        .lock()
        .await
        .clone()
        .ok_or_else(|| "queue storage is not initialized".to_string())?;
    let snapshot = state.queue.lock().await.clone();
    let json = serde_json::to_string_pretty(&snapshot)
        .map_err(|err| format!("failed to serialize queue: {err}"))?;
    fs::write(path, json).map_err(|err| format!("failed to persist queue: {err}"))
}

pub async fn restart_retry_task(app: &AppHandle, state: &AppState, epoch: u64) {
    let state_for_retry = state.clone();
    let app_for_retry = app.clone();
    let retry_task = tauri::async_runtime::spawn(async move {
        loop {
            if state_for_retry.connection_epoch.load(Ordering::SeqCst) != epoch {
                break;
            }

            let now = now_ms();
            let due: Vec<u64> = {
                let queue = state_for_retry.queue.lock().await;
                queue
                    .iter()
                    .filter(|msg| {
                        (msg.status == DeliveryState::Queued || msg.status == DeliveryState::Failed)
                            && msg.next_retry_at_ms.map(|deadline| deadline <= now).unwrap_or(true)
                    })
                    .map(|msg| msg.id)
                    .collect()
            };

            for message_id in due {
                let _ = send_attempt(&app_for_retry, &state_for_retry, message_id, "retry").await;
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });

    let mut retry_lock = state.retry_task.lock().await;
    if let Some(previous_task) = retry_lock.take() {
        previous_task.abort();
    }
    *retry_lock = Some(retry_task);
}

async fn send_attempt(
    app: &AppHandle,
    state: &AppState,
    message_id: u64,
    reason: &str,
) -> Result<(), String> {
    enum ActiveTransport {
        Tnc(mpsc::UnboundedSender<Vec<u8>>),
        AprsIs(mpsc::UnboundedSender<String>),
    }

    let transport = {
        let client_lock = state.client.lock().await;
        if let Some(client) = client_lock.as_ref() {
            ActiveTransport::Tnc(client.tx.clone())
        } else {
            drop(client_lock);
            let aprsis_state = app.state::<crate::aprsis::AprsIsState>().inner().clone();
            let handle_lock = aprsis_state.handle.lock().await;
            let handle = handle_lock
                .as_ref()
                .ok_or_else(|| "No active TNC or APRS-IS connection".to_string())?;
            ActiveTransport::AprsIs(handle.tx.clone())
        }
    };

    let queued_message = {
        let queue = state.queue.lock().await;
        let Some(item) = queue.iter().find(|msg| msg.id == message_id) else {
            return Err(format!("Queued message {} not found", message_id));
        };

        if item.status == DeliveryState::Acked || item.status == DeliveryState::Expired {
            return Ok(());
        }

        if item.attempt_count >= MAX_ATTEMPTS {
            drop(queue);
            let mut queue = state.queue.lock().await;
            let Some(item) = queue.iter_mut().find(|msg| msg.id == message_id) else {
                return Err(format!("Queued message {} not found", message_id));
            };
            item.status = DeliveryState::Expired;
            item.next_retry_at_ms = None;
            let expired = item.clone();
            drop(queue);
            persist_queue(state).await?;
            emit_delivery(app, &expired, Some("max retry attempts reached".to_string()));
            return Ok(());
        }

        item.clone()
    };

    let pending_reply_ack = latest_reply_ack_for_station(state, &queued_message.destination).await;
    let payloads = build_queued_message_payloads(
        &queued_message.source,
        &queued_message.destination,
        &queued_message.path,
        &queued_message.body,
        &queued_message.aprs_id,
        pending_reply_ack.as_deref(),
    )?;

    let mut queue = state.queue.lock().await;
    let Some(item) = queue.iter_mut().find(|msg| msg.id == message_id) else {
        return Err(format!("Queued message {} not found", message_id));
    };

    if item.status == DeliveryState::Acked || item.status == DeliveryState::Expired {
        return Ok(());
    }

    if item.attempt_count >= MAX_ATTEMPTS {
        item.status = DeliveryState::Expired;
        item.next_retry_at_ms = None;
        let expired = item.clone();
        drop(queue);
        persist_queue(state).await?;
        emit_delivery(app, &expired, Some("max retry attempts reached".to_string()));
        return Ok(());
    }

    item.payload = payloads.ax25_hex.clone();
    item.ax25_hex = payloads.ax25_hex.clone();
    item.aprsis_payload = payloads.aprsis_payload.clone();

    let outbound_detail = match &transport {
        ActiveTransport::Tnc(_) => format!("transport=tnc ax25_bytes={}", payloads.ax25_frame.len()),
        ActiveTransport::AprsIs(_) => format!("transport=aprsis line_bytes={}", payloads.aprsis_payload.len()),
    };
    item.attempt_count += 1;
    item.status = DeliveryState::Sent;
    item.last_error = None;
    item.next_retry_at_ms = Some(now_ms().saturating_add(retry_delay_ms(item.attempt_count)));
    let sent_snapshot = item.clone();
    
    // Emit detailed TX debug info
    let debug_msg = format!(
        "[TX] id={} reason={} src={} dst={} body_len={} {}",
        message_id,
        reason,
        item.source,
        item.destination,
        item.body.len(),
        outbound_detail
    );
    let _ = app.emit(
        "tnc_status",
        ConnectionStatus {
            state: "tx_debug".to_string(),
            detail: Some(debug_msg),
        },
    );
    
    drop(queue);

    let send_result = match transport {
        ActiveTransport::Tnc(tx) => {
            let frame = encode_data_frame(&payloads.ax25_frame);
            tx.send(frame).map_err(|err| err.to_string())
        }
        ActiveTransport::AprsIs(tx) => tx
            .send(payloads.aprsis_payload.clone())
            .map_err(|err| err.to_string()),
    };

    if let Err(err) = send_result {
        let mut queue = state.queue.lock().await;
        if let Some(item) = queue.iter_mut().find(|msg| msg.id == message_id) {
            item.status = DeliveryState::Failed;
            item.last_error = Some(err.to_string());
            item.next_retry_at_ms = Some(now_ms().saturating_add(RETRY_BASE_MS));
            let failed_snapshot = item.clone();
            drop(queue);
            persist_queue(state).await?;
            let _ = app.emit(
                "tnc_status",
                ConnectionStatus {
                    state: "tx_error".to_string(),
                    detail: Some(format!("[TX ERROR] id={} msg_id={}: {}", message_id, message_id, err)),
                },
            );
            emit_delivery(app, &failed_snapshot, Some(format!("{} failed", reason)));
            return Ok(());
        }
        return Err("Failed to update queue after send error".to_string());
    }

    persist_queue(state).await?;
    emit_delivery(app, &sent_snapshot, Some(reason.to_string()));
    Ok(())
}

/// Mark the newest non-final queued message with APRS id `ack_id` as acked.
///
/// `from` is the station that sent the ACK: it must match the queued
/// message's destination (base-callsign comparison) so that an unrelated
/// station's `ackNN` with a colliding id cannot false-ack our message.
pub async fn mark_acked(
    app: &AppHandle,
    state: &AppState,
    ack_id: &str,
    from: &str,
) -> Result<(), String> {
    let mut queue = state.queue.lock().await;
    let candidate_index = queue
        .iter()
        .enumerate()
        .rev()
        .find(|(_, msg)| {
            msg.status != DeliveryState::Acked
                && msg.status != DeliveryState::Expired
                && msg.aprs_id == ack_id
                && callsign_matches_station(&msg.destination, from, true)
        })
        .map(|(idx, _)| idx);

    let Some(idx) = candidate_index else {
        return Ok(());
    };

    let item = &mut queue[idx];

    item.status = DeliveryState::Acked;
    item.next_retry_at_ms = None;
    item.last_error = None;
    let snapshot = item.clone();
    drop(queue);

    persist_queue(state).await?;
    emit_delivery(app, &snapshot, Some("ack received".to_string()));
    Ok(())
}

fn format_callsign(address: &ParsedAddress) -> String {
    if address.ssid == 0 {
        address.callsign.clone()
    } else {
        format!("{}-{}", address.callsign, address.ssid)
    }
}

/// True for AGWPE monitor text such as `" 1:Fm W1AW To APRS Via WIDE1-1
/// <UI pid=F0 Len=12 >[12:00:00]"`: a port number, a colon, then "Fm ".
/// Graywolf's 'U' frames are TNC2 lines (`W1AW>APRS,WIDE1-1:...`) and
/// don't match.
fn is_agwpe_monitor_text(text: &str) -> bool {
    let trimmed = text.trim_start();
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    digits > 0 && trimmed[digits..].starts_with(":Fm ")
}

pub(crate) fn parse_tnc2_protocol_view(line: &str) -> Option<(String, String, Vec<String>, String)> {
    let gt = line.find('>')?;
    let source = line[..gt].trim().to_string();
    let after_src = &line[gt + 1..];

    let colon = after_src.find(':')?;
    let addr_part = &after_src[..colon];
    let info = after_src[colon + 1..].to_string();

    let parts: Vec<&str> = addr_part.split(',').collect();
    let destination = parts.first().map(|s| s.trim().to_string()).unwrap_or_default();
    let path = parts.iter().skip(1).map(|s| s.trim().to_string()).collect();

    Some((source, destination, path, info))
}

fn parsed_frame_to_received_message(payload: &[u8], parsed: ParsedUiFrame) -> ReceivedMessage {
    let info_text = String::from_utf8_lossy(&parsed.info).to_string();
    let mic_e_text = decode_mic_e(&parsed);
    let display_text = mic_e_text.clone().unwrap_or_else(|| info_text.clone());
    let outer_source = format_callsign(&parsed.source);
    let outer_destination = format_callsign(&parsed.destination);
    let outer_path: Vec<String> = parsed.digipeaters.iter().map(format_callsign).collect();
    let (protocol_source, protocol_destination, protocol_path, protocol_info_text) = info_text
        .strip_prefix('}')
        .and_then(parse_tnc2_protocol_view)
        .map(|(s, d, p, i)| (Some(s), Some(d), Some(p), Some(i)))
        .unwrap_or_else(|| {
            (
                Some(outer_source.clone()),
                Some(outer_destination.clone()),
                Some(outer_path.clone()),
                Some(info_text.clone()),
            )
        });
    let normalized = crate::settings::normalize_received_fields(
        crate::settings::NormalizeFieldsInput {
            source: Some(&outer_source),
            destination: Some(&outer_destination),
            raw_body: &info_text,
            protocol_source: protocol_source.as_deref(),
            protocol_destination: protocol_destination.as_deref(),
            protocol_info_text: protocol_info_text.as_deref(),
            mic_e_text: mic_e_text.as_deref(),
            ..Default::default()
        },
    );
    ReceivedMessage {
        text: format!(
            "{} > {}{}: {}",
            outer_source,
            outer_destination,
            if outer_path.is_empty() {
                String::new()
            } else {
                format!(
                    " via {}",
                    outer_path.join(",")
                )
            },
            display_text
        ),
        payload_hex: to_hex(payload),
        bytes: payload.len(),
        source: Some(format_callsign(&parsed.source)),
        destination: Some(format_callsign(&parsed.destination)),
        path: Some(parsed.digipeaters.iter().map(format_callsign).collect()),
        info_text: Some(info_text),
        protocol_source,
        protocol_destination,
        protocol_path,
        protocol_info_text,
        from: normalized.from,
        to: normalized.to,
        body: normalized.body,
        aprs_type: normalized.aprs_type,
        is_bulletin: normalized.is_bulletin,
        mic_e_decoded: normalized.mic_e_decoded,
        summary_source: normalized.summary_source,
        summary_destination: normalized.summary_destination,
        mic_e_text,
    }
}

/// Write a KISS frame to the TNC's TX writer. Returns `Ok(())` on success,
/// `Err(())` if the writer is not connected or the write/flush fails.
async fn write_kiss_frame(
    writer: &Arc<Mutex<Option<OwnedWriteHalf>>>,
    data: &[u8],
) -> Result<(), ()> {
    let mut writer_lock = writer.lock().await;
    let Some(ref mut writer) = *writer_lock else {
        return Err(());
    };
    writer.write_all(data).await.map_err(|_| ())?;
    writer.flush().await.map_err(|_| ())?;
    Ok(())
}

/// Process a single AGW frame from the TNC receiver.
/// Handles K-frames (AX.25 parsing, ACK detection, auto-reply, UI events)
/// and T-frames (Dire Wolf reports of our own transmissions, logged only).
async fn process_agw_frame(
    agw_frame: AgwFrame,
    app: &AppHandle,
    state: &AppState,
    tx_writer: &Arc<Mutex<Option<OwnedWriteHalf>>>,
    my_callsign: &str,
    my_call: &str,
    my_ssid: u8,
) {
    let agw_hex = to_hex(&agw_frame.data);
    let debug_detail = format!(
        "[RX AGW] kind={} data_len={} hex={}",
        agw_frame.kind as char,
        agw_frame.data.len(),
        if agw_hex.len() > 100 {
            format!("{}...", &agw_hex[..100])
        } else {
            agw_hex.clone()
        }
    );

    if agw_frame.kind == b'K' {
        let ax25_bytes = if agw_frame.data.len() > 1 {
            &agw_frame.data[1..]
        } else {
            &agw_frame.data[..]
        };
        match parse_ui_frame(ax25_bytes) {
            Ok(ax25_payload) => {
                let info_text = String::from_utf8_lossy(&ax25_payload.info).to_string();

                // Unwrap third-party format (`}`) so IGate-gated messages get
                // the same message/ACK handling as on the APRS-IS path; the
                // inner source is the station we ACK and attribute acks to.
                let (aprs_source, aprs_info) = if let Some((inner_src, _dst, _path, inner_info)) =
                    info_text
                        .strip_prefix('}')
                        .and_then(parse_tnc2_protocol_view)
                {
                    (inner_src, inner_info)
                } else {
                    (format_callsign(&ax25_payload.source), info_text.clone())
                };

                let settings = crate::settings::load_settings(app.clone());
                for ack_id in ack_ids_for_station(&aprs_info, my_callsign, settings.promiscuous_base_callsign) {
                    let _ = mark_acked(app, state, &ack_id, &aprs_source).await;
                }
                if let Some(aprs_msg) = parse_aprs_message(&aprs_info) {
                    if callsign_matches_station(
                        &aprs_msg.addressee,
                        my_callsign,
                        settings.promiscuous_base_callsign,
                    ) {
                        crate::runtime::maybe_play_addressed_message_sound(app, &aprs_msg.text);
                        if let Some(ref msgid) = aprs_msg.msgid {
                            remember_latest_reply_ack(state, &aprs_source, msgid).await;
                            let ack_info = build_aprs_ack_info(&aprs_source, msgid);
                            // ACK over the configured digipeater path so it can
                            // reach distant stations, same as our own messages.
                            let digi_strings =
                                parse_digipeater_path(&settings.path).unwrap_or_default();
                            let digis: Vec<&str> =
                                digi_strings.iter().map(String::as_str).collect();
                            let ack_frame =
                                build_ui_frame(APRS_TOCALL, my_call, my_ssid, &digis, &ack_info);
                            let kiss_frame = crate::kiss::encode_data_frame(&ack_frame);
                            let _ = write_kiss_frame(tx_writer, &kiss_frame).await;
                        }
                    }
                }
                let event = parsed_frame_to_received_message(ax25_bytes, ax25_payload);
                if let Ok(value) = serde_json::to_value(&event) {
                    let _ = crate::settings::append_received_message(app.clone(), value);
                }
                let _ = app.emit("tnc_message", event);
                emit_status(app, "rx_parsed", Some(format!("{} [PARSED OK]", debug_detail)));
            }
            Err(parse_err) => {
                emit_status(
                    app,
                    "rx_parse_error",
                    Some(format!("{} [PARSE ERROR: {:?}]", debug_detail, parse_err)),
                );
            }
        }
    } else if agw_frame.kind == b'U' {
        // Graywolf sends monitored UI frames as kind 'U' with TNC2-format
        // text data (e.g. "SRC>DST,PATH:info text\r").
        let monitor_text = String::from_utf8_lossy(&agw_frame.data).to_string();
        let tnc2_line = monitor_text.trim_matches(|c| c == '\r' || c == '\0');
        if is_agwpe_monitor_text(tnc2_line) {
            // Dire Wolf sends every received packet twice once both raw ('k')
            // and monitor ('m') delivery are on: as a raw 'K' frame, handled
            // above, and as a 'U' frame in AGWPE's human-readable monitor
            // format. That format isn't TNC2, so parsing it here produced a
            // second, garbled copy of the packet. The 'K' copy is the one we
            // keep.
            emit_status(app, "rx_debug", Some(format!("{debug_detail} [monitor-format U frame skipped; raw K frame carries the packet]")));
            return;
        }
        emit_status(
            app,
            "rx_debug",
            Some(format!(
                "{} tnc2={:?}",
                debug_detail,
                tnc2_line,
            )),
        );
        if let Some((aprs_source, _dest, _path, info_text)) =
            parse_tnc2_protocol_view(tnc2_line)
        {
            let settings = crate::settings::load_settings(app.clone());
            for ack_id in ack_ids_for_station(&info_text, my_callsign, settings.promiscuous_base_callsign) {
                let _ = mark_acked(app, state, &ack_id, &aprs_source).await;
            }
            if let Some(aprs_msg) = parse_aprs_message(&info_text) {
                if callsign_matches_station(
                    &aprs_msg.addressee,
                    my_callsign,
                    settings.promiscuous_base_callsign,
                ) {
                    crate::runtime::maybe_play_addressed_message_sound(app, &aprs_msg.text);
                    if let Some(ref msgid) = aprs_msg.msgid {
                        remember_latest_reply_ack(state, &aprs_source, msgid).await;
                        let ack_info = build_aprs_ack_info(&aprs_source, msgid);
                        let digi_strings =
                            parse_digipeater_path(&settings.path).unwrap_or_default();
                        let digis: Vec<&str> =
                            digi_strings.iter().map(String::as_str).collect();
                        let ack_frame =
                            build_ui_frame(APRS_TOCALL, my_call, my_ssid, &digis, &ack_info);
                        let kiss_frame = crate::kiss::encode_data_frame(&ack_frame);
                        let _ = write_kiss_frame(tx_writer, &kiss_frame).await;
                    }
                }
            }
            // Build a parsable AX.25 frame-like payload for the received
            // message store and event emission.
            if let Some((_src, dst, path, info)) = parse_tnc2_protocol_view(tnc2_line) {
                use crate::ax25::ParsedAddress;
                let path_addrs: Vec<ParsedAddress> = path.iter()
                    .filter_map(|p| {
                        let parts: Vec<&str> = p.splitn(2, '-').collect();
                        let callsign = parts[0].to_string();
                        let ssid: u8 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                        Some(ParsedAddress { callsign, ssid, repeated: false })
                    })
                    .collect();
                let parsed = ParsedUiFrame {
                    source: ParsedAddress {
                        callsign: aprs_source.clone(),
                        ssid: 0,
                        repeated: false,
                    },
                    destination: ParsedAddress {
                        callsign: dst.split('-').next().unwrap_or(&dst).to_string(),
                        ssid: dst.split('-').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0),
                        repeated: false,
                    },
                    digipeaters: path_addrs,
                    control: 0x03, // UI frame
                    pid: 0xF0,     // no layer 3
                    info: info.as_bytes().to_vec(),
                };
                let event = parsed_frame_to_received_message(&[], parsed);
                if let Ok(value) = serde_json::to_value(&event) {
                    let _ = crate::settings::append_received_message(app.clone(), value);
                }
                let _ = app.emit("tnc_message", event);
            }
            emit_status(app, "rx_parsed", Some(format!("{} [PARSED OK]", debug_detail)));
        } else {
            emit_status(
                app,
                "rx_parse_error",
                Some(format!("{} [TNC2 PARSE ERROR]", debug_detail)),
            );
        }
    } else if agw_frame.kind == b'T' {
        let monitor_text = String::from_utf8_lossy(&agw_frame.data).to_string();
        emit_status(
            app,
            "rx_debug",
            Some(format!(
                "{} monitor_text={:?}",
                debug_detail,
                monitor_text.trim_end_matches('\0')
            )),
        );
        // A 'T' frame is Dire Wolf telling us it put one of our own frames
        // on the air. That proves the frame was transmitted, not that the
        // other station received it, so it must not change delivery state.
        // Only an ack from the addressee marks a message Acked; until
        // then the retry task keeps resending.
    } else {
        emit_status(app, "rx_debug", Some(debug_detail));
    }
}

#[tauri::command]
pub async fn connect_tnc(
    app: AppHandle,
    callsign: String,
    host: String,
    port: u16,
) -> Result<(), String> {
    let state = app.state::<AppState>().inner().clone();
    let aprsis_state = app.state::<crate::aprsis::AprsIsState>().inner().clone();

    ensure_queue_storage(&app, &state).await?;

    // Lock ordering for cross-transport exclusivity:
    // always acquire TNC client lock first, then APRS-IS handle lock.
    let mut lock = state.client.lock().await;
    if lock.is_some() {
        return Err("TNC connection already active".to_string());
    }

    let aprsis_lock = aprsis_state.handle.lock().await;
    if aprsis_lock.is_some() {
        return Err("APRS-IS is active. Disconnect APRS-IS before connecting TNC.".to_string());
    }
    drop(aprsis_lock);

    let (my_call, my_ssid) = parse_ax25_station(&callsign, "Source callsign")?;
    let my_callsign_norm = format_station_with_ssid(&my_call, my_ssid);
    *state.my_callsign.lock().await = Some(my_callsign_norm.clone());

    let current_epoch = state.connection_epoch.fetch_add(1, Ordering::SeqCst) + 1;

    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let app_for_task = app.clone();
    let host_for_task = host.clone();
    let state_for_task = state.clone();
    let client_slot = state.client.clone();
    let epoch_ref = state.connection_epoch.clone();
    let my_callsign_for_task = my_callsign_norm.clone();
    let my_call_for_task = my_call.clone();
    let my_ssid_for_task = my_ssid;

    let task = tauri::async_runtime::spawn(async move {
        // Connect TX on the configured KISS port.
        let tx_port = port;
        emit_status(&app_for_task, "tx_connecting", Some(format!("{host_for_task}:{tx_port}")));

        let tx_stream = match TcpStream::connect((host_for_task.as_str(), tx_port)).await {
            Ok(stream) => {
                emit_status(&app_for_task, "tx_connected", None);
                Some(stream)
            }
            Err(err) => {
                emit_status(&app_for_task, "tx_error", Some(err.to_string()));
                None
            }
        };

        // Connect RX on the AGW port. An explicit `agw_port` setting wins;
        // otherwise fall back to the Dire Wolf / Graywolf convention of KISS
        // port - 1 (8001/8000), or 8000 if the subtraction would underflow.
        let agw_override = crate::settings::load_settings(app_for_task.clone())
            .agw_port
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|p| *p > 0);
        let rx_port = agw_override.unwrap_or_else(|| tx_port.checked_sub(1).unwrap_or(8000));
        emit_status(&app_for_task, "rx_connecting", Some(format!("{host_for_task}:{rx_port}")));

        let rx_stream = match TcpStream::connect((host_for_task.as_str(), rx_port)).await {
            Ok(stream) => {
                emit_status(&app_for_task, "rx_connected", Some(format!("{host_for_task}:{rx_port}")));
                Some(stream)
            }
            Err(err) => {
                emit_status(&app_for_task, "rx_error", Some(format!("connect error: {err}")));
                None
            }
        };

        if tx_stream.is_none() || rx_stream.is_none() {
            let detail = match (tx_stream.is_some(), rx_stream.is_some()) {
                (false, false) => "both TX and RX connections failed".to_string(),
                (false, true) => "TX connection failed; closing RX session".to_string(),
                (true, false) => "RX connection failed; closing TX session".to_string(),
                (true, true) => "unknown TNC connection failure".to_string(),
            };
            emit_status(&app_for_task, "disconnected", Some(detail));
            if epoch_ref.load(Ordering::SeqCst) == current_epoch {
                let mut client_lock = client_slot.lock().await;
                *client_lock = None;
            }
            return;
        }

        // Split streams and wrap in Arc<Mutex> for shared access
        let (tx_reader, tx_writer) = tx_stream
            .map(|s| s.into_split())
            .map(|(r, w)| (Some(r), Some(w)))
            .unwrap_or((None, None));

        let (rx_reader, rx_writer) = rx_stream
            .map(|s| s.into_split())
            .map(|(r, w)| (Some(r), Some(w)))
            .unwrap_or((None, None));

        let rx_writer = Arc::new(Mutex::new(rx_writer));

        // Send AGW startup sequence and keep writer alive for session lifetime.
        let mut agw_startup_failed = false;
        {
            let mut writer_lock = rx_writer.lock().await;
            if let Some(ref mut writer) = *writer_lock {
                // Build registration frames for both Dire Wolf and Graywolf.
                // Each TNC recognizes a different register command:
                //   R = register callsign (Dire Wolf)
                //   X = register callsign (Graywolf)
                let mut r_frame = [0u8; AGW_HEADER_LEN];
                r_frame[4] = b'R';
                let call_bytes = my_call_for_task.as_bytes();
                let copy_len = call_bytes.len().min(10);
                r_frame[8..8 + copy_len].copy_from_slice(&call_bytes[..copy_len]);

                let mut x_frame = [0u8; AGW_HEADER_LEN];
                x_frame[4] = b'X';
                x_frame[8..8 + copy_len].copy_from_slice(&call_bytes[..copy_len]);

                // Build monitoring-start frames. Both TNCs are sent both
                // commands; whichever one they recognize will enable RX:
                //   k = start KISS monitoring (Dire Wolf)
                //   m = start monitoring (Graywolf)
                let k_frame = build_startup_command_frames()[2]; // k
                let m_frame = build_startup_command_frames()[3]; // m

                let startup_cmds: &[&[u8]] = &[
                    &r_frame,  // Dire Wolf register (R)
                    &x_frame,  // Graywolf register (X)
                    build_startup_command_frames()[1], // G (port info)
                    k_frame,   // Dire Wolf monitor start (k)
                    m_frame,   // Graywolf monitor start (m)
                ];

                for &cmd in startup_cmds {
                    let cmd_kind = cmd[4] as char;
                    let cmd_hex = to_hex(cmd);
                    if let Err(err) = writer.write_all(cmd).await {
                        emit_status(&app_for_task, "rx_error", Some(format!("AGW cmd '{cmd_kind}' write failed: {err}")));
                        agw_startup_failed = true;
                        break;
                    }
                    if let Err(err) = writer.flush().await {
                        emit_status(&app_for_task, "rx_error", Some(format!("AGW cmd '{cmd_kind}' flush failed: {err}")));
                        agw_startup_failed = true;
                        break;
                    }
                    emit_status(&app_for_task, "rx_debug", Some(format!("[AGW TX] cmd='{cmd_kind}' len={} hex={}", cmd.len(), cmd_hex)));
                }

                if !agw_startup_failed {
                    emit_status(&app_for_task, "rx_monitoring", Some("AGW startup sent (R,X,G,k,m)".to_string()));
                }
            }
        }

        if agw_startup_failed {
            emit_status(&app_for_task, "disconnected", Some("AGW startup failed".to_string()));
            if epoch_ref.load(Ordering::SeqCst) == current_epoch {
                let mut client_lock = client_slot.lock().await;
                *client_lock = None;
            }
            return;
        }

        let tx_writer = Arc::new(Mutex::new(tx_writer));
        let tx_reader = Arc::new(Mutex::new(tx_reader));
        let rx_reader = Arc::new(Mutex::new(rx_reader));

        let mut agw_decoder = AgwBinaryDecoder::default();

        // Signal that both connections are ready and event loop is starting
        emit_status(&app_for_task, "tx_rx_loop_start", Some("Event loop starting, ready to send/receive".to_string()));
        auto_beacon_from_settings(&app_for_task, "tnc connect").await;
        start_periodic_beacon(&app_for_task, &state_for_task).await;

        loop {
            tokio::select! {
                maybe_frame = rx.recv() => {
                    match maybe_frame {
                        Some(frame) => {
                            if write_kiss_frame(&tx_writer, &frame).await.is_err() {
                                emit_status(&app_for_task, "tx_error", Some("TX write failed".to_string()));
                                break;
                            }
                        }
                        None => break,
                    }
                }
                tx_read = async {
                    let mut reader_lock = tx_reader.lock().await;
                    if let Some(ref mut reader) = *reader_lock {
                        let mut buf = [0u8; 4096];
                        reader.read(&mut buf).await.map(|n| (n, buf))
                    } else {
                        Err(std::io::Error::other("no tx reader"))
                    }
                } => {
                    // TX port is send-only, so ignore any reads (shouldn't happen with KISS)
                    match tx_read {
                        Ok((0, _)) => {
                            emit_status(&app_for_task, "tx_error", Some("TX connection closed".to_string()));
                            break;
                        }
                        Err(_) => {
                            // Reader not available, continue
                        }
                        _ => {}
                    }
                }
                rx_read = async {
                    let mut reader_lock = rx_reader.lock().await;
                    if let Some(ref mut reader) = *reader_lock {
                        let mut buf = [0u8; 4096];
                        reader.read(&mut buf).await.map(|n| (n, buf))
                    } else {
                        Err(std::io::Error::other("no rx reader"))
                    }
                } => {
                    match rx_read {
                        Ok((0, _)) => {
                            emit_status(&app_for_task, "rx_eof", Some("RX connection closed (read 0 bytes)".to_string()));
                            break; // Exit loop on EOF
                        }
                        Ok((n, buf)) => {
                            // Raw hex dumps are dev-build only: on a busy feed
                            // they flood the webview bridge in release builds.
                            if cfg!(debug_assertions) {
                                let raw_hex = to_hex(&buf[..n]);
                                emit_status(&app_for_task, "rx_raw", Some(format!("{n}B: {raw_hex}")));
                            }

                            // Parse AGW binary frames
                            let frames = agw_decoder.ingest(&buf[..n]);
                            if frames.is_empty() && n > 0 && cfg!(debug_assertions) {
                                // Data received but no complete frames yet (buffered)
                                emit_status(&app_for_task, "rx_buffered", Some(format!("[AGW buffering] received {n}B, accumulating frames...")));
                            }

                            for agw_frame in frames {
                                process_agw_frame(
                                    agw_frame,
                                    &app_for_task,
                                    &state_for_task,
                                    &tx_writer,
                                    &my_callsign_for_task,
                                    &my_call_for_task,
                                    my_ssid_for_task,
                                ).await;
                            }
                        }
                        Err(err) => {
                            // Log RX read error
                            emit_status(&app_for_task, "rx_error", Some(format!("RX read error: {err}")));
                            break;
                        }
                    }
                }
            }
        }

        emit_status(&app_for_task, "disconnected", None);
        if epoch_ref.load(Ordering::SeqCst) == current_epoch {
            let mut client_lock = client_slot.lock().await;
            *client_lock = None;
        }
    });

    *lock = Some(ClientHandle { tx, task });
    drop(lock);

    restart_retry_task(&app, &state, current_epoch).await;

    Ok(())
}

#[tauri::command]
pub async fn disconnect_tnc(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>().inner().clone();
    state.connection_epoch.fetch_add(1, Ordering::SeqCst);

    let mut lock = state.client.lock().await;
    let Some(client) = lock.take() else {
        emit_status(&app, "disconnected", Some("already disconnected".to_string()));
        return Ok(());
    };

    drop(client.tx);
    client.task.abort();

    let mut retry_lock = state.retry_task.lock().await;
    if let Some(retry_task) = retry_lock.take() {
        retry_task.abort();
    }

    stop_periodic_beacon(&state).await;

    emit_status(&app, "disconnected", Some("manual disconnect".to_string()));
    Ok(())
}

#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    source: String,
    destination: String,
    path: String,
    message: String,
) -> Result<SentMessage, String> {
    let state = app.state::<AppState>().inner().clone();
    let aprsis_state = app.state::<crate::aprsis::AprsIsState>().inner().clone();

    ensure_queue_storage(&app, &state).await?;

    if source.trim().is_empty() {
        return Err("Source callsign is required".to_string());
    }

    if destination.trim().is_empty() {
        return Err("Destination is required".to_string());
    }

    if message.trim().is_empty() {
        return Err("Message is required".to_string());
    }

    let message_id = {
        let mut lock = state.next_id.lock().await;
        let id = *lock;
        *lock = lock.saturating_add(1);
        id
    };

    let source = source.trim().to_uppercase();
    let destination = destination.trim().to_uppercase();
    // Strip embedded CR/LF — must not appear in APRS info fields (per spec)
    let body = strip_crlf(message.trim());
    validate_aprs_message_text(&body)?;

    let (src_call, src_ssid) = parse_ax25_station(&source, "Source callsign")?;

    let digi_strings = parse_digipeater_path(&path)?;
    let normalized_path = digi_strings.join(",");

    let aprs_message_id = format!("{:02}", message_id % 100);
    let source_with_ssid = format_station_with_ssid(&src_call, src_ssid);
    let pending_reply_ack = latest_reply_ack_for_station(&state, &destination).await;
    let payloads = build_queued_message_payloads(
        &source_with_ssid,
        &destination,
        &normalized_path,
        &body,
        &aprs_message_id,
        pending_reply_ack.as_deref(),
    )?;

    {
        let mut queue = state.queue.lock().await;
        queue.push(QueuedMessage {
            id: message_id,
            destination,
            body,
            path: normalized_path,
            payload: payloads.ax25_hex.clone(),
            ax25_hex: payloads.ax25_hex.clone(),
            aprsis_payload: payloads.aprsis_payload.clone(),
            source: source_with_ssid.clone(),
            aprs_id: aprs_message_id.clone(),
            created_at_ms: now_ms(),
            attempt_count: 0,
            status: DeliveryState::Queued,
            next_retry_at_ms: Some(now_ms()),
            last_error: None,
        });
    }

    persist_queue(&state).await?;

    let has_transport = {
        if state.client.lock().await.is_some() {
            true
        } else {
            aprsis_state.handle.lock().await.is_some()
        }
    };
    if has_transport {
        let _ = send_attempt(&app, &state, message_id, "initial send").await;
    }

    let queue = state.queue.lock().await;
    let snapshot = queue
        .iter()
        .find(|msg| msg.id == message_id)
        .cloned()
        .ok_or_else(|| "Failed to resolve queued message state".to_string())?;
    drop(queue);

    if !has_transport {
        emit_delivery(&app, &snapshot, Some("queued".to_string()));
    }

    Ok(SentMessage {
        id: message_id,
        status: snapshot.status,
        payload_hex: payloads.ax25_hex,
        bytes: payloads.ax25_frame.len(),
    })
}

#[tauri::command]
pub async fn send_beacon(
    app: AppHandle,
    source: String,
    path: String,
    lat: f64,
    lon: f64,
    symbol: String,
    comment: String,
) -> Result<(), String> {
    send_beacon_for_app(app, source, path, lat, lon, symbol, comment, true).await
}

#[allow(clippy::too_many_arguments)] // command payload maps 1:1 to the UI fields
pub async fn send_beacon_for_app(
    app: AppHandle,
    source: String,
    path: String,
    lat: f64,
    lon: f64,
    symbol: String,
    comment: String,
    manual: bool,
) -> Result<(), String> {
    if manual {
        let remaining_ms = app.state::<crate::runtime::RuntimeState>().manual_beacon_remaining_ms(MANUAL_BEACON_COOLDOWN_MS);
        if remaining_ms > 0 {
            let remaining_secs = remaining_ms.div_ceil(1000).max(1);
            return Err(format!(
                "Manual beacon cooldown active. Wait {} second(s).",
                remaining_secs
            ));
        }
    }

    let state = app.state::<AppState>().inner().clone();
    let aprsis_state = app.state::<crate::aprsis::AprsIsState>().inner().clone();

    validate_position(lat, lon)?;

    let (symbol_table, symbol_code) = resolve_uncompressed_symbol(&symbol)?;

    // Strip embedded CR/LF from comment before embedding in the info field.
    let comment = strip_crlf(&comment);
    validate_uncompressed_position_comment(&comment)?;
    let info = crate::ax25::build_aprs_position_info(lat, lon, symbol_table, symbol_code, &comment);

    let source = source.trim().to_uppercase();
    let (src_call, src_ssid) = parse_ax25_station(&source, "Source callsign")?;

    let digi_strings = parse_digipeater_path(&path)?;
    let digis: Vec<&str> = digi_strings.iter().map(String::as_str).collect();

    let ax25_frame = build_ui_frame(APRS_TOCALL, &src_call, src_ssid, &digis, &info);
    let kiss_frame = crate::kiss::encode_data_frame(&ax25_frame);

    let is_tnc = { state.client.lock().await.is_some() };
    let is_aprsis = { aprsis_state.handle.lock().await.is_some() };

    if is_tnc {
        let client = state.client.lock().await;
        if let Some(handle) = client.as_ref() {
            handle.tx.send(kiss_frame).map_err(|_| "TNC TX channel closed".to_string())?;
        }
    } else if is_aprsis {
        let info_str = String::from_utf8_lossy(&info).to_string();
        let tnc2_line = format!("{source}>{APRS_TOCALL}:{info_str}");
        crate::aprsis::try_send(&aprsis_state, tnc2_line).await?;
    } else {
        return Err("No active TNC or APRS-IS connection".to_string());
    }

    if manual {
        app.state::<crate::runtime::RuntimeState>().set_last_manual_beacon_ms(now_ms());
    }

    Ok(())
}

#[tauri::command]
pub async fn clear_queue(app: AppHandle) -> Result<usize, String> {
    let state = app.state::<AppState>().inner().clone();
    let mut queue = state.queue.lock().await;
    let mut count = 0usize;
    for msg in queue.iter_mut() {
        if msg.status != DeliveryState::Acked && msg.status != DeliveryState::Expired {
            msg.status = DeliveryState::Expired;
            msg.next_retry_at_ms = None;
            count += 1;
        }
    }
    drop(queue);
    persist_queue(&state).await?;
    Ok(count)
}

#[derive(Clone, Serialize)]
pub struct QueueSnapshot {
    id: u64,
    status: String,
    attempts: u32,
    source: String,
    destination: String,
    body: String,
    ax25_hex: String,
    next_retry_at_ms: Option<u64>,
}

#[tauri::command]
pub async fn inspect_queue(app: AppHandle) -> Result<Vec<QueueSnapshot>, String> {
    let state = app.state::<AppState>().inner().clone();
    let queue = state.queue.lock().await;
    let snapshot: Vec<QueueSnapshot> = queue
        .iter()
        .map(|msg| QueueSnapshot {
            id: msg.id,
            status: format!("{:?}", msg.status),
            attempts: msg.attempt_count,
            source: msg.source.clone(),
            destination: msg.destination.clone(),
            body: msg.body.clone(),
            ax25_hex: msg.ax25_hex.clone(),
            next_retry_at_ms: msg.next_retry_at_ms,
        })
        .collect();
    Ok(snapshot)
}

pub async fn connect_tnc_for_app(
    app: AppHandle,
    callsign: String,
    host: String,
    port: u16,
) -> Result<(), String> {
    connect_tnc(app, callsign, host, port).await
}

pub async fn disconnect_tnc_for_app(app: AppHandle) -> Result<(), String> {
    disconnect_tnc(app).await
}

pub async fn send_message_for_app(
    app: AppHandle,
    source: String,
    destination: String,
    path: String,
    message: String,
) -> Result<SentMessage, String> {
    send_message(app, source, destination, path, message).await
}

pub async fn clear_queue_for_app(app: AppHandle) -> Result<usize, String> {
    clear_queue(app).await
}

pub async fn inspect_queue_for_app(app: AppHandle) -> Result<Vec<QueueSnapshot>, String> {
    inspect_queue(app).await
}

#[tauri::command]
pub fn get_manual_beacon_cooldown_remaining(app: AppHandle) -> u64 {
    let remaining_ms = app.state::<crate::runtime::RuntimeState>().manual_beacon_remaining_ms(MANUAL_BEACON_COOLDOWN_MS);
    remaining_ms.div_ceil(1000)
}

#[cfg(test)]
mod tests {
    use super::{build_queued_message_payloads, callsign_matches_station, ack_ids_for_station, is_agwpe_monitor_text, parse_ack_id, parse_ax25_station, parse_digipeater_path, resolve_uncompressed_symbol, split_reply_ack_id, strip_crlf, validate_aprs_message_text, validate_ax25_callsign, validate_position, validate_uncompressed_position_comment, APRS_TOCALL};
    use crate::ax25::build_aprs_position_info;

    #[test]
    fn callsign_matching_accepts_exact_and_base_calls() {
        assert!(callsign_matches_station("WT1W-15", "WT1W-15", false));
        assert!(callsign_matches_station("WT1W", "WT1W-15", true));
        assert!(callsign_matches_station("wt1w", "wt1w-15", true));
    }

    #[test]
    fn callsign_matching_rejects_different_calls() {
        assert!(!callsign_matches_station("WT1X", "WT1W-15", true));
        assert!(!callsign_matches_station("N0CALL", "WT1W-15", true));
        assert!(!callsign_matches_station("WT1W", "WT1W-15", false));
    }

    #[test]
    fn parse_ack_id_accepts_numeric() {
        assert_eq!(parse_ack_id("ack42"), Some("42".to_string()));
        assert_eq!(parse_ack_id("ACK:42"), Some("42".to_string()));
        assert_eq!(parse_ack_id("ack1"), Some("1".to_string()));
        assert_eq!(parse_ack_id("ACK:99"), Some("99".to_string()));
    }

    #[test]
    fn parse_ack_id_accepts_alphanumeric() {
        assert_eq!(parse_ack_id("ackAB1"), Some("AB1".to_string()));
        assert_eq!(parse_ack_id("ACK:ABC"), Some("ABC".to_string()));
        assert_eq!(parse_ack_id("ack1A2B3"), Some("1A2B3".to_string()));
        assert_eq!(parse_ack_id("ack12}34"), Some("12".to_string()));
    }

    #[test]
    fn parse_ack_id_rejects_overlong() {
        // More than 5 characters should be rejected
        assert_eq!(parse_ack_id("ackABCDE1"), None);
        assert_eq!(parse_ack_id("ACK:123456"), None);
    }

    #[test]
    fn parse_ack_id_does_not_panic_on_multibyte_input() {
        let lossy = String::from_utf8_lossy(&[b'a', 0xFF, b'x']).to_string();
        for input in ["a€", "ab€", "ack€", lossy.as_str()] {
            assert_eq!(parse_ack_id(input), None);
        }
    }

    #[test]
    fn parse_ack_id_rejects_empty() {
        // Just "ack" or "ACK:" with nothing after should be rejected
        assert_eq!(parse_ack_id("ack"), None);
        assert_eq!(parse_ack_id("ACK:"), None);
    }

    #[test]
    fn parse_ack_id_rejects_non_alphanumeric() {
        // Non-alphanumeric characters should be rejected
        assert_eq!(parse_ack_id("ack@42"), None);
        assert_eq!(parse_ack_id("ack 42"), None);
        assert_eq!(parse_ack_id("ack-42"), None);
    }

    #[test]
    fn agwpe_monitor_text_is_told_apart_from_tnc2() {
        // Dire Wolf's monitor format, as its server.c documents it.
        assert!(is_agwpe_monitor_text(" 1:Fm ZL4FOX-8 To Q7P2U2 <UI pid=F0 Len=28 >[08:25:07]\r`I1*l V>/\"9<}[:Barts Tracker"));
        assert!(is_agwpe_monitor_text("1:Fm W1AW To APRS Via WIDE1-1 <UI pid=F0 Len=5 >[12:00:00]\rhello"));
        // Graywolf's TNC2 lines must still be parsed.
        assert!(!is_agwpe_monitor_text("W1AW>APRS,WIDE1-1:hello"));
        assert!(!is_agwpe_monitor_text("W1AW-9>APRS::WT1W-15  :Fm the field{12"));
    }

    #[test]
    fn ack_ids_for_station_reads_acks_addressed_to_us() {
        assert_eq!(ack_ids_for_station(":W1XYZ    :ackAB1", "W1XYZ", false), vec!["AB1"]);
        assert_eq!(ack_ids_for_station(":W1XYZ    :ack42", "W1XYZ", false), vec!["42"]);
        assert_eq!(ack_ids_for_station(":W1XYZ    :ack12}34", "W1XYZ", false), vec!["12"]);
        // Reply-ack: an incoming message {MM}AA acknowledges our AA.
        assert_eq!(ack_ids_for_station(":W1XYZ    :hello{05}07", "W1XYZ", false), vec!["07"]);
    }

    #[test]
    fn ack_ids_for_station_ignores_acks_to_other_stations() {
        assert!(ack_ids_for_station(":K2ABC    :ack07", "W1XYZ", false).is_empty());
        assert!(ack_ids_for_station(":K2ABC    :hello{05}07", "W1XYZ", false).is_empty());
        // Another SSID of ours counts only when base-callsign matching is on.
        assert!(ack_ids_for_station(":W1XYZ-7  :ack07", "W1XYZ-9", false).is_empty());
        assert_eq!(ack_ids_for_station(":W1XYZ-7  :ack07", "W1XYZ-9", true), vec!["07"]);
        // A bare "ack07" info field names no addressee, so it can't be matched.
        assert!(ack_ids_for_station("ack07", "W1XYZ", false).is_empty());
    }

    #[test]
    fn split_reply_ack_id_parses_new_message_number_format() {
        assert_eq!(split_reply_ack_id("12}"), Some(("12", None)));
        assert_eq!(split_reply_ack_id("12}34"), Some(("12", Some("34"))));
        assert_eq!(split_reply_ack_id("ABCDE"), Some(("ABCDE", None)));
        assert_eq!(split_reply_ack_id("12}345678"), None);
    }

    #[test]
    fn validate_ax25_callsign_accepts_valid() {
        assert!(validate_ax25_callsign("N2GH").is_ok());
        assert!(validate_ax25_callsign("KB1ABC").is_ok());  // 6 chars
        assert!(validate_ax25_callsign("A").is_ok());       // 1 char minimum
        assert!(validate_ax25_callsign("W1AW").is_ok());
        assert!(validate_ax25_callsign("AB1CDE").is_ok());  // 6 chars with digits
    }

    #[test]
    fn validate_ax25_callsign_rejects_empty() {
        assert!(validate_ax25_callsign("").is_err());
    }

    #[test]
    fn validate_ax25_callsign_rejects_too_long() {
        assert!(validate_ax25_callsign("ABCDEFG").is_err()); // 7 chars
    }

    #[test]
    fn validate_ax25_callsign_rejects_lowercase() {
        assert!(validate_ax25_callsign("n2gh").is_err());
        assert!(validate_ax25_callsign("Kb1abc").is_err());
    }

    #[test]
    fn validate_ax25_callsign_rejects_special_chars() {
        assert!(validate_ax25_callsign("KB1AB!").is_err());
        assert!(validate_ax25_callsign("N2G H").is_err()); // embedded space
        assert!(validate_ax25_callsign("N2G-H").is_err()); // hyphen belongs to SSID, not base
    }

    #[test]
    fn parse_ax25_station_rejects_invalid_ssids() {
        assert!(parse_ax25_station("N2GH-0", "Source callsign").is_err());
        assert!(parse_ax25_station("N2GH-16", "Source callsign").is_err());
        assert!(parse_ax25_station("N2GH-A", "Source callsign").is_err());
    }

    #[test]
    fn parse_digipeater_path_rejects_more_than_eight_entries() {
        let path = "WIDE1-1,WIDE2-1,WIDE3-1,WIDE4-1,WIDE5-1,WIDE6-1,WIDE7-1,WIDE8-1,WIDE9-1";
        assert!(parse_digipeater_path(path).is_err());
    }

    #[test]
    fn parse_digipeater_path_rejects_obsolete_aliases() {
        assert!(parse_digipeater_path("RELAY").is_err());
        assert!(parse_digipeater_path("TRACE").is_err());
        assert!(parse_digipeater_path("WIDE").is_err());
    }

    #[test]
    fn validate_position_rejects_out_of_range_values() {
        assert!(validate_position(91.0, 0.0).is_err());
        assert!(validate_position(-91.0, 0.0).is_err());
        assert!(validate_position(0.0, 181.0).is_err());
        assert!(validate_position(0.0, -181.0).is_err());
    }

    #[test]
    fn validate_uncompressed_position_comment_rejects_overlong() {
        assert!(validate_uncompressed_position_comment(&"X".repeat(44)).is_err());
        assert!(validate_uncompressed_position_comment(&"X".repeat(43)).is_ok());
    }

    #[test]
    fn resolve_uncompressed_symbol_accepts_overlay_and_symbol_pair() {
        assert_eq!(resolve_uncompressed_symbol("/-"), Ok(('/', '-')));
        assert_eq!(resolve_uncompressed_symbol("B>"), Ok(('B', '>')));
        assert!(resolve_uncompressed_symbol("b>").is_err());
        assert!(resolve_uncompressed_symbol("3?").is_err());
    }

    #[test]
    fn build_queued_message_payloads_appends_reply_ack_at_send_time() {
        let payloads = build_queued_message_payloads(
            "N0CALL-9",
            "KB1ABC-15",
            "WIDE1-1,WIDE2-1",
            "hello",
            "12",
            Some("34"),
        )
        .expect("payloads should build");

        assert!(payloads.aprsis_payload.contains(":KB1ABC-15:hello{12}34"));
        assert!(!payloads.aprsis_payload.contains("WIDE1-1"));
        assert!(!payloads.ax25_hex.is_empty());
        assert!(!payloads.ax25_frame.is_empty());
    }

    #[test]
    fn aprsis_position_payload_omits_rf_path() {
        let info = build_aprs_position_info(42.0, -71.0, '/', '-', "test");
        let info_str = String::from_utf8_lossy(&info).to_string();
        let aprsis_line = format!("{}>{}:{}", "N0CALL-9", APRS_TOCALL, info_str);

        assert!(!aprsis_line.contains("WIDE1-1"));
        assert!(aprsis_line.starts_with("N0CALL-9>APWOLF:!"));
    }

    #[test]
    fn strip_crlf_removes_trailing_and_embedded() {
        assert_eq!(strip_crlf("hello\r\n"), "hello");
        assert_eq!(strip_crlf("line1\r\nline2"), "line1line2");
        assert_eq!(strip_crlf("no newlines"), "no newlines");
        assert_eq!(strip_crlf("\r\nhello\r\n"), "hello");
    }

    #[test]
    fn validate_aprs_message_text_rejects_over_67_chars() {
        let too_long = "X".repeat(68);
        assert_eq!(
            validate_aprs_message_text(&too_long),
            Err("Message exceeds the APRS 67-character limit".to_string())
        );
    }

    #[test]
    fn validate_aprs_message_text_accepts_67_chars() {
        let valid = "X".repeat(67);
        assert_eq!(validate_aprs_message_text(&valid), Ok(()));
    }
}

