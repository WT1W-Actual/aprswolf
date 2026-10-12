//! Received-message history in a local SQLite database.
//!
//! Every received packet gets a row with a UUID that never changes, so the
//! history survives restarts and can later be exported, imported, and
//! searched. Conversation (messages, queries, bulletins) is kept
//! indefinitely. Short-lived packet types (positions, weather, telemetry,
//! status, objects, items, Mic-E, acks) are trimmed once they are older than
//! the retention setting, 7 days by default.
//!
//! The windows never hold the whole history: they load one page of
//! `PAGE_SIZE` rows at a time, newest first.

use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use tauri::{AppHandle, Manager};

const DB_FILE: &str = "messages.db";
/// The JSON file that held received messages before the database existed.
const LEGACY_FILE: &str = "received_messages.json";
/// Rows per page for the windows, the web remote, and search results.
pub const PAGE_SIZE: usize = 200;
/// Parts of a multi-part message arriving within this window are joined
/// into one row, matching the main window's live merge.
const MERGE_WINDOW_MS: i64 = 30_000;
const PRUNE_INTERVAL: Duration = Duration::from_secs(60 * 60);
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

const SCHEMA: &str = "
CREATE TABLE received (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid       TEXT NOT NULL UNIQUE,
    stored_at  INTEGER NOT NULL,
    timestamp  INTEGER NOT NULL,
    from_call  TEXT NOT NULL,
    to_call    TEXT,
    body       TEXT NOT NULL,
    aprs_type  TEXT NOT NULL,
    is_bulletin INTEGER NOT NULL,
    keep       INTEGER NOT NULL,
    data       TEXT NOT NULL
);
CREATE INDEX received_prune ON received(keep, stored_at);
CREATE VIRTUAL TABLE received_fts USING fts5(
    from_call, to_call, body,
    content='received', content_rowid='seq'
);
CREATE TRIGGER received_ai AFTER INSERT ON received BEGIN
    INSERT INTO received_fts(rowid, from_call, to_call, body)
    VALUES (new.seq, new.from_call, coalesce(new.to_call, ''), new.body);
END;
CREATE TRIGGER received_ad AFTER DELETE ON received BEGIN
    INSERT INTO received_fts(received_fts, rowid, from_call, to_call, body)
    VALUES ('delete', old.seq, old.from_call, coalesce(old.to_call, ''), old.body);
END;
CREATE TRIGGER received_au AFTER UPDATE ON received BEGIN
    INSERT INTO received_fts(received_fts, rowid, from_call, to_call, body)
    VALUES ('delete', old.seq, old.from_call, coalesce(old.to_call, ''), old.body);
    INSERT INTO received_fts(rowid, from_call, to_call, body)
    VALUES (new.seq, new.from_call, coalesce(new.to_call, ''), new.body);
END;
";

struct Store {
    conn: Connection,
    last_prune: Option<Instant>,
}

fn store() -> &'static Mutex<Option<Store>> {
    static STORE: OnceLock<Mutex<Option<Store>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(None))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Open (or create) the database at `path` and bring its schema current.
fn open_at(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 1 {
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", 1)?;
    }
    Ok(())
}

/// Run `f` against the open database, opening it (and importing the legacy
/// JSON file once) on first use. Errors leave the app running without
/// history rather than failing the caller.
fn with_store<T>(app: &AppHandle, f: impl FnOnce(&mut Store) -> Result<T, String>) -> Result<T, String> {
    let mut guard = store().lock().map_err(|_| "message store lock poisoned".to_string())?;
    if guard.is_none() {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|err| format!("failed to resolve app data directory: {err}"))?;
        std::fs::create_dir_all(&dir)
            .map_err(|err| format!("failed to create app data directory: {err}"))?;
        let conn = open_at(&dir.join(DB_FILE)).map_err(|err| format!("failed to open message database: {err}"))?;
        import_legacy_file(&conn, &dir.join(LEGACY_FILE), now_ms());
        *guard = Some(Store { conn, last_prune: None });
    }
    f(guard.as_mut().expect("store opened above"))
}

