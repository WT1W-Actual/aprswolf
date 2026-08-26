/// APRS-IS (APRS Internet Service) TCP connection handler.
///
/// APRS-IS uses plain-text TCP.  Each packet is a TNC2-format line
/// terminated by `\r\n`.  Comment/server lines start with `#`.
///
/// Login sequence:
///   user {CALLSIGN} pass {PASSCODE} vers APRSWolf 1.0
///
/// The passcode is the standard 5-digit APRS-IS passcode derived from
/// the callsign.  Use "-1" for a receive-only (unverified) connection.
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri::async_runtime::JoinHandle;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};

use crate::ax25::{build_aprs_ack_info, decode_mic_e_from_parts, parse_aprs_message};

#[derive(Clone, Serialize)]
struct AprsIsStatus {
    state: String,
    detail: Option<String>,
}

/// Serializable received message — mirrors `tnc::ReceivedMessage` so the
/// frontend receives the same JSON shape from both connection types.
#[derive(Clone, Serialize)]
struct AprsIsMessage {
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

pub struct AprsIsHandle {
    pub(crate) tx: mpsc::UnboundedSender<String>,
    task: JoinHandle<()>,
}

/// Maximum accepted APRS-IS line length. TNC2 lines are a few hundred bytes
/// at most; the cap stops a broken or hostile server from growing a line
/// buffer without bound. The remainder of an over-long line is consumed as
/// a separate (unparseable) line on the next read, so framing recovers.
const MAX_APRSIS_LINE_BYTES: u64 = 8192;

fn normalize_aprsis_filter(filter: &str) -> String {
    let mut trimmed = filter.trim();
    if let Some(rest) = trimmed.strip_prefix("#filter") {
        trimmed = rest.trim_start();
    } else if let Some(rest) = trimmed.strip_prefix("filter") {
        trimmed = rest.trim_start();
    }

    trimmed
        .chars()
        .filter(|&ch| ch != '\r' && ch != '\n')
        .collect::<String>()
        .trim()
        .to_string()
}

#[derive(Clone, Default)]
pub struct AprsIsState {
    pub handle: Arc<Mutex<Option<AprsIsHandle>>>,
    pub epoch: Arc<AtomicU64>,
}

fn emit_status(app: &AppHandle, state: &str, detail: Option<String>) {
    app.state::<crate::runtime::RuntimeState>().record_aprsis_status(state, detail.clone());
    let _ = app.emit(
        "aprsis_status",
        AprsIsStatus {
            state: state.to_string(),
            detail,
        },
    );
}

/// Parse a TNC2-format line into a message event.
///
/// TNC2 format: `SOURCE>DEST,PATH:INFO`
fn parse_tnc2(line: &str) -> Option<AprsIsMessage> {
    if line.starts_with('#') || line.is_empty() {
        return None;
    }

    let (source, destination, path, info) = crate::tnc::parse_tnc2_protocol_view(line)?;

    // Decode MIC-E once, reuse for both normalization and output
    let dst_base = destination.split('-').next().unwrap_or(&destination);
    let dst_base_upper = dst_base.to_uppercase();
    let mic_e_decoded = decode_mic_e_from_parts(&dst_base_upper, info.as_bytes());

    let normalized = crate::settings::normalize_received_fields(
        crate::settings::NormalizeFieldsInput {
            source: Some(&source),
            destination: Some(&destination),
            raw_body: &info,
            protocol_source: Some(&source),
            protocol_destination: Some(&destination),
            protocol_info_text: Some(&info),
            mic_e_text: mic_e_decoded.as_deref(),
            ..Default::default()
        },
    );

    Some(AprsIsMessage {
        text: line.to_string(),
        payload_hex: String::new(),
        bytes: line.len(),
        source: Some(source.clone()),
        destination: Some(destination.clone()),
        path: Some(path.clone()),
        info_text: Some(info.clone()),
        protocol_source: Some(source),
        protocol_destination: Some(destination.clone()),
        protocol_path: Some(path),
        protocol_info_text: Some(info),
        from: normalized.from,
        to: normalized.to,
        body: normalized.body,
        aprs_type: normalized.aprs_type,
        is_bulletin: normalized.is_bulletin,
        mic_e_decoded: normalized.mic_e_decoded,
        summary_source: normalized.summary_source,
        summary_destination: normalized.summary_destination,
        mic_e_text: mic_e_decoded,
    })
}

/// If `info_text` is a third-party payload (starts with `}`), parse the
/// inner TNC2 packet and return it so the caller can use the inner source
/// and info fields for APRS protocol processing.
///
/// The outer packet's `info_text` is preserved unchanged for UI display;
/// this function only produces the unwrapped view used for protocol logic.
fn unwrap_third_party(info_text: &str) -> Option<AprsIsMessage> {
    let inner = info_text.strip_prefix('}')?;
    parse_tnc2(inner)
}

/// If APRS-IS is connected, queue a TNC2-format packet for transmission.
pub async fn try_send(state: &AprsIsState, line: String) -> Result<(), String> {
    let lock = state.handle.lock().await;
    let Some(handle) = lock.as_ref() else {
        return Err("APRS-IS is not connected".to_string());
    };

    handle
        .tx
        .send(line)
        .map_err(|_| "APRS-IS TX channel closed".to_string())
}

#[tauri::command]
pub async fn connect_aprsis(
    app: AppHandle,
    host: String,
    port: u16,
    callsign: String,
    passcode: String,
    filter: String,
) -> Result<(), String> {
    let state = app.state::<AprsIsState>().inner().clone();
    let tnc_state = app.state::<crate::tnc::AppState>().inner().clone();

    // Lock ordering for cross-transport exclusivity:
    // always acquire TNC client lock first, then APRS-IS handle lock.
    let tnc_lock = tnc_state.client.lock().await;
    if tnc_lock.is_some() {
        return Err("TNC is active. Disconnect TNC before connecting APRS-IS.".to_string());
    }

    let mut handle_lock = state.handle.lock().await;
    if handle_lock.is_some() {
        return Err("APRS-IS connection already active".to_string());
    }

    let epoch = state.epoch.fetch_add(1, Ordering::SeqCst) + 1;
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    let app_clone = app.clone();
    let handle_slot = state.handle.clone();
    let epoch_ref = state.epoch.clone();
    let callsign_norm = callsign.trim().to_uppercase();
    let passcode_trim = passcode.trim().to_string();
    let filter_trim = normalize_aprsis_filter(&filter);
    let host_clone = host.clone();
    let tnc_state_for_task = tnc_state.clone();

    let task = tauri::async_runtime::spawn(async move {
        emit_status(
            &app_clone,
            "aprsis_connecting",
            Some(format!("{host_clone}:{port}")),
        );

        let stream = match TcpStream::connect((host_clone.as_str(), port)).await {
            Ok(s) => s,
            Err(err) => {
                emit_status(&app_clone, "aprsis_error", Some(err.to_string()));
                emit_status(&app_clone, "aprsis_disconnected", None);
                if epoch_ref.load(Ordering::SeqCst) == epoch {
                    *handle_slot.lock().await = None;
                }
                return;
            }
        };

        let (reader_half, writer_half) = stream.into_split();
        let mut line_reader = BufReader::new(reader_half);
        let writer = Arc::new(Mutex::new(writer_half));

        // Send login
        let login = format!(
            "user {} pass {} vers APRSWolf 1.0\r\n",
            callsign_norm, passcode_trim
        );
        {
            let mut w = writer.lock().await;
            if let Err(err) = w.write_all(login.as_bytes()).await {
                emit_status(
                    &app_clone,
                    "aprsis_error",
                    Some(format!("login write failed: {err}")),
                );
                emit_status(&app_clone, "aprsis_disconnected", None);
                if epoch_ref.load(Ordering::SeqCst) == epoch {
                    *handle_slot.lock().await = None;
                }
                return;
            }
            let _ = w.flush().await;
        }

        // Send server-side receive filter if configured
        if !filter_trim.is_empty() {
            let filter_cmd = format!("#filter {}\r\n", filter_trim);
            let mut w = writer.lock().await;
            if let Err(err) = w.write_all(filter_cmd.as_bytes()).await {
                emit_status(
                    &app_clone,
                    "aprsis_debug",
                    Some(format!("filter command failed: {err}")),
                );
            } else {
                let _ = w.flush().await;
            }
        }

        emit_status(
            &app_clone,
            "aprsis_connected",
            Some(format!("{callsign_norm}@{host_clone}:{port}")),
        );

        crate::tnc::auto_beacon_from_settings(&app_clone, "aprsis connect").await;
        crate::tnc::start_periodic_beacon(&app_clone, &tnc_state_for_task).await;

        loop {
            tokio::select! {
                biased;

                outbound = rx.recv() => {
                    match outbound {
                        Some(line) => {
                            let mut w = writer.lock().await;
                            let packet = format!("{}\r\n",
                                line.trim_end_matches('\n').trim_end_matches('\r'));
                            if let Err(err) = w.write_all(packet.as_bytes()).await {
                                emit_status(
                                    &app_clone,
                                    "aprsis_error",
                                    Some(format!("write failed: {err}")),
                                );
                                break;
                            }
                            let _ = w.flush().await;
                        }
                        None => break,
                    }
                }

                read_result = async {
                    let mut line = String::new();
                    (&mut line_reader)
                        .take(MAX_APRSIS_LINE_BYTES)
                        .read_line(&mut line)
                        .await
                        .map(|n| (n, line))
                } => {
                    match read_result {
                        Ok((0, _)) => {
                            emit_status(
                                &app_clone,
                                "aprsis_disconnected",
                                Some("server closed connection".to_string()),
                            );
                            break;
                        }
                        Ok((_, line)) => {
                            let trimmed = line.trim_end();
                            if trimmed.starts_with('#') {
                                // Server comment / keepalive / logresp
                                let lower = trimmed.to_ascii_lowercase();
                                if lower.starts_with("# logresp") {
                                    // javAPRSSrvr login response: surface
                                    // verified/unverified so a bad passcode
                                    // doesn't masquerade as a working feed.
                                    // ("unverified" contains "verified" —
                                    // check it first.)
                                    if lower.contains("unverified") {
                                        emit_status(
                                            &app_clone,
                                            "aprsis_unverified",
                                            Some(trimmed.to_string()),
                                        );
                                    } else if lower.contains("verified") {
                                        emit_status(
                                            &app_clone,
                                            "aprsis_verified",
                                            Some(trimmed.to_string()),
                                        );
                                    }
                                }
                                emit_status(
                                    &app_clone,
                                    "aprsis_debug",
                                    Some(trimmed.to_string()),
                                );
                            } else if !trimmed.is_empty() {
                                    if let Some(mut msg) = parse_tnc2(trimmed) {
                                    // If this is a third-party packet (info_text starts with '}'),
                                    // unwrap the inner packet for APRS protocol processing.
                                    // The outer msg is kept intact for UI display.
                                    let (aprs_source, aprs_info) = if let Some(ref info_text) = msg.info_text {
                                        if let Some(inner) = unwrap_third_party(info_text) {
                                            msg.protocol_source = inner.source.clone();
                                            msg.protocol_destination = inner.destination.clone();
                                            msg.protocol_path = inner.path.clone();
                                            msg.protocol_info_text = inner.info_text.clone();
                                            let normalized = crate::settings::normalize_received_fields(
                                                crate::settings::NormalizeFieldsInput {
                                                    source: msg.source.as_deref(),
                                                    destination: msg.destination.as_deref(),
                                                    raw_body: info_text,
                                                    protocol_source: inner.source.as_deref(),
                                                    protocol_destination: inner.destination.as_deref(),
                                                    protocol_info_text: inner.info_text.as_deref(),
                                                    mic_e_text: msg.mic_e_text.as_deref(),
                                                    ..Default::default()
                                                },
                                            );
                                            msg.from = normalized.from;
                                            msg.to = normalized.to;
                                            msg.body = normalized.body;
                                            msg.aprs_type = normalized.aprs_type;
                                            msg.is_bulletin = normalized.is_bulletin;
                                            msg.mic_e_decoded = normalized.mic_e_decoded;
                                            msg.summary_source = normalized.summary_source;
                                            msg.summary_destination = normalized.summary_destination;
                                            (inner.source, inner.info_text)
                                        } else {
                                            (msg.source.clone(), msg.info_text.clone())
                                        }
                                    } else {
                                        (msg.source.clone(), msg.info_text.clone())
                                    };

                                    // If this is an APRS message addressed to us, send an ACK back.
                                    if let Some(ref info_text) = aprs_info {
                                        if let Some(aprs_msg) = parse_aprs_message(info_text) {
                                            if let Some(ref msgid) = aprs_msg.msgid {
                                                if let Some((_, Some(reply_ack))) = crate::tnc::split_reply_ack_id(msgid) {
                                                    let tnc_state = app_clone.state::<crate::tnc::AppState>().inner().clone();
                                                    let _ = crate::tnc::mark_acked(&app_clone, &tnc_state, reply_ack, aprs_source.as_deref().unwrap_or("")).await;
                                                }
                                            }
                                            let allow_base_callsign = crate::settings::load_settings(app_clone.clone()).promiscuous_base_callsign;
                                            if crate::tnc::callsign_matches_station(&aprs_msg.addressee, &callsign_norm, allow_base_callsign) {
                                                crate::runtime::maybe_play_addressed_message_sound(&app_clone, &aprs_msg.text);
                                                if let Some(ref msgid) = aprs_msg.msgid {
                                                    if let Some(ref source) = aprs_source {
                                                        let tnc_state = app_clone.state::<crate::tnc::AppState>().inner().clone();
                                                        crate::tnc::remember_latest_reply_ack(&tnc_state, source, msgid).await;
                                                        let ack_info = build_aprs_ack_info(source, msgid);
                                                        let ack_str = String::from_utf8_lossy(&ack_info).to_string();
                                                        let ack_line = format!("{callsign_norm}>{}:{}\r\n", crate::tnc::APRS_TOCALL, ack_str);
                                                        let mut w = writer.lock().await;
                                                        let _ = w.write_all(ack_line.as_bytes()).await;
                                                        let _ = w.flush().await;
                                                    }
                                                }
                                            }
                                            // Check if this is an incoming ACK for one of our sent messages.
                                            if aprs_msg.msgid.is_none() && aprs_msg.text.to_lowercase().starts_with("ack") {
                                                if let Some(ack_id_str) = crate::tnc::parse_ack_id(&aprs_msg.text) {
                                                    let tnc_state = app_clone.state::<crate::tnc::AppState>().inner().clone();
                                                    let _ = crate::tnc::mark_acked(&app_clone, &tnc_state, &ack_id_str, aprs_source.as_deref().unwrap_or("")).await;
                                                }
                                            }
                                        }
                                    }
                                    if let Ok(value) = serde_json::to_value(&msg) {
                                        let _ = crate::settings::append_received_message(app_clone.clone(), value);
                                    }
                                    let _ = app_clone.emit("tnc_message", msg);
                                }
                            }
                        }
                        Err(err) => {
                            emit_status(&app_clone, "aprsis_error", Some(err.to_string()));
                            break;
                        }
                    }
                }
            }
        }

        emit_status(&app_clone, "aprsis_disconnected", None);
        if epoch_ref.load(Ordering::SeqCst) == epoch {
            *handle_slot.lock().await = None;
        }
    });

    *handle_lock = Some(AprsIsHandle { tx, task });
    drop(handle_lock);
    drop(tnc_lock);

    let retry_epoch = tnc_state.connection_epoch.fetch_add(1, Ordering::SeqCst) + 1;
    crate::tnc::restart_retry_task(&app, &tnc_state, retry_epoch).await;

    Ok(())
}

#[tauri::command]
pub async fn disconnect_aprsis(
    app: AppHandle,
) -> Result<(), String> {
    let state = app.state::<AprsIsState>().inner().clone();
    let tnc_state = app.state::<crate::tnc::AppState>().inner().clone();
    state.epoch.fetch_add(1, Ordering::SeqCst);
    tnc_state.connection_epoch.fetch_add(1, Ordering::SeqCst);

    let mut lock = state.handle.lock().await;
    let Some(handle) = lock.take() else {
        return Err("No active APRS-IS connection".to_string());
    };

    drop(handle.tx);
    handle.task.abort();

    let mut retry_lock = tnc_state.retry_task.lock().await;
    if let Some(retry_task) = retry_lock.take() {
        retry_task.abort();
    }

    crate::tnc::stop_periodic_beacon(&tnc_state).await;

    emit_status(
        &app,
        "aprsis_disconnected",
        Some("manual disconnect".to_string()),
    );
    Ok(())
}

pub async fn connect_aprsis_for_app(
    app: AppHandle,
    host: String,
    port: u16,
    callsign: String,
    passcode: String,
    filter: String,
) -> Result<(), String> {
    connect_aprsis(app, host, port, callsign, passcode, filter).await
}

pub async fn disconnect_aprsis_for_app(app: AppHandle) -> Result<(), String> {
    disconnect_aprsis(app).await
}

#[cfg(test)]
mod tests {
    use super::{normalize_aprsis_filter, parse_tnc2, unwrap_third_party};

