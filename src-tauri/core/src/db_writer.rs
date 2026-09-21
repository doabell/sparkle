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
#[path = "tests/db_writer.rs"]
mod tests;
