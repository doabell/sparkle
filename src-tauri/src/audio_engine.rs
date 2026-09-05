use crate::analytics::{
    is_completed, is_meaningful_listen, new_trace_id, now_epoch_ms, ListenEndReason, ListenRecord,
    ListenStartReason, PlaybackContext, PlaybackEventKind, PlaybackEventRecord, PlaybackSource,
    LISTENING_SESSION_GAP_MS,
};
use crate::cache;
use crate::db_writer::DbWriter;
use crate::discord::DiscordPresence;
use crate::loudness::{GainAvailability, LoudnessController, NEXT_UP_COUNT};
use crate::models::{CachedImage, PlaybackState, QueueView, RepeatMode, Track};
use crate::providers::lyrics::{self, TrackMetadata};
use crate::settings::{load_album_art_sources, load_lyrics_sources, load_session, SessionSnapshot};
use rodio::{Decoder, DeviceSinkBuilder, Float, MixerDeviceSink, Player};
use serde::Serialize;
use std::fs::File;
use std::io::BufReader;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const PROGRESS_INTERVAL_MS: u64 = 250;
const COMMAND_REPLY_TIMEOUT: Duration = Duration::from_secs(3);
const DEVICE_RETRY_INTERVAL: Duration = Duration::from_secs(1);

type DeviceOpenResult = Result<MixerDeviceSink, String>;

/// Opening a WASAPI sink can block indefinitely while Windows is bringing an
/// audio endpoint back after sleep. Keep that operation away from the audio
/// command worker so Play/Pause and shutdown can still be serviced.
fn spawn_device_open() -> mpsc::Receiver<DeviceOpenResult> {
    let (result_tx, result_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = DeviceSinkBuilder::open_default_sink()
            .inspect(|sink| {
                #[cfg(target_os = "windows")]
                log::info!(
                    target: "sparkle::audio",
                    "event=output_opened backend=wasapi mode=shared internal_precision_bits=64 config={:?}",
                    sink.config()
                );
                #[cfg(not(target_os = "windows"))]
                log::info!(
                    target: "sparkle::audio",
                    "event=output_opened backend=system mode=default internal_precision_bits=64 config={:?}",
                    sink.config()
                );
            })
            .map_err(|error| error.to_string());
        let _ = result_tx.send(result);
    });
    result_rx
}

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
fn slider_to_gain(volume: f64) -> Float {
    let v = volume.clamp(0.0, 1.0);
    v.powf(5.0 / 3.0) as Float
}

fn db_to_gain(gain_db: f64) -> Float {
    10.0_f64.powf(gain_db.min(0.0) / 20.0) as Float
}

fn combined_gain(volume: f64, sound_check_gain_db: f64) -> Float {
    slider_to_gain(volume) * db_to_gain(sound_check_gain_db)
}

fn apply_player_volume(player: &Player, state: &Arc<Mutex<SharedState>>) {
    let (volume, sound_check_gain_db) = {
        let s = lock_state(state);
        (s.volume, s.latched_sound_check_gain_db)
    };
    player.set_volume(combined_gain(volume, sound_check_gain_db));
}

fn gain_for_track(
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    track_id: i64,
) -> GainAvailability {
    let enabled = lock_state(state).sound_check_enabled;
    if !enabled {
        return GainAvailability::Ready(0.0);
    }
    let conn = lock_db(db);
    match crate::loudness::gain_for_track(&conn, track_id) {
        Ok(gain) => gain,
        Err(error) => {
            // A database failure must not strand playback in a permanent
            // loading state. Unity is the only non-destructive fallback.
            log::warn!(
                target: "sparkle::loudness",
                "event=playback_gain_unavailable track_id={track_id} error={error} fallback=unity"
            );
            GainAvailability::Ready(0.0)
        }
    }
}

/// Chooses a fixed gain without delaying playback. A pending measurement gets
/// transparent unity for this play; its result is available on a later load.
fn immediate_start_gain(gain: GainAvailability) -> (f64, bool) {
    match gain {
        GainAvailability::Ready(gain_db) => (gain_db, false),
        GainAvailability::Pending => (0.0, true),
    }
}

fn refresh_loudness_priorities(state: &Arc<Mutex<SharedState>>) {
    let (loudness, track_ids) = {
        let s = lock_state(state);
        let ids = if let Some(order_pos) = s.order_pos {
            s.play_order
                .iter()
                .skip(order_pos)
                .take(NEXT_UP_COUNT + 1)
                .filter_map(|queue_index| s.queue.get(*queue_index).copied())
                .collect()
        } else {
            s.queue.iter().take(NEXT_UP_COUNT + 1).copied().collect()
        };
        (s.loudness.clone(), ids)
    };
    loudness.prioritize(track_ids);
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
    worker: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl AudioController {
    pub fn new(
        app_handle: AppHandle,
        db: Arc<Mutex<rusqlite::Connection>>,
        discord: DiscordPresence,
        loudness: LoudnessController,
        sound_check_enabled: bool,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(SharedState {
            queue: Vec::new(),
            queue_index: None,
            play_order: Vec::new(),
            order_pos: None,
            current_track: None,
            first_lyric_line: None,
            album_art: None,
            is_playing: false,
            play_when_device_ready: false,
            pending_play_source: PlaybackSource::Unknown,
            position_ms: 0,
            duration_ms: 0,
            volume: 1.0,
            sound_check_enabled,
            latched_sound_check_gain_db: 0.0,
            shuffle: false,
            repeat_mode: RepeatMode::Off,
            seek_target: None,
            context: PlaybackContext::default(),
            active_session_id: None,
            active_listen_id: None,
            listen_started_at_ms: None,
            listen_start_position_ms: 0,
            listen_start_source: PlaybackSource::Unknown,
            listen_start_reason: ListenStartReason::Unknown,
            session_last_active_at_ms: None,
            listened_ms: 0,
            last_counted_position_ms: None,
            loudness,
            discord,
        }));
        let state_clone = state.clone();
        let writer = DbWriter::new(crate::db::db_path(&app_handle));
        let worker = std::thread::spawn(move || {
            audio_thread(rx, app_handle, state_clone, db, writer);
        });
        Self {
            tx,
            state,
            worker: Arc::new(Mutex::new(Some(worker))),
        }
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
        source: PlaybackSource,
        context: PlaybackContext,
    ) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::LoadQueue(
                track_ids,
                start_index,
                shuffle,
                source,
                context.sanitized(),
            ))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn play_track(
        &self,
        track_id: i64,
        source: PlaybackSource,
        context: PlaybackContext,
    ) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::PlayTrack(
                track_id,
                source,
                context.sanitized(),
            ))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn play(&self, source: PlaybackSource) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::Play(source))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn pause(&self, source: PlaybackSource) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::Pause(source))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn stop(&self, source: PlaybackSource) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::Stop(source))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn seek(&self, position_ms: i64, source: PlaybackSource) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::Seek(position_ms, source))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn next_track(&self, source: PlaybackSource) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::Next(source))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn previous_track(&self, source: PlaybackSource) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::Previous(source))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn set_volume(&self, volume: f64, source: PlaybackSource) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::SetVolume(volume, source))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn set_sound_check_enabled(&self, enabled: bool) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::SetSoundCheckEnabled(enabled))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn set_shuffle(
        &self,
        shuffle: bool,
        source: PlaybackSource,
    ) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::SetShuffle(shuffle, source))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn cycle_repeat_mode(&self, source: PlaybackSource) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::CycleRepeatMode(source))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn play_next(
        &self,
        track_id: i64,
        source: PlaybackSource,
    ) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::PlayNext(track_id, source))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn get_queue(&self) -> Result<QueueView, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(AudioCommand::GetQueue(reply_tx))
            .map_err(|e| e.to_string())?;
        receive_audio_reply(reply_rx, COMMAND_REPLY_TIMEOUT, "queue")
    }

    pub fn play_queue_index(
        &self,
        order_pos: usize,
        source: PlaybackSource,
    ) -> Result<PlaybackState, String> {
        self.tx
            .send(AudioCommand::PlayAt(order_pos, source))
            .map_err(|e| e.to_string())?;
        self.get_playback_state()
    }

    pub fn get_playback_state(&self) -> Result<PlaybackState, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(AudioCommand::GetState(reply_tx))
            .map_err(|e| e.to_string())?;
        receive_audio_reply(reply_rx, COMMAND_REPLY_TIMEOUT, "playback state")
    }

    /// Stops playback, persists the final meaningful listen and session, then
    /// waits for both the audio and database writer threads to finish.
    pub fn shutdown(&self) -> Result<(), String> {
        let mut worker_guard = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        if worker_guard.is_none() {
            return Ok(());
        }

        let (reply_tx, reply_rx) = mpsc::channel();
        let send_result = self.tx.send(AudioCommand::Shutdown(reply_tx));
        if send_result.is_err() {
            let worker = worker_guard.take();
            drop(worker_guard);
            join_audio_worker(worker)?;
            return Err("audio engine stopped before shutdown was requested".to_string());
        }

        // Database operations have their own bounded busy retries. Wait for
        // the explicit durability barrier here so application exit cannot
        // overtake the final session/history writes.
        let shutdown_result = match reply_rx.recv_timeout(COMMAND_REPLY_TIMEOUT) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Do not join a worker that is stuck in an OS audio call. The
                // handle is intentionally detached so application exit can
                // continue and release the native media session.
                worker_guard.take();
                drop(worker_guard);
                return Err(format!(
                    "audio engine timed out waiting for shutdown after {} seconds",
                    COMMAND_REPLY_TIMEOUT.as_secs()
                ));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let worker = worker_guard.take();
                drop(worker_guard);
                join_audio_worker(worker)?;
                return Err("audio engine stopped before confirming shutdown".to_string());
            }
        };

        let worker = worker_guard.take();
        drop(worker_guard);
        let join_result = join_audio_worker(worker);
        shutdown_result.and(join_result)
    }
}

