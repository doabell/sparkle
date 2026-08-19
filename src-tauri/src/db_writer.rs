use crate::analytics::{ListenRecord, PlaybackEventRecord};
use crate::models::RepeatMode;
use crate::settings::{save_session, SessionSnapshot};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MAX_LISTENS: i64 = 250_000;
const MAX_PLAYBACK_EVENTS: i64 = 750_000;
const ANALYTICS_PRUNE_INTERVAL: usize = 512;

enum WriteRequest {
    SaveSession(SessionSnapshot),
    UpsertListen(ListenRecord),
    RecordEvent(PlaybackEventRecord),
    Shutdown(mpsc::Sender<Result<(), String>>),
}

/// Serializes the audio engine's database writes onto its own thread and
/// connection. Playback code must never block on SQLite: session snapshots
/// are coalesced (only the newest is written) and analytics records are
/// queued, so a busy database can never stall audio.
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

    pub fn upsert_listen(&self, record: ListenRecord) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(WriteRequest::UpsertListen(record));
        }
    }

    pub fn record_event(&self, event: PlaybackEventRecord) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(WriteRequest::RecordEvent(event));
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
            log::error!(target: "sparkle::analytics::writer", "event=database_open_failed error={e}");
            return;
        }
    };
    log::debug!(target: "sparkle::analytics::writer", "event=writer_started");
    let mut pending_session: Option<SessionSnapshot> = None;
    let mut analytics_writes_since_prune = 0;
    let mut first_write_error: Option<String> = None;
    if let Err(error) = prune_analytics(&conn) {
        log::warn!(target: "sparkle::analytics::writer", "event=initial_prune_failed error={error}");
    }
    loop {
        let mut shutdown_reply = match rx.recv() {
            Ok(WriteRequest::SaveSession(s)) => {
                pending_session = Some(s);
                None
            }
            Ok(WriteRequest::UpsertListen(record)) => {
                let finalized = record.finalized;
                capture_write_error(
                    &mut first_write_error,
                    "upsert listen",
                    write_listen(&conn, &record),
                );
                if finalized {
                    log::debug!(
                        target: "sparkle::analytics::writer",
                        "event=listen_persisted listen_id={} session_id={} track_id={} listened_ms={} meaningful={} completed={} end_reason={}",
                        record.id,
                        record.session_id,
                        record.track_id,
                        record.listened_ms,
                        record.meaningful,
                        record.completed,
                        record.end_reason.map(|reason| reason.as_str()).unwrap_or("none")
                    );
                } else {
                    log::trace!(
                        target: "sparkle::analytics::writer",
                        "event=listen_checkpointed listen_id={} track_id={} listened_ms={} position_ms={}",
                        record.id,
                        record.track_id,
                        record.listened_ms,
                        record.end_position_ms
                    );
                }
                maintain_analytics_limits(&conn, &mut analytics_writes_since_prune);
                None
            }
            Ok(WriteRequest::RecordEvent(event)) => {
                capture_write_error(
                    &mut first_write_error,
                    "record playback event",
                    write_event(&conn, &event),
                );
                log::trace!(
                    target: "sparkle::analytics::writer",
                    "event=trace_persisted event_id={} event_type={} listen_id={} track_id={}",
                    event.id,
                    event.kind.as_str(),
                    event.listen_id.as_deref().unwrap_or("none"),
                    event.track_id.map(|id| id.to_string()).unwrap_or_else(|| "none".to_string())
                );
                maintain_analytics_limits(&conn, &mut analytics_writes_since_prune);
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
                WriteRequest::UpsertListen(record) => {
                    capture_write_error(
                        &mut first_write_error,
                        "upsert listen",
                        write_listen(&conn, &record),
                    );
                    maintain_analytics_limits(&conn, &mut analytics_writes_since_prune);
                }
                WriteRequest::RecordEvent(event) => {
                    capture_write_error(
                        &mut first_write_error,
                        "record playback event",
                        write_event(&conn, &event),
                    );
                    maintain_analytics_limits(&conn, &mut analytics_writes_since_prune);
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
            log::debug!(
                target: "sparkle::analytics::writer",
                "event=writer_stopped success={}",
                result.is_ok()
            );
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
        log::error!(target: "sparkle::analytics::writer", "event=write_failed operation={operation:?} error={error}");
        if first_error.is_none() {
            *first_error = Some(message);
        }
    }
}

fn maintain_analytics_limits(conn: &Connection, writes_since_prune: &mut usize) {
    *writes_since_prune += 1;
    if *writes_since_prune < ANALYTICS_PRUNE_INTERVAL {
        return;
    }
    *writes_since_prune = 0;
    match prune_analytics(conn) {
        Ok((listens, events)) if listens > 0 || events > 0 => log::debug!(
            target: "sparkle::analytics::writer",
            "event=retention_pruned listens={listens} playback_events={events}"
        ),
        Ok(_) => {}
        Err(error) => {
            log::warn!(target: "sparkle::analytics::writer", "event=retention_prune_failed error={error}");
        }
    }
}

fn prune_analytics(conn: &Connection) -> rusqlite::Result<(usize, usize)> {
    Ok((
        prune_listens_to(conn, MAX_LISTENS)?,
        prune_events_to(conn, MAX_PLAYBACK_EVENTS)?,
    ))
}

fn prune_listens_to(conn: &Connection, limit: i64) -> rusqlite::Result<usize> {
    retry_busy(|| {
        conn.execute(
            "DELETE FROM listens WHERE id IN (\
             SELECT id FROM listens WHERE finalized = 1 \
             ORDER BY started_at_ms DESC, id DESC LIMIT -1 OFFSET ?1\
             )",
            [limit],
        )
    })
}

fn prune_events_to(conn: &Connection, limit: i64) -> rusqlite::Result<usize> {
    retry_busy(|| {
        conn.execute(
            "DELETE FROM playback_events WHERE id IN (\
             SELECT id FROM playback_events \
             ORDER BY occurred_at_ms DESC, id DESC LIMIT -1 OFFSET ?1\
             )",
            [limit],
        )
    })
}

fn write_listen(conn: &Connection, record: &ListenRecord) -> rusqlite::Result<()> {
    retry_busy(|| {
        conn.execute(
            "INSERT INTO listens (
                id, session_id, track_id, started_at_ms, ended_at_ms,
                last_activity_at_ms, start_position_ms, end_position_ms,
                duration_ms, listened_ms, meaningful, completed, finalized,
                start_source, start_reason, end_reason, context_type, context_id,
                queue_index, play_order_index, queue_length, shuffle, repeat_mode
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23
             )
             ON CONFLICT(id) DO UPDATE SET
                ended_at_ms = excluded.ended_at_ms,
                last_activity_at_ms = excluded.last_activity_at_ms,
                end_position_ms = excluded.end_position_ms,
                duration_ms = excluded.duration_ms,
                listened_ms = excluded.listened_ms,
                meaningful = excluded.meaningful,
                completed = excluded.completed,
                finalized = excluded.finalized,
                end_reason = excluded.end_reason",
            rusqlite::params![
                record.id,
                record.session_id,
                record.track_id,
                record.started_at_ms,
                record.ended_at_ms,
                record.last_activity_at_ms,
                record.start_position_ms,
                record.end_position_ms,
                record.duration_ms,
                record.listened_ms,
                record.meaningful as i64,
                record.completed as i64,
                record.finalized as i64,
                record.start_source.as_str(),
                record.start_reason.as_str(),
                record.end_reason.map(|reason| reason.as_str()),
                record.context.kind,
                record.context.id,
                record.queue_index.map(|index| index as i64),
                record.play_order_index.map(|index| index as i64),
                record.queue_length as i64,
                record.shuffle as i64,
                repeat_mode_name(record.repeat_mode),
            ],
        )
    })
    .map(|_| ())
}

fn write_event(conn: &Connection, event: &PlaybackEventRecord) -> rusqlite::Result<()> {
    retry_busy(|| {
        conn.execute(
            "INSERT OR IGNORE INTO playback_events (
                id, listen_id, session_id, occurred_at_ms, event_type, source,
                reason, track_id, position_ms, target_position_ms, context_type,
                context_id, queue_index, play_order_index, queue_length, shuffle, repeat_mode
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, ?15, ?16, ?17
             )",
            rusqlite::params![
                event.id,
                event.listen_id,
                event.session_id,
                event.occurred_at_ms,
                event.kind.as_str(),
                event.source.as_str(),
                event.reason,
                event.track_id,
                event.position_ms,
                event.target_position_ms,
                event.context.kind,
                event.context.id,
                event.queue_index.map(|index| index as i64),
                event.play_order_index.map(|index| index as i64),
                event.queue_length as i64,
                event.shuffle as i64,
                repeat_mode_name(event.repeat_mode),
            ],
        )
    })
    .map(|_| ())
}

