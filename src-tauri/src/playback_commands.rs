use crate::analytics::{PlaybackContext, PlaybackSource};
use crate::audio_engine::AudioCommand;
use crate::commands::AppState;
use crate::models::{PlaybackState, QueueView};
use crate::playback_observation::{CommandReply, PlaybackFailure};
use tauri::State;

#[tauri::command]
#[allow(non_snake_case)]
pub fn load_queue(
    state: State<'_, AppState>,
    trackIds: Vec<i64>,
    startIndex: usize,
    shuffle: Option<bool>,
    source: Option<PlaybackSource>,
    context: Option<PlaybackContext>,
    commandId: Option<String>,
) -> Result<CommandReply, PlaybackFailure> {
    state.audio.execute_with_id(
        AudioCommand::LoadQueue(
            trackIds,
            startIndex,
            shuffle,
            source.unwrap_or(PlaybackSource::Ui),
            context.unwrap_or_default().sanitized(),
        ),
        commandId,
    )
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn play_track(
    state: State<'_, AppState>,
    trackId: i64,
    source: Option<PlaybackSource>,
    context: Option<PlaybackContext>,
    commandId: Option<String>,
) -> Result<CommandReply, PlaybackFailure> {
    state.audio.execute_with_id(
        AudioCommand::PlayTrack(
            trackId,
            source.unwrap_or(PlaybackSource::Ui),
            context
                .unwrap_or(PlaybackContext {
                    kind: "single".into(),
                    id: None,
                })
                .sanitized(),
        ),
        commandId,
    )
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn play(
    state: State<'_, AppState>,
    source: Option<PlaybackSource>,
    commandId: Option<String>,
) -> Result<CommandReply, PlaybackFailure> {
    state.audio.execute_with_id(
        AudioCommand::Play(source.unwrap_or(PlaybackSource::Ui)),
        commandId,
    )
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn pause(
    state: State<'_, AppState>,
    source: Option<PlaybackSource>,
    commandId: Option<String>,
) -> Result<CommandReply, PlaybackFailure> {
    state.audio.execute_with_id(
        AudioCommand::Pause(source.unwrap_or(PlaybackSource::Ui)),
        commandId,
    )
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn stop(
    state: State<'_, AppState>,
    source: Option<PlaybackSource>,
    commandId: Option<String>,
) -> Result<CommandReply, PlaybackFailure> {
    state.audio.execute_with_id(
        AudioCommand::Stop(source.unwrap_or(PlaybackSource::Ui)),
        commandId,
    )
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn seek(
    state: State<'_, AppState>,
    positionMs: i64,
    source: Option<PlaybackSource>,
    commandId: Option<String>,
) -> Result<CommandReply, PlaybackFailure> {
    state.audio.execute_with_id(
        AudioCommand::Seek(positionMs, source.unwrap_or(PlaybackSource::Ui)),
        commandId,
    )
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn seek_lyrics(
    state: State<'_, AppState>,
    trackId: i64,
    positionMs: i64,
    commandId: Option<String>,
) -> Result<CommandReply, PlaybackFailure> {
    state
        .audio
        .execute_with_id(AudioCommand::SeekLyrics(trackId, positionMs), commandId)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn next_track(
    state: State<'_, AppState>,
    source: Option<PlaybackSource>,
    commandId: Option<String>,
) -> Result<CommandReply, PlaybackFailure> {
    state.audio.execute_with_id(
        AudioCommand::Next(source.unwrap_or(PlaybackSource::Ui)),
        commandId,
    )
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn previous_track(
    state: State<'_, AppState>,
    source: Option<PlaybackSource>,
    commandId: Option<String>,
) -> Result<CommandReply, PlaybackFailure> {
    state.audio.execute_with_id(
        AudioCommand::Previous(source.unwrap_or(PlaybackSource::Ui)),
        commandId,
    )
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn set_volume(
    state: State<'_, AppState>,
    volume: f64,
    source: Option<PlaybackSource>,
    commandId: Option<String>,
) -> Result<CommandReply, PlaybackFailure> {
    state.audio.execute_with_id(
        AudioCommand::SetVolume(volume, source.unwrap_or(PlaybackSource::Ui)),
        commandId,
    )
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn set_shuffle(
    state: State<'_, AppState>,
    shuffle: bool,
    source: Option<PlaybackSource>,
    commandId: Option<String>,
) -> Result<CommandReply, PlaybackFailure> {
    state.audio.execute_with_id(
        AudioCommand::SetShuffle(shuffle, source.unwrap_or(PlaybackSource::Ui)),
        commandId,
    )
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn cycle_repeat_mode(
    state: State<'_, AppState>,
    source: Option<PlaybackSource>,
    commandId: Option<String>,
) -> Result<CommandReply, PlaybackFailure> {
    state.audio.execute_with_id(
        AudioCommand::CycleRepeatMode(source.unwrap_or(PlaybackSource::Ui)),
        commandId,
    )
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn play_next(
    state: State<'_, AppState>,
    trackId: i64,
    source: Option<PlaybackSource>,
    commandId: Option<String>,
) -> Result<CommandReply, PlaybackFailure> {
    state.audio.execute_with_id(
        AudioCommand::PlayNext(trackId, source.unwrap_or(PlaybackSource::Ui)),
        commandId,
    )
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
    source: Option<PlaybackSource>,
    commandId: Option<String>,
) -> Result<CommandReply, PlaybackFailure> {
    state.audio.execute_with_id(
        AudioCommand::PlayAt(orderPos, source.unwrap_or(PlaybackSource::Ui)),
        commandId,
    )
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
    drop(conn);
    state.audio.refresh_track_lyrics(trackId)
}