fn receive_audio_reply<T>(
    receiver: mpsc::Receiver<T>,
    timeout: Duration,
    description: &str,
) -> Result<T, String> {
    match receiver.recv_timeout(timeout) {
        Ok(value) => Ok(value),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "audio engine timed out waiting for {description} after {} seconds",
            timeout.as_secs()
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(format!(
            "audio engine stopped before replying with {description}"
        )),
    }
}

fn join_audio_worker(worker: Option<std::thread::JoinHandle<()>>) -> Result<(), String> {
    if let Some(worker) = worker {
        worker
            .join()
            .map_err(|_| "audio engine thread panicked during shutdown".to_string())?;
    }
    Ok(())
}

enum AudioCommand {
    LoadQueue(
        Vec<i64>,
        usize,
        Option<bool>,
        PlaybackSource,
        PlaybackContext,
    ),
    PlayTrack(i64, PlaybackSource, PlaybackContext),
    Play(PlaybackSource),
    Pause(PlaybackSource),
    Stop(PlaybackSource),
    Seek(i64, PlaybackSource),
    Next(PlaybackSource),
    Previous(PlaybackSource),
    SetVolume(f64, PlaybackSource),
    SetSoundCheckEnabled(bool),
    SetShuffle(bool, PlaybackSource),
    CycleRepeatMode(PlaybackSource),
    PlayNext(i64, PlaybackSource),
    GetQueue(mpsc::Sender<QueueView>),
    PlayAt(usize, PlaybackSource),
    GetState(mpsc::Sender<PlaybackState>),
    Shutdown(mpsc::Sender<Result<(), String>>),
}

struct SharedState {
    queue: Vec<i64>,
    queue_index: Option<usize>,
    play_order: Vec<usize>,
    order_pos: Option<usize>,
    current_track: Option<Track>,
    first_lyric_line: Option<String>,
    album_art: Option<CachedImage>,
    is_playing: bool,
    /// User intent retained while the OS output endpoint is unavailable.
    play_when_device_ready: bool,
    pending_play_source: PlaybackSource,
    position_ms: i64,
    duration_ms: i64,
    volume: f64,
    /// Sound Check is latched per track: changing the setting or completing a
    /// scan never changes gain after audible playback has begun.
    sound_check_enabled: bool,
    latched_sound_check_gain_db: f64,
    shuffle: bool,
    repeat_mode: RepeatMode,
    seek_target: Option<(i64, Instant)>,
    context: PlaybackContext,
    active_session_id: Option<String>,
    active_listen_id: Option<String>,
    listen_started_at_ms: Option<i64>,
    listen_start_position_ms: i64,
    listen_start_source: PlaybackSource,
    listen_start_reason: ListenStartReason,
    session_last_active_at_ms: Option<i64>,
    /// Actual forward-moving audio time. This is intentionally independent
    /// from position so seeking cannot manufacture listening minutes.
    listened_ms: i64,
    last_counted_position_ms: Option<i64>,
    loudness: LoudnessController,
    discord: DiscordPresence,
}

#[derive(Serialize, Clone)]
struct PlaybackStateChangedEvent {
    is_playing: bool,
    current_track: Option<Track>,
    first_lyric_line: Option<String>,
    album_art: Option<CachedImage>,
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
        context: s.context.clone(),
    };
    drop(s);
    // Non-blocking: the writer thread persists the newest snapshot.
    writer.save_session(snapshot);
    checkpoint_active_listen(state, writer);
}

fn listen_record_locked(
    state: &SharedState,
    finalized: bool,
    ended_at_ms: Option<i64>,
    end_reason: Option<ListenEndReason>,
) -> Option<ListenRecord> {
    let track = state.current_track.as_ref()?;
    Some(ListenRecord {
        id: state.active_listen_id.clone()?,
        session_id: state.active_session_id.clone()?,
        track_id: track.id,
        started_at_ms: state.listen_started_at_ms?,
        ended_at_ms,
        last_activity_at_ms: state.session_last_active_at_ms.unwrap_or_else(now_epoch_ms),
        start_position_ms: state.listen_start_position_ms.max(0),
        end_position_ms: state.position_ms.max(0),
        duration_ms: state.duration_ms.max(0),
        listened_ms: state.listened_ms.max(0),
        meaningful: is_meaningful_listen(state.listened_ms, state.duration_ms),
        completed: finalized && is_completed(state.position_ms, state.duration_ms),
        finalized,
        start_source: state.listen_start_source,
        start_reason: state.listen_start_reason,
        end_reason,
        context: state.context.clone(),
        queue_index: state.queue_index,
        play_order_index: state.order_pos,
        queue_length: state.play_order.len(),
        shuffle: state.shuffle,
        repeat_mode: state.repeat_mode,
    })
}

