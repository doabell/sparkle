//! Bounded, local diagnostic captures. Library metadata and credentials are never queried.
use crate::analytics::now_epoch_ms;
use crate::logging::sanitize;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::{json, Value};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

const EVENT_WINDOW_MS: i64 = 30 * 60 * 1_000;
const EVENT_LIMIT: usize = 2_000;
const LOG_BYTES: u64 = 512 * 1024;
const LOG_FILES: usize = 3;

#[derive(Clone, Serialize)]
pub struct CaptureInfo {
    pub id: String,
    pub incident_at_ms: i64,
}

#[derive(Serialize)]
pub struct DiagnosticBundle {
    pub format: &'static str,
    pub version: u8,
    pub capture: CaptureInfo,
    pub captured_at_ms: i64,
    pub app_version: String,
    pub platform: &'static str,
    pub runtime: Value,
    pub events_since_ms: i64,
    pub events_truncated: bool,
    pub recent_events: Vec<Value>,
    pub logs: Vec<DiagnosticLog>,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
pub struct DiagnosticLog {
    pub name: String,
    pub truncated: bool,
    pub text: String,
}

pub fn capture(
    db_path: &Path,
    log_dir: &Path,
    stem: &str,
    info: CaptureInfo,
    app_version: &str,
    runtime: Value,
) -> DiagnosticBundle {
    let mut warnings = Vec::new();
    let database =
        Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY).and_then(|conn| {
            conn.busy_timeout(Duration::from_millis(250))?;
            recent_events(&conn, info.incident_at_ms)
        });
    let (recent_events, events_truncated) = match database {
        Ok(events) => events,
        Err(error) => {
            warnings.push(sanitize(&format!("Playback events unavailable: {error}")));
            (Vec::new(), false)
        }
    };
    let logs = collect_logs(log_dir, stem, &mut warnings);
    DiagnosticBundle {
        format: "sparkle-playback-diagnostics",
        version: 1,
        captured_at_ms: now_epoch_ms(),
        app_version: app_version.into(),
        platform: std::env::consts::OS,
        runtime,
        events_since_ms: info.incident_at_ms.saturating_sub(EVENT_WINDOW_MS),
        capture: info,
        events_truncated,
        recent_events,
        logs,
        warnings,
    }
}

fn recent_events(conn: &Connection, incident_at_ms: i64) -> rusqlite::Result<(Vec<Value>, bool)> {
    let mut stmt = conn.prepare(
        "SELECT id, listen_id, session_id, occurred_at_ms, event_type, source, reason,
         track_id, position_ms, target_position_ms, context_type, context_id,
         queue_index, play_order_index, queue_length, shuffle, repeat_mode
         FROM playback_events WHERE occurred_at_ms >= ?1 AND occurred_at_ms <= ?2
         ORDER BY occurred_at_ms DESC, id DESC LIMIT ?3",
    )?;
    let mut events = stmt.query_map(rusqlite::params![
        incident_at_ms.saturating_sub(EVENT_WINDOW_MS), incident_at_ms, (EVENT_LIMIT + 1) as i64
    ], |row| Ok(json!({
        "id": row.get::<_, String>(0)?, "listen_id": row.get::<_, Option<String>>(1)?,
        "session_id": row.get::<_, Option<String>>(2)?, "occurred_at_ms": row.get::<_, i64>(3)?,
        "event_type": row.get::<_, String>(4)?, "source": row.get::<_, String>(5)?,
        "reason": row.get::<_, Option<String>>(6)?, "track_id": row.get::<_, Option<i64>>(7)?,
        "position_ms": row.get::<_, Option<i64>>(8)?, "target_position_ms": row.get::<_, Option<i64>>(9)?,
        "context_type": row.get::<_, String>(10)?, "context_id": row.get::<_, Option<String>>(11)?,
        "queue_index": row.get::<_, Option<i64>>(12)?, "play_order_index": row.get::<_, Option<i64>>(13)?,
        "queue_length": row.get::<_, i64>(14)?, "shuffle": row.get::<_, bool>(15)?,
        "repeat_mode": row.get::<_, String>(16)?,
    })))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let truncated = events.len() > EVENT_LIMIT;
    events.truncate(EVENT_LIMIT);
    events.reverse();
    Ok((events, truncated))
}

fn collect_logs(log_dir: &Path, stem: &str, warnings: &mut Vec<String>) -> Vec<DiagnosticLog> {
    let entries = match std::fs::read_dir(log_dir) {
        Ok(entries) => entries,
        Err(error) => {
            warnings.push(sanitize(&format!("Log directory unavailable: {error}")));
            return Vec::new();
        }
    };
    let active = format!("{stem}.log");
    let archive_prefix = format!("{stem}_");
    let mut candidates = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name != active && !(name.starts_with(&archive_prefix) && name.ends_with(".log")) {
                return None;
            }
            // Do not follow a replaced log entry into an unrelated directory/file.
            let metadata = entry.metadata().ok()?;
            if !entry.file_type().ok()?.is_file() {
                return None;
            }
            Some((name == active, metadata.modified().ok(), name, entry.path()))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    candidates.truncate(LOG_FILES);
    candidates
        .into_iter()
        .filter_map(|(_, _, name, path)| match read_log_tail(&path, name) {
            Ok(log) => Some(log),
            Err(error) => {
                warnings.push(sanitize(&format!("A log file could not be read: {error}")));
                None
            }
        })
        .collect()
}

fn read_log_tail(path: &Path, name: String) -> std::io::Result<DiagnosticLog> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let offset = size.saturating_sub(LOG_BYTES);
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(LOG_BYTES).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let text = if offset > 0 {
        text.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
    } else {
        &text
    };
    // Redact URLs and bound each record again, including files from older builds.
    let text = text.lines().map(sanitize).collect::<Vec<_>>().join("\n");
    Ok(DiagnosticLog {
        name,
        truncated: offset > 0,
        text,
    })
}

#[cfg(test)]
#[path = "tests/diagnostics.rs"]
mod tests;
