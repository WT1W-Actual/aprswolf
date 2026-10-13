//! CSV import of a message export, including one edited by hand.
//!
//! The file is read whole and every row is validated before anything is
//! stored: a file with any bad row is refused with the row numbers and what
//! is wrong, so an import never lands half done. Columns are found by name,
//! in any order; `direction`, `from`, `body`, and `timestamp` are required,
//! the rest optional, and unknown columns are ignored. The leading
//! apostrophe that export adds in front of formula-like text is removed.

use std::collections::HashMap;

/// Largest file accepted. An export of every message a busy station hears
/// for years stays well under this.
pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_BODY_CHARS: usize = 1000;
const MAX_PATH_CHARS: usize = 120;
/// Errors listed before the rest are summarized as a count.
const MAX_LISTED_ERRORS: usize = 10;
const REQUIRED: [&str; 4] = ["direction", "from", "body", "timestamp"];
const APRS_TYPES: [&str; 11] = ["msg", "qry", "pos", "pos+t", "status", "obj", "item", "mic-e", "tlm", "wx", "other"];

#[derive(Clone, Debug, PartialEq)]
pub enum Direction {
    Sent,
    Received,
}

/// One validated row, ready to store.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportRow {
    pub uuid: Option<String>,
    pub direction: Direction,
    pub timestamp: i64,
    pub from: String,
    pub to: String,
    pub body: String,
    pub aprs_type: String,
    pub path: String,
    pub message_number: String,
    pub status: String,
    pub hidden: bool,
}

/// Split CSV text into records of fields (RFC 4180: quoted fields may hold
/// commas, doubled quotes, and line breaks). Fails on an unclosed quote.
fn parse_records(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut in_quotes = false;
    let mut field_started = false;
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => in_quotes = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if !field_started => {
                in_quotes = true;
                field_started = true;
            }
            ',' => {
                record.push(std::mem::take(&mut field));
                field_started = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                record.push(std::mem::take(&mut field));
                field_started = false;
                records.push(std::mem::take(&mut record));
            }
            _ => {
                field.push(c);
                field_started = true;
            }
        }
    }
    if in_quotes {
        return Err("The file ends inside a quoted field, so it is cut off or not a CSV export.".to_string());
    }
    if field_started || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    // Blank lines carry no message.
    records.retain(|r| !(r.len() == 1 && r[0].trim().is_empty()));
    Ok(records)
}

/// Undo export's guard: an apostrophe in front of text a spreadsheet would
/// run as a formula.
fn unguard(value: &str) -> &str {
    match value.strip_prefix('\'') {
        Some(rest) if rest.trim_start_matches('\'').starts_with(['=', '+', '-', '@', '\t', '\r']) => rest,
        _ => value,
    }
}

