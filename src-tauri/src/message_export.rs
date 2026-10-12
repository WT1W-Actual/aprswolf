//! CSV export of the message history.
//!
//! By default the file holds conversation only: sent messages, and received
//! messages, queries, and bulletins. Short-lived packet types join it only
//! when the export asks for them. An `ExportFilter` can narrow the file to
//! sent or received, a date range, some callsigns, or what is new since the
//! last new-only export. Rows run oldest first, sent and received
//! interleaved by time, and hidden messages are included with `hidden` set.
//!
//! The file is UTF-8 with a byte-order mark, so spreadsheets read accented
//! text correctly, and uses CRLF line endings with every field quoted
//! (RFC 4180). A field a spreadsheet would read as a formula gets a leading
//! apostrophe.

use std::io::{self, Write};

use rusqlite::Connection;
use serde_json::Value;

pub const COLUMNS: [&str; 11] = [
    "uuid",
    "timestamp",
    "direction",
    "from",
    "to",
    "body",
    "aprs_type",
    "path",
    "message_number",
    "status",
    "hidden",
];

const FROM: usize = 3;
const TO: usize = 4;

/// Which messages an export holds. The default is a full export of
/// conversation.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportFilter {
    /// "sent", "received", or anything else for both.
    pub direction: String,
    /// Only messages stored since the last export made with this set.
    pub new_only: bool,
    /// Message time bounds in milliseconds: `from_ms` inclusive, `to_ms`
    /// exclusive. The window turns the chosen local dates into these.
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    /// A message matches when its sender or addressee is one of these. A
    /// callsign without an SSID matches every SSID of it.
    pub callsigns: Vec<String>,
    /// Add positions, weather, telemetry, and the other short-lived types.
    pub include_short_lived: bool,
}

impl ExportFilter {
    /// True when anything besides new-only leaves messages out. Such an
    /// export doesn't cover everything new, so it must not move the mark.
    pub fn narrows(&self) -> bool {
        matches!(self.direction.as_str(), "sent" | "received")
            || self.from_ms.is_some()
            || self.to_ms.is_some()
            || self.callsigns.iter().any(|c| !c.trim().is_empty())
    }
}

/// One exported message, in `COLUMNS` order.
struct Row {
    timestamp: i64,
    stored_at: i64,
    fields: [String; 11],
}

fn received_rows(conn: &Connection, include_short_lived: bool) -> rusqlite::Result<Vec<Row>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT uuid, timestamp, from_call, to_call, body, aprs_type, data, hidden, stored_at
         FROM received{}",
        if include_short_lived { "" } else { " WHERE keep = 1" }
    ))?;
    let rows = stmt.query_map([], |row| {
        let timestamp: i64 = row.get(1)?;
        let data: Value = serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or(Value::Null);
        Ok(Row {
            timestamp,
            stored_at: row.get(8)?,
            fields: [
                row.get(0)?,
                iso_utc(timestamp),
                "received".to_string(),
                row.get(2)?,
                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                row.get(4)?,
                row.get(5)?,
                received_path(&data),
                crate::message_store::message_number(&data).unwrap_or("").to_string(),
                String::new(),
                yes_no(row.get(7)?),
            ],
        })
    })?;
    rows.collect()
}

fn sent_rows(conn: &Connection) -> rusqlite::Result<Vec<Row>> {
    let mut stmt = conn.prepare(
        "SELECT uuid, created_at, from_call, to_call, body, path, aprs_id, status, hidden, stored_at FROM sent",
    )?;
    let rows = stmt.query_map([], |row| {
        let timestamp: i64 = row.get(1)?;
        Ok(Row {
            timestamp,
            stored_at: row.get(9)?,
            fields: [
                row.get(0)?,
                iso_utc(timestamp),
                "sent".to_string(),
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                "msg".to_string(),
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                yes_no(row.get(8)?),
            ],
        })
    })?;
    rows.collect()
}

/// Whether `call` is one of `wanted`, where a wanted callsign without an
/// SSID matches every SSID of that base call.
fn callsign_matches(call: &str, wanted: &[String]) -> bool {
    let call = call.trim().to_ascii_uppercase();
    let base = call.split('-').next().unwrap_or("");
    wanted.iter().any(|w| {
        let w = w.trim().to_ascii_uppercase();
        !w.is_empty() && (w == call || (!w.contains('-') && w == base))
    })
}

/// The digipeater path a received packet travelled. A third-party packet
/// carries the inner packet's path, which is the one the sender used.
fn received_path(data: &Value) -> String {
    let hops = |key: &str| -> Vec<String> {
        data.get(key)
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default()
    };
    let inner = hops("protocol_path");
    if inner.is_empty() { hops("path") } else { inner }.join(",")
}

