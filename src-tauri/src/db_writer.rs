use crate::settings::{save_session, SessionSnapshot};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
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
    Shutdown(mpsc::Sender<Result<(), String>>),
}

/// Serializes the audio engine's database writes onto its own thread and
/// connection. Playback code must never block on SQLite: session snapshots
/// are coalesced (only the newest is written) and play records are queued,
/// so a busy database can never stall audio.
pub struct DbWriter {
    tx: Option<mpsc::Sender<WriteRequest>>,
    worker: Option<JoinHandle<()>>,
}

impl DbWriter {
    pub fn new(db_path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel::<WriteRequest>();
        let worker = thread::spawn(move || writer_loop(db_path, rx));
        Self {
            tx: Some(tx),
            worker: Some(worker),
        }
    }

    pub fn save_session(&self, snapshot: SessionSnapshot) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(WriteRequest::SaveSession(snapshot));
        }
    }

    pub fn record_play(&self, record: PlayRecord) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(WriteRequest::RecordPlay(record));
        }
    }

    /// Flushes all requests sent before this call and joins the writer thread.
    /// The explicit acknowledgement means shutdown does not merely enqueue the
    /// final listening event and race process termination.
    pub fn shutdown(mut self) -> Result<(), String> {
        let tx = self
            .tx
            .take()
            .ok_or_else(|| "database writer is already shut down".to_string())?;
        let (reply_tx, reply_rx) = mpsc::channel();
        tx.send(WriteRequest::Shutdown(reply_tx))
            .map_err(|_| "database writer stopped before shutdown".to_string())?;
        drop(tx);

        let flush_result = reply_rx
            .recv()
            .map_err(|_| "database writer stopped before confirming its final flush".to_string())?;
        let join_result = self
            .worker
            .take()
            .expect("database writer worker is present while running")
            .join()
            .map_err(|_| "database writer thread panicked during shutdown".to_string());

        flush_result.and(join_result)
    }
}

impl Drop for DbWriter {
    fn drop(&mut self) {
        // Dropping the last sender makes the writer drain everything already
        // queued before it exits. Explicit application shutdown uses
        // `shutdown` above so write failures can also be reported.
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
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
    let mut first_write_error: Option<String> = None;
    if let Err(e) = prune_history(&conn) {
        log::warn!("Failed to trim listening history: {e}");
    }
    loop {
        let mut shutdown_reply = match rx.recv() {
            Ok(WriteRequest::SaveSession(s)) => {
                pending_session = Some(s);
                None
            }
            Ok(WriteRequest::RecordPlay(r)) => {
                capture_write_error(&mut first_write_error, "record play", write_play(&conn, &r));
                maintain_history_limit(&conn, &mut plays_since_prune);
                None
            }
            Ok(WriteRequest::Shutdown(reply)) => Some(reply),
            Err(_) => break,
        };
        // Drain the backlog before writing: only the newest session snapshot
        // is worth persisting, so a burst of volume ticks becomes one write.
        while shutdown_reply.is_none() {
            let msg = match rx.try_recv() {
                Ok(msg) => msg,
                Err(_) => break,
            };
            match msg {
                WriteRequest::SaveSession(s) => pending_session = Some(s),
                WriteRequest::RecordPlay(r) => {
                    capture_write_error(
                        &mut first_write_error,
                        "record play",
                        write_play(&conn, &r),
                    );
                    maintain_history_limit(&conn, &mut plays_since_prune);
                }
                WriteRequest::Shutdown(reply) => shutdown_reply = Some(reply),
            }
        }
        if let Some(snapshot) = pending_session.take() {
            capture_write_error(
                &mut first_write_error,
                "save session",
                retry_busy(|| save_session(&conn, &snapshot)),
            );
        }
        if let Some(reply) = shutdown_reply {
            let result = first_write_error.take().map_or(Ok(()), Err);
            let _ = reply.send(result);
            break;
        }
    }

    // A disconnected owner still gets best-effort durability. This path is
    // used if the audio thread exits unexpectedly rather than via Shutdown.
    if let Some(snapshot) = pending_session {
        capture_write_error(
            &mut first_write_error,
            "save final session",
            retry_busy(|| save_session(&conn, &snapshot)),
        );
    }
}

fn capture_write_error<T>(
    first_error: &mut Option<String>,
    operation: &str,
    result: rusqlite::Result<T>,
) {
    if let Err(error) = result {
        let message = format!("Failed to {operation}: {error}");
        log::warn!("{message}");
        if first_error.is_none() {
            *first_error = Some(message);
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

fn write_play(conn: &Connection, record: &PlayRecord) -> rusqlite::Result<()> {
    retry_busy(|| {
        conn.execute(
            "INSERT OR IGNORE INTO play_history (track_id, started_at, played_ms, completed) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                record.track_id,
                record.started_at,
                record.played_ms,
                record.completed as i64
            ],
        )
    })
    .map(|_| ())
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
        )
        .unwrap();
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

    #[test]
    fn shutdown_flushes_session_and_play_before_joining() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "sparkle-db-writer-shutdown-{}-{unique}.db",
            std::process::id()
        ));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                CREATE TABLE play_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    track_id INTEGER NOT NULL,
                    started_at INTEGER NOT NULL,
                    played_ms INTEGER NOT NULL,
                    completed INTEGER NOT NULL DEFAULT 0
                );
                CREATE UNIQUE INDEX idx_play_history_event
                    ON play_history(track_id, started_at, played_ms);",
            )
            .unwrap();
        }

        let writer = DbWriter::new(path.clone());
        let snapshot = SessionSnapshot {
            queue: vec![42],
            queue_index: Some(0),
            position_ms: 61_000,
            ..Default::default()
        };
        writer.save_session(snapshot);
        writer.record_play(PlayRecord {
            track_id: 42,
            started_at: 1_700_000_000,
            played_ms: 61_000,
            completed: false,
        });

        writer.shutdown().unwrap();

        let conn = Connection::open(&path).unwrap();
        let restored = crate::settings::load_session(&conn).unwrap();
        assert_eq!(restored.queue, vec![42]);
        assert_eq!(restored.position_ms, 61_000);
        let played_ms: i64 = conn
            .query_row("SELECT played_ms FROM play_history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(played_ms, 61_000);
        drop(conn);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }
}