fn event_record_locked(
    state: &SharedState,
    kind: PlaybackEventKind,
    source: PlaybackSource,
    reason: Option<&str>,
    target_position_ms: Option<i64>,
) -> PlaybackEventRecord {
    PlaybackEventRecord {
        id: new_trace_id("event"),
        listen_id: state.active_listen_id.clone(),
        session_id: state.active_session_id.clone(),
        occurred_at_ms: now_epoch_ms(),
        kind,
        source,
        reason: reason.map(str::to_string),
        track_id: state.current_track.as_ref().map(|track| track.id),
        position_ms: state
            .current_track
            .as_ref()
            .map(|_| state.position_ms.max(0)),
        target_position_ms,
        context: state.context.clone(),
        queue_index: state.queue_index,
        play_order_index: state.order_pos,
        queue_length: state.play_order.len(),
        shuffle: state.shuffle,
        repeat_mode: state.repeat_mode,
    }
}

fn record_event(
    state: &Arc<Mutex<SharedState>>,
    writer: &DbWriter,
    kind: PlaybackEventKind,
    source: PlaybackSource,
    reason: Option<&str>,
    target_position_ms: Option<i64>,
) {
    let event = event_record_locked(&lock_state(state), kind, source, reason, target_position_ms);
    writer.record_event(event);
}

fn checkpoint_active_listen(state: &Arc<Mutex<SharedState>>, writer: &DbWriter) {
    let record = listen_record_locked(&lock_state(state), false, None, None);
    if let Some(record) = record {
        writer.upsert_listen(record);
    }
}

fn begin_active_listen(
    state: &Arc<Mutex<SharedState>>,
    writer: &DbWriter,
    source: PlaybackSource,
    reason: ListenStartReason,
) {
    let (record, event, new_session) = {
        let mut s = lock_state(state);
        if s.current_track.is_none() || s.active_listen_id.is_some() {
            return;
        }
        let now = now_epoch_ms();
        let needs_session = s.active_session_id.is_none()
            || s.session_last_active_at_ms
                .is_some_and(|last| now.saturating_sub(last) > LISTENING_SESSION_GAP_MS);
        if needs_session {
            s.active_session_id = Some(new_trace_id("session"));
        }
        s.active_listen_id = Some(new_trace_id("listen"));
        s.listen_started_at_ms = Some(now);
        s.listen_start_position_ms = s.position_ms.max(0);
        s.listen_start_source = source;
        s.listen_start_reason = reason;
        s.session_last_active_at_ms = Some(now);
        s.listened_ms = 0;
        s.last_counted_position_ms = None;
        let record = listen_record_locked(&s, false, None, None)
            .expect("a listen is complete immediately after it starts");
        let event = event_record_locked(
            &s,
            PlaybackEventKind::TrackStarted,
            source,
            Some(reason.as_str()),
            None,
        );
        (record, event, needs_session)
    };
    writer.upsert_listen(record.clone());
    writer.record_event(event);
    log::info!(
        target: "sparkle::playback",
        "event=listen_started listen_id={} session_id={} track_id={} source={} reason={} context={} queue_index={} play_order_index={} queue_length={} new_session={}",
        record.id,
        record.session_id,
        record.track_id,
        source.as_str(),
        reason.as_str(),
        record.context.kind,
        record.queue_index.map(|index| index.to_string()).unwrap_or_else(|| "none".to_string()),
        record.play_order_index.map(|index| index.to_string()).unwrap_or_else(|| "none".to_string()),
        record.queue_length,
        new_session
    );
}

fn finalize_active_listen(
    state: &Arc<Mutex<SharedState>>,
    writer: &DbWriter,
    reason: ListenEndReason,
    source: PlaybackSource,
) -> bool {
    let result = {
        let mut s = lock_state(state);
        let ended_at_ms = now_epoch_ms();
        let Some(record) = listen_record_locked(&s, true, Some(ended_at_ms), Some(reason)) else {
            return false;
        };
        let event = event_record_locked(
            &s,
            PlaybackEventKind::ListenEnded,
            source,
            Some(reason.as_str()),
            None,
        );
        s.active_listen_id = None;
        s.listen_started_at_ms = None;
        s.listen_start_position_ms = 0;
        s.listen_start_source = PlaybackSource::Unknown;
        s.listen_start_reason = ListenStartReason::Unknown;
        s.listened_ms = 0;
        s.last_counted_position_ms = None;
        Some((record, event))
    };
    let Some((record, event)) = result else {
        return false;
    };
    writer.upsert_listen(record.clone());
    writer.record_event(event);
    log::info!(
        target: "sparkle::playback",
        "event=listen_ended listen_id={} session_id={} track_id={} source={} reason={} listened_ms={} position_ms={} duration_ms={} meaningful={} completed={}",
        record.id,
        record.session_id,
        record.track_id,
        source.as_str(),
        reason.as_str(),
        record.listened_ms,
        record.end_position_ms,
        record.duration_ms,
        record.meaningful,
        record.completed
    );
    true
}

fn end_active_session(state: &Arc<Mutex<SharedState>>) {
    let mut s = lock_state(state);
    s.active_session_id = None;
    s.session_last_active_at_ms = None;
}

fn resume_or_begin_listen(
    state: &Arc<Mutex<SharedState>>,
    writer: &DbWriter,
    source: PlaybackSource,
) {
    let (has_active, timed_out, pending_source) = {
        let s = lock_state(state);
        (
            s.active_listen_id.is_some(),
            s.active_listen_id.is_some()
                && s.session_last_active_at_ms.is_some_and(|last| {
                    now_epoch_ms().saturating_sub(last) > LISTENING_SESSION_GAP_MS
                }),
            s.pending_play_source,
        )
    };
    if timed_out {
        finalize_active_listen(state, writer, ListenEndReason::SessionTimeout, source);
        end_active_session(state);
        begin_active_listen(
            state,
            writer,
            source,
            ListenStartReason::ResumeAfterInactivity,
        );
    } else if has_active {
        record_event(
            state,
            writer,
            PlaybackEventKind::PlaybackResumed,
            source,
            Some("user_resume"),
            None,
        );
        let (listen_id, track_id) = {
            let s = lock_state(state);
            (
                s.active_listen_id.clone(),
                s.current_track.as_ref().map(|track| track.id),
            )
        };
        log::info!(
            target: "sparkle::playback",
            "event=playback_resumed listen_id={} track_id={} source={}",
            listen_id.as_deref().unwrap_or("none"),
            track_id.map(|id| id.to_string()).unwrap_or_else(|| "none".to_string()),
            source.as_str()
        );
    } else {
        let reason = if pending_source == PlaybackSource::Restore {
            ListenStartReason::RestoredResume
        } else {
            ListenStartReason::Replay
        };
        begin_active_listen(state, writer, source, reason);
    }
}

