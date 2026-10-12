use std::fs;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

const SETTINGS_FILE: &str = "settings.json";

/// In-memory settings cache. The settings file is only written by this app,
/// so a read-through cache invalidated on save is always coherent, and the
/// hot RX paths (2-3 lookups per received packet) stop hitting the disk.
static SETTINGS_CACHE: OnceLock<RwLock<Option<AppSettings>>> = OnceLock::new();

fn settings_cache() -> &'static RwLock<Option<AppSettings>> {
    SETTINGS_CACHE.get_or_init(|| RwLock::new(None))
}


#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub source: String,
    pub path: String,
    pub host: String,
    pub port: String,
    /// Optional AGW (RX monitor) port override. When empty, the RX port is
    /// derived as KISS port - 1 (Dire Wolf / Graywolf 8001/8000 convention).
    pub agw_port: String,
    pub aprsis_host: String,
    pub aprsis_port: String,
    pub aprsis_passcode: String,
    pub aprsis_filter: String,
    pub beacon_lat: String,
    pub beacon_lon: String,
    pub beacon_symbol: String,
    pub web_server_port: String,
    pub web_server_auto_start: bool,
    /// Shared token required for web remote-control API calls. Empty means
    /// remote access is only allowed from localhost (loopback exempt).
    pub web_server_token: String,
    pub auto_beacon_on_connect: bool,
    pub auto_beacon_interval_secs: u32,
    /// Days to keep positions, weather, telemetry and the other short-lived
    /// packet types in the message history; 0 keeps everything. Messages,
    /// queries and bulletins are kept regardless.
    pub short_lived_retention_days: u32,
    pub show_ack_messages: bool,
    pub play_message_sound: bool,
    /// Show the unread To Me count on the app icon (dock badge, or a
    /// taskbar overlay on Windows).
    pub show_app_badge: bool,
    pub promiscuous_base_callsign: bool,
    pub theme_mode: String,
    pub msg_font_size: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            source: String::new(),
            path: "WIDE1-1,WIDE2-1".to_string(),
            host: "127.0.0.1".to_string(),
            port: "8001".to_string(),
            agw_port: String::new(),
            aprsis_host: "rotate.aprs2.net".to_string(),
            aprsis_port: "14580".to_string(),
            aprsis_passcode: String::new(),
            aprsis_filter: String::new(),
            beacon_lat: String::new(),
            beacon_lon: String::new(),
            beacon_symbol: "/-".to_string(),
            web_server_port: "18420".to_string(),
            web_server_auto_start: false,
            web_server_token: String::new(),
            auto_beacon_on_connect: false,
            auto_beacon_interval_secs: 0,
            short_lived_retention_days: 7,
            show_ack_messages: false,
            play_message_sound: true,
            show_app_badge: true,
            promiscuous_base_callsign: false,
            theme_mode: "system".to_string(),
            msg_font_size: 15,
        }
    }
}

impl AppSettings {
    /// Fields with a standard value get it back whenever they are blank, so
    /// Preferences shows a working value instead of an empty box. Personal
    /// fields (callsign, passcode, position, token) have no such value and
    /// stay as the user left them.
    fn fill_blank_defaults(&mut self) {
        let defaults = AppSettings::default();
        for (field, default) in [
            (&mut self.path, defaults.path),
            (&mut self.host, defaults.host),
            (&mut self.port, defaults.port),
            (&mut self.aprsis_host, defaults.aprsis_host),
            (&mut self.aprsis_port, defaults.aprsis_port),
            (&mut self.beacon_symbol, defaults.beacon_symbol),
            (&mut self.web_server_port, defaults.web_server_port),
        ] {
            if field.trim().is_empty() {
                *field = default;
            }
        }
    }
}

/// Write `contents` to `path` without ever leaving a half-written file.
///
/// The data goes to a temporary file beside the target, which is then
/// renamed over it. A rename within one directory is atomic, so a crash or
/// power cut mid-write leaves either the old file or the new one, never a
/// truncated mix that fails to parse.
fn write_atomic(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp_path = path.with_file_name(tmp_name);
    fs::write(&tmp_path, contents)?;
    fs::rename(&tmp_path, path)
}