/// Import the pre-database JSON file once, then rename it so it is never
/// read again. The file is newest first; rows go in oldest first so the
/// sequence order matches arrival order.
fn import_legacy_file(conn: &Connection, path: &Path, now: i64) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let messages: Vec<Value> = serde_json::from_str(&raw).unwrap_or_default();
    for message in messages.into_iter().rev() {
        let normalized = crate::settings::normalize_received_message(message);
        let timestamp = normalized.get("timestamp").and_then(Value::as_i64).unwrap_or(now);
        let _ = insert_row(conn, &normalized, now, timestamp);
    }
    let _ = std::fs::rename(path, path.with_extension("json.migrated"));
}

fn text_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// Acks and rejects (`ack12`, `rej3`) are short-lived even though they
/// arrive as messages.
fn is_ack_or_reject(body: &str) -> bool {
    let trimmed = body.trim();
    let Some((prefix, rest)) = trimmed.split_at_checked(3) else {
        return false;
    };
    (prefix.eq_ignore_ascii_case("ack") || prefix.eq_ignore_ascii_case("rej"))
        && (1..=5).contains(&rest.len())
        && rest.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Conversation is kept forever; everything else is subject to retention.
fn is_conversation(aprs_type: &str, is_bulletin: bool, body: &str) -> bool {
    (is_bulletin || aprs_type == "msg" || aprs_type == "qry") && !is_ack_or_reject(body)
}

/// Same rule as the main window's live merge (`shouldMergeMultipart`).
fn should_merge_multipart(previous: &str, incoming: &str) -> bool {
    let prev = previous.trim();
    let next = incoming.trim();
    if prev.is_empty() || next.is_empty() {
        return false;
    }
    let is_ack_number = |s: &str| {
        s.split_at_checked(3).is_some_and(|(prefix, rest)| {
            prefix.eq_ignore_ascii_case("ack") && !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
        })
    };
    if is_ack_number(prev) || is_ack_number(next) {
        return false;
    }
    if next.starts_with("mm,") || next.starts_with("...") || next.starts_with("and ") {
        return true;
    }
    if prev.ends_with(',') || prev.ends_with("...") {
        return true;
    }
    prev.ends_with("{EW") || prev.ends_with("{EX")
}

fn insert_row(conn: &Connection, message: &Value, stored_at: i64, timestamp: i64) -> rusqlite::Result<(String, i64)> {
    let uuid = text_field(message, "uuid")
        .map(ToString::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let from = text_field(message, "from").unwrap_or("?");
    let to = text_field(message, "to");
    let body = text_field(message, "body").unwrap_or("");
    let aprs_type = text_field(message, "aprsType").unwrap_or("other");
    let is_bulletin = message.get("isBulletin").and_then(Value::as_bool).unwrap_or(false);
    let keep = is_conversation(aprs_type, is_bulletin, body);

    let mut data = message.clone();
    if let Some(obj) = data.as_object_mut() {
        obj.remove("id");
        obj.remove("seq");
        obj.insert("uuid".to_string(), Value::String(uuid.clone()));
        obj.insert("timestamp".to_string(), Value::from(timestamp));
    }

    conn.execute(
        "INSERT OR IGNORE INTO received
            (uuid, stored_at, timestamp, from_call, to_call, body, aprs_type, is_bulletin, keep, data)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![uuid, stored_at, timestamp, from, to, body, aprs_type, is_bulletin, keep, data.to_string()],
    )?;
    let seq = conn.query_row("SELECT seq FROM received WHERE uuid = ?1", params![uuid], |row| row.get(0))?;
    Ok((uuid, seq))
}

/// The most recent row, read to decide whether a new packet continues it.
struct LatestRow {
    seq: i64,
    uuid: String,
    timestamp: i64,
    from: String,
    to: Option<String>,
    body: String,
    data: String,
}

/// Store one received packet, joining it to the previous row when it is the
/// next part of a multi-part message. Returns the row's UUID and sequence.
fn record(conn: &Connection, message: &Value, now: i64) -> rusqlite::Result<(String, i64)> {
    let from = text_field(message, "from").unwrap_or("?");
    let to = text_field(message, "to");
    let body = text_field(message, "body").unwrap_or("");

    let latest = conn
        .query_row(
            "SELECT seq, uuid, timestamp, from_call, to_call, body, data FROM received ORDER BY seq DESC LIMIT 1",
            [],
            |row| {
                Ok(LatestRow {
                    seq: row.get(0)?,
                    uuid: row.get(1)?,
                    timestamp: row.get(2)?,
                    from: row.get(3)?,
                    to: row.get(4)?,
                    body: row.get(5)?,
                    data: row.get(6)?,
                })
            },
        )
        .optional()?;

    if let Some(LatestRow { seq, uuid, timestamp, from: latest_from, to: latest_to, body: latest_body, data: latest_data }) = latest {
        if latest_from == from
            && latest_to.as_deref() == to
            && now - timestamp < MERGE_WINDOW_MS
            && should_merge_multipart(&latest_body, body)
        {
            let merged_body = format!("{latest_body} {body}");
            let aprs_type = text_field(message, "aprsType").unwrap_or("other");
            let is_bulletin = message.get("isBulletin").and_then(Value::as_bool).unwrap_or(false);
            let keep = is_conversation(aprs_type, is_bulletin, &merged_body);
            let mut data: Value = serde_json::from_str(&latest_data).unwrap_or(Value::Null);
            if let Some(obj) = data.as_object_mut() {
                obj.insert("body".to_string(), Value::String(merged_body.clone()));
                obj.insert("timestamp".to_string(), Value::from(now));
                obj.insert("aprsType".to_string(), Value::String(aprs_type.to_string()));
                obj.insert("isBulletin".to_string(), Value::Bool(is_bulletin));
            }
            conn.execute(
                "UPDATE received SET body = ?1, timestamp = ?2, aprs_type = ?3, is_bulletin = ?4, keep = ?5, data = ?6
                 WHERE seq = ?7",
                params![merged_body, now, aprs_type, is_bulletin, keep, data.to_string(), seq],
            )?;
            return Ok((uuid, seq));
        }
    }

    insert_row(conn, message, now, now)
}

/// Delete short-lived rows stored more than `retention_days` ago. Zero
/// keeps everything.
fn prune(conn: &Connection, retention_days: u32, now: i64) -> rusqlite::Result<usize> {
    if retention_days == 0 {
        return Ok(0);
    }
    let cutoff = now - i64::from(retention_days) * DAY_MS;
    conn.execute("DELETE FROM received WHERE keep = 0 AND stored_at < ?1", params![cutoff])
}

fn row_to_value(data: String, seq: i64) -> Value {
    let mut value: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
    if let Some(obj) = value.as_object_mut() {
        obj.insert("seq".to_string(), Value::from(seq));
    }
    value
}

/// One page of history, newest first, older than `before_seq` when given.
fn page(conn: &Connection, before_seq: Option<i64>, limit: usize) -> rusqlite::Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT data, seq FROM received WHERE seq < ?1 ORDER BY seq DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![before_seq.unwrap_or(i64::MAX), limit as i64], |row| {
        Ok(row_to_value(row.get(0)?, row.get(1)?))
    })?;
    rows.collect()
}

