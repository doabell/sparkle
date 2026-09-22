use crate::analytics::{now_epoch_ms, ListenRecord, PlaybackEventRecord};
use crate::models::RepeatMode;
use crate::settings::{save_session, SessionSnapshot};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MAX_PLAYBACK_EVENTS: i64 = 50_000;
pub const PLAYBACK_EVENT_RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1_000;
const ANALYTICS_PRUNE_INTERVAL: usize = 512;
const WRITE_QUEUE_CAPACITY: usize = 1024;

#[derive(Clone, Debug, Default, Serialize)]
pub struct WriterHealth {
    pub running: bool,
    pub pending_writes: usize,
    pub oldest_pending_age_ms: i64,
    pub last_success_at_ms: Option<i64>,
    pub last_error_at_ms: Option<i64>,
    pub last_error: Option<String>,
    pub failed_writes: u64,
    pub dropped_writes: u64,
    pub consecutive_failures: u64,
}

#[derive(Default)]
struct WriterStatus {
    health: WriterHealth,
    next_id: u64,
    pending: BTreeMap<u64, i64>,
    delivery_failure_logged: bool,
}

#[derive(Clone, Default)]
pub struct WriterMonitor(Arc<Mutex<WriterStatus>>);

impl WriterMonitor {
    pub fn snapshot(&self) -> WriterHealth {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut health = state.health.clone();
        health.pending_writes = state.pending.len();
        health.oldest_pending_age_ms = state
            .pending
            .values()
            .next()
            .map(|queued| now_epoch_ms().saturating_sub(*queued).max(0))
            .unwrap_or(0);
        health
    }

    fn reserve(&self) -> u64 {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let id = state.next_id;
        state.next_id += 1;
        state.pending.insert(id, now_epoch_ms());
        id
    }

    fn complete(&self, id: u64) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending
            .remove(&id);
    }

    fn delivery_failed(&self, id: u64, reason: &str) {
        let first = {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            state.pending.remove(&id);
            state.health.dropped_writes += 1;
            state.health.last_error_at_ms = Some(now_epoch_ms());
            state.health.last_error = Some(reason.into());
            let first = !state.delivery_failure_logged;
            state.delivery_failure_logged = true;
            first
        };
        if first {
            log::error!(target: "sparkle::analytics::writer", "event=delivery_failed reason={reason}");
        } else {
            log::debug!(target: "sparkle::analytics::writer", "event=delivery_failed reason={reason}");
        }
    }

    fn result<T>(&self, operation: &str, result: &rusqlite::Result<T>) {
        let (first_failure, recovered) = {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            let health = &mut state.health;
            match result {
                Ok(_) => {
                    let recovered = health.consecutive_failures;
                    health.consecutive_failures = 0;
                    health.last_success_at_ms = Some(now_epoch_ms());
                    (false, recovered)
                }
                Err(error) => {
                    let message = crate::logging::sanitize(&format!("{operation}: {error}"));
                    let first = health.consecutive_failures == 0
                        || health.last_error.as_ref() != Some(&message);
                    health.failed_writes += 1;
                    health.consecutive_failures += 1;
                    health.last_error_at_ms = Some(now_epoch_ms());
                    health.last_error = Some(message);
                    (first, 0)
                }
            }
        };
        if let Err(error) = result {
            log::log!(target: "sparkle::analytics::writer", if first_failure { log::Level::Error } else { log::Level::Debug },
                "event=write_failed operation={operation:?} error={error}");
        } else if recovered > 0 {
            log::info!(target: "sparkle::analytics::writer", "event=writer_recovered failed_writes={recovered}");
        }
    }
}

struct QueuedWrite {
    id: u64,
    request: WriteRequest,
}

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
    tx: Option<mpsc::SyncSender<QueuedWrite>>,
    worker: Option<JoinHandle<()>>,
    monitor: WriterMonitor,
}

