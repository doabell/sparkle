use crate::models::LoudnessStatus;
use ebur128::{EbuR128, Mode};
use rodio::{Decoder, Source};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

pub const TARGET_LUFS: f64 = -18.0;
pub const TRUE_PEAK_CEILING_DBTP: f64 = -1.0;
pub const ANALYZER_VERSION: i64 = 1;
pub const NEXT_UP_COUNT: usize = 3;

const MAX_ATTEMPTS: i64 = 3;
const ANALYSIS_CHUNK_FRAMES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GainAvailability {
    Ready(f64),
    Pending,
}

#[derive(Clone)]
pub struct LoudnessController {
    inner: Arc<Inner>,
    worker: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

struct Inner {
    app: AppHandle,
    db_path: PathBuf,
    scheduler: Mutex<SchedulerState>,
    wake: Condvar,
}

struct SchedulerState {
    enabled: bool,
    shutdown: bool,
    priority: Vec<i64>,
    generation: u64,
    current_track_id: Option<i64>,
}

#[derive(Clone, Debug)]
struct Candidate {
    track_id: i64,
    file_path: String,
    file_mtime: i64,
    file_size_bytes: Option<i64>,
    previous_attempts: i64,
}

#[derive(Debug)]
enum AnalysisResult {
    Complete {
        integrated_lufs: f64,
        true_peak_dbtp: f64,
        gain_db: f64,
    },
    PeakOnly {
        true_peak_dbtp: f64,
        gain_db: f64,
    },
    Silent,
}

#[derive(Debug)]
enum AnalysisFailure {
    Cancelled,
    Failed { code: &'static str, detail: String },
}

impl LoudnessController {
    pub fn new(app: AppHandle, db_path: PathBuf, enabled: bool) -> Self {
        let inner = Arc::new(Inner {
            app,
            db_path,
            scheduler: Mutex::new(SchedulerState {
                enabled,
                shutdown: false,
                priority: Vec::new(),
                generation: 0,
                current_track_id: None,
            }),
            wake: Condvar::new(),
        });
        let worker_inner = inner.clone();
        let worker = std::thread::Builder::new()
            .name("sparkle-loudness".to_string())
            .spawn(move || worker_loop(worker_inner))
            .expect("failed to start loudness scanner");
        let controller = Self {
            inner,
            worker: Arc::new(Mutex::new(Some(worker))),
        };
        controller.emit_status();
        controller
    }

    pub fn set_enabled(&self, enabled: bool) {
        {
            let mut scheduler = lock_scheduler(&self.inner);
            if scheduler.enabled == enabled {
                return;
            }
            scheduler.enabled = enabled;
            scheduler.generation = scheduler.generation.wrapping_add(1);
        }
        self.inner.wake.notify_all();
        self.emit_status();
        log::info!(
            target: "sparkle::loudness",
            "event=setting_changed enabled={enabled}"
        );
    }

    /// Replaces the urgent work list. Callers pass the current track followed
    /// by at most the next three tracks in actual play order.
    pub fn prioritize(&self, track_ids: Vec<i64>) {
        let mut seen = HashSet::new();
        let priority: Vec<i64> = track_ids
            .into_iter()
            .filter(|track_id| seen.insert(*track_id))
            .take(NEXT_UP_COUNT + 1)
            .collect();
        {
            let mut scheduler = lock_scheduler(&self.inner);
            if scheduler.priority == priority {
                return;
            }
            scheduler.priority = priority;
            scheduler.generation = scheduler.generation.wrapping_add(1);
        }
        self.inner.wake.notify_all();
        // Queue changes run on the audio command worker. Status aggregation
        // opens SQLite and can wait on a concurrent writer, so leave both the
        // work query and status event to the scanner thread we just woke.
    }

    pub fn refresh_library(&self) {
        self.inner.wake.notify_all();
        self.emit_status();
    }

    pub fn status(&self) -> Result<LoudnessStatus, String> {
        status_for_inner(&self.inner)
    }

    pub fn rescan_all(&self) -> Result<(), String> {
        let conn = crate::db::open_connection(&self.inner.db_path).map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM track_loudness", [])
            .map_err(|e| e.to_string())?;
        {
            let mut scheduler = lock_scheduler(&self.inner);
            scheduler.generation = scheduler.generation.wrapping_add(1);
        }
        self.inner.wake.notify_all();
        self.emit_status();
        log::info!(target: "sparkle::loudness", "event=full_rescan_requested");
        Ok(())
    }