/// Turn what the user typed into a safe FTS5 query: each word becomes a
/// quoted prefix term, so punctuation in callsigns and messages can never
/// be read as query syntax. Returns None when nothing searchable is left.
fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|word| word.replace('"', ""))
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .map(|word| format!("\"{word}\"*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

/// Rows whose callsigns or text match every word in `query`, newest first.
fn search(conn: &Connection, query: &str, before_seq: Option<i64>, limit: usize) -> rusqlite::Result<Vec<Value>> {
    let Some(fts) = fts_query(query) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT r.data, r.seq FROM received_fts f JOIN received r ON r.seq = f.rowid
         WHERE received_fts MATCH ?1 AND r.seq < ?2
         ORDER BY r.seq DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![fts, before_seq.unwrap_or(i64::MAX), limit as i64], |row| {
        Ok(row_to_value(row.get(0)?, row.get(1)?))
    })?;
    rows.collect()
}

/// Store a received packet and stamp `event` with the row's UUID and
/// sequence so the windows can tell it apart from older pages. Called from
/// the TNC and APRS-IS receive paths before the `tnc_message` event goes
/// out. A storage failure leaves the event unstamped and the app running.
pub fn record_received(app: &AppHandle, event: &mut Value) {
    let normalized = crate::settings::normalize_received_message(event.clone());
    let retention_days = crate::settings::load_settings(app.clone()).short_lived_retention_days;
    let stored = with_store(app, |store| {
        let now = now_ms();
        let stored = record(&store.conn, &normalized, now).map_err(|err| err.to_string())?;
        if store.last_prune.is_none_or(|at| at.elapsed() >= PRUNE_INTERVAL) {
            let _ = prune(&store.conn, retention_days, now);
            store.last_prune = Some(Instant::now());
        }
        Ok(stored)
    });
    if let (Ok((uuid, seq)), Some(obj)) = (stored, event.as_object_mut()) {
        obj.insert("uuid".to_string(), Value::String(uuid));
        obj.insert("seq".to_string(), Value::from(seq));
    }
}

/// The newest page of history, for the web remote's state snapshot.
pub fn recent(app: &AppHandle) -> Vec<Value> {
    with_store(app, |store| page(&store.conn, None, PAGE_SIZE).map_err(|err| err.to_string())).unwrap_or_default()
}

/// Newest-first history, one page at a time. Pass the smallest `seq`
/// already shown as `before_seq` to get the next older page.
#[tauri::command]
pub fn load_received_messages(app: AppHandle, before_seq: Option<i64>, limit: Option<usize>) -> Vec<Value> {
    let limit = limit.unwrap_or(PAGE_SIZE).min(PAGE_SIZE);
    with_store(&app, |store| page(&store.conn, before_seq, limit).map_err(|err| err.to_string())).unwrap_or_default()
}

/// Full-text search over callsigns and message text across all history.
#[tauri::command]
pub fn search_received_messages(
    app: AppHandle,
    query: String,
    before_seq: Option<i64>,
    limit: Option<usize>,
) -> Result<Vec<Value>, String> {
    let limit = limit.unwrap_or(PAGE_SIZE).min(PAGE_SIZE);
    with_store(&app, |store| search(&store.conn, &query, before_seq, limit).map_err(|err| err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    fn message(from: &str, to: Option<&str>, body: &str, aprs_type: &str) -> Value {
        json!({ "from": from, "to": to, "body": body, "aprsType": aprs_type, "isBulletin": false })
    }

    #[test]
    fn history_outlives_the_old_200_cap_and_pages_newest_first() {
        let conn = memory_db();
        for i in 0..450 {
            record(&conn, &message("W1AW", Some("AD6DM"), &format!("note {i}"), "msg"), i * 60_000).unwrap();
        }
        let first = page(&conn, None, PAGE_SIZE).unwrap();
        assert_eq!(first.len(), 200);
        assert_eq!(first[0]["body"], "note 449");
        let oldest_shown = first.last().unwrap()["seq"].as_i64().unwrap();
        let second = page(&conn, Some(oldest_shown), PAGE_SIZE).unwrap();
        assert_eq!(second[0]["body"], "note 249");
        let third = page(&conn, Some(second.last().unwrap()["seq"].as_i64().unwrap()), PAGE_SIZE).unwrap();
        assert_eq!(third.len(), 50);
        assert_eq!(third.last().unwrap()["body"], "note 0");
    }

    #[test]
    fn uuid_is_assigned_once_and_kept() {
        let conn = memory_db();
        let (uuid, seq) = record(&conn, &message("W1AW", Some("AD6DM"), "hello", "msg"), 1_000).unwrap();
        let stored = page(&conn, None, 10).unwrap();
        assert_eq!(stored[0]["uuid"], uuid.as_str());
        assert_eq!(stored[0]["seq"], seq);
        assert!(uuid::Uuid::parse_str(&uuid).is_ok());
    }

    #[test]
    fn multipart_parts_join_into_one_row() {
        let conn = memory_db();
        let (first_uuid, _) = record(&conn, &message("W1AW", Some("AD6DM"), "meet at the club,", "msg"), 1_000).unwrap();
        let (second_uuid, _) = record(&conn, &message("W1AW", Some("AD6DM"), "bring the radio", "msg"), 5_000).unwrap();
        assert_eq!(first_uuid, second_uuid);
        let stored = page(&conn, None, 10).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0]["body"], "meet at the club, bring the radio");
    }

    #[test]
    fn retention_trims_short_lived_rows_and_keeps_conversation() {
        let conn = memory_db();
        let day = DAY_MS;
        record(&conn, &message("W1AW", Some("AD6DM"), "hello", "msg"), 0).unwrap();
        record(&conn, &message("W1AW", None, "!3745.50N/12223.45W-", "pos"), 1).unwrap();
        record(&conn, &message("W1AW", Some("AD6DM"), "ack12", "msg"), 2).unwrap();
        record(&conn, &message("K6XYZ", None, "_10111213c220s004", "wx"), 9 * day).unwrap();

        assert_eq!(prune(&conn, 7, 10 * day).unwrap(), 2);
        let bodies: Vec<String> = page(&conn, None, 10).unwrap().iter().map(|v| v["body"].as_str().unwrap().to_string()).collect();
        assert_eq!(bodies, vec!["_10111213c220s004", "hello"]);

        assert_eq!(prune(&conn, 0, 100 * day).unwrap(), 0);
    }

    #[test]
    fn search_finds_messages_past_the_first_page() {
        let conn = memory_db();
        record(&conn, &message("KM4ACK-7", Some("AD6DM-15"), "net tonight at 8", "msg"), 0).unwrap();
        for i in 0..300 {
            record(&conn, &message("W1AW", None, &format!("!pos {i}"), "pos"), 60_000 * (i + 1)).unwrap();
        }
        let hits = search(&conn, "km4ack", None, PAGE_SIZE).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["body"], "net tonight at 8");
        assert_eq!(search(&conn, "tonight net", None, PAGE_SIZE).unwrap().len(), 1);
        assert_eq!(search(&conn, "net\" OR \"x", None, PAGE_SIZE).unwrap().len(), 0);
        assert!(search(&conn, "\"*()", None, PAGE_SIZE).unwrap().is_empty());
    }

    #[test]
    fn search_follows_merged_bodies() {
        let conn = memory_db();
        record(&conn, &message("W1AW", Some("AD6DM"), "first part,", "msg"), 1_000).unwrap();
        record(&conn, &message("W1AW", Some("AD6DM"), "second part", "msg"), 2_000).unwrap();
        assert_eq!(search(&conn, "second", None, 10).unwrap().len(), 1);
    }

    #[test]
    fn legacy_file_imports_once_in_arrival_order() {
        let dir = std::env::temp_dir().join(format!("aprswolf-store-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let legacy = dir.join(LEGACY_FILE);
        std::fs::write(
            &legacy,
            json!([
                { "from": "W1AW", "to": "AD6DM", "body": "newer", "aprsType": "msg", "timestamp": 2_000 },
                { "from": "W1AW", "to": "AD6DM", "body": "older", "aprsType": "msg", "timestamp": 1_000 }
            ])
            .to_string(),
        )
        .unwrap();

        let conn = open_at(&dir.join(DB_FILE)).unwrap();
        import_legacy_file(&conn, &legacy, 5_000);
        import_legacy_file(&conn, &legacy, 6_000);

        let rows = page(&conn, None, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["body"], "newer");
        assert_eq!(rows[1]["timestamp"], 1_000);
        assert!(!legacy.exists());
        assert!(dir.join("received_messages.json.migrated").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ack_and_reject_bodies_are_short_lived() {
        assert!(is_ack_or_reject("ack12"));
        assert!(is_ack_or_reject("REJ3"));
        assert!(!is_ack_or_reject("acknowledged, thanks"));
        assert!(!is_ack_or_reject("ack"));
        assert!(!is_ack_or_reject("acknowledged"));
        assert!(is_conversation("msg", false, "hello"));
        assert!(is_conversation("pos", true, "BLN1 net tonight"));
        assert!(!is_conversation("wx", false, "_101"));
    }
}
