use crate::settings::{save_session, SessionSnapshot};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const MAX_HISTORY_EVENTS: i64 = 100_000;
const HISTORY_PRUNE_INTERVAL: usize = 256;

#[derive(Debug, Clone)]
pub struct PlayRecord {
    pub track_id: i64,
    pub started_at: i64,
    pub played_ms: i64,
    pub completed: bool,
}

enum WriteRequest {
    SaveSession(SessionSnapshot),
    RecordPlay(PlayRecord),
}

/// Serializes the audio engine's database writes onto its own thread and
/// connection. Playback code must never block on SQLite: session snapshots
/// are coalesced (only the newest is written) and play records are queued,
/// so a busy database can never stall audio.
#[derive(Clone)]
pub struct DbWriter {
    tx: mpsc::Sender<WriteRequest>,
}

impl DbWriter {
    pub fn new(db_path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel::<WriteRequest>();
        thread::spawn(move || writer_loop(db_path, rx));
        Self { tx }
    }

    pub fn save_session(&self, snapshot: SessionSnapshot) {
        let _ = self.tx.send(WriteRequest::SaveSession(snapshot));
    }

    pub fn record_play(&self, record: PlayRecord) {
        let _ = self.tx.send(WriteRequest::RecordPlay(record));
    }
}

fn writer_loop(db_path: PathBuf, rx: mpsc::Receiver<WriteRequest>) {
    let conn = match crate::db::open_connection(&db_path) {
        Ok(c) => c,
        Err(e) => {
            log::error!("db writer: cannot open {}: {e}", db_path.display());
            return;
        }
    };
    let mut pending_session: Option<SessionSnapshot> = None;
    let mut plays_since_prune = 0;
    if let Err(e) = prune_history(&conn) {
        log::warn!("Failed to trim listening history: {e}");
    }
    loop {
        match rx.recv() {
            Ok(WriteRequest::SaveSession(s)) => pending_session = Some(s),
            Ok(WriteRequest::RecordPlay(r)) => {
                write_play(&conn, &r);
                maintain_history_limit(&conn, &mut plays_since_prune);
            }
            Err(_) => break,
        }
        // Drain the backlog before writing: only the newest session snapshot
        // is worth persisting, so a burst of volume ticks becomes one write.
        while let Ok(msg) = rx.try_recv() {
            match msg {
                WriteRequest::SaveSession(s) => pending_session = Some(s),
                WriteRequest::RecordPlay(r) => {
                    write_play(&conn, &r);
                    maintain_history_limit(&conn, &mut plays_since_prune);
                }
            }
        }
        if let Some(snapshot) = pending_session.take() {
            if let Err(e) = retry_busy(|| save_session(&conn, &snapshot)) {
                log::warn!("Failed to save session: {e}");
            }
        }
    }
}

fn maintain_history_limit(conn: &Connection, plays_since_prune: &mut usize) {
    *plays_since_prune += 1;
    if *plays_since_prune < HISTORY_PRUNE_INTERVAL {
        return;
    }
    *plays_since_prune = 0;
    if let Err(e) = prune_history(conn) {
        log::warn!("Failed to trim listening history: {e}");
    }
}

fn prune_history(conn: &Connection) -> rusqlite::Result<usize> {
    prune_history_to(conn, MAX_HISTORY_EVENTS)
}

fn prune_history_to(conn: &Connection, limit: i64) -> rusqlite::Result<usize> {
    retry_busy(|| {
        conn.execute(
            "DELETE FROM play_history WHERE id IN (\
             SELECT id FROM play_history ORDER BY started_at DESC, id DESC LIMIT -1 OFFSET ?1\
             )",
            [limit],
        )
    })
}

fn write_play(conn: &Connection, record: &PlayRecord) {
    let result = retry_busy(|| {
        conn.execute(
            "INSERT OR IGNORE INTO play_history (track_id, started_at, played_ms, completed) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                record.track_id,
                record.started_at,
                record.played_ms,
                record.completed as i64
            ],
        )
    });
    if let Err(e) = result {
        log::warn!("Failed to record play: {e}");
    }
}

fn retry_busy<T, F>(mut operation: F) -> rusqlite::Result<T>
where
    F: FnMut() -> rusqlite::Result<T>,
{
    let mut delay = Duration::from_millis(50);
    for attempt in 0..20 {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) if is_busy(&error) && attempt < 19 => {
                thread::sleep(delay);
                delay = (delay * 2).min(Duration::from_millis(500));
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("retry loop always returns")
}

fn is_busy(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _)
            if matches!(
                inner.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_record_insert() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE play_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                track_id INTEGER NOT NULL,
                started_at INTEGER NOT NULL,
                played_ms INTEGER NOT NULL,
                completed INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        write_play(
            &conn,
            &PlayRecord {
                track_id: 7,
                started_at: 1_700_000_000,
                played_ms: 123_000,
                completed: true,
            },
        );
        let (track_id, played_ms, completed): (i64, i64, i64) = conn
            .query_row(
                "SELECT track_id, played_ms, completed FROM play_history",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(track_id, 7);
        assert_eq!(played_ms, 123_000);
        assert_eq!(completed, 1);
    }

    #[test]
    fn busy_writes_are_retried() {
        let mut attempts = 0;
        let value = retry_busy(|| {
            attempts += 1;
            if attempts < 3 {
                Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error {
                        code: rusqlite::ErrorCode::DatabaseBusy,
                        extended_code: 5,
                    },
                    None,
                ))
            } else {
                Ok("saved")
            }
        })
        .unwrap();
        assert_eq!(value, "saved");
        assert_eq!(attempts, 3);
    }

    #[test]
    fn history_limit_keeps_the_newest_events() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE play_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                track_id INTEGER NOT NULL,
                started_at INTEGER NOT NULL,
                played_ms INTEGER NOT NULL,
                completed INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO play_history (track_id, started_at, played_ms, completed) VALUES
                (1, 1, 30000, 0), (1, 2, 30000, 0), (1, 3, 30000, 0),
                (1, 4, 30000, 0), (1, 5, 30000, 0);",
        )
        .unwrap();

        assert_eq!(prune_history_to(&conn, 3).unwrap(), 2);
        let timestamps = conn
            .prepare("SELECT started_at FROM play_history ORDER BY started_at")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<i64>, _>>()
            .unwrap();
        assert_eq!(timestamps, vec![3, 4, 5]);
    }
}