fn yes_no(flag: bool) -> String {
    if flag { "yes" } else { "no" }.to_string()
}

/// Milliseconds since the Unix epoch as an ISO 8601 UTC time, to the second.
fn iso_utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from a day count (Howard Hinnant's days_from_civil inverse).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// A quoted CSV field. Text a spreadsheet would run as a formula (starting
/// with =, +, -, @, tab, or carriage return) gets a leading apostrophe so it
/// shows as typed.
fn field(value: &str) -> String {
    // Import strips one apostrophe from any run of them before a formula
    // character, so a value that already starts that way gets one more.
    let guarded = if value.trim_start_matches('\'').starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.to_string()
    };
    format!("\"{}\"", guarded.replace('"', "\"\""))
}

/// Write the messages `filter` selects to `out`; returns how many the file
/// holds. With `new_only` set, a message counts as new when it entered the
/// store after `since` (every message when there is no earlier mark) and no
/// later than `until`.
pub fn write_csv(
    conn: &Connection,
    out: &mut impl Write,
    filter: &ExportFilter,
    since: Option<i64>,
    until: i64,
) -> io::Result<usize> {
    let to_io = |err: rusqlite::Error| io::Error::other(err.to_string());
    let mut rows = Vec::new();
    if filter.direction != "sent" {
        rows.extend(received_rows(conn, filter.include_short_lived).map_err(to_io)?);
    }
    if filter.direction != "received" {
        rows.extend(sent_rows(conn).map_err(to_io)?);
    }
    let wanted: Vec<String> = filter.callsigns.iter().filter(|c| !c.trim().is_empty()).cloned().collect();
    rows.retain(|row| {
        (!filter.new_only || (since.is_none_or(|mark| row.stored_at > mark) && row.stored_at <= until))
            && filter.from_ms.is_none_or(|from| row.timestamp >= from)
            && filter.to_ms.is_none_or(|to| row.timestamp < to)
            && (wanted.is_empty()
                || callsign_matches(&row.fields[FROM], &wanted)
                || callsign_matches(&row.fields[TO], &wanted))
    });
    rows.sort_by_key(|row| row.timestamp);

    out.write_all("\u{feff}".as_bytes())?;
    let header: Vec<String> = COLUMNS.iter().map(|name| field(name)).collect();
    out.write_all(format!("{}\r\n", header.join(",")).as_bytes())?;
    for row in &rows {
        let line: Vec<String> = row.fields.iter().map(|value| field(value)).collect();
        out.write_all(format!("{}\r\n", line.join(",")).as_bytes())?;
    }
    out.flush()?;
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_utc_formats_known_instants() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(iso_utc(1_791_830_400_999), "2026-10-12T18:40:00Z");
    }

    #[test]
    fn fields_are_quoted_and_formulas_neutralized() {
        assert_eq!(field("plain"), "\"plain\"");
        assert_eq!(field("say \"hi\", ok"), "\"say \"\"hi\"\", ok\"");
        assert_eq!(field("=1+1"), "\"'=1+1\"");
        assert_eq!(field("+1"), "\"'+1\"");
        assert_eq!(field("-73"), "\"'-73\"");
        assert_eq!(field("@home"), "\"'@home\"");
        assert_eq!(field("'-) wink"), "\"''-) wink\"");
        assert_eq!(field("'tis"), "\"'tis\"");
        assert_eq!(field("WIDE1-1,WIDE2-1"), "\"WIDE1-1,WIDE2-1\"");
    }

    #[test]
    fn received_path_prefers_the_inner_third_party_path() {
        let rf = serde_json::json!({ "path": ["WIDE1-1*", "WIDE2-1"] });
        assert_eq!(received_path(&rf), "WIDE1-1*,WIDE2-1");
        let third_party = serde_json::json!({ "path": ["WIDE1-1"], "protocol_path": ["TCPIP", "WZ0C-4*"] });
        assert_eq!(received_path(&third_party), "TCPIP,WZ0C-4*");
        assert_eq!(received_path(&serde_json::json!({})), "");
    }

    #[test]
    fn a_base_callsign_matches_every_ssid() {
        let wanted = vec!["k6test".to_string(), "N6TEST-7".to_string()];
        assert!(callsign_matches("K6TEST-9", &wanted));
        assert!(callsign_matches("K6TEST", &wanted));
        assert!(callsign_matches("N6TEST-7", &wanted));
        assert!(!callsign_matches("N6TEST-8", &wanted));
        assert!(!callsign_matches("K6TESTX", &wanted));
    }
}
