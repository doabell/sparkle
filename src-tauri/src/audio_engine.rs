use crate::db_writer::{DbWriter, PlayRecord};
use crate::discord::DiscordPresence;
use crate::models::{PlaybackState, QueueView, RepeatMode, Track};
use crate::settings::{load_session, SessionSnapshot};
use rodio::{Decoder, DeviceSinkBuilder, Player};
use serde::Serialize;
use std::fs::File;
use std::io::BufReader;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const PROGRESS_INTERVAL_MS: u64 = 250;

/// Returns the stable identifier of the current default output device, used
/// to detect device switches that the stuck-position heuristic misses (a dead
/// endpoint can keep consuming samples, so position alone is not reliable).
fn default_output_device_id() -> Option<String> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    let host = rodio::cpal::default_host();
    host.default_output_device()
        .and_then(|d| d.id().ok().map(|id| id.to_string()))
}

struct XorShift(u64);

impl XorShift {
    fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e37_79b9_7f4a_7c15);
        XorShift(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % n as u64) as usize
    }
}

/// Builds the playback order for a queue. When `shuffle` is on the order is a
/// permutation that keeps `start_index` first and shuffles the rest; otherwise
/// it is the identity permutation. Returns (play_order, order_pos).
fn build_play_order(queue_len: usize, start_index: usize, shuffle: bool) -> (Vec<usize>, usize) {
    if queue_len == 0 {
        return (Vec::new(), 0);
    }
    let start = start_index.min(queue_len - 1);
    if !shuffle {
        return ((0..queue_len).collect(), start);
    }
    let mut rng = XorShift::new();
    let mut rest: Vec<usize> = (0..queue_len).filter(|&i| i != start).collect();
    for i in (1..rest.len()).rev() {
        let j = rng.below(i + 1);
        rest.swap(i, j);
    }
    let mut order = Vec::with_capacity(queue_len);
    order.push(start);
    order.extend(rest);
    (order, 0)
}

/// Returns true if `order` is a permutation of 0..queue_len.
fn is_valid_play_order(order: &[usize], queue_len: usize) -> bool {
    if order.len() != queue_len {
        return false;
    }
    let mut seen = vec![false; queue_len];
    for &i in order {
        if i >= queue_len || seen[i] {
            return false;
        }
        seen[i] = true;
    }
    true
}

/// Maps the linear UI volume slider to an amplifier gain. Stevens' power
/// law: perceived loudness grows with amplitude^0.6, so gain = x^(5/3)
/// makes loudness proportional to slider travel — half the slider genuinely
/// sounds half as loud. dB tapers feel too quiet in the lower half and
/// linear feels too loud; this is the perceptually linear curve.
fn slider_to_gain(volume: f64) -> f32 {
    let v = volume.clamp(0.0, 1.0);
    v.powf(5.0 / 3.0) as f32
}

/// Removes duplicate track ids, keeping the first occurrence. The returned
/// start index points at the first occurrence of the track that was at
/// `start_index` before deduplication.
fn dedup_queue(track_ids: Vec<i64>, start_index: usize) -> (Vec<i64>, usize) {
    let start_id = track_ids.get(start_index).copied();
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::with_capacity(track_ids.len());
    for id in track_ids {
        if seen.insert(id) {
            deduped.push(id);
        }
    }
    let new_start = match start_id {
        Some(id) => deduped.iter().position(|&t| t == id).unwrap_or(0),
        None => 0,
    };
    (deduped, new_start)
}

#[derive(Clone)]
pub struct AudioController {
    tx: mpsc::Sender<AudioCommand>,
    #[allow(dead_code)]
    state: Arc<Mutex<SharedState>>,
}

impl AudioController {
    pub fn new(
        app_handle: AppHandle,
        db: Arc<Mutex<rusqlite::Connection>>,
        discord: DiscordPresence,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(SharedState {
            queue: Vec::new(),
            queue_index: None,
            play_order: Vec::new(),
            order_pos: None,
            current_track: None,
            is_playing: false,
            position_ms: 0,
            duration_ms: 0,
            volume: 1.0,
            shuffle: false,
            repeat_mode: RepeatMode::Off,
            seek_target: None,
            play_started_at: None,
            listened_ms: 0,
            last_counted_position_ms: None,
            discord,
        }));
        let state_clone = state.clone();
        let writer = DbWriter::new(crate::db::db_path(&app_handle));
        std::thread::spawn(move || {
            audio_thread(rx, app_handle, state_clone, db, writer);
        });
        Self { tx, state }
    }

    /// Loads a queue and starts at `start_index`. `shuffle` is an explicit
    /// context switch: Some(true/false) sets the mode first (the page Play
    /// and Shuffle buttons mean ordered vs. shuffled playback); None keeps
    /// whatever mode the player is already in (tapping an individual track).
    pub fn load_queue(
        &self,
        track_ids: Vec<i64>,
        start_index: usize,
        shuffle: Option<bool>,
    ) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::LoadQueue(track_ids, start_index, shuffle))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn play_track(&self, track_id: i64) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::PlayTrack(track_id))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn play(&self) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::Play)
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn pause(&self) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::Pause)
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn stop(&self) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::Stop)
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn seek(&self, position_ms: i64) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::Seek(position_ms))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn next_track(&self) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::Next)
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn previous_track(&self) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::Previous)
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn set_volume(&self, volume: f64) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::SetVolume(volume))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn set_shuffle(&self, shuffle: bool) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::SetShuffle(shuffle))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn cycle_repeat_mode(&self) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::CycleRepeatMode)
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn play_next(&self, track_id: i64) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::PlayNext(track_id))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn get_queue(&self) -> Result<QueueView, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(AudioCommand::GetQueue(reply_tx))
            .map_err(|e| e.to_string())?;
        reply_rx.recv().map_err(|e| e.to_string())
    }

    pub fn play_queue_index(&self, order_pos: usize) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::PlayAt(order_pos))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn get_playback_state(&self) -> Result<PlaybackState, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(AudioCommand::GetState(reply_tx))
            .map_err(|e| e.to_string())?;
        reply_rx.recv().map_err(|e| e.to_string())
    }
}

