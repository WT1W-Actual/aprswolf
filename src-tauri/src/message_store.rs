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
//!
//! A packet often arrives more than once: over RF and APRS-IS, or through
//! more than one digipeater. Each received packet gets a fingerprint from
//! its sender, addressee, body, and APRS message number, and a packet whose
//! fingerprint was stored within `DUPLICATE_WINDOW_MS` is not stored again.
//!
//! Messages can be cleared from either list, always by naming them: one,
//! a selected set, or everything a list shows. A cleared message is hidden: it leaves the list but stays in the database,
//! and search still finds it, marked as hidden. Clearing with delete removes
//! it from the database.
//!
//! Sent messages live in their own table, one row per message keyed by a
//! UUID, with the delivery status (queued, sent, acked, failed, expired)
//! updated as it changes. They are conversation, so retention never trims
//! them.

use std::collections::HashMap;
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
/// A copy of a packet heard again within this window is a duplicate. Thirty
/// seconds matches the duplicate check in APRS-IS servers and digipeaters,
/// so a sender's retry after it is stored as a new row.
const DUPLICATE_WINDOW_MS: i64 = 30_000;
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
    recent: RecentPackets,
}

/// Fingerprints of packets stored within the duplicate window, each with the
/// row it went into. Held in memory: the window is shorter than any restart.
#[derive(Default)]
struct RecentPackets {
    seen: HashMap<String, (String, i64, i64)>,
}

impl RecentPackets {
    /// The row a packet with this fingerprint went into, if it was stored
    /// within the window.
    fn find(&mut self, fingerprint: &str, now: i64) -> Option<(String, i64)> {
        self.seen.retain(|_, (_, _, at)| now - *at < DUPLICATE_WINDOW_MS);
        self.seen.get(fingerprint).map(|(uuid, seq, _)| (uuid.clone(), *seq))
    }