    #[test]
    fn normalize_filter_strips_command_prefix_and_newlines() {
        assert_eq!(normalize_aprsis_filter("m/50"), "m/50");
        assert_eq!(normalize_aprsis_filter("filter m/50"), "m/50");
        assert_eq!(normalize_aprsis_filter("#filter r/42/-71/25\r\n"), "r/42/-71/25");
    }

    #[test]
    fn unwrap_third_party_extracts_inner_source_and_info() {
        // Example from WB2OSZ doc: IGate-wrapped WHO-IS message to WB2OSZ-7
        let info = "}WHO-IS>APJIW4,TCPIP,WB2OSZ-5*::WB2OSZ-7 :ack0";
        let inner = unwrap_third_party(info).expect("should unwrap third-party payload");
        assert_eq!(inner.source.as_deref(), Some("WHO-IS"));
        assert_eq!(inner.info_text.as_deref(), Some(":WB2OSZ-7 :ack0"));
    }

    #[test]
    fn unwrap_third_party_returns_none_for_non_third_party() {
        // A normal message info field (starts with ':') should not be unwrapped.
        let info = ":WB2OSZ-7 :Hello there{42";
        assert!(unwrap_third_party(info).is_none());
    }

    #[test]
    fn unwrap_third_party_handles_full_message_with_id() {
        // Full third-party with message body and message id
        let info = "}WHO-IS>APJIW4,TCPIP,WB2OSZ-5*::WB2OSZ-7 :C/ARRL HQ OPERATORS CLUB{1012";
        let inner = unwrap_third_party(info).expect("should unwrap");
        assert_eq!(inner.source.as_deref(), Some("WHO-IS"));
        assert_eq!(
            inner.info_text.as_deref(),
            Some(":WB2OSZ-7 :C/ARRL HQ OPERATORS CLUB{1012")
        );
    }

    #[test]
    fn parse_tnc2_extracts_outer_source_and_path() {
        // Confirms the outer wrapper is parsed correctly (IGate as source, not inner source)
        let line = "WB2OSZ-5>APDW17,WIDE1-1,WIDE2-1:}WHO-IS>APJIW4,TCPIP,WB2OSZ-5*::WB2OSZ-7 :ack0";
        let msg = parse_tnc2(line).expect("should parse TNC2 line");
        assert_eq!(msg.source.as_deref(), Some("WB2OSZ-5"));
        assert_eq!(
            msg.info_text.as_deref(),
            Some("}WHO-IS>APJIW4,TCPIP,WB2OSZ-5*::WB2OSZ-7 :ack0")
        );
    }

    #[test]
    fn unwrap_third_party_preserves_original_source_case() {
        let info = "}who-is>APJIW4,TCPIP,WB2OSZ-5*::WB2OSZ-7 :ack0";
        let inner = unwrap_third_party(info).expect("should unwrap third-party payload");
        assert_eq!(inner.source.as_deref(), Some("who-is"));
        assert_eq!(inner.destination.as_deref(), Some("APJIW4"));
    }
}
