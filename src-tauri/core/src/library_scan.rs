use crate::models::{ScanProgress, ScanResult};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, Default, Serialize)]
pub struct ScanStatus {
    pub revision: u64,
    pub running: bool,
    pub progress: Option<ScanProgress>,
    pub result: Option<ScanResult>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct LibraryScan {
    status: Arc<Mutex<ScanStatus>>,
}

impl LibraryScan {
    pub fn status(&self) -> ScanStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn begin(&self, notify: impl Fn(ScanStatus) + Send + 'static) -> Result<ScanRun, String> {
        let initial = {
            let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
            if status.running {
                return Err("A library scan is already running".into());
            }
            *status = ScanStatus {
                revision: status.revision + 1,
                running: true,
                ..Default::default()
            };
            status.clone()
        };
        let run = ScanRun {
            status: self.status.clone(),
            notify: Box::new(notify),
            finished: false,
            last_progress: None,
            started: Instant::now(),
        };
        log::info!(target: "sparkle::scanner", "event=scan_started");
        (run.notify)(initial);
        Ok(run)
    }
}

/// Owns the active scan independently of pages, IPC requests, or subscribers.
pub struct ScanRun {
    status: Arc<Mutex<ScanStatus>>,
    notify: Box<dyn Fn(ScanStatus) + Send>,
    finished: bool,
    last_progress: Option<(String, Instant)>,
    started: Instant,
}

impl ScanRun {
    fn update(&self, notify: bool, update: impl FnOnce(&mut ScanStatus)) {
        let snapshot = {
            let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
            update(&mut status);
            status.revision += 1;
            notify.then(|| status.clone())
        };
        // Never hold the state lock while notifying the webview.
        if let Some(snapshot) = snapshot {
            (self.notify)(snapshot);
        }
    }

    pub fn progress(&mut self, progress: ScanProgress) {
        self.progress_at(progress, Instant::now());
    }

    fn progress_at(&mut self, progress: ScanProgress, now: Instant) {
        let notify = self.last_progress.as_ref().is_none_or(|(phase, last)| {
            *phase != progress.phase || now.saturating_duration_since(*last) >= PROGRESS_INTERVAL
        });
        if notify {
            self.last_progress = Some((progress.phase.clone(), now));
        }
        // Retain every update for snapshots after navigation, but coalesce
        // webview events. Phase changes and completion are always immediate.
        self.update(notify, |status| status.progress = Some(progress));
    }

    pub fn finish(mut self, result: &Result<ScanResult, String>) {
        self.complete(result);
    }

    fn complete(&mut self, result: &Result<ScanResult, String>) {
        self.finished = true;
        let elapsed_ms = self.started.elapsed().as_millis();
        match result {
            Ok(result) => {
                let level = if result.errors > 0 {
                    log::Level::Warn
                } else {
                    log::Level::Info
                };
                log::log!(target: "sparkle::scanner", level,
                    "event=scan_completed scanned={} added={} updated={} removed={} errors={} elapsed_ms={elapsed_ms}",
                    result.scanned, result.added, result.updated, result.removed, result.errors);
            }
            Err(error) => {
                log::error!(target: "sparkle::scanner", "event=scan_failed elapsed_ms={elapsed_ms} error={error}")
            }
        }
        self.update(true, |status| {
            status.running = false;
            status.progress = None;
            status.result = result.as_ref().ok().cloned();
            status.error = result.as_ref().err().cloned();
        });
    }
}

impl Drop for ScanRun {
    fn drop(&mut self) {
        if !self.finished {
            self.complete(&Err("Library scan stopped unexpectedly".into()));
        }
    }
}

#[cfg(test)]
#[path = "tests/library_scan.rs"]
mod tests;