/// An APRS station, addressee, or path hop as the app stores it: up to nine
/// visible ASCII characters. Addressees such as NWS-TSTM don't follow
/// callsign rules, and a packet with no source is stored as `?`, so only the
/// characters that separate fields in a packet header are refused.
fn valid_callsign(call: &str) -> bool {
    !call.is_empty()
        && call.len() <= 9
        && call.chars().all(|c| c.is_ascii_graphic() && !matches!(c, ':' | ',' | '>' | '*'))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Parse an ISO 8601 time as export writes it (`2026-10-12T18:40:00Z`), or
/// as a spreadsheet may rewrite it: a space for the T, fractional seconds,
/// no seconds, a `+hh:mm` offset, or a bare date. A time with no zone is
/// read as UTC, which is what export writes. Returns Unix milliseconds.
pub fn parse_timestamp(value: &str) -> Option<i64> {
    let value = value.trim();
    let (date, rest) = value.split_at_checked(10)?;
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if date.as_bytes()[4] != b'-' || date.as_bytes()[7] != b'-' {
        return None;
    }
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    let mut ms = days_from_civil(year, month, day) * 86_400_000;
    if rest.is_empty() {
        return Some(ms);
    }
    let rest = rest.strip_prefix(['T', ' '])?;
    let (clock, offset_ms) = if let Some(clock) = rest.strip_suffix(['Z', 'z']) {
        (clock, 0)
    } else if let Some(i) = rest.rfind(['+', '-']).filter(|&i| i >= 5) {
        let (clock, zone) = rest.split_at(i);
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        let (h, m) = zone[1..].split_once(':').unwrap_or((zone.get(1..3)?, zone.get(3..).unwrap_or("00")));
        let (h, m): (i64, i64) = (h.parse().ok()?, if m.is_empty() { 0 } else { m.parse().ok()? });
        if h > 14 || m > 59 {
            return None;
        }
        (clock, sign * (h * 3600 + m * 60) * 1000)
    } else {
        (rest, 0)
    };
    // Digits only: `parse` alone would take a sign, so `T-1:30` became 23:30
    // the day before.
    let digits = |s: &str| -> Option<i64> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    let mut fields = clock.split(':');
    let hour = digits(fields.next()?)?;
    let minute = digits(fields.next()?)?;
    let (second, millis) = match fields.next() {
        Some(sec) => {
            let (whole, frac) = sec.split_once('.').unwrap_or((sec, ""));
            let millis = if frac.is_empty() { 0 } else { digits(format!("{frac:0<3}").get(..3)?)? };
            (digits(whole)?, millis)
        }
        None => (0, 0),
    };
    if fields.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    ms += ((hour * 60 + minute) * 60 + second) * 1000 + millis;
    Some(ms - offset_ms)
}

/// Clock difference allowed between the file's times and this computer.
const FUTURE_SLACK_MS: i64 = 5 * 60_000;

/// Parse and validate a whole export. `now` bounds timestamps: nothing
/// before 1990 or more than a few minutes in the future is accepted, since
/// lists sort by message time and a future row would sit above live traffic.
pub fn parse_csv(text: &str, now: i64) -> Result<Vec<ImportRow>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let records = parse_records(text)?;
    let Some((header, rows)) = records.split_first() else {
        return Err("The file is empty.".to_string());
    };
    let mut columns: HashMap<String, usize> = HashMap::new();
    for (i, name) in header.iter().enumerate() {
        let name = name.trim().to_ascii_lowercase();
        if !name.is_empty() && columns.insert(name.clone(), i).is_some() {
            return Err(format!("The header names the column \"{name}\" twice."));
        }
    }
    let missing: Vec<&str> = REQUIRED.iter().copied().filter(|c| !columns.contains_key(*c)).collect();
    if !missing.is_empty() {
        return Err(format!(
            "This doesn't look like an APRSWolf export. Its first line must name the columns, and it has no {} column{}.",
            missing.join(", "),
            if missing.len() == 1 { "" } else { "s" },
        ));
    }

    let earliest = parse_timestamp("1990-01-01").expect("valid date");
    let latest = now + FUTURE_SLACK_MS;
    let mut out = Vec::with_capacity(rows.len());
    let mut errors = Vec::new();
    for (index, record) in rows.iter().enumerate() {
        // Line 1 is the header; a quoted line break can make this a record
        // number rather than a line number, which matches what a spreadsheet
        // shows as its row.
        let row_number = index + 2;
        if record.len() > header.len() {
            errors.push(format!("Row {row_number}: {} fields but the header has {}.", record.len(), header.len()));
            continue;
        }
        let get = |name: &str| -> String {
            columns
                .get(name)
                .and_then(|&i| record.get(i))
                .map(|v| unguard(v).to_string())
                .unwrap_or_default()
        };
        match validate_row(&get, earliest, latest) {
            Ok(row) => out.push(row),
            Err(problem) => errors.push(format!("Row {row_number}: {problem}")),
        }
    }
    if !errors.is_empty() {
        let total = errors.len();
        let mut message = format!(
            "Nothing was imported. {total} row{} need{} fixing:\n",
            if total == 1 { "" } else { "s" },
            if total == 1 { "s" } else { "" }
        );
        for line in errors.iter().take(MAX_LISTED_ERRORS) {
            message.push_str(line);
            message.push('\n');
        }
        if total > MAX_LISTED_ERRORS {
            message.push_str(&format!("...and {} more.", total - MAX_LISTED_ERRORS));
        }
        return Err(message.trim_end().to_string());
    }
    Ok(out)
}