fn settings_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data directory: {err}"))?;
    fs::create_dir_all(&dir)
        .map_err(|err| format!("failed to create app data directory: {err}"))?;
    Ok(dir.join(SETTINGS_FILE))
}

fn unwrap_third_party_info(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    let inner = trimmed.strip_prefix('}')?;
    let colon = inner.find(':')?;
    Some(&inner[colon + 1..])
}

fn extract_aprs_message_text(info_text: &str) -> String {
    if !info_text.starts_with(':') || info_text.len() < 11 {
        return info_text.to_string();
    }

    let inner = &info_text[1..];
    if inner.find(':') != Some(9) {
        return info_text.to_string();
    }

    let body = &inner[10..];
    if let Some(ack_body) = normalize_ack_display_body(body) {
        ack_body
    } else if let Some(brace_idx) = body.rfind('{') {
        body[..brace_idx].to_string()
    } else {
        body.to_string()
    }
}

fn normalize_ack_display_body(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if !trimmed.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("ack")) {
        return None;
    }

    // Some networks occasionally surface reply-ack delimiters as ')' instead of '}'.
    // Canonicalize once so ACK display stays stable and parseable.
    let canonical = if trimmed.contains(')') && !trimmed.contains('}') {
        trimmed.replacen(')', "}", 1)
    } else {
        trimmed.to_string()
    };

    let ack_id = crate::tnc::parse_ack_id(&canonical)?;
    Some(format!("ack{ack_id}"))
}

fn extract_aprs_display_body(raw: &str) -> String {
    let trimmed = raw.trim();
    let protocol_body = unwrap_third_party_info(trimmed).unwrap_or(trimmed);
    extract_aprs_message_text(protocol_body).trim().to_string()
}

fn parse_received_addressee(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if !trimmed.starts_with(':') || trimmed.len() < 11 {
        return None;
    }

    let inner = &trimmed[1..];
    if inner.find(':') != Some(9) {
        return None;
    }

    Some(inner[..9].trim().to_uppercase())
}

fn aprs_frame_type(body: &str) -> &'static str {
    match body.chars().next() {
        Some('!') | Some('=') => "pos",
        Some('@') | Some('/') => "pos+t",
        Some(':') => "msg",
        Some('>') => "status",
        Some(';') => "obj",
        Some(')') => "item",
        Some('`') | Some('\'') => "mic-e",
        Some('T') => "tlm",
        Some('_') => "wx",
        _ => "other",
    }
}

fn is_aprs_telemetry_metadata(body: &str) -> bool {
    let message_text = extract_aprs_message_text(body).trim().to_uppercase();
    message_text.starts_with("PARM.")
        || message_text.starts_with("UNIT.")
        || message_text.starts_with("EQNS.")
        || message_text.starts_with("BITS.")
}

fn is_aprs_weather_report(body: &str) -> bool {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with('_') {
        return true;
    }

    let bytes = trimmed.as_bytes();
    if bytes.len() < 20 {
        return false;
    }

    let starts_with_position = matches!(bytes.first(), Some(b'!') | Some(b'='));
    let starts_with_timestamped_position = matches!(bytes.first(), Some(b'@') | Some(b'/'));

    // Uncompressed layout: data type (1), latitude "4903.50N" (8), symbol
    // table (1), longitude "07201.75W" (9), symbol code (1). The table sits
    // at byte 9 and the symbol code at byte 19; a timestamp ("092345z", 7)
    // pushes both 7 bytes later. A weather station's symbol code is '_'.
    let is_symbol_table = |ch: &u8| ch.is_ascii_alphanumeric() || *ch == b'/' || *ch == b'\\';

    if starts_with_position {
        return bytes.get(9).is_some_and(is_symbol_table) && bytes.get(19) == Some(&b'_');
    }

    if starts_with_timestamped_position {
        return bytes.get(16).is_some_and(is_symbol_table) && bytes.get(26) == Some(&b'_');
    }

    false
}