impl DbWriter {
    pub fn new(db_path: PathBuf) -> Self {
        let (tx, rx) = mpsc::sync_channel(WRITE_QUEUE_CAPACITY);
        let monitor = WriterMonitor::default();
        let worker_monitor = monitor.clone();
        let worker = thread::spawn(move || writer_loop(db_path, rx, worker_monitor));
        Self {
            tx: Some(tx),
            worker: Some(worker),
            monitor,
        }
    }

    pub fn save_session(&self, snapshot: SessionSnapshot) {
        self.enqueue(WriteRequest::SaveSession(snapshot));
    }

    pub fn upsert_listen(&self, record: ListenRecord) {
        self.enqueue(WriteRequest::UpsertListen(record));
    }

    pub fn record_event(&self, event: PlaybackEventRecord) {
        self.enqueue(WriteRequest::RecordEvent(event));
    }

    pub fn monitor(&self) -> WriterMonitor {
        self.monitor.clone()
    }

    fn enqueue(&self, request: WriteRequest) {
        let id = self.monitor.reserve();
        let result = self
            .tx
            .as_ref()
            .map(|tx| tx.try_send(QueuedWrite { id, request }));
        match result {
            Some(Ok(())) => {}
            Some(Err(mpsc::TrySendError::Full(_))) => {
                self.monitor.delivery_failed(id, "queue_full")
            }
            _ => self.monitor.delivery_failed(id, "writer_stopped"),
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
        let id = self.monitor.reserve();
        if tx
            .send(QueuedWrite {
                id,
                request: WriteRequest::Shutdown(reply_tx),
            })
            .is_err()
        {
            self.monitor.delivery_failed(id, "writer_stopped");
            return Err("database writer stopped before shutdown".to_string());
        }
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

fn writer_loop(db_path: PathBuf, rx: mpsc::Receiver<QueuedWrite>, monitor: WriterMonitor) {
    // The guard also marks a panicked/disconnected writer as unavailable.
    struct RunningGuard(WriterMonitor);
    impl Drop for RunningGuard {
        fn drop(&mut self) {
            let mut state = self.0 .0.lock().unwrap_or_else(|e| e.into_inner());
            state.health.running = false;
            state.health.dropped_writes += state.pending.len() as u64;
            state.pending.clear();
        }
    }
    let _running = RunningGuard(monitor.clone());
    let conn = match crate::db::open_connection(&db_path) {
        Ok(conn) => conn,
        Err(error) => {
            monitor.result::<()>("open database", &Err(error));
            return;
        }
    };
    monitor
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .health
        .running = true;
    log::debug!(target: "sparkle::analytics::writer", "event=writer_started capacity={WRITE_QUEUE_CAPACITY}");
    let mut writes_since_prune = 0;
    let mut first_write_error = None;
    if let Err(error) = prune_analytics(&conn) {
        log::warn!(target: "sparkle::analytics::writer", "event=initial_prune_failed error={error}");
    }
    while let Ok(first) = rx.recv() {
        let mut pending_session: Option<(u64, SessionSnapshot)> = None;
        let mut shutdown_reply = None;
        // Bound each batch so session snapshots cannot starve behind new traffic.
        let batch = std::iter::once(first).chain(rx.try_iter().take(63));
        for queued in batch {
            match queued.request {
                WriteRequest::SaveSession(snapshot) => {
                    if let Some((old_id, _)) = pending_session.replace((queued.id, snapshot)) {
                        monitor.complete(old_id);
                    }
                }
                WriteRequest::UpsertListen(record) => {
                    let result = write_listen(&conn, &record);
                    let persisted = result.is_ok();
                    capture_write_error(
                        &monitor,
                        &mut first_write_error,
                        &format!(
                            "upsert_listen listen_id={} track_id={}",
                            record.id, record.track_id
                        ),
                        result,
                    );
                    if persisted {
                        log::trace!(target: "sparkle::analytics::writer",
                            "event=listen_persisted listen_id={} track_id={} finalized={} listened_ms={}",
                            record.id, record.track_id, record.finalized, record.listened_ms);
                    }
                    monitor.complete(queued.id);
                    maintain_analytics_limits(&conn, &mut writes_since_prune);
                }
                WriteRequest::RecordEvent(event) => {
                    let result = write_event(&conn, &event);
                    let persisted = result.is_ok();
                    capture_write_error(
                        &monitor,
                        &mut first_write_error,
                        &format!("record_event event_id={}", event.id),
                        result,
                    );
                    if persisted {
                        log::trace!(target: "sparkle::analytics::writer",
                            "event=trace_persisted event_id={} event_type={} listen_id={}",
                            event.id, event.event.kind().as_str(), event.listen_id.as_deref().unwrap_or("none"));
                    }
                    monitor.complete(queued.id);
                    maintain_analytics_limits(&conn, &mut writes_since_prune);
                }
                WriteRequest::Shutdown(reply) => {
                    monitor.complete(queued.id);
                    shutdown_reply = Some(reply);
                    break;
                }
            }
        }
        if let Some((id, snapshot)) = pending_session {
            capture_write_error(
                &monitor,
                &mut first_write_error,
                "save session",
                retry_busy(|| save_session(&conn, &snapshot)),
            );
            monitor.complete(id);
        }
        if let Some(reply) = shutdown_reply {
            let health = monitor.snapshot();
            let result = first_write_error.map_or_else(
                || {
                    if health.dropped_writes > 0 {
                        Err(format!(
                            "{} database writes were dropped",
                            health.dropped_writes
                        ))
                    } else {
                        Ok(())
                    }
                },
                Err,
            );
            log::debug!(target: "sparkle::analytics::writer", "event=writer_stopped success={}", result.is_ok());
            let _ = reply.send(result);
            break;
        }
    }
}

fn capture_write_error<T>(
    monitor: &WriterMonitor,
    first_error: &mut Option<String>,
    operation: &str,
    result: rusqlite::Result<T>,
) {
    monitor.result(operation, &result);
    if let Err(error) = result {
        if first_error.is_none() {
            *first_error = Some(format!("Failed to {operation}: {error}"));
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
        Ok(events) if events > 0 => log::debug!(target: "sparkle::analytics::writer",
            "event=retention_pruned playback_events={events}"),
        Ok(_) => {}
        Err(error) => {
            log::warn!(target: "sparkle::analytics::writer", "event=retention_prune_failed error={error}")
        }
    }
}

fn prune_analytics(conn: &Connection) -> rusqlite::Result<usize> {
    prune_events_to(
        conn,
        MAX_PLAYBACK_EVENTS,
        now_epoch_ms() - PLAYBACK_EVENT_RETENTION_MS,
    )
}

// Listening history is user data and has no automatic retention limit.
fn prune_events_to(conn: &Connection, limit: i64, oldest_ms: i64) -> rusqlite::Result<usize> {
    retry_busy(|| {
        conn.execute(
            "DELETE FROM playback_events WHERE occurred_at_ms < ?1 OR id IN (
         SELECT id FROM playback_events ORDER BY occurred_at_ms DESC, id DESC LIMIT -1 OFFSET ?2)",
            rusqlite::params![oldest_ms, limit],
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
                context_id, queue_index, play_order_index, queue_length, shuffle, repeat_mode,
                run_id, command_id, target_track_id, command, failure_stage
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22
             )",
            rusqlite::params![
                event.id,
                event.listen_id,
                event.session_id,
                event.occurred_at_ms,
                event.event.kind().as_str(),
                event.source.as_str(),
                event.event.reason(),
                event.track_id,
                event.position_ms,
                event.event.target_position_ms(),
                event.context.kind,
                event.context.id,
                event.queue_index.map(|index| index as i64),
                event.play_order_index.map(|index| index as i64),
                event.queue_length as i64,
                event.shuffle as i64,
                repeat_mode_name(event.repeat_mode),
                event.run_id,
                event.command_id,
                event.event.target_track_id(),
                event.event.command(),
                event.event.failure_stage(),
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