    fn remember(&mut self, fingerprint: String, uuid: &str, seq: i64, now: i64) {
        self.seen.insert(fingerprint, (uuid.to_string(), seq, now));
    }
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

const SCHEMA_SENT: &str = "
CREATE TABLE sent (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid       TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    from_call  TEXT NOT NULL,
    to_call    TEXT NOT NULL,
    body       TEXT NOT NULL,
    path       TEXT NOT NULL,
    aprs_id    TEXT NOT NULL,
    status     TEXT NOT NULL,
    attempts   INTEGER NOT NULL,
    detail     TEXT
);
";

/// Full-text search over sent messages, and a hidden flag on both tables for
/// messages cleared from the lists but kept in the database.
const SCHEMA_SEARCH_SENT: &str = "
ALTER TABLE sent ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
ALTER TABLE received ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
CREATE VIRTUAL TABLE sent_fts USING fts5(
    to_call, body,
    content='sent', content_rowid='seq'
);
CREATE TRIGGER sent_ai AFTER INSERT ON sent BEGIN
    INSERT INTO sent_fts(rowid, to_call, body) VALUES (new.seq, new.to_call, new.body);
END;
CREATE TRIGGER sent_ad AFTER DELETE ON sent BEGIN
    INSERT INTO sent_fts(sent_fts, rowid, to_call, body) VALUES ('delete', old.seq, old.to_call, old.body);
END;
CREATE TRIGGER sent_au AFTER UPDATE OF to_call, body ON sent BEGIN
    INSERT INTO sent_fts(sent_fts, rowid, to_call, body) VALUES ('delete', old.seq, old.to_call, old.body);
    INSERT INTO sent_fts(rowid, to_call, body) VALUES (new.seq, new.to_call, new.body);
END;
INSERT INTO sent_fts(sent_fts) VALUES ('rebuild');
";

/// When each sent message entered this store, for exports of what is new
/// since the last one, and a small key-value table for that export mark.
const SCHEMA_EXPORT_MARK: &str = "
ALTER TABLE sent ADD COLUMN stored_at INTEGER NOT NULL DEFAULT 0;
UPDATE sent SET stored_at = created_at;
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

/// Indexes for lists ordered by message time, so imported messages sit
/// where their time puts them.
const SCHEMA_TIME_ORDER: &str = "
CREATE INDEX received_time ON received(timestamp, seq);
CREATE INDEX sent_time ON sent(created_at, seq);
";

/// Each step and its version bump commit together, so a crash mid-step
/// leaves the database at the old version, ready to run the step again.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let steps = [SCHEMA, SCHEMA_SENT, SCHEMA_SEARCH_SENT, SCHEMA_EXPORT_MARK, SCHEMA_TIME_ORDER];
    for (target, sql) in (1..).zip(steps) {
        if version < target {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", target)?;
            tx.commit()?;
        }
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
        let _ = expire_unfinished_sent(&conn, now_ms());
        *guard = Some(Store { conn, last_prune: None, recent: RecentPackets::default() });
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

/// The APRS message number (`{12`, or `{12}34` with a reply-ack) of a
/// message packet, read from the raw info field since the stored body has
/// it stripped.
pub(crate) fn message_number(message: &Value) -> Option<&str> {
    let raw = text_field(message, "protocol_info_text").or_else(|| text_field(message, "info_text"))?;
    let info = crate::settings::unwrap_third_party_info(raw).unwrap_or(raw).trim();
    let text = info.strip_prefix(':')?.get(10..)?;
    let (_, id) = text.rsplit_once('{')?;
    crate::tnc::split_reply_ack_id(id.trim()).map(|(number, _)| number)
}

/// Sender, addressee, body, and message number: what makes two copies of a
/// packet the same packet, whatever path each took.
fn fingerprint(message: &Value) -> String {
    [
        text_field(message, "from").unwrap_or("?"),
        text_field(message, "to").unwrap_or(""),
        text_field(message, "body").unwrap_or("").trim(),
        message_number(message).unwrap_or(""),
    ]
    .join("\u{1f}")
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
            "SELECT seq, uuid, timestamp, from_call, to_call, body, data FROM received WHERE hidden = 0
             ORDER BY seq DESC LIMIT 1",
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

/// Store a packet unless a copy of it was stored within the duplicate
/// window. Returns the row it is in and whether it was a duplicate.
fn receive(conn: &Connection, recent: &mut RecentPackets, message: &Value, now: i64) -> rusqlite::Result<((String, i64), bool)> {
    let fingerprint = fingerprint(message);
    if let Some(first) = recent.find(&fingerprint, now) {
        return Ok((first, true));
    }
    let stored = record(conn, message, now)?;
    recent.remember(fingerprint, &stored.0, stored.1, now);
    Ok((stored, false))
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

fn row_to_value(data: String, seq: i64, hidden: bool) -> Value {
    let mut value: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
    if let Some(obj) = value.as_object_mut() {
        obj.insert("seq".to_string(), Value::from(seq));
        obj.insert("hidden".to_string(), Value::Bool(hidden));
    }
    value
}

/// Where the next older page starts: the message time and sequence of the
/// last message already shown. Lists run newest first by message time, so
/// an imported message sits where its time puts it rather than at the top.
type Cursor = (i64, i64);

fn cursor_values(before: Option<Cursor>) -> (i64, i64) {
    before.unwrap_or((i64::MAX, i64::MAX))
}

fn cursor(before_time: Option<i64>, before_seq: Option<i64>) -> Option<Cursor> {
    before_seq.map(|seq| (before_time.unwrap_or(i64::MAX), seq))
}

/// One page of the list, newest first by message time, after `before`
/// when given. Hidden rows are left out.
fn page(conn: &Connection, before: Option<Cursor>, limit: usize) -> rusqlite::Result<Vec<Value>> {
    let (time, seq) = cursor_values(before);
    let mut stmt = conn.prepare(
        "SELECT data, seq, hidden FROM received WHERE hidden = 0 AND (timestamp, seq) < (?1, ?2)
         ORDER BY timestamp DESC, seq DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![time, seq, limit as i64], |row| {
        Ok(row_to_value(row.get(0)?, row.get(1)?, row.get(2)?))
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

/// Rows whose callsigns or text match every word in `query`, newest first,
/// hidden ones included.
fn search(conn: &Connection, query: &str, before: Option<Cursor>, limit: usize) -> rusqlite::Result<Vec<Value>> {
    let Some(fts) = fts_query(query) else {
        return Ok(Vec::new());
    };
    let (time, seq) = cursor_values(before);
    let mut stmt = conn.prepare(
        "SELECT r.data, r.seq, r.hidden FROM received_fts f JOIN received r ON r.seq = f.rowid
         WHERE received_fts MATCH ?1 AND (r.timestamp, r.seq) < (?2, ?3)
         ORDER BY r.timestamp DESC, r.seq DESC LIMIT ?4",
    )?;
    let rows = stmt.query_map(params![fts, time, seq, limit as i64], |row| {
        Ok(row_to_value(row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.collect()
}

/// Store a received packet and stamp `event` with the row's UUID and
/// sequence so the windows can tell it apart from older pages. Called from
/// the TNC and APRS-IS receive paths before the `tnc_message` event goes
/// out. A duplicate is not stored: it is stamped with the first copy's UUID
/// and sequence and marked `duplicate`, so the Traffic Log still shows every
/// copy heard while the message lists show one. A storage failure leaves the
/// event unstamped and the app running.
pub fn record_received(app: &AppHandle, event: &mut Value) {
    let normalized = crate::settings::normalize_received_message(event.clone());
    let retention_days = crate::settings::load_settings(app.clone()).short_lived_retention_days;
    let stored = with_store(app, |store| {
        let now = now_ms();
        let stored = receive(&store.conn, &mut store.recent, &normalized, now).map_err(|err| err.to_string())?;
        if store.last_prune.is_none_or(|at| at.elapsed() >= PRUNE_INTERVAL) {
            let _ = prune(&store.conn, retention_days, now);
            store.last_prune = Some(Instant::now());
        }
        Ok(stored)
    });
    if let (Ok(((uuid, seq), duplicate)), Some(obj)) = (stored, event.as_object_mut()) {
        obj.insert("uuid".to_string(), Value::String(uuid));
        obj.insert("seq".to_string(), Value::from(seq));
        if duplicate {
            obj.insert("duplicate".to_string(), Value::Bool(true));
        }
    }
}

/// One sent message as the delivery queue reports it.
pub struct SentRecord<'a> {
    pub uuid: &'a str,
    pub created_at: i64,
    pub from_call: &'a str,
    pub to_call: &'a str,
    pub body: &'a str,
    pub path: &'a str,
    pub aprs_id: &'a str,
    pub status: &'a str,
    pub attempts: u32,
    pub detail: Option<&'a str>,
}

/// Insert a sent message the first time it is seen, then keep its status,
/// attempt count and detail current on every later report.
fn upsert_sent(conn: &Connection, rec: &SentRecord, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sent (uuid, created_at, updated_at, stored_at, from_call, to_call, body, path, aprs_id, status, attempts, detail)
         VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(uuid) DO UPDATE SET
             updated_at = excluded.updated_at,
             status = excluded.status,
             attempts = excluded.attempts,
             detail = excluded.detail",
        params![
            rec.uuid,
            rec.created_at,
            now,
            rec.from_call,
            rec.to_call,
            rec.body,
            rec.path,
            rec.aprs_id,
            rec.status,
            rec.attempts,
            rec.detail,
        ],
    )?;
    Ok(())
}

/// The outbound queue starts empty at every launch, so a message still
/// queued or awaiting its ack when the app last closed will never be retried
/// or acked. Runs once when the store opens, before this session records
/// anything.
fn expire_unfinished_sent(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE sent SET status = 'expired', detail = 'app closed before delivery', updated_at = ?1
         WHERE status IN ('queued', 'sent')",
        params![now],
    )
}

const SENT_COLUMNS: &str =
    "s.seq, s.uuid, s.created_at, s.updated_at, s.from_call, s.to_call, s.body, s.path, s.status, s.attempts, s.detail, s.hidden";

fn sent_row_to_value(row: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(serde_json::json!({
        "seq": row.get::<_, i64>(0)?,
        "uuid": row.get::<_, String>(1)?,
        "timestamp": row.get::<_, i64>(2)?,
        "updatedAt": row.get::<_, i64>(3)?,
        "source": row.get::<_, String>(4)?,
        "destination": row.get::<_, String>(5)?,
        "body": row.get::<_, String>(6)?,
        "path": row.get::<_, String>(7)?,
        "status": row.get::<_, String>(8)?,
        "attempts": row.get::<_, u32>(9)?,
        "detail": row.get::<_, Option<String>>(10)?,
        "hidden": row.get::<_, bool>(11)?,
    }))
}

/// One page of the sent list, newest first by message time, after `before` when
/// given. Hidden rows are left out.
fn sent_page(conn: &Connection, before: Option<Cursor>, limit: usize) -> rusqlite::Result<Vec<Value>> {
    let (time, seq) = cursor_values(before);
    let mut stmt = conn.prepare(&format!(
        "SELECT {SENT_COLUMNS} FROM sent s WHERE s.hidden = 0 AND (s.created_at, s.seq) < (?1, ?2)
         ORDER BY s.created_at DESC, s.seq DESC LIMIT ?3"
    ))?;
    let rows = stmt.query_map(params![time, seq, limit as i64], sent_row_to_value)?;
    rows.collect()
}

/// Sent messages whose addressee or text match every word in `query`,
/// newest first, hidden ones included.
fn search_sent(conn: &Connection, query: &str, before: Option<Cursor>, limit: usize) -> rusqlite::Result<Vec<Value>> {
    let Some(fts) = fts_query(query) else {
        return Ok(Vec::new());
    };
    let (time, seq) = cursor_values(before);
    let mut stmt = conn.prepare(&format!(
        "SELECT {SENT_COLUMNS} FROM sent_fts f JOIN sent s ON s.seq = f.rowid
         WHERE sent_fts MATCH ?1 AND (s.created_at, s.seq) < (?2, ?3)
         ORDER BY s.created_at DESC, s.seq DESC LIMIT ?4"
    ))?;
    let rows = stmt.query_map(params![fts, time, seq, limit as i64], sent_row_to_value)?;
    rows.collect()
}

/// Record a sent message or its latest delivery status. Called on every
/// delivery report; a storage failure leaves the app running.
pub fn record_sent(app: &AppHandle, rec: &SentRecord) {
    let _ = with_store(app, |store| upsert_sent(&store.conn, rec, now_ms()).map_err(|err| err.to_string()));
}

/// Newest-first sent messages, one page at a time.
#[tauri::command]
pub fn load_sent_messages(
    app: AppHandle,
    before_time: Option<i64>,
    before_seq: Option<i64>,
    limit: Option<usize>,
) -> Vec<Value> {
    let limit = limit.unwrap_or(PAGE_SIZE).min(PAGE_SIZE);
    with_store(&app, |store| sent_page(&store.conn, cursor(before_time, before_seq), limit).map_err(|err| err.to_string())).unwrap_or_default()
}

/// Which list a clear applies to.
#[derive(Clone, Copy, Debug, serde::Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageList {
    Sent,
    Received,
}

impl MessageList {
    fn table(self) -> &'static str {
        match self {
            MessageList::Sent => "sent",
            MessageList::Received => "received",
        }
    }
}

/// Hide the named messages from a list, or delete them from the database
/// when `delete` is set. There is deliberately no "every row" form: a clear
/// only ever touches messages the window listed. Returns how many rows
/// changed.
fn clear(conn: &Connection, list: MessageList, uuids: &[String], delete: bool) -> rusqlite::Result<usize> {
    let table = list.table();
    let action = if delete {
        format!("DELETE FROM {table}")
    } else {
        format!("UPDATE {table} SET hidden = 1")
    };
    let tx = conn.unchecked_transaction()?;
    let mut changed = 0;
    {
        let mut stmt = tx.prepare(&format!("{action} WHERE uuid = ?1"))?;
        for uuid in uuids {
            changed += stmt.execute(params![uuid])?;
        }
    }
    tx.commit()?;
    Ok(changed)
}

/// Put hidden messages back in their list.
fn restore(conn: &Connection, list: MessageList, uuids: &[String]) -> rusqlite::Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut changed = 0;
    {
        let mut stmt = tx.prepare(&format!("UPDATE {} SET hidden = 0 WHERE uuid = ?1 AND hidden = 1", list.table()))?;
        for uuid in uuids {
            changed += stmt.execute(params![uuid])?;
        }
    }
    tx.commit()?;
    Ok(changed)
}

/// Unhide the named messages so their list shows them again.
#[tauri::command]
pub fn restore_messages(app: AppHandle, list: MessageList, uuids: Vec<String>) -> Result<usize, String> {
    with_store(&app, |store| restore(&store.conn, list, &uuids).map_err(|err| err.to_string()))
}

/// Clear the named messages from a list: hidden by default, deleted from the
/// database when `delete` is set.
#[tauri::command]
pub fn clear_messages(app: AppHandle, list: MessageList, uuids: Vec<String>, delete: bool) -> Result<usize, String> {
    with_store(&app, |store| {
        let changed = clear(&store.conn, list, &uuids, delete).map_err(|err| err.to_string())?;
        // A later copy of a cleared packet should not be matched to its row.
        store.recent = RecentPackets::default();
        Ok(changed)
    })
}

/// Full-text search over sent messages' addressees and text.
#[tauri::command]
pub fn search_sent_messages(
    app: AppHandle,
    query: String,
    before_time: Option<i64>,
    before_seq: Option<i64>,
    limit: Option<usize>,
) -> Result<Vec<Value>, String> {
    let limit = limit.unwrap_or(PAGE_SIZE).min(PAGE_SIZE);
    with_store(&app, |store| search_sent(&store.conn, &query, cursor(before_time, before_seq), limit).map_err(|err| err.to_string()))
}

const LAST_EXPORT_KEY: &str = "last_new_only_export";

fn last_export(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    let value: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", params![LAST_EXPORT_KEY], |row| row.get(0))
        .optional()?;
    Ok(value.and_then(|v| v.parse().ok()))
}

/// Write the messages `filter` selects to `out`. A new-only export with no
/// other filter moves the mark to `now`, so the next one starts where this
/// one stopped; a narrower one leaves the mark alone.
fn export(
    conn: &Connection,
    out: &mut impl std::io::Write,
    filter: &crate::message_export::ExportFilter,
    now: i64,
) -> Result<usize, String> {
    let since = last_export(conn).map_err(|err| err.to_string())?;
    let count = crate::message_export::write_csv(conn, out, filter, since, now)
        .map_err(|err| format!("export failed: {err}"))?;
    if filter.new_only && !filter.narrows() {
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![LAST_EXPORT_KEY, now.to_string()],
        )
        .map_err(|err| err.to_string())?;
    }
    Ok(count)
}

/// Write the messages `filter` selects to a CSV file at `path`; returns how
/// many it holds.
#[tauri::command]
pub fn export_messages(
    app: AppHandle,
    path: String,
    filter: crate::message_export::ExportFilter,
) -> Result<usize, String> {
    with_store(&app, |store| {
        let file = std::fs::File::create(&path).map_err(|err| format!("could not create {path}: {err}"))?;
        let mut out = std::io::BufWriter::new(file);
        export(&store.conn, &mut out, &filter, now_ms())
    })
}

/// When the last new-only export ran, in milliseconds, if one has.
#[tauri::command]
pub fn last_new_only_export(app: AppHandle) -> Result<Option<i64>, String> {
    with_store(&app, |store| last_export(&store.conn).map_err(|err| err.to_string()))
}

/// How an import went.
#[derive(Debug, PartialEq)]
pub struct ImportSummary {
    pub added: usize,
    pub skipped: usize,
}

/// Received rows in `rows` of a short-lived type older than the retention
/// window, which the import dialog warns about. Zero when retention keeps
/// everything.
pub fn old_short_lived(rows: &[crate::message_import::ImportRow], retention_days: u32, now: i64) -> usize {
    if retention_days == 0 {
        return 0;
    }
    let cutoff = now - i64::from(retention_days) * DAY_MS;
    rows.iter()
        .filter(|row| row.direction == crate::message_import::Direction::Received)
        .filter(|row| !is_conversation(&row.aprs_type, row.to.starts_with("BLN"), &row.body))
        .filter(|row| row.timestamp < cutoff)
        .count()
}

/// Store validated rows in one transaction. A row whose UUID is already
/// stored is skipped, so importing a file twice changes nothing. A row
/// without a UUID gets one, unless a message with the same direction,
/// sender, addressee, text, and time is already stored. Imported rows count
/// as stored now, for retention and for new-only exports.
fn import(conn: &Connection, rows: &[crate::message_import::ImportRow], now: i64) -> rusqlite::Result<ImportSummary> {
    use crate::message_import::Direction;
    let tx = conn.unchecked_transaction()?;
    let mut summary = ImportSummary { added: 0, skipped: 0 };
    for row in rows {
        let table = if row.direction == Direction::Sent { "sent" } else { "received" };
        let exists = match &row.uuid {
            Some(uuid) => tx
                .query_row(&format!("SELECT 1 FROM {table} WHERE uuid = ?1"), params![uuid], |_| Ok(()))
                .optional()?
                .is_some(),
            None => {
                let time_column = if row.direction == Direction::Sent { "created_at" } else { "timestamp" };
                tx.query_row(
                    &format!(
                        "SELECT 1 FROM {table} WHERE from_call = ?1 AND coalesce(to_call, '') = ?2
                         AND body = ?3 AND {time_column} = ?4"
                    ),
                    params![row.from, row.to, row.body, row.timestamp],
                    |_| Ok(()),
                )
                .optional()?
                .is_some()
            }
        };
        if exists {
            summary.skipped += 1;
            continue;
        }
        let uuid = row.uuid.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        match row.direction {
            Direction::Sent => {
                tx.execute(
                    "INSERT INTO sent (uuid, created_at, updated_at, stored_at, from_call, to_call, body, path,
                                       aprs_id, status, attempts, detail, hidden)
                     VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, 'imported', ?10)",
                    params![
                        uuid,
                        row.timestamp,
                        now,
                        row.from,
                        row.to,
                        row.body,
                        row.path,
                        row.message_number,
                        row.status,
                        row.hidden
                    ],
                )?;
            }
            Direction::Received => {
                let to = (!row.to.is_empty()).then_some(row.to.as_str());
                // Rebuild the message frame so the message number reads
                // back the way it does for a live packet.
                let info_text = match to {
                    Some(to) if row.aprs_type == "msg" || row.aprs_type == "qry" => {
                        let number = if row.message_number.is_empty() { String::new() } else { format!("{{{}", row.message_number) };
                        format!(":{to:<9}:{}{number}", row.body)
                    }
                    _ => row.body.clone(),
                };
                let path: Vec<&str> = row.path.split(',').filter(|hop| !hop.is_empty()).collect();
                let message = serde_json::json!({
                    "uuid": uuid,
                    "from": row.from,
                    "to": to,
                    "body": row.body,
                    "aprsType": row.aprs_type,
                    "isBulletin": row.to.starts_with("BLN"),
                    "micEDecoded": false,
                    "path": path,
                    "source": row.from,
                    "info_text": info_text,
                    "text": format!("{} > IMPORT: {info_text}", row.from),
                    "imported": true,
                });
                insert_row(&tx, &message, now, row.timestamp)?;
                if row.hidden {
                    tx.execute("UPDATE received SET hidden = 1 WHERE uuid = ?1", params![uuid])?;
                }
            }
        }
        summary.added += 1;
    }
    tx.commit()?;
    Ok(summary)
}

/// Store rows read from an import file.
pub fn import_rows(app: &AppHandle, rows: &[crate::message_import::ImportRow]) -> Result<ImportSummary, String> {
    with_store(app, |store| {
        // Importing only adds rows, so the duplicate check's memory of live
        // packets stays valid.
        import(&store.conn, rows, now_ms()).map_err(|err| format!("import failed: {err}"))
    })
}

/// The newest page of history, for the web remote's state snapshot.
pub fn recent(app: &AppHandle) -> Vec<Value> {
    with_store(app, |store| page(&store.conn, None, PAGE_SIZE).map_err(|err| err.to_string())).unwrap_or_default()
}

/// Newest-first history, one page at a time. Pass the time and `seq` of
/// the last message shown as `before_time` and `before_seq` to get the
/// next older page.
#[tauri::command]
pub fn load_received_messages(
    app: AppHandle,
    before_time: Option<i64>,
    before_seq: Option<i64>,
    limit: Option<usize>,
) -> Vec<Value> {
    let limit = limit.unwrap_or(PAGE_SIZE).min(PAGE_SIZE);
    with_store(&app, |store| page(&store.conn, cursor(before_time, before_seq), limit).map_err(|err| err.to_string())).unwrap_or_default()
}

/// Full-text search over callsigns and message text across all history.
#[tauri::command]
pub fn search_received_messages(
    app: AppHandle,
    query: String,
    before_time: Option<i64>,
    before_seq: Option<i64>,
    limit: Option<usize>,
) -> Result<Vec<Value>, String> {
    let limit = limit.unwrap_or(PAGE_SIZE).min(PAGE_SIZE);
    with_store(&app, |store| search(&store.conn, &query, cursor(before_time, before_seq), limit).map_err(|err| err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The cursor just past `row`, as a window builds it from the last
    /// message it shows.
    fn after(row: &Value) -> Option<Cursor> {
        Some((row["timestamp"].as_i64().unwrap(), row["seq"].as_i64().unwrap()))
    }

    fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    fn message(from: &str, to: Option<&str>, body: &str, aprs_type: &str) -> Value {
        json!({ "from": from, "to": to, "body": body, "aprsType": aprs_type, "isBulletin": false })
    }

    fn sent<'a>(uuid: &'a str, to: &'a str, status: &'a str, attempts: u32) -> SentRecord<'a> {
        SentRecord {
            uuid,
            created_at: 1_000,
            from_call: "WT1W-15",
            to_call: to,
            body: "Hello",
            path: "WIDE1-1,WIDE2-1",
            aprs_id: "01",
            status,
            attempts,
            detail: None,
        }
    }

    fn heard(from: &str, info: &str, body: &str) -> Value {
        json!({ "from": from, "to": "AD6DM-15", "body": body, "aprsType": "msg", "isBulletin": false, "info_text": info })
    }

    fn row_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT count(*) FROM received", [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn packet_heard_over_two_paths_is_stored_once() {
        let conn = memory_db();
        let mut recent = RecentPackets::default();
        let rf = heard("N6TEST-7", ":AD6DM-15 :Hello there{12", "Hello there");
        let aprsis = heard("N6TEST-7", "}N6TEST-7>APRS,TCPIP,AD6DM-15*::AD6DM-15 :Hello there{12", "Hello there");
        let (first, dup_first) = receive(&conn, &mut recent, &rf, 1_000).unwrap();
        let (second, dup_second) = receive(&conn, &mut recent, &aprsis, 4_000).unwrap();
        assert!(!dup_first);
        assert!(dup_second);
        assert_eq!(first, second);
        assert_eq!(row_count(&conn), 1);
    }

    #[test]
    fn resend_after_the_window_is_stored_again() {
        let conn = memory_db();
        let mut recent = RecentPackets::default();
        let packet = heard("N6TEST-7", ":AD6DM-15 :Hello there{12", "Hello there");
        receive(&conn, &mut recent, &packet, 1_000).unwrap();
        let (_, duplicate) = receive(&conn, &mut recent, &packet, 1_000 + DUPLICATE_WINDOW_MS).unwrap();
        assert!(!duplicate);
        assert_eq!(row_count(&conn), 2);
    }

    #[test]
    fn same_text_with_a_new_message_number_is_a_new_message() {
        let conn = memory_db();
        let mut recent = RecentPackets::default();
        receive(&conn, &mut recent, &heard("N6TEST-7", ":AD6DM-15 :73{12", "73"), 1_000).unwrap();
        let (_, duplicate) = receive(&conn, &mut recent, &heard("N6TEST-7", ":AD6DM-15 :73{13}", "73"), 2_000).unwrap();
        assert!(!duplicate);
        assert_eq!(row_count(&conn), 2);
    }

    #[test]
    fn message_number_reads_plain_reply_ack_and_third_party_forms() {
        assert_eq!(message_number(&heard("A", ":AD6DM-15 :hi{12", "hi")), Some("12"));
        assert_eq!(message_number(&heard("A", ":AD6DM-15 :hi{12}34", "hi")), Some("12"));
        assert_eq!(message_number(&heard("A", "}A>APRS,TCPIP*::AD6DM-15 :hi{AB", "hi")), Some("AB"));
        assert_eq!(message_number(&heard("A", ":AD6DM-15 :no number", "no number")), None);
        assert_eq!(message_number(&heard("A", "!3830.00N/12130.00W>", "")), None);
    }

    #[test]
    fn sent_message_keeps_one_row_with_its_latest_status() {
        let conn = memory_db();
        upsert_sent(&conn, &sent("s-1", "N6TEST-7", "queued", 0), 1_000).unwrap();
        upsert_sent(&conn, &sent("s-1", "N6TEST-7", "sent", 1), 2_000).unwrap();
        upsert_sent(&conn, &sent("s-1", "N6TEST-7", "acked", 1), 3_000).unwrap();

        let rows = sent_page(&conn, None, PAGE_SIZE).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["status"], "acked");
        assert_eq!(rows[0]["attempts"], 1);
        assert_eq!(rows[0]["timestamp"], 1_000);
        assert_eq!(rows[0]["updatedAt"], 3_000);
    }

