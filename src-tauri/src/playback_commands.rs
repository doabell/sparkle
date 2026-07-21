use crate::commands::AppState;
use crate::models::{PlaybackState, QueueView};
use tauri::State;

#[tauri::command]
#[allow(non_snake_case)]
pub fn load_queue(
    state: State<'_, AppState>,
    trackIds: Vec<i64>,
    startIndex: usize,
    shuffle: Option<bool>,
) -> Result<PlaybackState, String> {
    state.audio.load_queue(trackIds, startIndex, shuffle)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn play_track(state: State<'_, AppState>, trackId: i64) -> Result<PlaybackState, String> {
    state.audio.play_track(trackId)
}

#[tauri::command]
pub fn play(state: State<'_, AppState>) -> Result<PlaybackState, String> {
    state.audio.play()
}

#[tauri::command]
pub fn pause(state: State<'_, AppState>) -> Result<PlaybackState, String> {
    state.audio.pause()
}

#[tauri::command]
pub fn stop(state: State<'_, AppState>) -> Result<PlaybackState, String> {
    state.audio.stop()
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn seek(state: State<'_, AppState>, positionMs: i64) -> Result<PlaybackState, String> {
    state.audio.seek(positionMs)
}

#[tauri::command]
pub fn next_track(state: State<'_, AppState>) -> Result<PlaybackState, String> {
    state.audio.next_track()
}

#[tauri::command]
pub fn previous_track(state: State<'_, AppState>) -> Result<PlaybackState, String> {
    state.audio.previous_track()
}

#[tauri::command]
pub fn set_volume(state: State<'_, AppState>, volume: f64) -> Result<PlaybackState, String> {
    state.audio.set_volume(volume)
}

#[tauri::command]
pub fn set_shuffle(state: State<'_, AppState>, shuffle: bool) -> Result<PlaybackState, String> {
    state.audio.set_shuffle(shuffle)
}

#[tauri::command]
pub fn cycle_repeat_mode(state: State<'_, AppState>) -> Result<PlaybackState, String> {
    state.audio.cycle_repeat_mode()
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn play_next(state: State<'_, AppState>, trackId: i64) -> Result<PlaybackState, String> {
    state.audio.play_next(trackId)
}

#[tauri::command]
pub fn get_queue(state: State<'_, AppState>) -> Result<QueueView, String> {
    state.audio.get_queue()
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn play_queue_index(
    state: State<'_, AppState>,
    orderPos: usize,
) -> Result<PlaybackState, String> {
    state.audio.play_queue_index(orderPos)
}

#[tauri::command]
pub fn get_playback_state(state: State<'_, AppState>) -> Result<PlaybackState, String> {
    state.audio.get_playback_state()
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn get_lrc_offset(state: State<'_, AppState>, trackId: i64) -> Result<i64, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let offset: i64 = conn
        .query_row(
            "SELECT lrc_offset_ms FROM tracks WHERE id = ?",
            [trackId],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(offset)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn set_lrc_offset(
    state: State<'_, AppState>,
    trackId: i64,
    offsetMs: i64,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE tracks SET lrc_offset_ms = ? WHERE id = ?",
        [offsetMs, trackId],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
