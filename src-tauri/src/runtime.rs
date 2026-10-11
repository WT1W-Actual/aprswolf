use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(target_os = "macos")]
use std::process::Command;

use serde::Serialize;
use tauri::AppHandle;

#[derive(Clone, Serialize)]
pub struct StatusRecord {
    pub state: String,
    pub detail: Option<String>,
}

impl StatusRecord {
    pub fn new(state: &str, detail: Option<String>) -> Self {
        Self {
            state: state.to_string(),
            detail,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct RuntimeSnapshot {
    pub tx_status: StatusRecord,
    pub rx_status: StatusRecord,
    pub aprsis_status: StatusRecord,
    pub tnc_connected: bool,
    pub aprsis_connected: bool,
}

/// Runtime state with a single Mutex for atomic access to all fields.
pub struct RuntimeState {
    inner: Mutex<RuntimeStateInner>,
}

struct RuntimeStateInner {
    tx_status: StatusRecord,
    rx_status: StatusRecord,
    aprsis_status: StatusRecord,
    tnc_connected: bool,
    aprsis_connected: bool,
    last_manual_beacon_ms: Option<u64>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(RuntimeStateInner {
                tx_status: StatusRecord::new("disconnected", None),
                rx_status: StatusRecord::new("disconnected", None),
                aprsis_status: StatusRecord::new("aprsis_disconnected", None),
                tnc_connected: false,
                aprsis_connected: false,
                last_manual_beacon_ms: None,
            }),
        }
    }
}

impl RuntimeState {
    pub fn snapshot(&self) -> RuntimeSnapshot {
        let inner = self.inner.lock().unwrap();
        RuntimeSnapshot {
            tx_status: inner.tx_status.clone(),
            rx_status: inner.rx_status.clone(),
            aprsis_status: inner.aprsis_status.clone(),
            tnc_connected: inner.tnc_connected,
            aprsis_connected: inner.aprsis_connected,
        }
    }

    pub fn record_tnc_status(&self, state: &str, detail: Option<String>) {
        let mut inner = self.inner.lock().unwrap();
        let record = StatusRecord::new(state, detail);

        if state.starts_with("tx_") || state == "disconnected" {
            inner.tx_status = record.clone();
        }
        if state.starts_with("rx_") || state == "disconnected" {
            inner.rx_status = record.clone();
        }
        if state == "tx_rx_loop_start" {
            inner.tnc_connected = true;
        }
        if state == "disconnected" || state == "tx_error" || state == "rx_error" || state == "rx_eof" {
            inner.tnc_connected = false;
        }
    }

    pub fn record_aprsis_status(&self, state: &str, detail: Option<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner.aprsis_status = StatusRecord::new(state, detail);
        if state == "aprsis_connected" {
            inner.aprsis_connected = true;
        } else if state == "aprsis_disconnected" {
            inner.aprsis_connected = false;
        }
    }

    pub fn manual_beacon_remaining_ms(&self, cooldown_ms: u64) -> u64 {
        let inner = self.inner.lock().unwrap();
        let now = now_ms();
        match inner.last_manual_beacon_ms {
            Some(previous) => {
                let next_allowed = previous.saturating_add(cooldown_ms);
                next_allowed.saturating_sub(now)
            }
            None => 0,
        }
    }

    pub(crate) fn set_last_manual_beacon_ms(&self, ms: u64) {
        self.inner.lock().unwrap().last_manual_beacon_ms = Some(ms);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn play_default_system_alert_sound() {
    #[cfg(target_os = "macos")]
    let _ = Command::new("osascript").args(["-e", "beep"]).spawn();
}

pub fn maybe_play_addressed_message_sound(app: &AppHandle, text: &str) {
    if is_aprs_ack_text(text) {
        return;
    }

    let settings = crate::settings::load_settings(app.clone());
    if settings.play_message_sound {
        play_default_system_alert_sound();
    }
}

fn is_aprs_ack_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() <= 3 {
        return false;
    }

    // `split_at` panics if byte 3 falls inside a multibyte character, so
    // check the boundary first.
    let Some((prefix, rest)) = trimmed.split_at_checked(3) else {
        return false;
    };
    prefix.eq_ignore_ascii_case("ack") && rest.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::is_aprs_ack_text;

    #[test]
    fn ack_text_check_does_not_panic_on_multibyte_input() {
        let lossy = String::from_utf8_lossy(&[b'a', 0xFF, b'x']).to_string();
        for input in ["a€", "ab€", lossy.as_str()] {
            assert!(!is_aprs_ack_text(input));
        }
        assert!(is_aprs_ack_text("ack12"));
    }
}