    #[test]
    fn sent_messages_page_newest_first_and_survive_retention() {
        let conn = memory_db();
        for i in 0..3 {
            let (uuid, to) = (format!("s-{i}"), format!("N{i}TEST"));
            upsert_sent(&conn, &sent(&uuid, &to, "failed", 3), 1_000).unwrap();
        }
        prune(&conn, 1, 100 * DAY_MS).unwrap();

        let rows = sent_page(&conn, None, PAGE_SIZE).unwrap();
        let order: Vec<&str> = rows.iter().map(|r| r["uuid"].as_str().unwrap()).collect();
        assert_eq!(order, ["s-2", "s-1", "s-0"]);
        let older = sent_page(&conn, after(&rows[0]), PAGE_SIZE).unwrap();
        assert_eq!(older.len(), 2);
    }

    #[test]
    fn unfinished_sends_from_a_closed_session_expire() {
        let conn = memory_db();
        upsert_sent(&conn, &sent("s-q", "N6TEST-7", "queued", 0), 1_000).unwrap();
        upsert_sent(&conn, &sent("s-s", "N6TEST-7", "sent", 2), 1_000).unwrap();
        upsert_sent(&conn, &sent("s-a", "N6TEST-7", "acked", 1), 1_000).unwrap();
        upsert_sent(&conn, &sent("s-f", "N6TEST-7", "failed", 3), 1_000).unwrap();

        assert_eq!(expire_unfinished_sent(&conn, 5_000).unwrap(), 2);
        let rows = sent_page(&conn, None, PAGE_SIZE).unwrap();
        let status = |uuid: &str| rows.iter().find(|r| r["uuid"] == uuid).unwrap()["status"].clone();
        assert_eq!(status("s-q"), "expired");
        assert_eq!(status("s-s"), "expired");
        assert_eq!(status("s-a"), "acked");
        assert_eq!(status("s-f"), "failed");
    }