fn is_aprs_query(body: &str) -> bool {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with('?') {
        return true;
    }
    trimmed.starts_with(':') && extract_aprs_message_text(trimmed).trim().starts_with('?')
}

fn is_aprs_bulletin(body: &str) -> bool {
    let trimmed = body.trim();
    let upper = trimmed.to_uppercase();
    upper.starts_with(":BLN") && upper.chars().nth(10) == Some(':')
}

#[derive(Clone)]
pub struct NormalizedReceivedFields {
    pub from: String,
    pub to: Option<String>,
    pub body: String,
    pub aprs_type: String,
    pub is_bulletin: bool,
    pub mic_e_decoded: bool,
    pub summary_source: String,
    pub summary_destination: Option<String>,
}

/// Input struct for `normalize_received_fields`. Use `..Default::default()`
/// for the four `existing_*` fields when starting from scratch.
#[derive(Default)]
pub struct NormalizeFieldsInput<'a> {
    pub source: Option<&'a str>,
    pub destination: Option<&'a str>,
    pub raw_body: &'a str,
    pub protocol_source: Option<&'a str>,
    pub protocol_destination: Option<&'a str>,
    pub protocol_info_text: Option<&'a str>,
    pub mic_e_text: Option<&'a str>,
    pub existing_to: Option<&'a str>,
    pub existing_aprs_type: Option<&'a str>,
    pub existing_is_bulletin: Option<bool>,
    pub existing_mic_e_decoded: Option<bool>,
}

pub fn normalize_received_fields(input: NormalizeFieldsInput) -> NormalizedReceivedFields {
    let NormalizeFieldsInput {
        source,
        destination,
        raw_body,
        protocol_source,
        protocol_destination,
        protocol_info_text,
        mic_e_text,
        existing_to,
        existing_aprs_type,
        existing_is_bulletin,
        existing_mic_e_decoded,
    } = input;
    let protocol_body = protocol_info_text
        .or_else(|| unwrap_third_party_info(raw_body))
        .unwrap_or(raw_body);

    let from = protocol_source.or(source).unwrap_or("?").to_string();
    let body = mic_e_text
        .filter(|text| !text.trim().is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| extract_aprs_display_body(raw_body));
    let to = existing_to.map(ToString::to_string).or_else(|| parse_received_addressee(protocol_body));
    let aprs_type = existing_aprs_type.map(ToString::to_string).unwrap_or_else(|| {
        if is_aprs_telemetry_metadata(protocol_body) {
            "tlm".to_string()
        } else if is_aprs_weather_report(protocol_body) {
            "wx".to_string()
        } else if is_aprs_query(protocol_body) {
            "qry".to_string()
        } else {
            aprs_frame_type(protocol_body).to_string()
        }
    });
    let is_bulletin = existing_is_bulletin.unwrap_or_else(|| is_aprs_bulletin(protocol_body));
    let mic_e_decoded = existing_mic_e_decoded.unwrap_or_else(|| mic_e_text.is_some_and(|text| !text.trim().is_empty()));

    NormalizedReceivedFields {
        from,
        to,
        body,
        aprs_type,
        is_bulletin,
        mic_e_decoded,
        summary_source: protocol_source.or(source).unwrap_or("?").to_string(),
        summary_destination: protocol_destination.or(destination).map(ToString::to_string),
    }
}