fn restore_session(
    player: Option<&Player>,
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
) {
    let snapshot_result = {
        let conn = lock_db(db);
        load_session(&conn)
    };
    let snapshot = match snapshot_result {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                target: "sparkle::playback",
                "event=session_restore_failed error={e}"
            );
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
        s.is_playing = false;
        s.play_when_device_ready = false;
        s.pending_play_source = PlaybackSource::Restore;
        s.context = snapshot.context.clone().sanitized();
        s.active_session_id = None;
        s.active_listen_id = None;
        s.listen_started_at_ms = None;
        s.listen_start_position_ms = 0;
        s.listen_start_source = PlaybackSource::Unknown;
        s.listen_start_reason = ListenStartReason::Unknown;
        s.session_last_active_at_ms = None;
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
    if let Some(player) = player {
        apply_player_volume(player, state);
    }

    let index = snapshot.queue_index.unwrap_or(0);
    // Load the restored track paused so startup never produces audible output.
    if let Some(player) = player {
        player.pause();
    }
    if !load_track_at_index_with_autoplay(
        player,
        state,
        db,
        writer,
        app_handle,
        index,
        false,
        PlaybackSource::Restore,
        ListenStartReason::RestoredResume,
        ListenEndReason::QueueReplaced,
    ) {
        update_state_for_stop(
            state,
            writer,
            ListenEndReason::PlaybackError,
            PlaybackSource::Restore,
        );
        emit_state_changed(app_handle, state);
        return;
    }

    if snapshot.position_ms > 0 {
        if let Some(player) = player {
            let pos = Duration::from_millis(snapshot.position_ms as u64);
            if player.try_seek(pos).is_err() {
                reload_source_at_position(player, state, snapshot.position_ms);
                player.pause();
            }
        }
        {
            let mut s = lock_state(state);
            s.position_ms = snapshot.position_ms;
            s.seek_target = Some((snapshot.position_ms, Instant::now()));
        }
    }

    // Always start paused on launch, even if the saved session was playing.
    if let Some(player) = player {
        player.pause();
    }
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
            log::error!(
                target: "sparkle::audio",
                "event=source_open_failed track_id={} error={e}",
                track.id
            );
            return false;
        }
    };
    let decoded_source = match Decoder::new(BufReader::new(file)) {
        Ok(s) => s,
        Err(e) => {
            log::error!(
                target: "sparkle::audio",
                "event=source_decode_failed track_id={} error={e}",
                track.id
            );
            return false;
        }
    };
    player.stop();
    player.clear();
    player.append(decoded_source);
    true
}

fn reload_current_for_device(
    player: &Player,
    state: &Arc<Mutex<SharedState>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
) {
    let (track, was_playing, position_ms, pending_source, had_active_listen, session_timed_out) = {
        let s = lock_state(state);
        (
            s.current_track.clone(),
            s.is_playing || s.play_when_device_ready,
            s.position_ms,
            s.pending_play_source,
            s.active_listen_id.is_some(),
            s.active_listen_id.is_some()
                && s.session_last_active_at_ms.is_some_and(|last| {
                    now_epoch_ms().saturating_sub(last) > LISTENING_SESSION_GAP_MS
                }),
        )
    };
    let track = match track {
        Some(t) => t,
        None => return,
    };

    player.pause();
    if !load_source_into_player(player, &track) {
        update_state_for_stop(
            state,
            writer,
            ListenEndReason::PlaybackError,
            PlaybackSource::Internal,
        );
        emit_state_changed(app_handle, state);
        return;
    }

    apply_player_volume(player, state);

    if position_ms > 0 {
        let pos = Duration::from_millis(position_ms as u64);
        let _ = player.try_seek(pos);
    }

    if was_playing {
        player.play();
    } else {
        player.pause();
    }

    {
        let mut s = lock_state(state);
        s.seek_target = Some((position_ms, Instant::now()));
        s.play_when_device_ready = false;
        s.is_playing = was_playing;
    }

    if was_playing {
        record_event(
            state,
            writer,
            PlaybackEventKind::OutputRestored,
            PlaybackSource::Internal,
            Some("device_available"),
            None,
        );
        if had_active_listen && session_timed_out {
            finalize_active_listen(
                state,
                writer,
                ListenEndReason::SessionTimeout,
                PlaybackSource::Internal,
            );
            end_active_session(state);
            begin_active_listen(
                state,
                writer,
                PlaybackSource::Internal,
                ListenStartReason::OutputRestored,
            );
        } else if !had_active_listen {
            begin_active_listen(
                state,
                writer,
                pending_source,
                ListenStartReason::OutputRestored,
            );
        } else {
            let mut s = lock_state(state);
            s.session_last_active_at_ms = Some(now_epoch_ms());
        }
        log::info!(
            target: "sparkle::audio",
            "event=output_restored track_id={} resumed=true session_rotated={session_timed_out}",
            track.id
        );
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
    // Session restoration is logical state first. It must not depend on an
    // output endpoint being present (common during RDP and device switching).
    restore_session(None, &state, &db, &writer, &app_handle);
    let mut device_error_logged = false;
    let mut pending_device_open = None;

    'pipeline: loop {
        let handle = match wait_for_device(
            &rx,
            &state,
            &db,
            &writer,
            &app_handle,
            &mut pending_device_open,
            &mut device_error_logged,
        ) {
            DeviceWaitFlow::Ready(handle) => handle,
            DeviceWaitFlow::Shutdown(reply) => {
                finish_audio_thread(None, &state, writer, Some(reply));
                return;
            }
            DeviceWaitFlow::Disconnected => {
                finish_audio_thread(None, &state, writer, None);
                return;
            }
        };
        let player = Player::connect_new(handle.mixer());
        let device_name = default_output_device_id();
        reload_current_for_device(&player, &state, &writer, &app_handle);

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
            loop {
                match rx.try_recv() {
                    Ok(cmd) => {
                        match handle_command(cmd, Some(&player), &state, &db, &writer, &app_handle)
                        {
                            CommandFlow::Continue => {}
                            CommandFlow::Shutdown(reply) => {
                                finish_audio_thread(Some(&player), &state, writer, Some(reply));
                                return;
                            }
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        finish_audio_thread(Some(&player), &state, writer, None);
                        return;
                    }
                }
            }

            // Detect default-device changes (headphones, USB DAC, Bluetooth).
            if last_device_check.elapsed() >= device_check_interval {
                last_device_check = Instant::now();
                let new_name = default_output_device_id();
                if new_name != device_name {
                    log::info!(target: "sparkle::audio", "event=output_device_changed");
                    mark_output_unavailable(&state, &writer, &app_handle);
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
                            s.session_last_active_at_ms = Some(now_epoch_ms());
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
                        log::warn!(
                            target: "sparkle::audio",
                            "event=output_device_lost recovery=recreate_pipeline"
                        );
                        mark_output_unavailable(&state, &writer, &app_handle);
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
                    advance(
                        Some(&player),
                        &state,
                        &db,
                        &writer,
                        &app_handle,
                        true,
                        PlaybackSource::Automatic,
                    );
                }
            }

            std::thread::sleep(Duration::from_millis(50));

            // Keep the device sink handle alive.
            let _ = &handle;
        }
    }
}

enum DeviceWaitFlow {
    Ready(MixerDeviceSink),
    Shutdown(mpsc::Sender<Result<(), String>>),
    Disconnected,
}