    pub fn shutdown(&self) -> Result<(), String> {
        let mut worker = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        let Some(handle) = worker.take() else {
            return Ok(());
        };
        {
            let mut scheduler = lock_scheduler(&self.inner);
            scheduler.shutdown = true;
            scheduler.generation = scheduler.generation.wrapping_add(1);
        }
        self.inner.wake.notify_all();
        drop(worker);
        handle
            .join()
            .map_err(|_| "loudness scanner panicked during shutdown".to_string())
    }

    fn emit_status(&self) {
        emit_status(&self.inner);
    }
}

/// Returns the gain for a valid measurement, unity for a recorded failure,
/// or Pending when the exact file revision still needs analysis.
pub fn gain_for_track(conn: &Connection, track_id: i64) -> Result<GainAvailability, String> {
    let row: Option<(String, Option<f64>)> = conn
        .query_row(
            "SELECT l.status, l.gain_db
             FROM tracks t
             JOIN track_loudness l ON l.track_id = t.id
             WHERE t.id = ?1
               AND l.analyzer_version = ?2
               AND l.analyzed_file_mtime = t.file_mtime
               AND COALESCE(l.analyzed_file_size_bytes, -1)
                   = COALESCE(t.file_size_bytes, -1)",
            params![track_id, ANALYZER_VERSION],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(match row {
        Some((status, _)) if status == "failed" => GainAvailability::Ready(0.0),
        Some((_, Some(gain))) if gain.is_finite() => GainAvailability::Ready(gain.min(0.0)),
        Some((status, _)) if status == "silent" => GainAvailability::Ready(0.0),
        _ => GainAvailability::Pending,
    })
}

pub fn effective_gain_db(integrated_lufs: f64, true_peak_dbtp: f64) -> f64 {
    let loudness_gain = TARGET_LUFS - integrated_lufs;
    let peak_safe_gain = TRUE_PEAK_CEILING_DBTP - true_peak_dbtp;
    loudness_gain.min(peak_safe_gain).min(0.0)
}

fn worker_loop(inner: Arc<Inner>) {
    let conn = match crate::db::open_connection(&inner.db_path) {
        Ok(conn) => conn,
        Err(error) => {
            log::error!(
                target: "sparkle::loudness",
                "event=worker_database_open_failed error={error}"
            );
            return;
        }
    };

    loop {
        let (generation, priority) = {
            let mut scheduler = lock_scheduler(&inner);
            while !scheduler.shutdown && !scheduler.enabled {
                scheduler = inner
                    .wake
                    .wait(scheduler)
                    .unwrap_or_else(|e| e.into_inner());
            }
            if scheduler.shutdown {
                return;
            }
            (scheduler.generation, scheduler.priority.clone())
        };

        let candidate = match next_candidate(&conn, &priority) {
            Ok(candidate) => candidate,
            Err(error) => {
                log::warn!(
                    target: "sparkle::loudness",
                    "event=work_query_failed error={error}"
                );
                wait_for_work(&inner, Duration::from_secs(2));
                continue;
            }
        };

        let Some(candidate) = candidate else {
            emit_status(&inner);
            wait_for_work(&inner, Duration::from_secs(30));
            continue;
        };

        {
            let mut scheduler = lock_scheduler(&inner);
            if scheduler.shutdown {
                return;
            }
            scheduler.current_track_id = Some(candidate.track_id);
        }
        emit_status(&inner);
        let prioritized = priority.contains(&candidate.track_id);
        let started_at = Instant::now();
        if prioritized {
            log::info!(
                target: "sparkle::loudness",
                "event=analysis_started track_id={} prioritized=true file_size_bytes={}",
                candidate.track_id,
                candidate.file_size_bytes.unwrap_or(-1)
            );
        } else {
            log::debug!(
                target: "sparkle::loudness",
                "event=analysis_started track_id={} prioritized=false file_size_bytes={}",
                candidate.track_id,
                candidate.file_size_bytes.unwrap_or(-1)
            );
        }

        let result = analyze_candidate(&candidate, || {
            let scheduler = lock_scheduler(&inner);
            scheduler.shutdown
                || !scheduler.enabled
                || (scheduler.generation != generation
                    && !scheduler.priority.contains(&candidate.track_id))
        });
        let elapsed_ms = started_at.elapsed().as_millis();

        match result {
            Ok(result) => {
                if let Err(error) = persist_success(&conn, &candidate, result) {
                    log::warn!(
                        target: "sparkle::loudness",
                        "event=analysis_store_failed track_id={} error={error}",
                        candidate.track_id
                    );
                } else {
                    log::info!(
                        target: "sparkle::loudness",
                        "event=analysis_completed track_id={} elapsed_ms={elapsed_ms}",
                        candidate.track_id,
                    );
                }
            }
            Err(AnalysisFailure::Cancelled) => {
                log::debug!(
                    target: "sparkle::loudness",
                    "event=analysis_preempted track_id={} elapsed_ms={elapsed_ms}",
                    candidate.track_id,
                );
            }
            Err(AnalysisFailure::Failed { code, detail }) => {
                if let Err(error) = persist_failure(&conn, &candidate, code) {
                    log::warn!(
                        target: "sparkle::loudness",
                        "event=analysis_failure_store_failed track_id={} error={error}",
                        candidate.track_id
                    );
                }
                log::warn!(
                    target: "sparkle::loudness",
                    "event=analysis_failed track_id={} code={} elapsed_ms={elapsed_ms} error={detail}",
                    candidate.track_id,
                    code
                );
            }
        }

        {
            let mut scheduler = lock_scheduler(&inner);
            if scheduler.current_track_id == Some(candidate.track_id) {
                scheduler.current_track_id = None;
            }
        }
        emit_status(&inner);
        std::thread::yield_now();
    }
}

fn wait_for_work(inner: &Inner, duration: Duration) {
    let scheduler = lock_scheduler(inner);
    let _ = inner
        .wake
        .wait_timeout(scheduler, duration)
        .unwrap_or_else(|e| e.into_inner());
}

fn next_candidate(conn: &Connection, priority: &[i64]) -> Result<Option<Candidate>, String> {
    for track_id in priority {
        if let Some(candidate) = candidate_for_track(conn, *track_id)? {
            return Ok(Some(candidate));
        }
    }

    conn.query_row(
        "SELECT t.id, t.file_path, t.file_mtime, t.file_size_bytes,
                CASE WHEN l.analyzer_version = ?1
                    AND l.analyzed_file_mtime = t.file_mtime
                    AND COALESCE(l.analyzed_file_size_bytes, -1)
                        = COALESCE(t.file_size_bytes, -1)
                    THEN COALESCE(l.attempt_count, 0) ELSE 0 END
         FROM tracks t
         LEFT JOIN track_loudness l ON l.track_id = t.id
         WHERE l.track_id IS NULL
            OR l.analyzer_version != ?1
            OR l.analyzed_file_mtime != t.file_mtime
            OR COALESCE(l.analyzed_file_size_bytes, -1)
                != COALESCE(t.file_size_bytes, -1)
            OR (l.status = 'failed' AND l.attempt_count < ?2
                AND COALESCE(l.retry_after, 0) <= unixepoch())
         ORDER BY t.id
         LIMIT 1",
        params![ANALYZER_VERSION, MAX_ATTEMPTS],
        candidate_from_row,
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn candidate_for_track(conn: &Connection, track_id: i64) -> Result<Option<Candidate>, String> {
    conn.query_row(
        "SELECT t.id, t.file_path, t.file_mtime, t.file_size_bytes,
                CASE WHEN l.analyzer_version = ?2
                    AND l.analyzed_file_mtime = t.file_mtime
                    AND COALESCE(l.analyzed_file_size_bytes, -1)
                        = COALESCE(t.file_size_bytes, -1)
                    THEN COALESCE(l.attempt_count, 0) ELSE 0 END
         FROM tracks t
         LEFT JOIN track_loudness l ON l.track_id = t.id
         WHERE t.id = ?1
           AND (l.track_id IS NULL
             OR l.analyzer_version != ?2
             OR l.analyzed_file_mtime != t.file_mtime
             OR COALESCE(l.analyzed_file_size_bytes, -1)
                 != COALESCE(t.file_size_bytes, -1)
             OR (l.status = 'failed' AND l.attempt_count < ?3
                 AND COALESCE(l.retry_after, 0) <= unixepoch()))",
        params![track_id, ANALYZER_VERSION, MAX_ATTEMPTS],
        candidate_from_row,
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn candidate_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Candidate> {
    Ok(Candidate {
        track_id: row.get(0)?,
        file_path: row.get(1)?,
        file_mtime: row.get(2)?,
        file_size_bytes: row.get(3)?,
        previous_attempts: row.get(4)?,
    })
}

fn analyze_candidate(
    candidate: &Candidate,
    mut cancelled: impl FnMut() -> bool,
) -> Result<AnalysisResult, AnalysisFailure> {
    if cancelled() {
        return Err(AnalysisFailure::Cancelled);
    }
    let before = file_revision(Path::new(&candidate.file_path)).map_err(|detail| {
        AnalysisFailure::Failed {
            code: "file_metadata",
            detail,
        }
    })?;
    verify_revision(candidate, before)?;

    let file = File::open(&candidate.file_path).map_err(|error| AnalysisFailure::Failed {
        code: "open",
        detail: error.to_string(),
    })?;
    let mut decoder =
        Decoder::new(BufReader::new(file)).map_err(|error| AnalysisFailure::Failed {
            code: "decode",
            detail: error.to_string(),
        })?;
    let channels = decoder.channels().get() as u32;
    let sample_rate = decoder.sample_rate().get();
    let mut analyzer =
        EbuR128::new(channels, sample_rate, Mode::I | Mode::TRUE_PEAK).map_err(|error| {
            AnalysisFailure::Failed {
                code: "analyzer_init",
                detail: error.to_string(),
            }
        })?;
    let chunk_samples = ANALYSIS_CHUNK_FRAMES.saturating_mul(channels as usize);
    let mut samples = Vec::with_capacity(chunk_samples);

    loop {
        samples.clear();
        samples.extend(decoder.by_ref().take(chunk_samples));
        if samples.is_empty() {
            break;
        }
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err(AnalysisFailure::Failed {
                code: "non_finite_sample",
                detail: "decoder produced a non-finite sample".to_string(),
            });
        }
        let complete_frames = samples.len() - samples.len() % channels as usize;
        if complete_frames > 0 {
            analyzer
                .add_frames_f64(&samples[..complete_frames])
                .map_err(|error| AnalysisFailure::Failed {
                    code: "analyze",
                    detail: error.to_string(),
                })?;
        }
        if cancelled() {
            return Err(AnalysisFailure::Cancelled);
        }
        if samples.len() < chunk_samples {
            break;
        }
        std::thread::yield_now();
    }

    let after = file_revision(Path::new(&candidate.file_path)).map_err(|detail| {
        AnalysisFailure::Failed {
            code: "file_metadata",
            detail,
        }
    })?;
    if before != after {
        return Err(AnalysisFailure::Failed {
            code: "file_changed",
            detail: "audio file changed during analysis".to_string(),
        });
    }

    let mut true_peak = 0.0_f64;
    for channel in 0..channels {
        true_peak = true_peak.max(analyzer.true_peak(channel).map_err(|error| {
            AnalysisFailure::Failed {
                code: "true_peak",
                detail: error.to_string(),
            }
        })?);
    }
    if !true_peak.is_finite() {
        return Err(AnalysisFailure::Failed {
            code: "non_finite_peak",
            detail: "decoder produced a non-finite true peak".to_string(),
        });
    }
    if true_peak <= f64::EPSILON {
        return Ok(AnalysisResult::Silent);
    }

    let true_peak_dbtp = 20.0 * true_peak.log10();
    let integrated_lufs = analyzer
        .loudness_global()
        .map_err(|error| AnalysisFailure::Failed {
            code: "integrated_loudness",
            detail: error.to_string(),
        })?;
    if integrated_lufs.is_finite() {
        Ok(AnalysisResult::Complete {
            integrated_lufs,
            true_peak_dbtp,
            gain_db: effective_gain_db(integrated_lufs, true_peak_dbtp),
        })
    } else {
        // Very short signals can fall outside the integrated loudness gate.
        // Their true peak is still useful to guarantee the safety ceiling.
        Ok(AnalysisResult::PeakOnly {
            true_peak_dbtp,
            gain_db: (TRUE_PEAK_CEILING_DBTP - true_peak_dbtp).min(0.0),
        })
    }
}

fn file_revision(path: &Path) -> Result<(i64, i64), String> {
    let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
    let modified = metadata
        .modified()
        .map_err(|e| e.to_string())?
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    Ok((modified, metadata.len().min(i64::MAX as u64) as i64))
}

fn verify_revision(candidate: &Candidate, actual: (i64, i64)) -> Result<(), AnalysisFailure> {
    let mtime_matches = candidate.file_mtime <= 0 || candidate.file_mtime == actual.0;
    let size_matches = candidate
        .file_size_bytes
        .is_none_or(|size| size == actual.1);
    if mtime_matches && size_matches {
        Ok(())
    } else {
        Err(AnalysisFailure::Failed {
            code: "file_changed",
            detail: "library metadata no longer matches the audio file".to_string(),
        })
    }
}

fn persist_success(
    conn: &Connection,
    candidate: &Candidate,
    result: AnalysisResult,
) -> Result<(), String> {
    let (status, integrated_lufs, true_peak_dbtp, gain_db) = match result {
        AnalysisResult::Complete {
            integrated_lufs,
            true_peak_dbtp,
            gain_db,
        } => (
            "complete",
            Some(integrated_lufs),
            Some(true_peak_dbtp),
            Some(gain_db),
        ),
        AnalysisResult::PeakOnly {
            true_peak_dbtp,
            gain_db,
        } => ("peak_only", None, Some(true_peak_dbtp), Some(gain_db)),
        AnalysisResult::Silent => ("silent", None, None, Some(0.0)),
    };
    conn.execute(
        "INSERT INTO track_loudness (
            track_id, status, integrated_lufs, true_peak_dbtp, gain_db,
            analyzed_file_mtime, analyzed_file_size_bytes, analyzer_version,
            analyzed_at, attempt_count, retry_after, error_code
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, unixepoch(), 0, NULL, NULL)
         ON CONFLICT(track_id) DO UPDATE SET
            status = excluded.status,
            integrated_lufs = excluded.integrated_lufs,
            true_peak_dbtp = excluded.true_peak_dbtp,
            gain_db = excluded.gain_db,
            analyzed_file_mtime = excluded.analyzed_file_mtime,
            analyzed_file_size_bytes = excluded.analyzed_file_size_bytes,
            analyzer_version = excluded.analyzer_version,
            analyzed_at = excluded.analyzed_at,
            attempt_count = 0,
            retry_after = NULL,
            error_code = NULL",
        params![
            candidate.track_id,
            status,
            integrated_lufs,
            true_peak_dbtp,
            gain_db,
            candidate.file_mtime,
            candidate.file_size_bytes,
            ANALYZER_VERSION,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn persist_failure(conn: &Connection, candidate: &Candidate, code: &str) -> Result<(), String> {
    let attempt_count = candidate.previous_attempts.saturating_add(1);
    let backoff_seconds =
        5_i64.saturating_mul(4_i64.saturating_pow(attempt_count.saturating_sub(1).min(4) as u32));
    conn.execute(
        "INSERT INTO track_loudness (
            track_id, status, integrated_lufs, true_peak_dbtp, gain_db,
            analyzed_file_mtime, analyzed_file_size_bytes, analyzer_version,
            analyzed_at, attempt_count, retry_after, error_code
         ) VALUES (?1, 'failed', NULL, NULL, NULL, ?2, ?3, ?4,
                   unixepoch(), ?5, unixepoch() + ?6, ?7)
         ON CONFLICT(track_id) DO UPDATE SET
            status = 'failed',
            integrated_lufs = NULL,
            true_peak_dbtp = NULL,
            gain_db = NULL,
            analyzed_file_mtime = excluded.analyzed_file_mtime,
            analyzed_file_size_bytes = excluded.analyzed_file_size_bytes,
            analyzer_version = excluded.analyzer_version,
            analyzed_at = excluded.analyzed_at,
            attempt_count = excluded.attempt_count,
            retry_after = excluded.retry_after,
            error_code = excluded.error_code",
        params![
            candidate.track_id,
            candidate.file_mtime,
            candidate.file_size_bytes,
            ANALYZER_VERSION,
            attempt_count,
            backoff_seconds,
            code,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn status_for_inner(inner: &Inner) -> Result<LoudnessStatus, String> {
    let (enabled, running, current_track_id, priority) = {
        let scheduler = lock_scheduler(inner);
        (
            scheduler.enabled,
            scheduler.current_track_id.is_some(),
            scheduler.current_track_id,
            scheduler.priority.clone(),
        )
    };
    let conn = crate::db::open_connection(&inner.db_path).map_err(|e| e.to_string())?;
    let (total, analyzed, failed): (i64, i64, i64) = conn
        .query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN
                        l.analyzer_version = ?1
                        AND l.analyzed_file_mtime = t.file_mtime
                        AND COALESCE(l.analyzed_file_size_bytes, -1)
                            = COALESCE(t.file_size_bytes, -1)
                        AND l.status IN ('complete', 'peak_only', 'silent')
                        THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN
                        l.analyzer_version = ?1
                        AND l.analyzed_file_mtime = t.file_mtime
                        AND COALESCE(l.analyzed_file_size_bytes, -1)
                            = COALESCE(t.file_size_bytes, -1)
                        AND l.status = 'failed'
                        THEN 1 ELSE 0 END), 0)
             FROM tracks t
             LEFT JOIN track_loudness l ON l.track_id = t.id",
            [ANALYZER_VERSION],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| e.to_string())?;
    let mut prioritized_pending = 0;
    for track_id in priority {
        if candidate_for_track(&conn, track_id)?.is_some() {
            prioritized_pending += 1;
        }
    }
    Ok(LoudnessStatus {
        enabled,
        running,
        current_track_id,
        total,
        analyzed,
        pending: total.saturating_sub(analyzed).saturating_sub(failed),
        failed,
        prioritized_pending,
    })
}

fn emit_status(inner: &Inner) {
    match status_for_inner(inner) {
        Ok(status) => {
            let _ = inner.app.emit("loudness-status-changed", status);
        }
        Err(error) => log::debug!(
            target: "sparkle::loudness",
            "event=status_unavailable error={error}"
        ),
    }
}

fn lock_scheduler(inner: &Inner) -> MutexGuard<'_, SchedulerState> {
    inner.scheduler.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measurement_connection() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tracks (
                id INTEGER PRIMARY KEY,
                file_path TEXT NOT NULL,
                file_mtime INTEGER NOT NULL,
                file_size_bytes INTEGER
             );
             CREATE TABLE track_loudness (
                track_id INTEGER PRIMARY KEY,
                status TEXT NOT NULL,
                gain_db REAL,
                analyzer_version INTEGER NOT NULL,
                analyzed_file_mtime INTEGER NOT NULL,
                analyzed_file_size_bytes INTEGER,
                attempt_count INTEGER NOT NULL,
                retry_after INTEGER
             );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn loud_tracks_are_attenuated_to_target() {
        assert!((effective_gain_db(-10.0, -2.0) - -8.0).abs() < 1e-9);
    }

    #[test]
    fn true_peak_ceiling_wins_when_it_is_stricter() {
        assert!((effective_gain_db(-16.0, 1.5) - -2.5).abs() < 1e-9);
    }

    #[test]
    fn attenuation_only_never_amplifies_quiet_tracks() {
        assert_eq!(effective_gain_db(-30.0, -12.0), 0.0);
    }

    #[test]
    fn playback_uses_only_measurements_for_the_exact_file_revision() {
        let conn = measurement_connection();
        conn.execute("INSERT INTO tracks VALUES (1, 'song.flac', 100, 2048)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO track_loudness VALUES
             (1, 'complete', -6.0, ?1, 100, 2048, 0, NULL)",
            [ANALYZER_VERSION],
        )
        .unwrap();
        assert_eq!(
            gain_for_track(&conn, 1).unwrap(),
            GainAvailability::Ready(-6.0)
        );

        conn.execute("UPDATE tracks SET file_mtime = 101 WHERE id = 1", [])
            .unwrap();
        assert_eq!(gain_for_track(&conn, 1).unwrap(), GainAvailability::Pending);
    }

    #[test]
    fn a_new_file_revision_gets_a_fresh_retry_budget() {
        let conn = measurement_connection();
        conn.execute("INSERT INTO tracks VALUES (1, 'song.flac', 101, 2048)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO track_loudness VALUES
             (1, 'failed', NULL, ?1, 100, 2048, 3, 0)",
            [ANALYZER_VERSION],
        )
        .unwrap();

        let candidate = candidate_for_track(&conn, 1).unwrap().unwrap();
        assert_eq!(candidate.previous_attempts, 0);
    }
}