pub(crate) fn normalize_received_message(mut message: Value) -> Value {
    let Some(obj) = message.as_object_mut() else {
        return message;
    };

    let raw_body = obj
        .get("protocol_info_text")
        .and_then(Value::as_str)
        .or_else(|| obj.get("info_text").and_then(Value::as_str))
        .or_else(|| obj.get("body").and_then(Value::as_str))
        .or_else(|| obj.get("text").and_then(Value::as_str))
        .unwrap_or("");
    let normalized = normalize_received_fields(NormalizeFieldsInput {
        source: obj.get("source").and_then(Value::as_str),
        destination: obj.get("destination").and_then(Value::as_str),
        raw_body,
        protocol_source: obj.get("protocol_source").and_then(Value::as_str),
        protocol_destination: obj.get("protocol_destination").and_then(Value::as_str),
        protocol_info_text: obj.get("protocol_info_text").and_then(Value::as_str),
        mic_e_text: obj.get("mic_e_text").and_then(Value::as_str),
        existing_to: obj.get("to").and_then(Value::as_str),
        existing_aprs_type: obj.get("aprsType").and_then(Value::as_str),
        existing_is_bulletin: obj.get("isBulletin").and_then(Value::as_bool),
        existing_mic_e_decoded: obj.get("micEDecoded").and_then(Value::as_bool),
    });

    obj.insert("from".to_string(), Value::String(normalized.from));
    if let Some(to) = normalized.to {
        obj.insert("to".to_string(), Value::String(to));
    }
    obj.insert("body".to_string(), Value::String(normalized.body));
    obj.insert("aprsType".to_string(), Value::String(normalized.aprs_type));
    obj.insert("isBulletin".to_string(), Value::Bool(normalized.is_bulletin));
    obj.insert("micEDecoded".to_string(), Value::Bool(normalized.mic_e_decoded));

    message
}


#[tauri::command]
pub fn load_settings(app: AppHandle) -> AppSettings {
    if let Some(cached) = settings_cache().read().unwrap().as_ref() {
        return cached.clone();
    }

    let settings = load_settings_from_disk(&app);
    *settings_cache().write().unwrap() = Some(settings.clone());
    settings
}

fn load_settings_from_disk(app: &AppHandle) -> AppSettings {
    let path = match settings_path(app) {
        Ok(p) => p,
        Err(_) => return AppSettings::default(),
    };

    if !path.exists() {
        return AppSettings::default();
    }

    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return AppSettings::default(),
    };

    let mut settings = serde_json::from_str::<AppSettings>(&raw).unwrap_or_default();
    settings.fill_blank_defaults();
    settings
}

/// Save settings with merge semantics: the payload may be a partial object;
/// keys it omits keep their current values instead of resetting to defaults.
/// This makes settings saves from different UIs (main window, Preferences,
/// web remote) safe to interleave — a client that doesn't know about a field
/// can't clobber it.
#[tauri::command]
pub fn save_settings(app: AppHandle, settings: serde_json::Value) -> Result<(), String> {
    let mut merged = serde_json::to_value(load_settings(app.clone()))
        .map_err(|err| format!("failed to serialize current settings: {err}"))?;

    let (Some(base), Some(patch)) = (merged.as_object_mut(), settings.as_object()) else {
        return Err("settings payload must be a JSON object".to_string());
    };
    for (key, value) in patch {
        base.insert(key.clone(), value.clone());
    }

    let mut merged_settings: AppSettings = serde_json::from_value(merged)
        .map_err(|err| format!("invalid settings payload: {err}"))?;
    merged_settings.fill_blank_defaults();

    let path = settings_path(&app)?;
    let json = serde_json::to_string_pretty(&merged_settings)
        .map_err(|err| format!("failed to serialize settings: {err}"))?;
    write_atomic(&path, &json).map_err(|err| format!("failed to write settings file: {err}"))?;
    // Only cache after a successful write so the cache never diverges from disk.
    *settings_cache().write().unwrap() = Some(merged_settings.clone());
    app.emit("settings_updated", merged_settings)
        .map_err(|err| format!("failed to broadcast settings update: {err}"))
}

#[cfg(test)]
mod tests {
    use super::{normalize_received_message, write_atomic, AppSettings};