fn wait_for_device(
    rx: &mpsc::Receiver<AudioCommand>,
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
    pending_device_open: &mut Option<mpsc::Receiver<DeviceOpenResult>>,
    device_error_logged: &mut bool,
) -> DeviceWaitFlow {
    let mut retry_at = Instant::now();
    loop {
        if pending_device_open.is_none() && Instant::now() >= retry_at {
            *pending_device_open = Some(spawn_device_open());
        }

        let open_result = match pending_device_open.as_ref() {
            Some(result_rx) => match result_rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "device opener stopped before returning a result".to_string(),
                )),
                Err(mpsc::TryRecvError::Empty) => None,
            },
            None => None,
        };

        if let Some(result) = open_result {
            pending_device_open.take();
            match result {
                Ok(handle) => {
                    if *device_error_logged {
                        log::info!(target: "sparkle::audio", "event=output_device_available");
                        *device_error_logged = false;
                    }
                    return DeviceWaitFlow::Ready(handle);
                }
                Err(error) => {
                    mark_output_unavailable(state, writer, app_handle);
                    if *device_error_logged {
                        log::debug!(target: "sparkle::audio", "event=output_device_retry_failed error={error}");
                    } else {
                        log::warn!(target: "sparkle::audio", "event=output_device_unavailable recovery=retry error={error}");
                        *device_error_logged = true;
                    }
                    retry_at = Instant::now() + DEVICE_RETRY_INTERVAL;
                }
            }
        }

        // Poll commands while the opener is in an OS call. In particular,
        // Shutdown must not wait for WASAPI to return before it can finish.
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(cmd) => match handle_command(cmd, None, state, db, writer, app_handle) {
                CommandFlow::Continue => {}
                CommandFlow::Shutdown(reply) => return DeviceWaitFlow::Shutdown(reply),
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return DeviceWaitFlow::Disconnected;
            }
        }
    }
}

fn mark_output_unavailable(
    state: &Arc<Mutex<SharedState>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
) {
    let (newly_unavailable, should_resume, track_id) = {
        let mut s = lock_state(state);
        let newly_unavailable = s.is_playing;
        let should_resume = s.is_playing || s.play_when_device_ready;
        s.is_playing = false;
        s.play_when_device_ready = should_resume;
        s.last_counted_position_ms = None;
        (
            newly_unavailable,
            should_resume,
            s.current_track.as_ref().map(|track| track.id),
        )
    };
    if newly_unavailable {
        record_event(
            state,
            writer,
            PlaybackEventKind::OutputUnavailable,
            PlaybackSource::Internal,
            Some("device_unavailable"),
            None,
        );
        log::warn!(
            target: "sparkle::audio",
            "event=output_lost track_id={} resume_pending={should_resume}",
            track_id.map(|id| id.to_string()).unwrap_or_else(|| "none".to_string())
        );
    }
    if newly_unavailable || should_resume {
        emit_state_changed(app_handle, state);
        save_session_to_db(state, writer);
    }
}

fn finish_audio_thread(
    player: Option<&Player>,
    state: &Arc<Mutex<SharedState>>,
    writer: DbWriter,
    reply: Option<mpsc::Sender<Result<(), String>>>,
) {
    if let Some(player) = player {
        player.pause();
    }
    {
        let mut s = lock_state(state);
        s.is_playing = false;
        s.play_when_device_ready = false;
        s.last_counted_position_ms = None;
    }
    finalize_active_listen(
        state,
        &writer,
        ListenEndReason::AppShutdown,
        PlaybackSource::Internal,
    );
    end_active_session(state);
    save_session_to_db(state, &writer);
    let result = writer.shutdown();
    if let Some(reply) = reply {
        let _ = reply.send(result);
    } else if let Err(error) = result {
        log::error!(
            target: "sparkle::analytics::writer",
            "event=shutdown_flush_failed error={error}"
        );
    }
}

enum CommandFlow {
    Continue,
    Shutdown(mpsc::Sender<Result<(), String>>),
}