enum AudioCommand {
    LoadQueue(Vec<i64>, usize, Option<bool>),
    PlayTrack(i64),
    Play,
    Pause,
    Stop,
    Seek(i64),
    Next,
    Previous,
    SetVolume(f64),
    SetShuffle(bool),
    CycleRepeatMode,
    PlayNext(i64),
    GetQueue(mpsc::Sender<QueueView>),
    PlayAt(usize),
    GetState(mpsc::Sender<PlaybackState>),
}

struct SharedState {
    queue: Vec<i64>,
    queue_index: Option<usize>,
    play_order: Vec<usize>,
    order_pos: Option<usize>,
    current_track: Option<Track>,
    is_playing: bool,
    position_ms: i64,
    duration_ms: i64,
    volume: f64,
    shuffle: bool,
    repeat_mode: RepeatMode,
    seek_target: Option<(i64, Instant)>,
    /// When the current play started (epoch seconds), for play_history.
    play_started_at: Option<i64>,
    /// Actual forward-moving audio time. This is intentionally independent
    /// from position so seeking cannot manufacture listening minutes.
    listened_ms: i64,
    last_counted_position_ms: Option<i64>,
    discord: DiscordPresence,
}

#[derive(Serialize, Clone)]
struct PlaybackStateChangedEvent {
    is_playing: bool,
    current_track: Option<Track>,
    position_ms: i64,
    duration_ms: i64,
    shuffle: bool,
    repeat_mode: RepeatMode,
}

#[derive(Serialize, Clone)]
struct ProgressEvent {
    track_id: i64,
    position_ms: i64,
    duration_ms: i64,
}

fn save_session_to_db(state: &Arc<Mutex<SharedState>>, writer: &DbWriter) {
    let s = lock_state(state);
    let snapshot = SessionSnapshot {
        queue: s.queue.clone(),
        queue_index: s.queue_index,
        position_ms: s.position_ms,
        volume: s.volume,
        is_playing: s.is_playing,
        shuffle: s.shuffle,
        repeat_mode: s.repeat_mode,
        play_order: s.play_order.clone(),
    };
    drop(s);
    // Non-blocking: the writer thread persists the newest snapshot.
    writer.save_session(snapshot);
}

const MIN_MEANINGFUL_LISTEN_MS: i64 = 30_000;
const MIN_SHORT_TRACK_LISTEN_MS: i64 = 5_000;

fn is_meaningful_listen(listened_ms: i64, duration_ms: i64) -> bool {
    listened_ms >= MIN_MEANINGFUL_LISTEN_MS
        || (duration_ms > 0
            && listened_ms >= MIN_SHORT_TRACK_LISTEN_MS
            && listened_ms.saturating_mul(2) >= duration_ms)
}

/// Records one compact row for a meaningful outgoing listen. Tiny previews,
/// accidental starts, and seek jumps are discarded before they reach SQLite.
fn record_outgoing_play(state: &Arc<Mutex<SharedState>>, writer: &DbWriter) {
    let record = {
        let mut s = lock_state(state);
        let started_at = s.play_started_at.take();
        match (s.current_track.as_ref(), started_at) {
            (Some(track), Some(started_at))
                if is_meaningful_listen(s.listened_ms, s.duration_ms) =>
            {
                let played_ms = s.listened_ms.max(0);
                let completed = s.duration_ms > 0
                    && s.position_ms.saturating_mul(10) >= s.duration_ms.saturating_mul(9);
                Some(PlayRecord {
                    track_id: track.id,
                    started_at,
                    played_ms,
                    completed,
                })
            }
            _ => None,
        }
    };
    if let Some(record) = record {
        writer.record_play(record);
    }
}

