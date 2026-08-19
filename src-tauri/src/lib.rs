mod analytics;
mod artwork_store;
mod audio_engine;
mod backup;
mod cache;
mod commands;
mod db;
mod db_writer;
mod discord;
mod loudness;
mod models;
mod normalizer;
mod online_commands;
mod playback_commands;
mod providers;
mod scanner;
mod settings;

use commands::AppState;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use tauri::{Manager, State};

const MEDIA_SEEK_STEP_MS: i64 = 15_000;
const LOG_MAX_FILE_SIZE_BYTES: u128 = 2 * 1024 * 1024;
const LOG_FILE_COUNT: usize = 3;

static DEBUG_LOGGING_ENABLED: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_debug_logging_enabled(enabled: bool) {
    DEBUG_LOGGING_ENABLED.store(enabled, Ordering::Relaxed);
}

fn should_emit_log(level: log::Level, target: &str, verbose: bool) -> bool {
    !matches!(level, log::Level::Debug | log::Level::Trace)
        || (verbose && target.starts_with("sparkle"))
}

#[derive(Serialize)]
struct AppStatus {
    db_path: String,
    log_path: String,
    schema_version: i32,
    audio_backend: &'static str,
    audio_output_mode: &'static str,
    audio_precision_bits: u8,
}

#[cfg(desktop)]
#[derive(Default)]
struct MediaStatusQueue {
    pending: Mutex<Option<crate::models::PlaybackState>>,
    wake: Condvar,
}

#[cfg(desktop)]
#[derive(Default)]
pub(crate) struct MediaControlBridge {
    registered: AtomicBool,
    session_ready: AtomicBool,
    shutting_down: AtomicBool,
    status_worker_started: AtomicBool,
    status_queue: Arc<MediaStatusQueue>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum NativeMediaAction {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
    FastForward,
    Rewind,
    SeekTo(f64),
    Ignore,
}

trait NativeMediaPlayback {
    fn is_playing(&self) -> Result<bool, String>;
    fn playback_position(&self) -> Result<(i64, i64), String>;
    fn play(&self) -> Result<(), String>;
    fn pause(&self) -> Result<(), String>;
    fn stop(&self) -> Result<(), String>;
    fn next(&self) -> Result<(), String>;
    fn previous(&self) -> Result<(), String>;
    fn seek(&self, position_ms: i64) -> Result<(), String>;
}

impl NativeMediaPlayback for crate::audio_engine::AudioController {
    fn is_playing(&self) -> Result<bool, String> {
        self.get_playback_state().map(|state| state.is_playing)
    }

    fn playback_position(&self) -> Result<(i64, i64), String> {
        self.get_playback_state()
            .map(|state| (state.position_ms, state.duration_ms))
    }

    fn play(&self) -> Result<(), String> {
        crate::audio_engine::AudioController::play(self, analytics::PlaybackSource::SystemMedia)
            .map(|_| ())
    }

    fn pause(&self) -> Result<(), String> {
        crate::audio_engine::AudioController::pause(self, analytics::PlaybackSource::SystemMedia)
            .map(|_| ())
    }

    fn stop(&self) -> Result<(), String> {
        crate::audio_engine::AudioController::stop(self, analytics::PlaybackSource::SystemMedia)
            .map(|_| ())
    }

    fn next(&self) -> Result<(), String> {
        crate::audio_engine::AudioController::next_track(
            self,
            analytics::PlaybackSource::SystemMedia,
        )
        .map(|_| ())
    }

    fn previous(&self) -> Result<(), String> {
        crate::audio_engine::AudioController::previous_track(
            self,
            analytics::PlaybackSource::SystemMedia,
        )
        .map(|_| ())
    }

