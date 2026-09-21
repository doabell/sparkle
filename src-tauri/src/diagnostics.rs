use crate::analytics::{new_trace_id, now_epoch_ms};
use crate::commands::AppState;
use sparkle_core::diagnostics::{self, CaptureInfo, DiagnosticBundle};
use std::collections::VecDeque;
use std::sync::Mutex;
use tauri::{Manager, State};

#[derive(Default)]
pub(crate) struct Captures(Mutex<VecDeque<DiagnosticBundle>>);

#[tauri::command]
pub(crate) async fn capture_playback_diagnostics(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<CaptureInfo, String> {
    // Mark and snapshot before any save dialog or disk reads can delay capture.
    let info = CaptureInfo {
        id: new_trace_id("capture"),
        incident_at_ms: now_epoch_ms(),
    };
    let runtime = state.audio.diagnostics_snapshot();
    log::info!(target: "sparkle::playback", "event=diagnostic_incident_marked capture_id={} incident_at_ms={}", info.id, info.incident_at_ms);
    log::logger().flush();
    let db_path = crate::db::db_path(&app);
    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|error| error.to_string())?;
    let capture_info = info.clone();
    let bundle = tauri::async_runtime::spawn_blocking(move || {
        diagnostics::capture(
            &db_path,
            &log_dir,
            crate::logging::FILE_STEM,
            capture_info,
            env!("CARGO_PKG_VERSION"),
            runtime,
        )
    })
    .await
    .map_err(|error| error.to_string())?;
    let captures = app.state::<Captures>();
    let mut captures = captures.0.lock().map_err(|error| error.to_string())?;
    if captures.len() == 2 {
        captures.pop_front();
    }
    captures.push_back(bundle);
    Ok(info)
}

#[tauri::command]
pub(crate) async fn export_playback_diagnostics(
    app: tauri::AppHandle,
    capture_id: String,
    path: String,
) -> Result<(), String> {
    let bytes = {
        let captures = app.state::<Captures>();
        let captures = captures.0.lock().map_err(|error| error.to_string())?;
        let bundle = captures
            .iter()
            .find(|bundle| bundle.capture.id == capture_id)
            .ok_or_else(|| {
                "This capture has expired. Capture playback diagnostics again.".to_string()
            })?;
        serde_json::to_vec_pretty(bundle).map_err(|error| error.to_string())?
    };
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::write(&path, bytes).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())??;
    Ok(())
}