fn now_epoch_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn restore_session(
    player: &Player,
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
) {
    let snapshot = match {
        let conn = lock_db(db);
        load_session(&conn)
    } {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Failed to load session: {e}");
            return;
        }
    };

    if snapshot.queue.is_empty() || snapshot.queue_index.is_none() {
        return;
    }

    {
        let mut s = lock_state(state);
        s.queue = snapshot.queue.clone();
        s.queue_index = snapshot.queue_index;
        s.volume = snapshot.volume.clamp(0.0, 1.0);
        s.position_ms = snapshot.position_ms.max(0);
        s.listened_ms = 0;
        s.last_counted_position_ms = None;
        s.play_started_at = None;
        s.is_playing = false;
        s.shuffle = snapshot.shuffle;
        s.repeat_mode = snapshot.repeat_mode;
        s.seek_target = None;
        if is_valid_play_order(&snapshot.play_order, s.queue.len()) {
            s.play_order = snapshot.play_order.clone();
        } else {
            let (order, _) = build_play_order(s.queue.len(), s.queue_index.unwrap_or(0), false);
            s.play_order = order;
        }
        s.order_pos = s
            .queue_index
            .and_then(|idx| s.play_order.iter().position(|&i| i == idx));
    }
    player.set_volume(slider_to_gain(snapshot.volume));

    let index = snapshot.queue_index.unwrap_or(0);
    // Load the restored track paused so startup never produces audible output.
    player.pause();
    if !load_track_at_index_with_autoplay(player, state, db, writer, app_handle, index, false) {
        update_state_for_stop(state, writer);
        emit_state_changed(app_handle, state);
        return;
    }

    if snapshot.position_ms > 0 {
        let pos = Duration::from_millis(snapshot.position_ms as u64);
        if player.try_seek(pos).is_err() {
            reload_source_at_position(player, state, snapshot.position_ms);
            player.pause();
        }
        {
            let mut s = lock_state(state);
            s.position_ms = snapshot.position_ms;
            s.seek_target = Some((snapshot.position_ms, Instant::now()));
        }
    }

    // Always start paused on launch, even if the saved session was playing.
    player.pause();
    {
        let mut s = lock_state(state);
        s.is_playing = false;
    }
    emit_state_changed(app_handle, state);
}

fn load_source_into_player(player: &Player, track: &Track) -> bool {
    let file = match File::open(&track.file_path) {
        Ok(f) => f,
        Err(e) => {
            log::error!("Failed to open {}: {}", track.file_path, e);
            return false;
        }
    };
    let source = match Decoder::new(BufReader::new(file)) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Failed to decode {}: {}", track.file_path, e);
            return false;
        }
    };
    player.stop();
    player.clear();
    player.append(source);
    true
}

fn reload_current_for_device(
    player: &Player,
    state: &Arc<Mutex<SharedState>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
) {
    let (track, was_playing, position_ms, volume) = {
        let s = lock_state(state);
        (
            s.current_track.clone(),
            s.is_playing,
            s.position_ms,
            s.volume,
        )
    };
    let track = match track {
        Some(t) => t,
        None => return,
    };

    if !was_playing {
        player.pause();
    }
    if !load_source_into_player(player, &track) {
        update_state_for_stop(state, writer);
        emit_state_changed(app_handle, state);
        return;
    }

    player.set_volume(slider_to_gain(volume));
    if was_playing {
        player.play();
    } else {
        player.pause();
    }

    if position_ms > 0 {
        let pos = Duration::from_millis(position_ms as u64);
        let _ = player.try_seek(pos);
    }

    {
        let mut s = lock_state(state);
        s.seek_target = Some((position_ms, Instant::now()));
    }

    if !was_playing {
        let mut s = lock_state(state);
        s.is_playing = false;
    }

    emit_state_changed(app_handle, state);
}