    fn seek(&self, position_ms: i64) -> Result<(), String> {
        crate::audio_engine::AudioController::seek(
            self,
            position_ms,
            analytics::PlaybackSource::SystemMedia,
        )
        .map(|_| ())
    }
}

fn dispatch_native_media_action(
    playback: &impl NativeMediaPlayback,
    action: NativeMediaAction,
) -> Result<(), String> {
    match action {
        NativeMediaAction::Play => playback.play(),
        NativeMediaAction::Pause => playback.pause(),
        NativeMediaAction::Toggle => {
            if playback.is_playing()? {
                playback.pause()
            } else {
                playback.play()
            }
        }
        NativeMediaAction::Stop => playback.stop(),
        NativeMediaAction::Next => playback.next(),
        NativeMediaAction::Previous => playback.previous(),
        NativeMediaAction::FastForward => seek_by(playback, MEDIA_SEEK_STEP_MS),
        NativeMediaAction::Rewind => seek_by(playback, -MEDIA_SEEK_STEP_MS),
        NativeMediaAction::SeekTo(seconds) => seek_to_seconds(playback, seconds),
        NativeMediaAction::Ignore => Ok(()),
    }
}

fn seek_by(playback: &impl NativeMediaPlayback, offset_ms: i64) -> Result<(), String> {
    let (position_ms, duration_ms) = playback.playback_position()?;
    seek_within_track(playback, position_ms.saturating_add(offset_ms), duration_ms)
}

fn seek_to_seconds(playback: &impl NativeMediaPlayback, seconds: f64) -> Result<(), String> {
    let (_, duration_ms) = playback.playback_position()?;
    let position_ms = if seconds.is_finite() {
        (seconds.max(0.0) * 1000.0).round() as i64
    } else {
        0
    };
    seek_within_track(playback, position_ms, duration_ms)
}

fn seek_within_track(
    playback: &impl NativeMediaPlayback,
    position_ms: i64,
    duration_ms: i64,
) -> Result<(), String> {
    let position_ms = position_ms.max(0);
    let position_ms = if duration_ms > 0 {
        position_ms.min(duration_ms)
    } else {
        position_ms
    };
    playback.seek(position_ms)
}

#[cfg(desktop)]
fn native_media_action(event: tauri_plugin_media::MediaControlEventType) -> NativeMediaAction {
    use tauri_plugin_media::MediaControlEventType;

    match event {
        MediaControlEventType::Play => NativeMediaAction::Play,
        MediaControlEventType::Pause => NativeMediaAction::Pause,
        MediaControlEventType::PlayPause => NativeMediaAction::Toggle,
        MediaControlEventType::Stop => NativeMediaAction::Stop,
        MediaControlEventType::Next => NativeMediaAction::Next,
        MediaControlEventType::Previous => NativeMediaAction::Previous,
        MediaControlEventType::FastForward => NativeMediaAction::FastForward,
        MediaControlEventType::Rewind => NativeMediaAction::Rewind,
        MediaControlEventType::SeekTo(position) | MediaControlEventType::SetPosition(position) => {
            NativeMediaAction::SeekTo(position)
        }
        MediaControlEventType::SetPlaybackRate(_) => NativeMediaAction::Ignore,
    }
}

/// Updates SMTC only after the frontend-created session is ready. Creating
/// the WinRT player from the audio worker prevents button callbacks.
#[cfg(desktop)]
pub(crate) fn sync_system_media_status(
    app: &tauri::AppHandle,
    playback: &crate::models::PlaybackState,
) -> Result<(), String> {
    use tauri_plugin_media::{MediaExt, PlaybackStatus};

    let (status, label) = match (playback.current_track.is_some(), playback.is_playing) {
        (false, _) => (PlaybackStatus::Stopped, "Stopped"),
        (true, true) => (PlaybackStatus::Playing, "Playing"),
        (true, false) => (PlaybackStatus::Paused, "Paused"),
    };
    app.media()
        .set_playback_status(status)
        .map_err(|err| err.to_string())?;
    log::debug!(
        target: "sparkle::media::smtc",
        "event=status_sync_completed source=audio_state status={label}"
    );
    Ok(())
}

/// Native WinRT status calls are outside the audio worker's control. Queue
/// only the newest state on a separate thread so a sleeping media endpoint
/// cannot stop playback commands or shutdown from being serviced.
#[cfg(desktop)]
fn queue_system_media_status(app: &tauri::AppHandle, playback: crate::models::PlaybackState) {
    let bridge = app.state::<MediaControlBridge>();
    if !bridge.session_ready.load(Ordering::Acquire) || bridge.shutting_down.load(Ordering::Acquire)
    {
        return;
    }

    if !bridge
        .status_worker_started
        .swap(true, std::sync::atomic::Ordering::AcqRel)
    {
        let queue = bridge.status_queue.clone();
        let worker_app = app.clone();
        std::thread::spawn(move || loop {
            let latest = {
                let bridge = worker_app.state::<MediaControlBridge>();
                let mut pending = queue
                    .pending
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                while pending.is_none() && !bridge.shutting_down.load(Ordering::Acquire) {
                    pending = queue
                        .wake
                        .wait(pending)
                        .unwrap_or_else(|error| error.into_inner());
                }
                if bridge.shutting_down.load(Ordering::Acquire) {
                    return;
                }
                pending.take()
            };

            let Some(latest) = latest else {
                continue;
            };
            let bridge = worker_app.state::<MediaControlBridge>();
            if bridge.shutting_down.load(Ordering::Acquire) {
                return;
            }
            if let Err(error) = sync_system_media_status(&worker_app, &latest) {
                log::warn!(
                    target: "sparkle::media::smtc",
                    "event=status_sync_failed source=audio_state error={error}"
                );
            }
        });
    }

    {
        let mut pending = bridge
            .status_queue
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *pending = Some(playback);
    }
    bridge.status_queue.wake.notify_one();
}

#[tauri::command]
fn get_status(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<AppStatus, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let version: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM _schema_version",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(AppStatus {
        db_path: db::db_path(&app).to_string_lossy().to_string(),
        log_path: app
            .path()
            .app_log_dir()
            .map_err(|e| e.to_string())?
            .join("sparkle.log")
            .to_string_lossy()
            .to_string(),
        schema_version: version,
        audio_backend: if cfg!(target_os = "windows") {
            "WASAPI"
        } else {
            "CPAL system backend"
        },
        audio_output_mode: if cfg!(target_os = "windows") {
            "shared"
        } else {
            "system default"
        },
        audio_precision_bits: 64,
    })
}