    #[test]
    fn hidden_messages_leave_the_list_and_stay_searchable() {
        let conn = memory_db();
        let (keep, _) = record(&conn, &message("N6TEST-7", Some("WT1W-15"), "Keep this one", "msg"), 1_000).unwrap();
        let (gone, _) = record(&conn, &message("K6TEST-9", Some("WT1W-15"), "Hide this one", "msg"), 2_000).unwrap();
        assert_eq!(clear(&conn, MessageList::Received, &[gone.clone()], false).unwrap(), 1);
        let listed = page(&conn, None, 200).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["uuid"], keep.as_str());
        assert_eq!(listed[0]["hidden"], false);
        let hits = search(&conn, "hide", None, 200).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["hidden"], true);
    }

    #[test]
    fn restored_messages_return_to_the_list() {
        let conn = memory_db();
        upsert_sent(&conn, &sent("a", "N6TEST-7", "acked", 1), 1_000).unwrap();
        clear(&conn, MessageList::Sent, &["a".to_string()], false).unwrap();
        assert!(sent_page(&conn, None, 200).unwrap().is_empty());
        assert_eq!(restore(&conn, MessageList::Sent, &["a".to_string()]).unwrap(), 1);
        let listed = sent_page(&conn, None, 200).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["hidden"], false);
    }

    #[test]
    fn deleted_messages_are_gone_from_list_and_search() {
        let conn = memory_db();
        let (gone, _) = record(&conn, &message("K6TEST-9", Some("WT1W-15"), "Delete this one", "msg"), 1_000).unwrap();
        assert_eq!(clear(&conn, MessageList::Received, &[gone], true).unwrap(), 1);
        assert!(page(&conn, None, 200).unwrap().is_empty());
        assert!(search(&conn, "delete", None, 200).unwrap().is_empty());
    }

    #[test]
    fn clearing_a_set_touches_only_the_named_messages() {
        let conn = memory_db();
        for (i, uuid) in ["a", "b", "c"].iter().enumerate() {
            upsert_sent(&conn, &sent(uuid, "N6TEST-7", "acked", 1), 1_000 + i as i64).unwrap();
        }
        assert_eq!(clear(&conn, MessageList::Sent, &["a".to_string(), "c".to_string()], true).unwrap(), 2);
        let listed = sent_page(&conn, None, 200).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["uuid"], "b");
    }

    #[test]
    fn a_new_part_never_joins_a_hidden_row() {
        let conn = memory_db();
        let (first, _) = record(&conn, &message("N6TEST-7", Some("WT1W-15"), "Heading out,", "msg"), 1_000).unwrap();
        clear(&conn, MessageList::Received, &[first.clone()], false).unwrap();
        let (second, _) = record(&conn, &message("N6TEST-7", Some("WT1W-15"), "back at noon", "msg"), 2_000).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn sent_search_matches_addressee_and_text_across_pages() {
        let conn = memory_db();
        for i in 0..5 {
            let uuid = format!("u{i}");
            let mut rec = sent(&uuid, if i % 2 == 0 { "N6TEST-7" } else { "K6TEST-9" }, "acked", 1);
            let body = format!("net check {i}");
            rec.body = &body;
            upsert_sent(&conn, &rec, 1_000 + i).unwrap();
        }
        let by_call = search_sent(&conn, "n6test", None, 200).unwrap();
        assert_eq!(by_call.len(), 3);
        assert_eq!(by_call[0]["destination"], "N6TEST-7");
        assert_eq!(by_call[0]["body"], "net check 4");
        let by_text = search_sent(&conn, "net check", None, 2).unwrap();
        assert_eq!(by_text.len(), 2);
        let older = search_sent(&conn, "net check", after(&by_text[1]), 200).unwrap();
        assert_eq!(older.len(), 3);
        assert!(search_sent(&conn, "zzz", None, 200).unwrap().is_empty());
    }

    #[test]
    fn sent_search_reports_hidden_messages() {
        let conn = memory_db();
        upsert_sent(&conn, &sent("u1", "N6TEST-7", "acked", 1), 1_000).unwrap();
        conn.execute("UPDATE sent SET hidden = 1 WHERE uuid = 'u1'", []).unwrap();
        let hits = search_sent(&conn, "N6TEST", None, 200).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["hidden"], true);
    }

    #[test]
    fn status_updates_keep_sent_search_current() {
        let conn = memory_db();
        upsert_sent(&conn, &sent("u1", "N6TEST-7", "queued", 0), 1_000).unwrap();
        upsert_sent(&conn, &sent("u1", "N6TEST-7", "acked", 1), 2_000).unwrap();
        let hits = search_sent(&conn, "N6TEST", None, 200).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["status"], "acked");
    }

    #[test]
    fn version_2_database_indexes_and_dates_existing_sent_messages() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute_batch(SCHEMA_SENT).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        conn.execute(
            "INSERT INTO sent (uuid, created_at, updated_at, from_call, to_call, body, path, aprs_id, status, attempts)
             VALUES ('old', 1000, 2000, 'WT1W-15', 'W6TEST-1', 'Hello', 'WIDE1-1', '01', 'acked', 1)",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(version, 5);
        assert_eq!(search_sent(&conn, "W6TEST", None, 200).unwrap().len(), 1);
        let stored_at: i64 = conn.query_row("SELECT stored_at FROM sent WHERE uuid = 'old'", [], |row| row.get(0)).unwrap();
        assert_eq!(stored_at, 1000);
    }

    #[test]
    fn a_failed_migration_step_leaves_the_old_version_intact() {
        let conn = Connection::open_in_memory().unwrap();
        for sql in [SCHEMA, SCHEMA_SENT, SCHEMA_SEARCH_SENT] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 3).unwrap();
        // Step 4 adds sent.stored_at, then fails creating a table that exists.
        conn.execute_batch("CREATE TABLE meta (x)").unwrap();
        assert!(migrate(&conn).is_err());
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(version, 3);
        assert!(conn.prepare("SELECT stored_at FROM sent").is_err());

        // With the obstacle gone, the step runs cleanly on the next launch.
        conn.execute_batch("DROP TABLE meta").unwrap();
        migrate(&conn).unwrap();
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(version, 5);
    }

    #[test]
    fn version_1_database_gains_the_sent_table() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        migrate(&conn).unwrap();
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(version, 5);
        upsert_sent(&conn, &sent("s-1", "N6TEST-7", "queued", 0), 1_000).unwrap();
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
        let second = page(&conn, after(first.last().unwrap()), PAGE_SIZE).unwrap();
        assert_eq!(second[0]["body"], "note 249");
        let third = page(&conn, after(second.last().unwrap()), PAGE_SIZE).unwrap();
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
    fn export_writes_conversation_oldest_first_with_hidden_rows() {
        let conn = memory_db();
        record(&conn, &message("N6TEST-7", Some("AD6DM-15"), "=SUM(A1)", "msg"), 1_000).unwrap();
        record(&conn, &message("N7TEST-9", None, "!3830.00N/12130.00W>", "pos"), 2_000).unwrap();
        record(&conn, &message("N6TEST-7", Some("AD6DM-15"), "ack12", "msg"), 3_000).unwrap();
        upsert_sent(&conn, &sent("s-1", "N6TEST-7", "acked", 1), 4_000).unwrap();
        let (hidden, _) = record(&conn, &message("W6TEST-1", Some("BLN1"), "Net tonight, 1930", "msg"), 5_000).unwrap();
        clear(&conn, MessageList::Received, &[hidden], false).unwrap();

        let mut out = Vec::new();
        assert_eq!(export(&conn, &mut out, &Default::default(), 10_000).unwrap(), 3);
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with('\u{feff}'));
        let lines: Vec<&str> = text.trim_start_matches('\u{feff}').split("\r\n").filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("\"uuid\",\"timestamp\",\"direction\""));
        assert!(lines[1].contains("\"received\",\"N6TEST-7\",\"AD6DM-15\",\"'=SUM(A1)\""));
        assert!(lines[2].contains("\"sent\""));
        assert!(lines[2].contains("\"acked\""));
        assert!(lines[3].contains("\"Net tonight, 1930\""));
        assert!(lines[3].ends_with("\"yes\""));
    }

    fn export_count(conn: &Connection, filter: &crate::message_export::ExportFilter, now: i64) -> usize {
        export(conn, &mut Vec::new(), filter, now).unwrap()
    }

    #[test]
    fn export_filters_narrow_the_file() {
        use crate::message_export::ExportFilter;
        let conn = memory_db();
        record(&conn, &message("N6TEST-7", Some("AD6DM-15"), "one", "msg"), 1_000).unwrap();
        record(&conn, &message("K6TEST-9", Some("AD6DM-15"), "two", "msg"), 2_000).unwrap();
        record(&conn, &message("N7TEST-9", None, "!3830.00N/12130.00W>", "pos"), 3_000).unwrap();
        upsert_sent(&conn, &sent("s-1", "K6TEST-9", "acked", 1), 4_000).unwrap();

        assert_eq!(export_count(&conn, &ExportFilter::default(), 9_000), 3);
        let only = |direction: &str| ExportFilter { direction: direction.to_string(), ..Default::default() };
        assert_eq!(export_count(&conn, &only("received"), 9_000), 2);
        assert_eq!(export_count(&conn, &only("sent"), 9_000), 1);
        let calls = ExportFilter { callsigns: vec!["k6test".to_string()], ..Default::default() };
        assert_eq!(export_count(&conn, &calls, 9_000), 2);
        let dates = ExportFilter { from_ms: Some(1_500), to_ms: Some(2_500), ..Default::default() };
        assert_eq!(export_count(&conn, &dates, 9_000), 1);
        let short_lived = ExportFilter { include_short_lived: true, ..Default::default() };
        assert_eq!(export_count(&conn, &short_lived, 9_000), 4);
    }

    #[test]
    fn new_only_exports_pick_up_where_the_last_one_stopped() {
        use crate::message_export::ExportFilter;
        let conn = memory_db();
        let new_only = ExportFilter { new_only: true, ..Default::default() };
        // A message imported later keeps its old timestamp but is stored now.
        record(&conn, &message("N6TEST-7", Some("AD6DM-15"), "first", "msg"), 1_000).unwrap();
        assert_eq!(export_count(&conn, &new_only, 2_000), 1);
        assert_eq!(export_count(&conn, &new_only, 3_000), 0);
        assert_eq!(last_export(&conn).unwrap(), Some(3_000));

        let mut old = message("K6TEST-9", Some("AD6DM-15"), "imported", "msg");
        old["timestamp"] = json!(500);
        insert_row(&conn, &old, 4_000, 500).unwrap();
        upsert_sent(&conn, &sent("s-1", "K6TEST-9", "acked", 1), 4_500).unwrap();
        assert_eq!(export_count(&conn, &new_only, 5_000), 2);
        // A full export leaves the mark alone.
        assert_eq!(export_count(&conn, &ExportFilter::default(), 6_000), 3);
        assert_eq!(last_export(&conn).unwrap(), Some(5_000));
    }

    #[test]
    fn a_narrowed_new_only_export_leaves_the_mark_alone() {
        use crate::message_export::ExportFilter;
        let conn = memory_db();
        record(&conn, &message("N6TEST-7", Some("AD6DM-15"), "heard", "msg"), 1_000).unwrap();
        upsert_sent(&conn, &sent("s-1", "K6TEST-9", "acked", 1), 1_500).unwrap();
        let sent_only = ExportFilter { new_only: true, direction: "sent".to_string(), ..Default::default() };
        assert_eq!(export_count(&conn, &sent_only, 2_000), 1);
        assert_eq!(last_export(&conn).unwrap(), None);
        // The received message it skipped still reaches the next new-only export.
        let new_only = ExportFilter { new_only: true, ..Default::default() };
        assert_eq!(export_count(&conn, &new_only, 3_000), 2);
        assert_eq!(last_export(&conn).unwrap(), Some(3_000));
    }

    fn export_text(conn: &Connection) -> String {
        let mut out = Vec::new();
        export(conn, &mut out, &Default::default(), i64::MAX).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn an_export_imported_into_an_empty_store_exports_the_same() {
        let conn = memory_db();
        let mut hello = message("N6TEST-7", Some("AD6DM-15"), "-12 dB, say \"hi\"", "msg");
        hello["info_text"] = json!(":AD6DM-15 :-12 dB, say \"hi\"{7");
        hello["path"] = json!(["WIDE1-1*", "WIDE2-1"]);
        record(&conn, &hello, 1_791_830_400_000).unwrap();
        record(&conn, &message("W6TEST-1", Some("BLN1"), "Net at 1930", "msg"), 1_791_830_460_000).unwrap();
        let mut sent_row = sent("6f1c2b7e-0d5a-4c1e-9f0b-2a3d4e5f6a7b", "N6TEST-7", "acked", 1);
        sent_row.created_at = 1_791_830_520_000;
        upsert_sent(&conn, &sent_row, 1_791_830_530_000).unwrap();
        let bulletin_uuid: String =
            conn.query_row("SELECT uuid FROM received WHERE to_call = 'BLN1'", [], |r| r.get(0)).unwrap();
        clear(&conn, MessageList::Received, &[bulletin_uuid], false).unwrap();
        let original = export_text(&conn);

        let rows = crate::message_import::parse_csv(&original, 1_791_830_600_000).unwrap();
        let fresh = memory_db();
        assert_eq!(import(&fresh, &rows, 1_791_830_600_000).unwrap(), ImportSummary { added: 3, skipped: 0 });
        assert_eq!(export_text(&fresh), original);

        // Importing the same file again changes nothing.
        assert_eq!(import(&fresh, &rows, 1_791_830_700_000).unwrap(), ImportSummary { added: 0, skipped: 3 });
        assert_eq!(export_text(&fresh), original);
    }

    #[test]
    fn a_hand_added_row_without_a_uuid_is_stored_once() {
        let conn = memory_db();
        let csv = "direction,from,to,body,timestamp\nreceived,K6TEST-9,AD6DM-15,Typed in by hand,2026-10-12 18:00:00\n";
        let rows = crate::message_import::parse_csv(csv, 1_791_830_400_000).unwrap();
        assert_eq!(import(&conn, &rows, 1_000).unwrap().added, 1);
        assert_eq!(import(&conn, &rows, 2_000).unwrap(), ImportSummary { added: 0, skipped: 1 });
        let uuid: String = conn.query_row("SELECT uuid FROM received", [], |r| r.get(0)).unwrap();
        assert!(uuid::Uuid::parse_str(&uuid).is_ok());
    }

    #[test]
    fn old_short_lived_rows_are_counted_for_the_warning() {
        let csv = "direction,from,to,body,timestamp,aprs_type\n\
                   received,N7TEST-9,,!3830.00N/12130.00W>,2026-09-01T00:00:00Z,pos\n\
                   received,N7TEST-9,,!3830.00N/12130.00W>,2026-10-12T00:00:00Z,pos\n\
                   received,K6TEST-9,AD6DM-15,old chat,2026-09-01T00:00:00Z,msg\n";
        let now = 1_791_830_400_000;
        let rows = crate::message_import::parse_csv(csv, now).unwrap();
        assert_eq!(old_short_lived(&rows, 7, now), 1);
        assert_eq!(old_short_lived(&rows, 0, now), 0);
    }

    #[test]
    fn lists_run_by_message_time_so_imports_land_in_place() {
        let conn = memory_db();
        record(&conn, &message("N6TEST-7", Some("AD6DM-15"), "live at noon", "msg"), 12 * 3_600_000).unwrap();
        // Stored later, but sent earlier: an import of an older message.
        let mut old = message("K6TEST-9", Some("AD6DM-15"), "imported from 10:00", "msg");
        old["uuid"] = json!("00000000-0000-4000-8000-000000000001");
        insert_row(&conn, &old, 13 * 3_600_000, 10 * 3_600_000).unwrap();
        record(&conn, &message("N6TEST-7", Some("AD6DM-15"), "live at 2pm", "msg"), 14 * 3_600_000).unwrap();

        let bodies = |rows: Vec<Value>| rows.iter().map(|v| v["body"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        let first = page(&conn, None, 2).unwrap();
        assert_eq!(bodies(first.clone()), vec!["live at 2pm", "live at noon"]);
        assert_eq!(bodies(page(&conn, after(&first[1]), 2).unwrap()), vec!["imported from 10:00"]);
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