fn audio_thread(
    rx: mpsc::Receiver<AudioCommand>,
    app_handle: AppHandle,
    state: Arc<Mutex<SharedState>>,
    db: Arc<Mutex<rusqlite::Connection>>,
    writer: DbWriter,
) {
    let mut first_run = true;

    'pipeline: loop {
        let handle = match DeviceSinkBuilder::open_default_sink() {
            Ok(v) => v,
            Err(e) => {
                log::error!("audio device error: {e}");
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let player = Player::connect_new(handle.mixer());
        let device_name = default_output_device_id();

        if first_run {
            restore_session(&player, &state, &db, &writer, &app_handle);
            first_run = false;
        } else {
            reload_current_for_device(&player, &state, &writer, &app_handle);
        }

        let mut last_progress_emit = Instant::now();
        let progress_interval = Duration::from_millis(PROGRESS_INTERVAL_MS);
        let mut last_session_save = Instant::now();
        let session_save_interval = Duration::from_secs(5);

        let mut last_device_check = Instant::now();
        let device_check_interval = Duration::from_millis(500);

        let mut last_pos = Duration::ZERO;
        let mut stuck_since: Option<Instant> = None;
        let stuck_timeout = Duration::from_secs(3);

        loop {
            while let Ok(cmd) = rx.try_recv() {
                handle_command(cmd, &player, &state, &db, &writer, &app_handle);
            }

            // Detect default-device changes (headphones, USB DAC, Bluetooth).
            if last_device_check.elapsed() >= device_check_interval {
                last_device_check = Instant::now();
                let new_name = default_output_device_id();
                if new_name != device_name {
                    log::info!(
                        "Default audio device changed ({device_name:?} -> {new_name:?}), recreating pipeline"
                    );
                    drop(player);
                    drop(handle);
                    continue 'pipeline;
                }
            }

            let pos = player.get_pos();
            let pos_ms = pos.as_millis() as i64;
            let player_empty = player.empty();
            let is_paused = player.is_paused();

            {
                let mut s = lock_state(&state);
                if let Some((target, start)) = s.seek_target {
                    if start.elapsed() < Duration::from_millis(900) {
                        s.position_ms = target;
                        if pos_ms >= target {
                            s.seek_target = None;
                        }
                    } else {
                        s.seek_target = None;
                        s.position_ms = pos_ms;
                    }
                } else {
                    s.position_ms = pos_ms;
                }
                if !player_empty {
                    s.is_playing = !is_paused;
                }
                let countable = !player_empty && !is_paused && s.current_track.is_some();
                if countable {
                    if let Some(previous_ms) = s.last_counted_position_ms {
                        let delta = pos_ms - previous_ms;
                        // Normal playback advances in tiny increments. A
                        // larger jump is a seek or pipeline reload, not time
                        // the listener actually heard.
                        if delta > 0 && delta <= 2_000 {
                            s.listened_ms = s.listened_ms.saturating_add(delta);
                        }
                    }
                    s.last_counted_position_ms = Some(pos_ms);
                } else {
                    s.last_counted_position_ms = None;
                }
            }

            // Detect stuck playback (device unplugged / audio endpoint changed)
            if !is_paused && !player_empty {
                if pos == last_pos {
                    if stuck_since.is_none() {
                        stuck_since = Some(Instant::now());
                    } else if stuck_since.unwrap().elapsed() >= stuck_timeout {
                        log::warn!("Audio device lost, recreating pipeline");
                        drop(player);
                        drop(handle);
                        continue 'pipeline;
                    }
                } else {
                    stuck_since = None;
                }
                last_pos = pos;
            } else {
                stuck_since = None;
                last_pos = pos;
            }

            if last_progress_emit.elapsed() >= progress_interval {
                let maybe_track = lock_state(&state).current_track.clone();
                if let Some(track) = maybe_track {
                    let (position_ms, duration_ms) = {
                        let s = lock_state(&state);
                        (s.position_ms, s.duration_ms)
                    };
                    let _ = app_handle.emit(
                        "playback-progress",
                        ProgressEvent {
                            track_id: track.id,
                            position_ms,
                            duration_ms,
                        },
                    );
                }
                last_progress_emit = Instant::now();
            }

            if last_session_save.elapsed() >= session_save_interval {
                save_session_to_db(&state, &writer);
                last_session_save = Instant::now();
            }

            if player_empty {
                let should_advance = {
                    let s = lock_state(&state);
                    s.is_playing && s.current_track.is_some()
                };
                if should_advance {
                    advance(&player, &state, &db, &writer, &app_handle, true);
                }
            }

            std::thread::sleep(Duration::from_millis(50));

            // Keep the device sink handle alive.
            let _ = &handle;
        }
    }
}

fn handle_command(
    cmd: AudioCommand,
    player: &Player,
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
) {
    match cmd {
        AudioCommand::LoadQueue(track_ids, start_index, shuffle_override) => {
            record_outgoing_play(state, writer);
            let (track_ids, start_index) = dedup_queue(track_ids, start_index);
            let shuffle = shuffle_override.unwrap_or_else(|| lock_state(state).shuffle);
            let (play_order, order_pos) = build_play_order(track_ids.len(), start_index, shuffle);
            let idx = if track_ids.is_empty() {
                None
            } else {
                Some(play_order[order_pos])
            };
            {
                let mut s = lock_state(state);
                if let Some(override_value) = shuffle_override {
                    s.shuffle = override_value;
                }
                s.queue = track_ids;
                s.queue_index = idx;
                s.play_order = play_order;
                s.order_pos = if idx.is_some() { Some(order_pos) } else { None };
                s.is_playing = false;
                s.current_track = None;
                s.position_ms = 0;
                s.duration_ms = 0;
                s.listened_ms = 0;
                s.last_counted_position_ms = None;
            }
            if let Some(i) = idx {
                if !load_track_at_index(player, state, db, writer, app_handle, i) {
                    update_state_for_stop(state, writer);
                    emit_state_changed(app_handle, state);
                }
            } else {
                player.stop();
                player.clear();
                update_state_for_stop(state, writer);
                emit_state_changed(app_handle, state);
            }
            emit_queue_changed(app_handle);
            save_session_to_db(state, writer);
        }
        AudioCommand::PlayTrack(track_id) => {
            {
                let mut s = lock_state(state);
                s.queue = vec![track_id];
                s.queue_index = Some(0);
                s.play_order = vec![0];
                s.order_pos = Some(0);
            }
            if !load_track_at_index(player, state, db, writer, app_handle, 0) {
                update_state_for_stop(state, writer);
                emit_state_changed(app_handle, state);
            }
            emit_queue_changed(app_handle);
            save_session_to_db(state, writer);
        }
        AudioCommand::Play => {
            let has_track = lock_state(state).current_track.is_some();
            if has_track {
                player.play();
                {
                    let mut s = lock_state(state);
                    s.is_playing = true;
                    // Resuming a restored track has no start marker yet.
                    if s.play_started_at.is_none() {
                        s.play_started_at = Some(now_epoch_seconds());
                    }
                }
                emit_state_changed(app_handle, state);
                save_session_to_db(state, writer);
            } else {
                let idx = lock_state(state).queue_index;
                if let Some(i) = idx {
                    if !load_track_at_index(player, state, db, writer, app_handle, i) {
                        update_state_for_stop(state, writer);
                        emit_state_changed(app_handle, state);
                    }
                    save_session_to_db(state, writer);
                }
            }
        }
        AudioCommand::Pause => {
            player.pause();
            {
                let mut s = lock_state(state);
                s.is_playing = false;
                s.last_counted_position_ms = None;
            }
            emit_state_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::Stop => {
            player.stop();
            player.clear();
            update_state_for_stop(state, writer);
            emit_state_changed(app_handle, state);
        }
        AudioCommand::Seek(position_ms) => {
            let position_ms = position_ms.max(0);
            let pos = Duration::from_millis(position_ms as u64);
            let was_playing = !player.is_paused();
            if was_playing {
                player.pause();
            }
            let seek_ok = player.try_seek(pos).is_ok();
            if !seek_ok {
                let reloaded = reload_source_at_position(player, state, position_ms);
                if !reloaded {
                    log::warn!("Seek failed and source reload was unsuccessful");
                }
            }
            if was_playing {
                player.play();
            }
            {
                let mut s = lock_state(state);
                s.position_ms = position_ms;
                s.seek_target = Some((position_ms, Instant::now()));
                s.last_counted_position_ms = None;
            }
            emit_state_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::Next => {
            advance(player, state, db, writer, app_handle, false);
            save_session_to_db(state, writer);
        }
        AudioCommand::Previous => {
            let (pos, order_pos) = {
                let s = lock_state(state);
                (s.position_ms, s.order_pos)
            };
            if pos > 3000 {
                seek_to_start(player, state);
            } else if let Some(p) = order_pos {
                if p > 0 {
                    let prev_pos = p - 1;
                    let prev_idx = {
                        let mut s = lock_state(state);
                        s.order_pos = Some(prev_pos);
                        s.queue_index = s.play_order.get(prev_pos).copied();
                        s.queue_index
                    };
                    if let Some(i) = prev_idx {
                        if !load_track_at_index(player, state, db, writer, app_handle, i) {
                            update_state_for_stop(state, writer);
                            emit_state_changed(app_handle, state);
                        }
                    }
                } else {
                    seek_to_start(player, state);
                }
            }
            emit_queue_changed(app_handle);
            save_session_to_db(state, writer);
        }
        AudioCommand::SetShuffle(shuffle) => {
            {
                let mut s = lock_state(state);
                s.shuffle = shuffle;
                let queue_len = s.queue.len();
                if queue_len > 0 {
                    if let Some(cur) = s.queue_index {
                        let (order, pos) = build_play_order(queue_len, cur, shuffle);
                        s.play_order = order;
                        s.order_pos = Some(pos);
                    } else {
                        s.play_order = (0..queue_len).collect();
                        s.order_pos = None;
                    }
                }
            }
            emit_state_changed(app_handle, state);
            emit_queue_changed(app_handle);
            save_session_to_db(state, writer);
        }
        AudioCommand::CycleRepeatMode => {
            {
                let mut s = lock_state(state);
                s.repeat_mode = s.repeat_mode.next();
            }
            emit_state_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::PlayNext(track_id) => {
            let current = {
                let s = lock_state(state);
                (s.queue_index, s.order_pos)
            };
            match current {
                (Some(cur_idx), Some(pos)) => {
                    let mut s = lock_state(state);
                    let already_current = s.queue.get(cur_idx) == Some(&track_id);
                    if !already_current {
                        // If the track is already queued, move that entry next
                        // instead of adding a duplicate.
                        if let Some(existing_qidx) = s.queue.iter().position(|&t| t == track_id) {
                            s.queue.remove(existing_qidx);
                            if let Some(pp) = s.play_order.iter().position(|&i| i == existing_qidx)
                            {
                                s.play_order.remove(pp);
                            }
                            for i in s.play_order.iter_mut() {
                                if *i > existing_qidx {
                                    *i -= 1;
                                }
                            }
                            let new_cur_idx = if cur_idx > existing_qidx {
                                cur_idx - 1
                            } else {
                                cur_idx
                            };
                            s.queue_index = Some(new_cur_idx);
                            let new_pos = s
                                .play_order
                                .iter()
                                .position(|&i| i == new_cur_idx)
                                .unwrap_or(0);
                            s.order_pos = Some(new_pos);
                            s.queue.push(track_id);
                            let new_idx = s.queue.len() - 1;
                            let insert_at = (new_pos + 1).min(s.play_order.len());
                            s.play_order.insert(insert_at, new_idx);
                        } else {
                            s.queue.push(track_id);
                            let new_idx = s.queue.len() - 1;
                            let insert_at = (pos + 1).min(s.play_order.len());
                            s.play_order.insert(insert_at, new_idx);
                        }
                    }
                }
                _ => {
                    {
                        let mut s = lock_state(state);
                        s.queue = vec![track_id];
                        s.play_order = vec![0];
                        s.queue_index = Some(0);
                        s.order_pos = Some(0);
                    }
                    if !load_track_at_index(player, state, db, writer, app_handle, 0) {
                        update_state_for_stop(state, writer);
                        emit_state_changed(app_handle, state);
                    }
                }
            }
            emit_queue_changed(app_handle);
            save_session_to_db(state, writer);
        }
        AudioCommand::GetQueue(reply) => {
            let view = build_queue_view(state, db);
            let _ = reply.send(view);
        }
        AudioCommand::PlayAt(order_pos) => {
            let next_idx = {
                let mut s = lock_state(state);
                if order_pos < s.play_order.len() {
                    s.order_pos = Some(order_pos);
                    s.queue_index = s.play_order.get(order_pos).copied();
                    s.queue_index
                } else {
                    None
                }
            };
            if let Some(i) = next_idx {
                if !load_track_at_index(player, state, db, writer, app_handle, i) {
                    update_state_for_stop(state, writer);
                    emit_state_changed(app_handle, state);
                }
            }
            emit_queue_changed(app_handle);
            save_session_to_db(state, writer);
        }
        AudioCommand::SetVolume(volume) => {
            let v = volume.clamp(0.0, 1.0);
            player.set_volume(slider_to_gain(v));
            {
                let mut s = lock_state(state);
                s.volume = v;
            }
            emit_state_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::GetState(reply) => {
            let ps = build_playback_state(state);
            let _ = reply.send(ps);
        }
    }
}

fn seek_to_start(player: &Player, state: &Arc<Mutex<SharedState>>) {
    let was_playing = !player.is_paused();
    if was_playing {
        player.pause();
    }
    if player.try_seek(Duration::from_millis(0)).is_err() {
        reload_source_at_position(player, state, 0);
    }
    if was_playing {
        player.play();
    }
    {
        let mut s = lock_state(state);
        s.position_ms = 0;
        s.seek_target = Some((0, Instant::now()));
    }
}

fn advance(
    player: &Player,
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
    auto: bool,
) {
    let (order_pos, order_len, repeat_mode, queue_index) = {
        let s = lock_state(state);
        (
            s.order_pos,
            s.play_order.len(),
            s.repeat_mode,
            s.queue_index,
        )
    };

    // Repeat-one replays the current track on automatic advance; a manual
    // skip still moves to the next track.
    if auto && repeat_mode == RepeatMode::One {
        if let Some(i) = queue_index {
            if !load_track_at_index(player, state, db, writer, app_handle, i) {
                update_state_for_stop(state, writer);
                emit_state_changed(app_handle, state);
            }
            save_session_to_db(state, writer);
            return;
        }
    }

    let next_pos = match order_pos {
        Some(p) if p + 1 < order_len => Some(p + 1),
        _ if repeat_mode == RepeatMode::All && order_len > 0 => Some(0),
        _ => None,
    };

    if let Some(p) = next_pos {
        let next_idx = {
            let mut s = lock_state(state);
            s.order_pos = Some(p);
            s.queue_index = s.play_order.get(p).copied();
            s.queue_index
        };
        if let Some(i) = next_idx {
            if !load_track_at_index(player, state, db, writer, app_handle, i) {
                update_state_for_stop(state, writer);
                emit_state_changed(app_handle, state);
            }
        } else {
            player.stop();
            player.clear();
            update_state_for_stop(state, writer);
            emit_state_changed(app_handle, state);
        }
    } else if auto {
        // End of the queue with repeat off: pause and keep the last track
        // loaded at position 0 so pressing play starts it again. The track
        // stays visible instead of the player emptying out.
        if let Some(i) = queue_index {
            if !load_track_at_index_with_autoplay(player, state, db, writer, app_handle, i, false) {
                update_state_for_stop(state, writer);
                emit_state_changed(app_handle, state);
            } else {
                let mut s = lock_state(state);
                s.is_playing = false;
            }
            emit_state_changed(app_handle, state);
        } else {
            player.stop();
            player.clear();
            update_state_for_stop(state, writer);
            emit_state_changed(app_handle, state);
        }
    }
    // A manual Next at the end of the queue (repeat off) is a no-op.
    save_session_to_db(state, writer);
}

fn update_state_for_stop(state: &Arc<Mutex<SharedState>>, writer: &DbWriter) {
    record_outgoing_play(state, writer);
    let mut s = lock_state(state);
    s.is_playing = false;
    s.current_track = None;
    s.position_ms = 0;
    s.duration_ms = 0;
    s.seek_target = None;
    s.play_started_at = None;
    s.listened_ms = 0;
    s.last_counted_position_ms = None;
}

fn load_track_at_index(
    player: &Player,
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
    index: usize,
) -> bool {
    load_track_at_index_with_autoplay(player, state, db, writer, app_handle, index, true)
}

fn load_track_at_index_with_autoplay(
    player: &Player,
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
    index: usize,
    autoplay: bool,
) -> bool {
    // A track transition ends the current play: record it before swapping.
    record_outgoing_play(state, writer);
    let track_id = {
        let s = lock_state(state);
        s.queue.get(index).copied()
    };
    let track_id = match track_id {
        Some(id) => id,
        None => return false,
    };

    let track = match load_track_from_db(db, track_id) {
        Ok(t) => t,
        Err(e) => {
            log::error!("Failed to load track {} from db: {}", track_id, e);
            return false;
        }
    };

    // Publish the new metadata before touching the audio file. Opening and
    // decoding can take long enough to make the player bar feel stuck; the
    // audio source can catch up independently while the UI shows the next
    // track immediately.
    {
        let mut s = lock_state(state);
        s.current_track = Some(track.clone());
        s.is_playing = autoplay;
        s.position_ms = 0;
        s.duration_ms = track.duration_ms.unwrap_or(0);
        s.seek_target = None;
        s.play_started_at = autoplay.then(now_epoch_seconds);
        s.listened_ms = 0;
        s.last_counted_position_ms = None;
    }
    emit_state_changed(app_handle, state);

    let file = match File::open(&track.file_path) {
        Ok(f) => f,
        Err(e) => {
            log::error!("Failed to open audio file {}: {}", track.file_path, e);
            return false;
        }
    };

    let source = match Decoder::new(BufReader::new(file)) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Failed to decode {}: {}", track.file_path, e);
            return false;
        }
    };

    // Pause before swapping the source. A fresh or currently-playing rodio
    // Player would otherwise start the appended source immediately, producing
    // an audible pop when the caller wants the track loaded but silent.
    if !autoplay {
        player.pause();
    }
    player.stop();
    player.clear();
    player.append(source);
    if autoplay {
        player.play();
    }
    {
        let v = lock_state(state).volume;
        player.set_volume(slider_to_gain(v));
    }

    true
}

fn reload_source_at_position(
    player: &Player,
    state: &Arc<Mutex<SharedState>>,
    position_ms: i64,
) -> bool {
    let track = {
        let s = lock_state(state);
        s.current_track.clone()
    };
    let track = match track {
        Some(t) => t,
        None => return false,
    };

    let file = match File::open(&track.file_path) {
        Ok(f) => f,
        Err(e) => {
            log::error!("Failed to reopen audio file {}: {}", track.file_path, e);
            return false;
        }
    };

    let source = match Decoder::new(BufReader::new(file)) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Failed to re-decode {}: {}", track.file_path, e);
            return false;
        }
    };

    // Keep a paused player paused across the reload so no audio blips out.
    let was_paused = player.is_paused();
    if was_paused {
        player.pause();
    }
    player.clear();
    player.append(source);
    if was_paused {
        player.pause();
    }

    let pos = Duration::from_millis(position_ms.max(0) as u64);
    if let Err(e) = player.try_seek(pos) {
        log::warn!("Reloaded source seek failed for {}: {}", track.file_path, e);
    }

    {
        let v = lock_state(state).volume;
        player.set_volume(slider_to_gain(v));
    }

    true
}

fn load_track_from_db(
    db: &Arc<Mutex<rusqlite::Connection>>,
    track_id: i64,
) -> Result<Track, String> {
    let conn = lock_db(db);
    let mut stmt = conn
        .prepare(
            "SELECT t.id, t.file_path, t.title, t.track_number, t.disc_number, t.duration_ms, \
             t.year, t.genre, t.album_id, t.embedded_lyrics, t.lrc_offset_ms, al.title AS album_title, t.lyrics_source \
             FROM tracks t \
             LEFT JOIN albums al ON al.id = t.album_id \
             WHERE t.id = ?",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map([track_id], |row| {
            Ok(Track {
                id: row.get(0)?,
                file_path: row.get(1)?,
                title: row.get(2)?,
                track_number: row.get(3)?,
                disc_number: row.get(4)?,
                duration_ms: row.get(5)?,
                year: row.get(6)?,
                genre: row.get(7)?,
                album_id: row.get(8)?,
                embedded_lyrics: row.get(9)?,
                lrc_offset_ms: row.get(10)?,
                lyrics_source: row.get(12)?,
                artist_ids: Vec::new(),
                artist_names: Vec::new(),
                album_title: row.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut track = rows
        .next()
        .ok_or_else(|| "track not found".to_string())?
        .map_err(|e| e.to_string())?;
    drop(rows);
    drop(stmt);
    let (ids, names) = load_track_artists(&conn, track_id)?;
    track.artist_ids = ids;
    track.artist_names = names;
    Ok(track)
}

fn load_track_artists(
    conn: &rusqlite::Connection,
    track_id: i64,
) -> Result<(Vec<i64>, Vec<String>), String> {
    let mut stmt = conn
        .prepare(
            "SELECT a.id, a.name FROM artists a \
             JOIN track_artists ta ON ta.artist_id = a.id \
             WHERE ta.track_id = ? AND ta.role = 'main' \
             ORDER BY a.name",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([track_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?;
    let mut ids = Vec::new();
    let mut names = Vec::new();
    for row in rows {
        let (id, name) = row.map_err(|e| e.to_string())?;
        ids.push(id);
        names.push(name);
    }
    Ok((ids, names))
}

fn emit_state_changed(app_handle: &AppHandle, state: &Arc<Mutex<SharedState>>) {
    let ps = build_playback_state(state);
    log::debug!(
        target: "sparkle::playback",
        "event=state_published playing={} track_id={:?} position_ms={} duration_ms={}",
        ps.is_playing,
        ps.current_track.as_ref().map(|track| track.id),
        ps.position_ms,
        ps.duration_ms
    );
    #[cfg(desktop)]
    {
        let bridge = app_handle.state::<crate::MediaControlBridge>();
        if bridge
            .session_ready
            .load(std::sync::atomic::Ordering::Acquire)
        {
            if let Err(err) = crate::sync_system_media_status(app_handle, &ps) {
                log::warn!(
                    target: "sparkle::media::smtc",
                    "event=status_sync_failed source=audio_state error={err}"
                );
            }
        }
    }
    let discord = lock_state(state).discord.clone();
    discord.update(&ps);
    let event = PlaybackStateChangedEvent {
        is_playing: ps.is_playing,
        current_track: ps.current_track,
        position_ms: ps.position_ms,
        duration_ms: ps.duration_ms,
        shuffle: ps.shuffle,
        repeat_mode: ps.repeat_mode,
    };
    let _ = app_handle.emit("playback-state-changed", event);
}

fn emit_queue_changed(app_handle: &AppHandle) {
    let _ = app_handle.emit("queue-changed", ());
}

fn build_queue_view(
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
) -> QueueView {
    let (queue_ids, play_order, order_pos) = {
        let s = lock_state(state);
        (s.queue.clone(), s.play_order.clone(), s.order_pos)
    };
    let mut tracks = Vec::with_capacity(play_order.len());
    for pos in &play_order {
        if let Some(id) = queue_ids.get(*pos) {
            match load_track_from_db(db, *id) {
                Ok(t) => tracks.push(t),
                Err(e) => {
                    log::error!("Failed to load queued track {} from db: {}", id, e);
                }
            }
        }
    }
    QueueView {
        tracks,
        current_pos: order_pos,
    }
}

fn build_playback_state(state: &Arc<Mutex<SharedState>>) -> PlaybackState {
    let s = lock_state(state);
    PlaybackState {
        is_playing: s.is_playing,
        current_track: s.current_track.clone(),
        position_ms: s.position_ms,
        duration_ms: s.duration_ms,
        volume: s.volume,
        shuffle: s.shuffle,
        repeat_mode: s.repeat_mode,
    }
}

fn lock_state(state: &Arc<Mutex<SharedState>>) -> MutexGuard<'_, SharedState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

fn lock_db(db: &Arc<Mutex<rusqlite::Connection>>) -> MutexGuard<'_, rusqlite::Connection> {
    db.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_order_when_not_shuffled() {
        let (order, pos) = build_play_order(5, 2, false);
        assert_eq!(order, vec![0, 1, 2, 3, 4]);
        assert_eq!(pos, 2);
    }

    #[test]
    fn shuffled_order_keeps_start_first_and_is_permutation() {
        let (order, pos) = build_play_order(10, 3, true);
        assert_eq!(pos, 0);
        assert_eq!(order[0], 3);
        assert!(is_valid_play_order(&order, 10));
    }

    #[test]
    fn empty_queue_builds_empty_order() {
        let (order, pos) = build_play_order(0, 0, true);
        assert!(order.is_empty());
        assert_eq!(pos, 0);
    }

    #[test]
    fn play_order_validation() {
        assert!(is_valid_play_order(&[2, 0, 1], 3));
        assert!(!is_valid_play_order(&[0, 0, 1], 3));
        assert!(!is_valid_play_order(&[0, 1], 3));
        assert!(!is_valid_play_order(&[0, 1, 5], 3));
        assert!(!is_valid_play_order(&[], 1));
    }

    #[test]
    fn dedup_keeps_first_occurrence_order() {
        let (deduped, start) = dedup_queue(vec![10, 20, 10, 30, 20], 0);
        assert_eq!(deduped, vec![10, 20, 30]);
        assert_eq!(start, 0);
    }

    #[test]
    fn dedup_remaps_start_index_to_same_track() {
        // start_index 2 is track id 10; after dedup it lives at index 0.
        let (deduped, start) = dedup_queue(vec![20, 20, 10, 30], 2);
        assert_eq!(deduped, vec![20, 10, 30]);
        assert_eq!(start, 1);
    }

    #[test]
    fn dedup_empty_queue() {
        let (deduped, start) = dedup_queue(Vec::new(), 0);
        assert!(deduped.is_empty());
        assert_eq!(start, 0);
    }

    #[test]
    fn meaningful_listens_ignore_previews_but_keep_short_tracks() {
        assert!(!is_meaningful_listen(4_999, 8_000));
        assert!(is_meaningful_listen(5_000, 8_000));
        assert!(!is_meaningful_listen(29_999, 180_000));
        assert!(is_meaningful_listen(30_000, 180_000));
    }
}