fn validate_row(get: &dyn Fn(&str) -> String, earliest: i64, latest: i64) -> Result<ImportRow, String> {
    let direction = match get("direction").trim().to_ascii_lowercase().as_str() {
        "sent" => Direction::Sent,
        "received" => Direction::Received,
        other => return Err(format!("direction is \"{other}\"; use sent or received.")),
    };

    let raw_from = get("from");
    let from = raw_from.trim().to_ascii_uppercase();
    if !valid_callsign(&from) {
        return Err(format!("from \"{}\" is not a callsign.", raw_from.trim()));
    }
    let raw_to = get("to");
    let to = raw_to.trim().to_ascii_uppercase();
    if to.is_empty() && direction == Direction::Sent {
        return Err("a sent message needs a to callsign.".to_string());
    }
    if !to.is_empty() && !valid_callsign(&to) {
        return Err(format!("to \"{}\" is not a callsign.", raw_to.trim()));
    }

    let body = get("body");
    if body.trim().is_empty() {
        return Err("body is empty.".to_string());
    }
    if body.chars().count() > MAX_BODY_CHARS {
        return Err(format!("body is longer than {MAX_BODY_CHARS} characters."));
    }
    // Export can write any stored text, line breaks included, so only a
    // NUL (never valid in APRS text) is refused.
    if body.contains('\0') {
        return Err("body holds a NUL character.".to_string());
    }

    let raw_time = get("timestamp");
    let timestamp = parse_timestamp(&raw_time)
        .ok_or_else(|| format!("timestamp \"{raw_time}\" is not a date and time like 2026-10-12T18:40:00Z."))?;
    if timestamp < earliest || timestamp > latest {
        return Err(format!(
            "timestamp \"{raw_time}\" is before 1990 or in the future. A time without a zone is read as UTC."
        ));
    }

    let uuid = match get("uuid").trim() {
        "" => None,
        raw => Some(
            uuid::Uuid::parse_str(raw)
                .map_err(|_| format!("uuid \"{raw}\" is not a UUID; clear the cell to have one made."))?
                .hyphenated()
                .to_string(),
        ),
    };

    let aprs_type = match get("aprs_type").trim().to_ascii_lowercase() {
        t if t.is_empty() => "msg".to_string(),
        t if APRS_TYPES.contains(&t.as_str()) => t,
        t => return Err(format!("aprs_type \"{t}\" is not one of {}.", APRS_TYPES.join(", "))),
    };
    if direction == Direction::Sent && aprs_type != "msg" {
        return Err(format!("a sent message has aprs_type msg, not \"{aprs_type}\"."));
    }

    let raw_path = get("path");
    let path = raw_path.trim().to_ascii_uppercase().replace(' ', "");
    if path.len() > MAX_PATH_CHARS
        || !path.split(',').filter(|hop| !hop.is_empty()).all(|hop| valid_callsign(hop.trim_end_matches('*')))
    {
        return Err(format!("path \"{}\" is not a list of digipeaters like WIDE1-1,WIDE2-1.", raw_path.trim()));
    }

    let message_number = get("message_number").trim().to_string();
    if message_number.len() > 5 || !message_number.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(format!("message_number \"{message_number}\" must be up to 5 letters or digits."));
    }

    let status = get("status").trim().to_ascii_lowercase();
    let status = match direction {
        Direction::Received => String::new(),
        // Nothing will retry an imported message, so one still in flight
        // is stored the way the app stores it after a restart.
        Direction::Sent => match status.as_str() {
            "acked" | "failed" | "expired" => status,
            "queued" | "sent" => "expired".to_string(),
            "" => return Err("a sent message needs a status: acked, failed, or expired.".to_string()),
            other => return Err(format!("status \"{other}\" is not acked, failed, or expired.")),
        },
    };

    let hidden = match get("hidden").trim().to_ascii_lowercase().as_str() {
        "" | "no" | "false" | "0" => false,
        "yes" | "true" | "1" => true,
        other => return Err(format!("hidden is \"{other}\"; use yes or no.")),
    };

    Ok(ImportRow { uuid, direction, timestamp, from, to, body, aprs_type, path, message_number, status, hidden })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_791_830_400_000; // 2026-10-12T18:40:00Z
    const HEADER: &str = "\"uuid\",\"timestamp\",\"direction\",\"from\",\"to\",\"body\",\"aprs_type\",\"path\",\"message_number\",\"status\",\"hidden\"\r\n";

    #[test]
    fn records_handle_quotes_commas_and_line_breaks() {
        let records = parse_records("a,\"b,c\",\"say \"\"hi\"\"\"\r\n\"multi\nline\",x\n\n").unwrap();
        assert_eq!(records, vec![vec!["a", "b,c", "say \"hi\""], vec!["multi\nline", "x"]]);
        assert!(parse_records("\"open").is_err());
    }

    #[test]
    fn timestamps_accept_export_and_spreadsheet_forms() {
        assert_eq!(parse_timestamp("2026-10-12T18:40:00Z"), Some(NOW));
        assert_eq!(parse_timestamp("2026-10-12 18:40:00"), Some(NOW));
        assert_eq!(parse_timestamp("2026-10-12T18:40"), Some(NOW));
        assert_eq!(parse_timestamp("2026-10-12T18:40:00.250Z"), Some(NOW + 250));
        assert_eq!(parse_timestamp("2026-10-12T11:40:00-07:00"), Some(NOW));
        assert_eq!(parse_timestamp("2026-10-12"), Some(NOW - (18 * 60 + 40) * 60_000));
        for bad in ["10/12/2026", "2026-13-01", "2026-02-30T00:00Z", "2026-10-12T25:00Z", "yesterday", ""] {
            assert_eq!(parse_timestamp(bad), None, "{bad}");
        }
    }

    #[test]
    fn an_export_round_trips_and_the_guard_apostrophe_is_removed() {
        let csv = format!(
            "\u{feff}{HEADER}\"0b6f4a4e-6d0b-4a53-9a51-6f2b8d8c1a01\",\"2026-10-12T18:00:00Z\",\"received\",\"N6TEST-7\",\"AD6DM-15\",\"'-12 dB, say \"\"hi\"\"\",\"msg\",\"WIDE1-1*,WIDE2-1\",\"7\",\"\",\"no\"\r\n\
             \"\",\"2026-10-12T18:05:00Z\",\"sent\",\"AD6DM-15\",\"N6TEST-7\",\"=fine\",\"msg\",\"WIDE1-1,WIDE2-1\",\"01\",\"acked\",\"yes\"\r\n"
        );
        let rows = parse_csv(&csv, NOW).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].body, "-12 dB, say \"hi\"");
        assert_eq!(rows[0].uuid.as_deref(), Some("0b6f4a4e-6d0b-4a53-9a51-6f2b8d8c1a01"));
        assert_eq!(rows[1].body, "=fine");
        assert_eq!(rows[1].uuid, None);
        assert!(rows[1].hidden);
    }

    #[test]
    fn a_body_that_starts_with_an_apostrophe_keeps_it() {
        assert_eq!(unguard("''-) wink"), "'-) wink");
        assert_eq!(unguard("'''=x"), "''=x");
        assert_eq!(unguard("'tis"), "'tis");
        assert_eq!(unguard("'-73"), "-73");
    }

    #[test]
    fn missing_required_columns_are_refused_by_name() {
        let err = parse_csv("\"uuid\",\"from\",\"body\"\r\n\"\",\"N6TEST\",\"hi\"\r\n", NOW).unwrap_err();
        assert!(err.contains("has no direction, timestamp columns"), "{err}");
        assert!(parse_csv("", NOW).unwrap_err().contains("empty"));
    }

    #[test]
    fn bad_rows_refuse_the_whole_file_and_name_each_row() {
        let csv = format!(
            "{HEADER}\"\",\"2026-10-12T18:00:00Z\",\"received\",\"N6TEST-7\",\"\",\"ok\",\"msg\",\"\",\"\",\"\",\"\"\r\n\
             \"not-a-uuid\",\"2026-10-12T18:00:00Z\",\"received\",\"N6TEST-7\",\"\",\"x\",\"msg\",\"\",\"\",\"\",\"\"\r\n\
             \"\",\"2026-10-12T18:00:00Z\",\"sideways\",\"N6TEST-7\",\"\",\"x\",\"msg\",\"\",\"\",\"\",\"\"\r\n\
             \"\",\"2026-10-12T18:00:00Z\",\"received\",\"<script>\",\"\",\"x\",\"msg\",\"\",\"\",\"\",\"\"\r\n\
             \"\",\"2099-01-01T00:00:00Z\",\"received\",\"N6TEST-7\",\"\",\"x\",\"msg\",\"\",\"\",\"\",\"\"\r\n\
             \"\",\"2026-10-12T18:00:00Z\",\"sent\",\"AD6DM-15\",\"N6TEST-7\",\"x\",\"msg\",\"\",\"\",\"\",\"\"\r\n\
             \"\",\"2026-10-12T18:00:00Z\",\"received\",\"N6TEST-7\",\"\",\"x\",\"msg\",\"\",\"123456\",\"\",\"maybe\"\r\n"
        );
        let err = parse_csv(&csv, NOW).unwrap_err();
        assert!(err.starts_with("Nothing was imported. 6 rows need fixing:"), "{err}");
        for expected in [
            "Row 3: uuid",
            "Row 4: direction",
            "Row 5: from \"<script>\"",
            "Row 6: timestamp",
            "Row 7: a sent message needs a status",
            "Row 8: message_number",
        ] {
            assert!(err.contains(expected), "missing {expected} in {err}");
        }
    }

    #[test]
    fn times_more_than_a_few_minutes_ahead_are_refused() {
        let row = |time: &str| {
            format!("{HEADER}\"\",\"{time}\",\"received\",\"N6TEST-7\",\"\",\"x\",\"msg\",\"\",\"\",\"\",\"\"\r\n")
        };
        // NOW is 18:40:00Z; a local time typed east of UTC reads as hours ahead.
        assert!(parse_csv(&row("2026-10-12 18:44:00"), NOW).is_ok());
        let err = parse_csv(&row("2026-10-12 20:40:00"), NOW).unwrap_err();
        assert!(err.contains("read as UTC"), "{err}");
    }

    #[test]
    fn callsigns_and_paths_are_checked() {
        assert!(valid_callsign("AD6DM-15"));
        assert!(valid_callsign("BLN1"));
        assert!(valid_callsign("WIDE1-1"));
        assert!(!valid_callsign("TOOLONGCALL"));
        assert!(!valid_callsign("A B"));
        assert!(!valid_callsign("A:B"));
        assert!(!valid_callsign(""));
    }

    #[test]
    fn every_address_the_app_stores_imports_back() {
        // NWS-TSTM is the addressee on thunderstorm alerts; `?` is what the
        // store writes for a packet with no source.
        let csv = format!(
            "{HEADER}\"\",\"2026-10-12T18:00:00Z\",\"received\",\"NWS\",\"NWS-TSTM\",\"Severe thunderstorm warning\",\"msg\",\"\",\"\",\"\",\"\"\r\n\
             \"\",\"2026-10-12T18:01:00Z\",\"received\",\"?\",\"AD6DM-15\",\"x\",\"msg\",\"\",\"\",\"\",\"\"\r\n"
        );
        let rows = parse_csv(&csv, NOW).unwrap();
        assert_eq!(rows[0].to, "NWS-TSTM");
        assert_eq!(rows[1].from, "?");
    }

    #[test]
    fn signed_clock_fields_are_refused() {
        for bad in ["2026-10-12T-1:30Z", "2026-10-12T10:-5Z", "2026-10-12T10:05:-1Z", "2026-10-12T+1:30Z"] {
            assert_eq!(parse_timestamp(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_body_that_ends_in_a_line_break_keeps_it() {
        let csv = format!(
            "{HEADER}\"\",\"2026-10-12T18:00:00Z\",\"received\",\"N6TEST-7\",\"AD6DM-15\",\"two lines\r\n\",\"msg\",\"\",\"\",\"\",\"\"\r\n"
        );
        assert_eq!(parse_csv(&csv, NOW).unwrap()[0].body, "two lines\r\n");
    }
}