fn handle_command(
    cmd: AudioCommand,
    player: Option<&Player>,
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
) -> CommandFlow {
    match cmd {
        AudioCommand::LoadQueue(track_ids, start_index, shuffle_override, source, context) => {
            finalize_active_listen(state, writer, ListenEndReason::QueueReplaced, source);
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
                s.play_when_device_ready = false;
                s.latched_sound_check_gain_db = 0.0;
                s.pending_play_source = source;
                s.context = context;
                s.current_track = None;
                s.first_lyric_line = None;
                s.album_art = None;
                s.position_ms = 0;
                s.duration_ms = 0;
                s.listened_ms = 0;
                s.last_counted_position_ms = None;
            }
            record_event(
                state,
                writer,
                PlaybackEventKind::QueueLoaded,
                source,
                Some("queue_replaced"),
                None,
            );
            // Take one snapshot and release the mutex before logging. The log
            // facade evaluates debug arguments because its global max level is
            // Trace, even when the plugin later filters the record. Locking
            // `state` separately for both arguments therefore self-deadlocked.
            let (context_kind, queue_length) = {
                let s = lock_state(state);
                (s.context.kind.clone(), s.queue.len())
            };
            log::debug!(
                target: "sparkle::playback",
                "event=queue_loaded source={} context={} queue_length={} start_index={} shuffle={}",
                source.as_str(),
                context_kind,
                queue_length,
                start_index,
                shuffle
            );
            if let Some(i) = idx {
                if !load_track_at_index(
                    player,
                    state,
                    db,
                    writer,
                    app_handle,
                    i,
                    source,
                    ListenStartReason::QueueStarted,
                    ListenEndReason::QueueReplaced,
                ) {
                    update_state_for_stop(state, writer, ListenEndReason::PlaybackError, source);
                    emit_state_changed(app_handle, state);
                }
            } else {
                if let Some(player) = player {
                    player.stop();
                    player.clear();
                }
                update_state_for_stop(state, writer, ListenEndReason::QueueReplaced, source);
                emit_state_changed(app_handle, state);
            }
            emit_queue_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::PlayTrack(track_id, source, context) => {
            finalize_active_listen(state, writer, ListenEndReason::TrackSelected, source);
            {
                let mut s = lock_state(state);
                s.queue = vec![track_id];
                s.queue_index = Some(0);
                s.play_order = vec![0];
                s.order_pos = Some(0);
                s.context = context;
                s.pending_play_source = source;
            }
            record_event(
                state,
                writer,
                PlaybackEventKind::QueueLoaded,
                source,
                Some("single_track"),
                None,
            );
            if !load_track_at_index(
                player,
                state,
                db,
                writer,
                app_handle,
                0,
                source,
                ListenStartReason::TrackSelected,
                ListenEndReason::TrackSelected,
            ) {
                update_state_for_stop(state, writer, ListenEndReason::PlaybackError, source);
                emit_state_changed(app_handle, state);
            }
            emit_queue_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::Play(source) => {
            let (has_track, was_playing) = {
                let s = lock_state(state);
                (
                    s.current_track.is_some(),
                    s.is_playing || s.play_when_device_ready,
                )
            };
            log::debug!(target: "sparkle::playback", "event=command_received command=play source={} was_playing={was_playing}", source.as_str());
            if has_track {
                if let Some(player) = player {
                    player.play();
                    {
                        let mut s = lock_state(state);
                        s.is_playing = true;
                        s.play_when_device_ready = false;
                        s.pending_play_source = source;
                    }
                    if !was_playing {
                        resume_or_begin_listen(state, writer, source);
                    }
                } else {
                    // Commands still resolve when no output exists, but the
                    // logical state must never claim inaudible playback.
                    let mut s = lock_state(state);
                    s.is_playing = false;
                    s.play_when_device_ready = true;
                    s.pending_play_source = source;
                    s.last_counted_position_ms = None;
                }
                emit_state_changed(app_handle, state);
                save_session_to_db(state, writer);
            } else {
                let idx = lock_state(state).queue_index;
                if let Some(i) = idx {
                    if !load_track_at_index(
                        player,
                        state,
                        db,
                        writer,
                        app_handle,
                        i,
                        source,
                        ListenStartReason::RestoredResume,
                        ListenEndReason::QueueReplaced,
                    ) {
                        update_state_for_stop(
                            state,
                            writer,
                            ListenEndReason::PlaybackError,
                            source,
                        );
                        emit_state_changed(app_handle, state);
                    }
                    save_session_to_db(state, writer);
                }
            }
        }
        AudioCommand::Pause(source) => {
            let was_active = {
                let s = lock_state(state);
                s.is_playing || s.play_when_device_ready
            };
            if let Some(player) = player {
                player.pause();
            }
            {
                let mut s = lock_state(state);
                s.is_playing = false;
                s.play_when_device_ready = false;
                s.pending_play_source = source;
                s.last_counted_position_ms = None;
            }
            if was_active {
                record_event(
                    state,
                    writer,
                    PlaybackEventKind::PlaybackPaused,
                    source,
                    Some("user_pause"),
                    None,
                );
                let (listen_id, track_id, position_ms) = {
                    let s = lock_state(state);
                    (
                        s.active_listen_id.clone(),
                        s.current_track.as_ref().map(|track| track.id),
                        s.position_ms,
                    )
                };
                log::info!(
                    target: "sparkle::playback",
                    "event=playback_paused listen_id={} track_id={} source={} position_ms={position_ms}",
                    listen_id.as_deref().unwrap_or("none"),
                    track_id.map(|id| id.to_string()).unwrap_or_else(|| "none".to_string()),
                    source.as_str()
                );
            }
            emit_state_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::Stop(source) => {
            record_event(
                state,
                writer,
                PlaybackEventKind::PlaybackStopped,
                source,
                Some("user_stop"),
                None,
            );
            if let Some(player) = player {
                player.stop();
                player.clear();
            }
            update_state_for_stop(state, writer, ListenEndReason::Stopped, source);
            end_active_session(state);
            emit_state_changed(app_handle, state);
            save_session_to_db(state, writer);
            log::info!(target: "sparkle::playback", "event=playback_stopped source={}", source.as_str());
        }
        AudioCommand::Seek(position_ms, source) => {
            let (old_position_ms, duration_ms) = {
                let s = lock_state(state);
                (s.position_ms, s.duration_ms)
            };
            let position_ms = if duration_ms > 0 {
                position_ms.clamp(0, duration_ms)
            } else {
                position_ms.max(0)
            };
            record_event(
                state,
                writer,
                PlaybackEventKind::Seeked,
                source,
                Some("absolute"),
                Some(position_ms),
            );
            if let Some(player) = player {
                let pos = Duration::from_millis(position_ms as u64);
                let was_playing = !player.is_paused();
                if was_playing {
                    player.pause();
                }
                let seek_ok = player.try_seek(pos).is_ok();
                if !seek_ok {
                    let reloaded = reload_source_at_position(player, state, position_ms);
                    if !reloaded {
                        log::warn!(target: "sparkle::audio", "event=seek_failed target_position_ms={position_ms}");
                    }
                }
                if was_playing {
                    player.play();
                }
            }
            {
                let mut s = lock_state(state);
                s.position_ms = position_ms;
                s.seek_target = Some((position_ms, Instant::now()));
                s.last_counted_position_ms = None;
            }
            emit_state_changed(app_handle, state);
            save_session_to_db(state, writer);
            log::debug!(
                target: "sparkle::playback",
                "event=seek_applied source={} from_position_ms={old_position_ms} to_position_ms={position_ms}",
                source.as_str()
            );
        }
        AudioCommand::Next(source) => {
            advance(player, state, db, writer, app_handle, false, source);
            save_session_to_db(state, writer);
        }
        AudioCommand::Previous(source) => {
            let (pos, order_pos) = {
                let s = lock_state(state);
                (s.position_ms, s.order_pos)
            };
            if pos > 3000 {
                record_event(
                    state,
                    writer,
                    PlaybackEventKind::Seeked,
                    source,
                    Some("previous_restart"),
                    Some(0),
                );
                seek_to_start(player, state);
            } else if let Some(p) = order_pos {
                if p > 0 {
                    let prev_pos = p - 1;
                    finalize_active_listen(state, writer, ListenEndReason::ManualPrevious, source);
                    let prev_idx = {
                        let mut s = lock_state(state);
                        s.order_pos = Some(prev_pos);
                        s.queue_index = s.play_order.get(prev_pos).copied();
                        s.queue_index
                    };
                    if let Some(i) = prev_idx {
                        if !load_track_at_index(
                            player,
                            state,
                            db,
                            writer,
                            app_handle,
                            i,
                            source,
                            ListenStartReason::ManualPrevious,
                            ListenEndReason::ManualPrevious,
                        ) {
                            update_state_for_stop(
                                state,
                                writer,
                                ListenEndReason::PlaybackError,
                                source,
                            );
                            emit_state_changed(app_handle, state);
                        }
                    }
                } else {
                    record_event(
                        state,
                        writer,
                        PlaybackEventKind::Seeked,
                        source,
                        Some("previous_restart"),
                        Some(0),
                    );
                    seek_to_start(player, state);
                }
            }
            emit_queue_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::SetShuffle(shuffle, source) => {
            let previous = lock_state(state).shuffle;
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
            if previous != shuffle {
                record_event(
                    state,
                    writer,
                    PlaybackEventKind::ShuffleChanged,
                    source,
                    Some(if shuffle { "enabled" } else { "disabled" }),
                    None,
                );
                log::debug!(
                    target: "sparkle::playback",
                    "event=shuffle_changed source={} enabled={shuffle}",
                    source.as_str()
                );
            }
            emit_state_changed(app_handle, state);
            emit_queue_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::CycleRepeatMode(source) => {
            let repeat_mode = {
                let mut s = lock_state(state);
                s.repeat_mode = s.repeat_mode.next();
                s.repeat_mode
            };
            record_event(
                state,
                writer,
                PlaybackEventKind::RepeatChanged,
                source,
                Some(match repeat_mode {
                    RepeatMode::Off => "off",
                    RepeatMode::All => "all",
                    RepeatMode::One => "one",
                }),
                None,
            );
            log::debug!(
                target: "sparkle::playback",
                "event=repeat_changed source={} mode={:?}",
                source.as_str(),
                repeat_mode
            );
            emit_state_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::PlayNext(track_id, source) => {
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
                        s.context = PlaybackContext {
                            kind: "queue".to_string(),
                            id: None,
                        };
                        s.pending_play_source = source;
                    }
                    if !load_track_at_index(
                        player,
                        state,
                        db,
                        writer,
                        app_handle,
                        0,
                        source,
                        ListenStartReason::PlayNext,
                        ListenEndReason::TrackSelected,
                    ) {
                        update_state_for_stop(
                            state,
                            writer,
                            ListenEndReason::PlaybackError,
                            source,
                        );
                        emit_state_changed(app_handle, state);
                    }
                }
            }
            let mut event = {
                let s = lock_state(state);
                event_record_locked(
                    &s,
                    PlaybackEventKind::QueuedNext,
                    source,
                    Some("play_next"),
                    None,
                )
            };
            event.track_id = Some(track_id);
            writer.record_event(event);
            log::debug!(
                target: "sparkle::playback",
                "event=track_queued_next source={} track_id={track_id}",
                source.as_str()
            );
            emit_queue_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::GetQueue(reply) => {
            let view = build_queue_view(state, db);
            let _ = reply.send(view);
        }
        AudioCommand::PlayAt(order_pos, source) => {
            let valid = order_pos < lock_state(state).play_order.len();
            if valid {
                finalize_active_listen(state, writer, ListenEndReason::QueueJump, source);
            }
            let next_idx = {
                let mut s = lock_state(state);
                if valid {
                    s.order_pos = Some(order_pos);
                    s.queue_index = s.play_order.get(order_pos).copied();
                    s.queue_index
                } else {
                    None
                }
            };
            if let Some(i) = next_idx {
                if !load_track_at_index(
                    player,
                    state,
                    db,
                    writer,
                    app_handle,
                    i,
                    source,
                    ListenStartReason::QueueJump,
                    ListenEndReason::QueueJump,
                ) {
                    update_state_for_stop(state, writer, ListenEndReason::PlaybackError, source);
                    emit_state_changed(app_handle, state);
                }
            }
            emit_queue_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::SetVolume(volume, source) => {
            let v = volume.clamp(0.0, 1.0);
            {
                let mut s = lock_state(state);
                s.volume = v;
            }
            if let Some(player) = player {
                apply_player_volume(player, state);
            }
            log::trace!(
                target: "sparkle::playback",
                "event=volume_changed source={} value={v:.3}",
                source.as_str()
            );
            emit_state_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::SetSoundCheckEnabled(enabled) => {
            {
                let mut s = lock_state(state);
                s.sound_check_enabled = enabled;
            }
            if enabled {
                refresh_loudness_priorities(state);
            }
            log::info!(
                target: "sparkle::loudness",
                "event=playback_setting_latched enabled={enabled} current_track_unchanged=true"
            );
        }
        AudioCommand::GetState(reply) => {
            let ps = build_playback_state(state);
            let _ = reply.send(ps);
        }
        AudioCommand::Shutdown(reply) => return CommandFlow::Shutdown(reply),
    }
    CommandFlow::Continue
}

fn seek_to_start(player: Option<&Player>, state: &Arc<Mutex<SharedState>>) {
    if let Some(player) = player {
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
    }
    {
        let mut s = lock_state(state);
        s.position_ms = 0;
        s.seek_target = Some((0, Instant::now()));
    }
}

fn advance(
    player: Option<&Player>,
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
    auto: bool,
    source: PlaybackSource,
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
            if !load_track_at_index(
                player,
                state,
                db,
                writer,
                app_handle,
                i,
                PlaybackSource::Automatic,
                ListenStartReason::RepeatOne,
                ListenEndReason::RepeatOne,
            ) {
                update_state_for_stop(
                    state,
                    writer,
                    ListenEndReason::PlaybackError,
                    PlaybackSource::Automatic,
                );
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
        let (start_reason, end_reason) = if auto {
            (ListenStartReason::AutoAdvance, ListenEndReason::Completed)
        } else {
            (ListenStartReason::ManualNext, ListenEndReason::ManualNext)
        };
        finalize_active_listen(state, writer, end_reason, source);
        let next_idx = {
            let mut s = lock_state(state);
            s.order_pos = Some(p);
            s.queue_index = s.play_order.get(p).copied();
            s.queue_index
        };
        if let Some(i) = next_idx {
            if !load_track_at_index(
                player,
                state,
                db,
                writer,
                app_handle,
                i,
                source,
                start_reason,
                end_reason,
            ) {
                update_state_for_stop(state, writer, ListenEndReason::PlaybackError, source);
                emit_state_changed(app_handle, state);
            }
        } else {
            if let Some(player) = player {
                player.stop();
                player.clear();
            }
            update_state_for_stop(state, writer, ListenEndReason::PlaybackError, source);
            emit_state_changed(app_handle, state);
        }
    } else if auto {
        // End of the queue with repeat off: pause and keep the last track
        // loaded at position 0 so pressing play starts it again. The track
        // stays visible instead of the player emptying out.
        if let Some(i) = queue_index {
            if !load_track_at_index_with_autoplay(
                player,
                state,
                db,
                writer,
                app_handle,
                i,
                false,
                PlaybackSource::Automatic,
                ListenStartReason::AutoAdvance,
                ListenEndReason::Completed,
            ) {
                update_state_for_stop(
                    state,
                    writer,
                    ListenEndReason::PlaybackError,
                    PlaybackSource::Automatic,
                );
                emit_state_changed(app_handle, state);
            } else {
                let mut s = lock_state(state);
                s.is_playing = false;
            }
            emit_state_changed(app_handle, state);
        } else {
            if let Some(player) = player {
                player.stop();
                player.clear();
            }
            update_state_for_stop(
                state,
                writer,
                ListenEndReason::PlaybackError,
                PlaybackSource::Automatic,
            );
            emit_state_changed(app_handle, state);
        }
    }
    if !auto && next_pos.is_none() {
        log::debug!(
            target: "sparkle::playback",
            "event=command_noop command=next reason=end_of_queue source={}",
            source.as_str()
        );
    }
    // A manual Next at the end of the queue (repeat off) is a no-op.
    save_session_to_db(state, writer);
}

fn update_state_for_stop(
    state: &Arc<Mutex<SharedState>>,
    writer: &DbWriter,
    reason: ListenEndReason,
    source: PlaybackSource,
) {
    finalize_active_listen(state, writer, reason, source);
    let mut s = lock_state(state);
    s.is_playing = false;
    s.play_when_device_ready = false;
    s.latched_sound_check_gain_db = 0.0;
    s.pending_play_source = PlaybackSource::Unknown;
    s.current_track = None;
    s.first_lyric_line = None;
    s.album_art = None;
    s.position_ms = 0;
    s.duration_ms = 0;
    s.seek_target = None;
    s.listened_ms = 0;
    s.last_counted_position_ms = None;
}

fn load_track_at_index(
    player: Option<&Player>,
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
    index: usize,
    source: PlaybackSource,
    start_reason: ListenStartReason,
    end_reason: ListenEndReason,
) -> bool {
    load_track_at_index_with_autoplay(
        player,
        state,
        db,
        writer,
        app_handle,
        index,
        true,
        source,
        start_reason,
        end_reason,
    )
}

fn load_track_at_index_with_autoplay(
    player: Option<&Player>,
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
    index: usize,
    autoplay: bool,
    source: PlaybackSource,
    start_reason: ListenStartReason,
    end_reason: ListenEndReason,
) -> bool {
    // A track transition finalizes the previous listen before metadata swaps.
    finalize_active_listen(state, writer, end_reason, source);
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
            log::error!(
                target: "sparkle::audio",
                "event=track_load_failed track_id={track_id} error={e}"
            );
            return false;
        }
    };
    let first_lyric_line = known_first_lyric_line(db, &track).unwrap_or_else(|error| {
        log::warn!(
            target: "sparkle::lyrics",
            "event=lyrics_availability_check_failed track_id={track_id} error={error}"
        );
        None
    });
    let album_art = known_album_art(db, app_handle, track.album_id).unwrap_or_else(|error| {
        log::warn!(
            target: "sparkle::album_art",
            "event=artwork_availability_check_failed album_id={:?} error={error}",
            track.album_id
        );
        None
    });

    // Make the newly selected track and its next three successors the
    // scanner's urgent work. Pending analysis never delays playback.
    refresh_loudness_priorities(state);
    let (latched_gain_db, analysis_pending) =
        immediate_start_gain(gain_for_track(state, db, track_id));

    // Without an output device the queue and metadata are still usable. Keep
    // an explicit autoplay request until the endpoint comes back so a Play,
    // Next, or track-selection command is not silently lost during recovery.
    let play_when_device_ready = autoplay && player.is_none();
    let autoplay = autoplay && player.is_some();

    // Publish the new metadata before touching the audio file. Opening and
    // decoding can take long enough to make the player bar feel stuck; the
    // audio source can catch up independently while the UI shows the next
    // track immediately.
    {
        let mut s = lock_state(state);
        s.current_track = Some(track.clone());
        s.first_lyric_line = first_lyric_line;
        s.album_art = album_art;
        s.is_playing = autoplay;
        s.play_when_device_ready = play_when_device_ready;
        s.pending_play_source = source;
        s.latched_sound_check_gain_db = latched_gain_db;
        s.position_ms = 0;
        s.duration_ms = track.duration_ms.unwrap_or(0);
        s.seek_target = None;
        s.listened_ms = 0;
        s.last_counted_position_ms = None;
    }
    emit_state_changed(app_handle, state);

    let Some(player) = player else {
        return true;
    };

    let file = match File::open(&track.file_path) {
        Ok(f) => f,
        Err(e) => {
            log::error!(
                target: "sparkle::audio",
                "event=source_open_failed track_id={} error={e}",
                track.id
            );
            return false;
        }
    };

    let decoded_source = match Decoder::new(BufReader::new(file)) {
        Ok(s) => s,
        Err(e) => {
            log::error!(
                target: "sparkle::audio",
                "event=source_decode_failed track_id={} error={e}",
                track.id
            );
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
    player.append(decoded_source);
    apply_player_volume(player, state);
    if autoplay {
        player.play();
        begin_active_listen(state, writer, source, start_reason);
    } else {
        player.pause();
    }

    if analysis_pending {
        log::info!(
            target: "sparkle::loudness",
            "event=playback_started_unscanned track_id={} fallback=unity gain_latched=true",
            track.id
        );
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
            log::error!(
                target: "sparkle::audio",
                "event=source_reopen_failed track_id={} error={e}",
                track.id
            );
            return false;
        }
    };

    let source = match Decoder::new(BufReader::new(file)) {
        Ok(s) => s,
        Err(e) => {
            log::error!(
                target: "sparkle::audio",
                "event=source_redecode_failed track_id={} error={e}",
                track.id
            );
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
        log::warn!(
            target: "sparkle::audio",
            "event=source_seek_after_reload_failed track_id={} position_ms={} error={e}",
            track.id,
            position_ms.max(0)
        );
    }

    apply_player_volume(player, state);

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

fn known_first_lyric_line(
    db: &Arc<Mutex<rusqlite::Connection>>,
    track: &Track,
) -> Result<Option<String>, String> {
    let conn = lock_db(db);
    let override_source: Option<String> = conn
        .query_row(
            "SELECT NULLIF(lyrics_source, '') FROM tracks WHERE id = ?",
            [track.id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let sources = match override_source {
        Some(source) => vec![source],
        None => load_lyrics_sources(&conn)?,
    };

    // get_lyrics returns a cached provider before probing any source. Mirror
    // that choice so the layout hint describes the lyrics it will return.
    for source in &sources {
        if let Some(cached) = cache::get_lyrics_from_source(&conn, track.id, source)? {
            return Ok(cached
                .synced_text
                .as_deref()
                .and_then(lyrics::first_synced_line));
        }
    }
    drop(conn);

    // Embedded and sidecar lyrics can be established without a network
    // lookup. Remote providers stay unknown until get_lyrics finishes.
    let metadata = TrackMetadata {
        file_path: Some(track.file_path.clone()),
        embedded_lyrics: track.embedded_lyrics.clone(),
        ..TrackMetadata::default()
    };
    for source in sources {
        let lyrics = match source.as_str() {
            "custom" => None,
            "embedded" => lyrics::embedded::fetch(&metadata)?,
            "lrc" => lyrics::lrc::fetch(&metadata)?,
            _ => return Ok(None),
        };
        if let Some(lyrics) = lyrics {
            return Ok(lyrics
                .synced_text
                .as_deref()
                .and_then(lyrics::first_synced_line));
        }
    }
    Ok(None)
}

fn known_album_art(
    db: &Arc<Mutex<rusqlite::Connection>>,
    app_handle: &AppHandle,
    album_id: Option<i64>,
) -> Result<Option<CachedImage>, String> {
    let Some(album_id) = album_id else {
        return Ok(None);
    };
    let cache_dir = crate::db::data_dir(app_handle).join("cache");
    let conn = lock_db(db);
    let custom_enabled = load_album_art_sources(&conn)?
        .iter()
        .any(|source| source == "custom");
    if custom_enabled {
        if let Some(custom) = cache::get_custom_image(&conn, &cache_dir, "album", album_id)? {
            return Ok(Some(custom));
        }
    }
    crate::providers::album_art::get_cached_album_art(&conn, &cache_dir, album_id)
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
            crate::queue_system_media_status(app_handle, ps.clone());
        }
    }
    let discord = lock_state(state).discord.clone();
    discord.update(&ps);
    let event = PlaybackStateChangedEvent {
        is_playing: ps.is_playing,
        current_track: ps.current_track,
        first_lyric_line: ps.first_lyric_line,
        album_art: ps.album_art,
        position_ms: ps.position_ms,
        duration_ms: ps.duration_ms,
        shuffle: ps.shuffle,
        repeat_mode: ps.repeat_mode,
    };
    let _ = app_handle.emit("playback-state-changed", event);
}

fn emit_queue_changed(app_handle: &AppHandle, state: &Arc<Mutex<SharedState>>) {
    refresh_loudness_priorities(state);
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
                    log::error!(
                        target: "sparkle::audio",
                        "event=queued_track_load_failed track_id={id} error={e}"
                    );
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
        first_lyric_line: s.first_lyric_line.clone(),
        album_art: s.album_art.clone(),
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
#[path = "tests/audio_engine.rs"]
mod tests;