    #[test]
    fn fill_blank_defaults_restores_standard_values_and_keeps_personal_ones() {
        let mut settings: AppSettings = serde_json::from_str(
            r#"{"source":"AD6DM-15","path":"","host":" ","port":"8010","web_server_port":"","beacon_lat":""}"#,
        )
        .unwrap();
        settings.fill_blank_defaults();
        assert_eq!(settings.source, "AD6DM-15");
        assert_eq!(settings.path, "WIDE1-1,WIDE2-1");
        assert_eq!(settings.host, "127.0.0.1");
        assert_eq!(settings.port, "8010");
        assert_eq!(settings.web_server_port, "18420");
        assert_eq!(settings.beacon_lat, "");
        assert_eq!(settings.aprsis_passcode, "");
    }

    #[test]
    fn write_atomic_replaces_the_file_and_leaves_no_temp_behind() {
        let dir = std::env::temp_dir().join(format!("aprswolf-write-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, "{\"old\":true}").unwrap();

        write_atomic(&path, "{\"new\":true}").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"new\":true}");
        assert!(!dir.join("settings.json.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
    use serde_json::{json, Value};

    #[test]
    fn normalize_received_message_uses_protocol_fields_for_wrapped_packets() {
        let value = json!({
            "text": "WZ0C-4>APN20H:}WHO-IS>APJIW4,TCPIP,WZ0C-4*::WT1W-15  :?APRSP",
            "source": "WZ0C-4",
            "destination": "APN20H",
            "path": ["WIDE1-1"],
            "info_text": "}WHO-IS>APJIW4,TCPIP,WZ0C-4*::WT1W-15  :?APRSP",
            "protocol_source": "WHO-IS",
            "protocol_destination": "APJIW4",
            "protocol_path": ["TCPIP", "WZ0C-4*"],
            "protocol_info_text": ":WT1W-15  :?APRSP"
        });

        let normalized = normalize_received_message(value);
        assert_eq!(normalized.get("from").and_then(Value::as_str), Some("WHO-IS"));
        assert_eq!(normalized.get("to").and_then(Value::as_str), Some("WT1W-15"));
        assert_eq!(normalized.get("body").and_then(Value::as_str), Some("?APRSP"));
        assert_eq!(normalized.get("aprsType").and_then(Value::as_str), Some("qry"));
    }

    #[test]
    fn normalize_received_message_strips_reply_ack_suffix_from_ack_display() {
        let value = json!({
            "body": ":WT1W-15  :ack12}34"
        });

        let normalized = normalize_received_message(value);
        assert_eq!(normalized.get("body").and_then(Value::as_str), Some("ack12"));
    }

    #[test]
    fn normalize_received_message_does_not_panic_on_multibyte_body() {
        for body in [":WT1W-15  :a€", ":WT1W-15  :ab€", ":WT1W-15  :a\u{FFFD}x"] {
            let normalized = normalize_received_message(json!({ "body": body }));
            assert!(normalized.get("body").and_then(Value::as_str).is_some());
        }
    }

    #[test]
    fn uncompressed_weather_reports_are_classified_as_wx() {
        // Weather stations in the uncompressed position layout, without and
        // with a timestamp.
        for body in [
            "!4903.50N/07201.75W_220/004g005t077r000p000P000h50b09900",
            "@092345z4903.50N/07201.75W_220/004g005t077r000p000P000h50b09900",
        ] {
            let normalized = normalize_received_message(json!({ "body": body }));
            assert_eq!(normalized.get("aprsType").and_then(Value::as_str), Some("wx"), "{body}");
        }
        // A plain house symbol at the same offsets is a position, not weather.
        let normalized = normalize_received_message(json!({ "body": "!4903.50N/07201.75W-Test" }));
        assert_eq!(normalized.get("aprsType").and_then(Value::as_str), Some("pos"));
    }

    #[test]
    fn normalize_received_message_tolerates_parenthesis_reply_ack_delimiter() {
        let value = json!({
            "body": ":WT1W-15  :ack14)61"
        });

        let normalized = normalize_received_message(value);
        assert_eq!(normalized.get("body").and_then(Value::as_str), Some("ack14"));
    }
}