/// Register the native handler once, after the frontend has initialized the
/// media session. Playback commands stay in Rust so they do not depend on a
/// webview event round-trip.
#[cfg(desktop)]
#[tauri::command]
fn enable_media_control_events(
    app: tauri::AppHandle,
    bridge: State<'_, MediaControlBridge>,
) -> Result<(), String> {
    use tauri_plugin_media::MediaExt;

    if bridge.registered.swap(true, Ordering::AcqRel) {
        log::debug!(
            target: "sparkle::media::smtc",
            "event=button_handler_registration_skipped reason=already_registered"
        );
        return Ok(());
    }

    log::debug!(
        target: "sparkle::media::smtc",
        "event=button_handler_registration_started"
    );
    let audio = app.state::<AppState>().audio.clone();
    app.media().set_event_handler(move |event| {
        let action = native_media_action(event.event_type);
        log::debug!(
            target: "sparkle::media::smtc",
            "event=button_pressed action={action:?}"
        );
        // Windows invokes this from its media-controls callback. Return from
        // that callback promptly; the audio controller already owns a worker
        // thread and will publish the resulting playback state to the UI.
        let audio = audio.clone();
        std::thread::spawn(move || match dispatch_native_media_action(&audio, action) {
            Ok(()) => log::debug!(
                target: "sparkle::media::smtc",
                "event=command_dispatched action={action:?}"
            ),
            Err(err) => log::warn!(
                target: "sparkle::media::smtc",
                "event=command_dispatch_failed action={action:?} error={err}"
            ),
        });
    });
    bridge.session_ready.store(true, Ordering::Release);
    let playback = app.state::<AppState>().audio.get_playback_state()?;
    if let Err(err) = sync_system_media_status(&app, &playback) {
        log::warn!(
            target: "sparkle::media::smtc",
            "event=status_sync_failed source=session_ready error={err}"
        );
    }
    log::debug!(
        target: "sparkle::media::smtc",
        "event=button_handler_registration_completed"
    );
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
fn enable_media_control_events() -> Result<(), String> {
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout).format(
                        |out, message, record| {
                            let level_colour = match record.level() {
                                log::Level::Error => "31",
                                log::Level::Warn => "33",
                                log::Level::Info => "36",
                                log::Level::Debug => "90",
                                log::Level::Trace => "90",
                            };
                            out.finish(format_args!(
                                "[{}][\x1b[{level_colour}m{}\x1b[0m] {message}",
                                record.target(),
                                record.level()
                            ));
                        },
                    ),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("sparkle".to_string()),
                    }),
                ])
                .level(log::LevelFilter::Trace)
                .max_file_size(LOG_MAX_FILE_SIZE_BYTES)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(LOG_FILE_COUNT))
                // Verbose mode adds Sparkle's debug and trace events without
                // allowing codec internals to drown out useful transitions.
                .filter(|metadata| {
                    should_emit_log(
                        metadata.level(),
                        metadata.target(),
                        DEBUG_LOGGING_ENABLED.load(Ordering::Relaxed),
                    )
                })
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_media::init())
        .setup(|app| {
            let (conn, fresh_db) = db::init_db(app.handle()).map_err(|e| e.to_string())?;
            let recovered_listens =
                db::recover_interrupted_listens(&conn).map_err(|e| e.to_string())?;
            if recovered_listens > 0 {
                log::warn!(
                    target: "sparkle::analytics",
                    "event=interrupted_listens_recovered count={recovered_listens}"
                );
            }
            log::info!(
                target: "sparkle::lifecycle",
                "event=application_started version={} fresh_database={} recovered_listens={recovered_listens}",
                env!("CARGO_PKG_VERSION"),
                fresh_db
            );
            if let Err(err) = commands::ensure_live_mix_playlists(&conn) {
                log::warn!(
                    target: "sparkle::database",
                    "event=live_mix_initialization_failed error={err}"
                );
            }
            let startup_settings = match settings::load_settings(&conn) {
                Ok(settings) => settings,
                Err(err) => {
                    log::warn!(
                        target: "sparkle::settings",
                        "event=debug_logging_setting_unavailable error={err}"
                    );
                    settings::Settings::default()
                }
            };
            set_debug_logging_enabled(startup_settings.debug_logging_enabled);
            let app_data_dir = db::data_dir(app.handle());
            let cache_dir = app_data_dir.join("cache");
            // The cache is never wiped automatically — not on startup, not on
            // a fresh database. Custom artist images live here, and losing
            // them is worse than a few orphaned files; clearing is manual
            // from Settings.
            // Legacy: lyrics were cached as files in schema v1; they live
            // in the database again since v2.
            let _ = std::fs::remove_dir_all(cache_dir.join("lyrics"));
            cache::ensure_dirs(&cache_dir);
            let db = Arc::new(Mutex::new(conn));
            #[cfg(desktop)]
            app.manage(MediaControlBridge::default());
            let discord =
                discord::DiscordPresence::new(db::db_path(app.handle()), cache_dir.clone());
            let loudness = loudness::LoudnessController::new(
                app.handle().clone(),
                db::db_path(app.handle()),
                startup_settings.sound_check_enabled,
            );
            let audio = audio_engine::AudioController::new(
                app.handle().clone(),
                db.clone(),
                discord.clone(),
                loudness.clone(),
                startup_settings.sound_check_enabled,
            );
            app.manage(AppState {
                db,
                audio,
                loudness: loudness.clone(),
                discord,
                cache_dir: cache_dir.clone(),
            });

            // Optional background scan at startup. Runs on its own connection
            // (WAL mode) so it never blocks library reads from the UI.
            let app_handle = app.handle().clone();
            let scan_loudness = loudness.clone();
            std::thread::spawn(move || {
                let path = db::db_path(&app_handle);
                let mut scan_conn = match db::open_connection(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        log::warn!(
                            target: "sparkle::scanner",
                            "event=startup_scan_database_open_failed error={e}"
                        );
                        return;
                    }
                };
                let settings = match settings::load_settings(&scan_conn) {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!(
                            target: "sparkle::scanner",
                            "event=startup_scan_settings_load_failed error={e}"
                        );
                        return;
                    }
                };
                if settings.scan_on_startup {
                    log::debug!(target: "sparkle::scanner", "event=startup_scan_started");
                    if let Err(e) =
                        scanner::scan_library(&mut scan_conn, &settings, false, &cache_dir)
                    {
                        log::warn!(
                            target: "sparkle::scanner",
                            "event=startup_scan_failed error={e}"
                        );
                    } else {
                        log::info!(target: "sparkle::scanner", "event=startup_scan_completed");
                    }
                } else {
                    log::debug!(target: "sparkle::scanner", "event=startup_scan_skipped reason=disabled");
                }
                if let Err(e) = commands::refresh_live_mix_playlists_with_connection(&mut scan_conn)
                {
                    log::warn!(
                        target: "sparkle::database",
                        "event=live_mix_refresh_failed source=startup error={e}"
                    );
                }
                scan_loudness.refresh_library();
            });

            // Windows delivers media-key presses through its System Media
            // Transport Controls session. Registering both mechanisms causes
            // duplicate skip/play actions, so retain this shortcut fallback
            // only on other desktop platforms.
            #[cfg(all(desktop, not(target_os = "windows")))]
            {
                // Media keys are not reliably available as global shortcuts on
                // every platform, so any failure is logged and the app continues.
                use tauri::Emitter;
                use tauri_plugin_global_shortcut::{Code, Modifiers, ShortcutState};

                match tauri_plugin_global_shortcut::Builder::new().with_shortcuts([
                    "MediaPlayPause",
                    "MediaTrackNext",
                    "MediaTrackPrevious",
                ]) {
                    Ok(builder) => {
                        let plugin = builder
                            .with_handler(|app, shortcut, event| {
                                if event.state == ShortcutState::Pressed {
                                    let _ = if shortcut
                                        .matches(Modifiers::empty(), Code::MediaPlayPause)
                                    {
                                        app.emit("media-key-play-pause", ())
                                    } else if shortcut
                                        .matches(Modifiers::empty(), Code::MediaTrackNext)
                                    {
                                        app.emit("media-key-next", ())
                                    } else if shortcut
                                        .matches(Modifiers::empty(), Code::MediaTrackPrevious)
                                    {
                                        app.emit("media-key-previous", ())
                                    } else {
                                        Ok(())
                                    };
                                }
                            })
                            .build();

                        if let Err(err) = app.handle().plugin(plugin) {
                            log::warn!(
                                target: "sparkle::media::shortcut",
                                "event=registration_failed error={err}"
                            );
                        }
                    }
                    Err(err) => {
                        log::info!(
                            target: "sparkle::media::shortcut",
                            "event=unsupported error={err}"
                        );
                    }
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            enable_media_control_events,
            commands::pick_folder,
            commands::add_folder,
            commands::remove_folder,
            commands::set_folder_enabled,
            commands::export_library_backup,
            commands::inspect_library_backup,
            commands::import_library_backup,
            commands::reveal_in_explorer,
            commands::list_folders,
            commands::scan_library,
            commands::get_loudness_status,
            commands::rescan_loudness,
            commands::set_track_lyrics_source,
            commands::set_track_custom_lyrics,
            commands::clear_track_custom_lyrics,
            commands::set_album_art_file,
            commands::clear_album_custom_art,
            commands::search,
            commands::get_listening_stats,
            commands::get_discovery_tracks,
            commands::get_library_health,
            commands::get_health_tracks,
            commands::get_artists,
            commands::get_albums,
            commands::get_album,
            commands::get_tracks,
            commands::get_artist,
            commands::get_related_artists,
            commands::get_tracks_by_artist,
            commands::get_genres,
            commands::get_tracks_by_genre,
            commands::get_genre_collage_album_ids,
            commands::get_playlists,
            commands::get_playlist,
            commands::get_playlist_collage_album_ids,
            commands::refresh_live_mix_playlists,
            commands::create_playlist,
            commands::update_playlist,
            commands::delete_playlist,
            commands::add_tracks_to_playlist,
            commands::remove_track_from_playlist,
            playback_commands::load_queue,
            playback_commands::play_track,
            playback_commands::play,
            playback_commands::pause,
            playback_commands::stop,
            playback_commands::seek,
            playback_commands::next_track,
            playback_commands::previous_track,
            playback_commands::set_volume,
            playback_commands::set_shuffle,
            playback_commands::cycle_repeat_mode,
            playback_commands::play_next,
            playback_commands::get_queue,
            playback_commands::play_queue_index,
            playback_commands::get_playback_state,
            playback_commands::get_lrc_offset,
            playback_commands::set_lrc_offset,
            online_commands::get_lyrics,
            online_commands::search_lyrics_online,
            online_commands::set_track_lyrics_choice,
            online_commands::get_artist_info,
            online_commands::get_artist_image,
            online_commands::get_album_art,
            online_commands::get_album_art_data,
            online_commands::get_online_settings,
            online_commands::set_online_settings,
            online_commands::test_artwork_storage,
            online_commands::set_artist_providers,
            online_commands::set_artist_bio,
            online_commands::search_artist_images,
            online_commands::download_artist_image_candidate,
            online_commands::set_artist_image_data,
            online_commands::set_artist_image_file,
            online_commands::clear_artist_custom_image,
            online_commands::clear_lyrics_cache,
            online_commands::clear_artist_info_cache,
            online_commands::clear_images_cache,
            online_commands::clear_all_caches,
            online_commands::get_cache_stats,
            online_commands::get_cache_dir,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            log::info!(target: "sparkle::lifecycle", "event=shutdown_started");
            #[cfg(desktop)]
            {
                let bridge = app_handle.state::<MediaControlBridge>();
                bridge.shutting_down.store(true, Ordering::Release);
                bridge.session_ready.store(false, Ordering::Release);
                bridge.status_queue.wake.notify_all();
            }
            #[cfg(desktop)]
            {
                use tauri_plugin_media::MediaExt;

                if let Err(error) = app_handle.media().shutdown() {
                    log::warn!(
                        target: "sparkle::media::smtc",
                        "event=shutdown_failed error={error}"
                    );
                }
            }
            let mut shutdown_failed = false;
            let audio = app_handle.state::<AppState>().audio.clone();
            if let Err(error) = audio.shutdown() {
                log::error!(target: "sparkle::lifecycle", "event=audio_shutdown_failed error={error}");
                shutdown_failed = true;
            }
            let loudness = app_handle.state::<AppState>().loudness.clone();
            if let Err(error) = loudness.shutdown() {
                log::error!(target: "sparkle::lifecycle", "event=loudness_shutdown_failed error={error}");
                shutdown_failed = true;
            }
            if shutdown_failed {
                log::warn!(target: "sparkle::lifecycle", "event=shutdown_completed_with_errors");
            } else {
                log::info!(target: "sparkle::lifecycle", "event=shutdown_completed");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn logging_filter_keeps_lifecycle_and_scopes_verbose_output() {
        assert!(should_emit_log(log::Level::Info, "dependency", false));
        assert!(should_emit_log(log::Level::Error, "sparkle::audio", false));
        assert!(!should_emit_log(
            log::Level::Debug,
            "sparkle::playback",
            false
        ));
        assert!(should_emit_log(
            log::Level::Trace,
            "sparkle::analytics::writer",
            true
        ));
        assert!(!should_emit_log(log::Level::Debug, "dependency", true));
    }

    struct FakePlayback {
        is_playing: bool,
        position_ms: i64,
        duration_ms: i64,
        actions: RefCell<Vec<String>>,
    }

    impl FakePlayback {
        fn new(is_playing: bool) -> Self {
            Self {
                is_playing,
                position_ms: 30_000,
                duration_ms: 60_000,
                actions: RefCell::new(Vec::new()),
            }
        }
    }

    impl NativeMediaPlayback for FakePlayback {
        fn is_playing(&self) -> Result<bool, String> {
            Ok(self.is_playing)
        }

        fn playback_position(&self) -> Result<(i64, i64), String> {
            Ok((self.position_ms, self.duration_ms))
        }

        fn play(&self) -> Result<(), String> {
            self.actions.borrow_mut().push("play".to_string());
            Ok(())
        }

        fn pause(&self) -> Result<(), String> {
            self.actions.borrow_mut().push("pause".to_string());
            Ok(())
        }

        fn stop(&self) -> Result<(), String> {
            self.actions.borrow_mut().push("stop".to_string());
            Ok(())
        }

        fn next(&self) -> Result<(), String> {
            self.actions.borrow_mut().push("next".to_string());
            Ok(())
        }

        fn previous(&self) -> Result<(), String> {
            self.actions.borrow_mut().push("previous".to_string());
            Ok(())
        }

        fn seek(&self, position_ms: i64) -> Result<(), String> {
            self.actions
                .borrow_mut()
                .push(format!("seek:{position_ms}"));
            Ok(())
        }
    }

    #[test]
    fn native_play_dispatches_directly_to_the_audio_backend() {
        let playback = FakePlayback::new(false);

        dispatch_native_media_action(&playback, NativeMediaAction::Play).unwrap();

        assert_eq!(playback.actions.into_inner(), vec!["play"]);
    }

    #[test]
    fn native_pause_dispatches_directly_to_the_audio_backend() {
        let playback = FakePlayback::new(true);

        dispatch_native_media_action(&playback, NativeMediaAction::Pause).unwrap();

        assert_eq!(playback.actions.into_inner(), vec!["pause"]);
    }

    #[test]
    fn native_toggle_uses_the_audio_backends_current_state() {
        let paused = FakePlayback::new(false);
        let playing = FakePlayback::new(true);

        dispatch_native_media_action(&paused, NativeMediaAction::Toggle).unwrap();
        dispatch_native_media_action(&playing, NativeMediaAction::Toggle).unwrap();

        assert_eq!(paused.actions.into_inner(), vec!["play"]);
        assert_eq!(playing.actions.into_inner(), vec!["pause"]);
    }

    #[test]
    fn native_seek_is_bounded_to_the_track_duration() {
        let playback = FakePlayback::new(false);

        dispatch_native_media_action(&playback, NativeMediaAction::SeekTo(120.0)).unwrap();

        assert_eq!(playback.actions.into_inner(), vec!["seek:60000"]);
    }
}