fn repeat_mode_name(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "off",
        RepeatMode::All => "all",
        RepeatMode::One => "one",
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
    use crate::analytics::{
        ListenEndReason, ListenStartReason, PlaybackContext, PlaybackEventKind, PlaybackSource,
    };

    fn create_test_schema(conn: &Connection) {
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );
             CREATE TABLE tracks (id INTEGER PRIMARY KEY);
             INSERT INTO tracks (id) VALUES (7), (42);
             CREATE TABLE listens (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
                started_at_ms INTEGER NOT NULL,
                ended_at_ms INTEGER,
                last_activity_at_ms INTEGER NOT NULL,
                start_position_ms INTEGER NOT NULL,
                end_position_ms INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                listened_ms INTEGER NOT NULL,
                meaningful INTEGER NOT NULL,
                completed INTEGER NOT NULL,
                finalized INTEGER NOT NULL,
                start_source TEXT NOT NULL,
                start_reason TEXT NOT NULL,
                end_reason TEXT,
                context_type TEXT NOT NULL,
                context_id TEXT,
                queue_index INTEGER,
                play_order_index INTEGER,
                queue_length INTEGER NOT NULL,
                shuffle INTEGER NOT NULL,
                repeat_mode TEXT NOT NULL
             );
             CREATE TABLE playback_events (
                id TEXT PRIMARY KEY,
                listen_id TEXT REFERENCES listens(id) ON DELETE CASCADE,
                session_id TEXT,
                occurred_at_ms INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                source TEXT NOT NULL,
                reason TEXT,
                track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE,
                position_ms INTEGER,
                target_position_ms INTEGER,
                context_type TEXT NOT NULL,
                context_id TEXT,
                queue_index INTEGER,
                play_order_index INTEGER,
                queue_length INTEGER NOT NULL,
                shuffle INTEGER NOT NULL,
                repeat_mode TEXT NOT NULL
             );",
        )
        .unwrap();
    }

    fn sample_listen(id: &str, started_at_ms: i64, finalized: bool) -> ListenRecord {
        ListenRecord {
            id: id.to_string(),
            session_id: "session-1".to_string(),
            track_id: 7,
            started_at_ms,
            ended_at_ms: finalized.then_some(started_at_ms + 123_000),
            last_activity_at_ms: started_at_ms + 123_000,
            start_position_ms: 0,
            end_position_ms: 123_000,
            duration_ms: 180_000,
            listened_ms: 123_000,
            meaningful: true,
            completed: false,
            finalized,
            start_source: PlaybackSource::Ui,
            start_reason: ListenStartReason::QueueStarted,
            end_reason: finalized.then_some(ListenEndReason::ManualNext),
            context: PlaybackContext {
                kind: "album".to_string(),
                id: Some("9".to_string()),
            },
            queue_index: Some(0),
            play_order_index: Some(0),
            queue_length: 10,
            shuffle: false,
            repeat_mode: RepeatMode::Off,
        }
    }

    fn sample_event(id: &str, listen_id: &str, occurred_at_ms: i64) -> PlaybackEventRecord {
        PlaybackEventRecord {
            id: id.to_string(),
            listen_id: Some(listen_id.to_string()),
            session_id: Some("session-1".to_string()),
            occurred_at_ms,
            kind: PlaybackEventKind::ListenEnded,
            source: PlaybackSource::Ui,
            reason: Some("manual_next".to_string()),
            track_id: Some(7),
            position_ms: Some(123_000),
            target_position_ms: None,
            context: PlaybackContext::default(),
            queue_index: Some(0),
            play_order_index: Some(0),
            queue_length: 10,
            shuffle: false,
            repeat_mode: RepeatMode::Off,
        }
    }

    #[test]
    fn listen_checkpoint_is_updated_by_final_record() {
        let conn = Connection::open_in_memory().unwrap();
        create_test_schema(&conn);
        let mut listen = sample_listen("listen-1", 1_700_000_000_000, false);
        listen.listened_ms = 30_000;
        listen.end_position_ms = 30_000;
        write_listen(&conn, &listen).unwrap();
        listen.listened_ms = 123_000;
        listen.end_position_ms = 123_000;
        listen.ended_at_ms = Some(1_700_000_123_000);
        listen.finalized = true;
        listen.end_reason = Some(ListenEndReason::ManualNext);
        write_listen(&conn, &listen).unwrap();
        let (listened_ms, finalized, reason): (i64, i64, String) = conn
            .query_row(
                "SELECT listened_ms, finalized, end_reason FROM listens",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(listened_ms, 123_000);
        assert_eq!(finalized, 1);
        assert_eq!(reason, "manual_next");
    }

    #[test]
    fn semantic_event_insert_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        create_test_schema(&conn);
        write_listen(&conn, &sample_listen("listen-1", 1_700_000_000_000, true)).unwrap();
        let event = sample_event("event-1", "listen-1", 1_700_000_123_000);
        write_event(&conn, &event).unwrap();
        write_event(&conn, &event).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM playback_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
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
    fn retention_keeps_newest_finalized_listens_and_cascades_events() {
        let conn = Connection::open_in_memory().unwrap();
        create_test_schema(&conn);
        for index in 1..=5 {
            let listen_id = format!("listen-{index}");
            write_listen(
                &conn,
                &sample_listen(&listen_id, 1_700_000_000_000 + index, true),
            )
            .unwrap();
            write_event(
                &conn,
                &sample_event(&format!("event-{index}"), &listen_id, index),
            )
            .unwrap();
        }

        assert_eq!(prune_listens_to(&conn, 3).unwrap(), 2);
        let timestamps = conn
            .prepare("SELECT started_at_ms FROM listens ORDER BY started_at_ms")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<i64>, _>>()
            .unwrap();
        assert_eq!(
            timestamps,
            vec![1_700_000_000_003, 1_700_000_000_004, 1_700_000_000_005]
        );
        let events: i64 = conn
            .query_row("SELECT COUNT(*) FROM playback_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(events, 3);
    }

    #[test]
    fn shutdown_flushes_session_listen_and_event_before_joining() {
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
            create_test_schema(&conn);
        }

        let writer = DbWriter::new(path.clone());
        let snapshot = SessionSnapshot {
            queue: vec![42],
            queue_index: Some(0),
            position_ms: 61_000,
            ..Default::default()
        };
        writer.save_session(snapshot);
        let mut listen = sample_listen("listen-42", 1_700_000_000_000, true);
        listen.track_id = 42;
        listen.listened_ms = 61_000;
        writer.upsert_listen(listen);
        let mut event = sample_event("event-42", "listen-42", 1_700_000_061_000);
        event.track_id = Some(42);
        writer.record_event(event);

        writer.shutdown().unwrap();

        let conn = Connection::open(&path).unwrap();
        let restored = crate::settings::load_session(&conn).unwrap();
        assert_eq!(restored.queue, vec![42]);
        assert_eq!(restored.position_ms, 61_000);
        let listened_ms: i64 = conn
            .query_row("SELECT listened_ms FROM listens", [], |row| row.get(0))
            .unwrap();
        assert_eq!(listened_ms, 61_000);
        let events: i64 = conn
            .query_row("SELECT COUNT(*) FROM playback_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(events, 1);
        drop(conn);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }
}
