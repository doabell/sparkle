use crate::analytics::{
    is_completed, is_meaningful_listen, new_trace_id, now_epoch_ms, ListenEndReason, ListenRecord,
    ListenStartReason, OutputUnavailableReason, PlaybackContext, PlaybackEvent,
    PlaybackEventRecord, PlaybackSource, QueueLoadReason, SeekReason, LISTENING_SESSION_GAP_MS,
};
use crate::cache;
use crate::db_writer::{DbWriter, WriterHealth, WriterMonitor};
use crate::discord::DiscordPresence;
use crate::loudness::{GainAvailability, LoudnessController, NEXT_UP_COUNT};
use crate::models::{CachedImage, PlaybackState, QueueView, RepeatMode, Track};
use crate::playback_observation::{
    CommandObservation, CommandOutcome, CommandReply, Operation, PlaybackFailure,
    PlaybackObservation,
};
use crate::providers::lyrics;
use crate::settings::{load_album_art_sources, load_session, SessionSnapshot};
use rodio::{Decoder, DeviceSinkBuilder, Float, MixerDeviceSink, Player, Source};
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

fn change_shuffle(
    shuffle: &mut bool,
    requested: bool,
    queue_len: usize,
    queue_index: Option<usize>,
    play_order: &mut Vec<usize>,
    order_pos: &mut Option<usize>,
) -> bool {
    if *shuffle == requested {
        return false;
    }
    *shuffle = requested;
    if let Some(current) = queue_index {
        let (order, pos) = build_play_order(queue_len, current, requested);
        *play_order = order;
        *order_pos = Some(pos);
    } else {
        *play_order = (0..queue_len).collect();
        *order_pos = None;
    }
    true
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

#[derive(Debug, PartialEq, Eq)]
enum AdvanceTarget {
    RepeatCurrent(usize),
    NextPosition(usize),
    Finish(Option<usize>),
    Noop,
}

// Positions refer to play_order, not the underlying queue: this distinction
// keeps next/previous correct when shuffle changes the traversal order.
fn advance_target(
    order_pos: Option<usize>,
    order_len: usize,
    queue_index: Option<usize>,
    repeat_mode: RepeatMode,
    auto: bool,
) -> AdvanceTarget {
    if auto && repeat_mode == RepeatMode::One {
        if let Some(index) = queue_index {
            return AdvanceTarget::RepeatCurrent(index);
        }
    }
    match order_pos.and_then(|pos| pos.checked_add(1)) {
        Some(next) if next < order_len => AdvanceTarget::NextPosition(next),
        _ if repeat_mode == RepeatMode::All && order_len > 0 => AdvanceTarget::NextPosition(0),
        _ if auto => AdvanceTarget::Finish(queue_index),
        _ => AdvanceTarget::Noop,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum PreviousTarget {
    Restart,
    Position(usize),
    Noop,
}

fn previous_target(position_ms: i64, order_pos: Option<usize>) -> PreviousTarget {
    if position_ms > 3000 {
        PreviousTarget::Restart
    } else {
        match order_pos {
            Some(0) => PreviousTarget::Restart,
            Some(pos) => PreviousTarget::Position(pos - 1),
            None => PreviousTarget::Noop,
        }
    }
}

/// Insert or move an entry after the current song without duplicating it or
/// changing which song is current. Returns remapped cursors only on a change.
fn queue_track_next(
    queue: &mut Vec<i64>,
    play_order: &mut Vec<usize>,
    mut current_index: usize,
    mut order_pos: usize,
    track_id: i64,
) -> Option<(usize, usize)> {
    if queue.get(current_index) == Some(&track_id)
        || play_order.get(order_pos + 1).and_then(|i| queue.get(*i)) == Some(&track_id)
    {
        return None;
    }
    if let Some(existing_index) = queue.iter().position(|&id| id == track_id) {
        queue.remove(existing_index);
        if let Some(pos) = play_order.iter().position(|&index| index == existing_index) {
            play_order.remove(pos);
        }
        for index in play_order.iter_mut() {
            if *index > existing_index {
                *index -= 1;
            }
        }
        if current_index > existing_index {
            current_index -= 1;
        }
        order_pos = play_order
            .iter()
            .position(|&index| index == current_index)
            .unwrap_or(0);
    }
    queue.push(track_id);
    let insert_at = (order_pos + 1).min(play_order.len());
    play_order.insert(insert_at, queue.len() - 1);
    Some((current_index, order_pos))
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
    writer_monitor: WriterMonitor,
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
            pending_start_reason: ListenStartReason::Unknown,
            revision: 0,
            progress_sequence: 0,
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
            observation: PlaybackObservation::default(),
        }));
        let state_clone = state.clone();
        let writer = DbWriter::new(crate::db::db_path(&app_handle));
        let writer_monitor = writer.monitor();
        let worker = std::thread::spawn(move || {
            audio_thread(rx, app_handle, state_clone, db, writer);
        });
        Self {
            tx,
            state,
            worker: Arc::new(Mutex::new(Some(worker))),
            writer_monitor,
        }
    }

    pub fn play(&self, source: PlaybackSource) -> Result<PlaybackState, String> {
        self.execute_with_id(AudioCommand::Play(source), None)
            .map(|reply| reply.state)
            .map_err(|error| error.to_string())
    }

    pub fn pause(&self, source: PlaybackSource) -> Result<PlaybackState, String> {
        self.execute_with_id(AudioCommand::Pause(source), None)
            .map(|reply| reply.state)
            .map_err(|error| error.to_string())
    }

    pub fn stop(&self, source: PlaybackSource) -> Result<PlaybackState, String> {
        self.execute_with_id(AudioCommand::Stop(source), None)
            .map(|reply| reply.state)
            .map_err(|error| error.to_string())
    }

    pub fn seek(&self, position_ms: i64, source: PlaybackSource) -> Result<PlaybackState, String> {
        self.execute_with_id(AudioCommand::Seek(position_ms, source), None)
            .map(|reply| reply.state)
            .map_err(|error| error.to_string())
    }

    pub fn refresh_track_lyrics(&self, track_id: i64) -> Result<(), String> {
        self.execute_with_id(AudioCommand::RefreshLyrics(track_id), None)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn next_track(&self, source: PlaybackSource) -> Result<PlaybackState, String> {
        self.execute_with_id(AudioCommand::Next(source), None)
            .map(|reply| reply.state)
            .map_err(|error| error.to_string())
    }

    pub fn previous_track(&self, source: PlaybackSource) -> Result<PlaybackState, String> {
        self.execute_with_id(AudioCommand::Previous(source), None)
            .map(|reply| reply.state)
            .map_err(|error| error.to_string())
    }

    pub fn set_sound_check_enabled(&self, enabled: bool) -> Result<PlaybackState, String> {
        self.execute_with_id(AudioCommand::SetSoundCheckEnabled(enabled), None)
            .map(|reply| reply.state)
            .map_err(|error| error.to_string())
    }

    pub fn get_queue(&self) -> Result<QueueView, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(AudioCommand::GetQueue(reply_tx))
            .map_err(|e| e.to_string())?;
        receive_audio_reply(reply_rx, COMMAND_REPLY_TIMEOUT, "queue")
    }

    pub fn get_playback_state(&self) -> Result<PlaybackState, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(AudioCommand::GetState(reply_tx))
            .map_err(|e| e.to_string())?;
        receive_audio_reply(reply_rx, COMMAND_REPLY_TIMEOUT, "playback state")
    }

    pub(crate) fn execute_with_id(
        &self,
        command: AudioCommand,
        id: Option<String>,
    ) -> Result<CommandReply, PlaybackFailure> {
        send_command(&self.tx, command, id, COMMAND_REPLY_TIMEOUT)
    }

    pub(crate) fn writer_health(&self) -> WriterHealth {
        self.writer_monitor.snapshot()
    }

    pub(crate) fn diagnostics_snapshot(&self) -> serde_json::Value {
        let s = lock_state(&self.state);
        serde_json::json!({
            "observation": s.observation,
            "current_command": s.observation.current_operation.as_ref().map(|op| serde_json::json!({
                "command_id": op.id, "command": op.name, "track_id": op.target_track_id,
                "elapsed_ms": op.started_at.elapsed().as_millis() as u64,
            })),
            "track_id": s.current_track.as_ref().map(|track| track.id),
            "listen_id": s.active_listen_id, "session_id": s.active_session_id,
            "is_playing": s.is_playing, "play_when_device_ready": s.play_when_device_ready,
            "position_ms": s.position_ms, "duration_ms": s.duration_ms,
            "volume": s.volume, "sound_check_gain_db": s.latched_sound_check_gain_db,
            "queue_length": s.queue.len(), "queue_index": s.queue_index,
            "play_order_index": s.order_pos, "shuffle": s.shuffle, "repeat_mode": s.repeat_mode,
            "writer": self.writer_monitor.snapshot(),
        })
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

fn send_command(
    tx: &mpsc::Sender<AudioCommand>,
    command: AudioCommand,
    id: Option<String>,
    timeout: Duration,
) -> Result<CommandReply, PlaybackFailure> {
    let (name, source, track_id) = command.description();
    let operation = Operation::new(id, name, source, track_id);
    let failure = |stage, message: String| {
        PlaybackFailure::new(stage, track_id, message).for_command(&operation.id, name)
    };
    let (reply, receiver) = mpsc::channel();
    log::log!(target: "sparkle::playback", if name == "set_volume" { log::Level::Trace } else { log::Level::Debug },
        "event=command_submitted command_id={} command={} source={} track_id={track_id:?}", operation.id, name, source.as_str());
    tx.send(AudioCommand::Execute(Box::new(CommandRequest {
        command,
        operation: operation.clone(),
        reply,
    })))
    .map_err(|_| failure("dispatch", "The audio engine has stopped.".into()))?;
    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let error = failure(
                "reply_timeout",
                format!(
                    "The audio engine did not respond within {} ms. The command may still finish.",
                    timeout.as_millis()
                ),
            );
            log::warn!(target: "sparkle::playback", "event=command_reply_timeout command_id={} command={} elapsed_ms={}", operation.id, name, operation.queued_at.elapsed().as_millis());
            Err(error)
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(failure(
            "reply_disconnected",
            "The audio engine stopped before replying.".into(),
        )),
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

pub(crate) enum AudioCommand {
    Execute(Box<CommandRequest>),
    AutoAdvance,
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
    SeekLyrics(i64, i64),
    RefreshLyrics(i64),
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

pub(crate) struct CommandRequest {
    command: AudioCommand,
    operation: Operation,
    reply: mpsc::Sender<Result<CommandReply, PlaybackFailure>>,
}

impl AudioCommand {
    fn description(&self) -> (&'static str, PlaybackSource, Option<i64>) {
        use AudioCommand::*;
        match self {
            LoadQueue(ids, index, _, source, _) => {
                ("load_queue", *source, ids.get(*index).copied())
            }
            PlayTrack(id, source, _) => ("play_track", *source, Some(*id)),
            Play(source) => ("play", *source, None),
            Pause(source) => ("pause", *source, None),
            Stop(source) => ("stop", *source, None),
            Seek(_, source) => ("seek", *source, None),
            SeekLyrics(id, _) => ("seek_lyrics", PlaybackSource::Ui, Some(*id)),
            RefreshLyrics(id) => ("refresh_lyrics", PlaybackSource::Internal, Some(*id)),
            Next(source) => ("next_track", *source, None),
            AutoAdvance => ("auto_advance", PlaybackSource::Automatic, None),
            Previous(source) => ("previous_track", *source, None),
            SetVolume(_, source) => ("set_volume", *source, None),
            SetSoundCheckEnabled(_) => ("set_sound_check_enabled", PlaybackSource::Ui, None),
            SetShuffle(_, source) => ("set_shuffle", *source, None),
            CycleRepeatMode(source) => ("cycle_repeat_mode", *source, None),
            PlayNext(id, source) => ("play_next", *source, Some(*id)),
            PlayAt(_, source) => ("play_queue_index", *source, None),
            GetQueue(_) => ("get_queue", PlaybackSource::Internal, None),
            GetState(_) => ("get_playback_state", PlaybackSource::Internal, None),
            Shutdown(_) => ("shutdown", PlaybackSource::Internal, None),
            Execute(request) => request.command.description(),
        }
    }
}

fn dispatch_command(
    cmd: AudioCommand,
    player: Option<&Player>,
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
) -> CommandFlow {
    match cmd {
        AudioCommand::Execute(request) => {
            let result = run_observed_command(request.command, request.operation, player, state, db, writer, app_handle);
            if request.reply.send(result).is_err() {
                log::debug!(target: "sparkle::playback", "event=command_reply_abandoned");
            }
            CommandFlow::Continue(CommandOutcome::Applied)
        }
        AudioCommand::AutoAdvance => {
            let operation = Operation::new(None, "auto_advance", PlaybackSource::Automatic, None);
            let _ = run_observed_command(AudioCommand::AutoAdvance, operation, player, state, db, writer, app_handle);
            CommandFlow::Continue(CommandOutcome::Applied)
        }
        other => handle_command(other, player, state, db, writer, app_handle).unwrap_or_else(|error| {
            log::error!(target: "sparkle::playback", "event=internal_command_failed stage={} error={error}", error.stage);
            CommandFlow::Continue(CommandOutcome::Noop)
        }),
    }
}

fn run_observed_command(
    cmd: AudioCommand,
    mut operation: Operation,
    player: Option<&Player>,
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
) -> Result<CommandReply, PlaybackFailure> {
    operation.started_at = Instant::now();
    operation.started_at_ms = now_epoch_ms();
    let queue_wait_ms = operation
        .started_at
        .duration_since(operation.queued_at)
        .as_millis() as u64;
    let run_id = {
        let mut s = lock_state(state);
        s.observation.current_operation = Some(operation.clone());
        s.observation.current_stages.clear();
        s.observation.run_id.clone()
    };
    let level = if operation.name == "set_volume" {
        log::Level::Trace
    } else {
        log::Level::Debug
    };
    log::log!(target: "sparkle::playback", level,
        "event=command_started run_id={} command_id={} command={} source={} queue_wait_ms={queue_wait_ms}",
        run_id, operation.id, operation.name, operation.source.as_str());
    let result = handle_command(cmd, player, state, db, writer, app_handle)
        .map(|flow| match flow {
            CommandFlow::Continue(outcome) => outcome,
            CommandFlow::Shutdown(_) => unreachable!(),
        })
        .map_err(|error| error.for_command(&operation.id, operation.name));
    if let Err(error) = &result {
        record_command_failure(state, writer, &operation, error);
        emit_state_changed(app_handle, state);
        emit_queue_changed(app_handle, state);
        save_session_to_db(state, writer);
    }
    let execution_ms = operation.started_at.elapsed().as_millis() as u64;
    let (track_id, listen_id) = {
        let s = lock_state(state);
        (
            result
                .as_ref()
                .err()
                .and_then(|error| error.track_id)
                .or(operation.target_track_id)
                .or_else(|| s.current_track.as_ref().map(|track| track.id)),
            s.active_listen_id.clone(),
        )
    };
    let outcome = result
        .as_ref()
        .map(|outcome| outcome.as_str())
        .unwrap_or("failed");
    log::log!(target: "sparkle::playback", if result.is_err() { log::Level::Error } else { level },
        "event=command_completed run_id={} command_id={} command={} source={} track_id={track_id:?} listen_id={} outcome={outcome} queue_wait_ms={queue_wait_ms} execution_ms={execution_ms} stage={} error={}",
        run_id, operation.id, operation.name, operation.source.as_str(), listen_id.as_deref().unwrap_or("none"),
        result.as_ref().err().map(|error| error.stage.as_str()).unwrap_or("none"),
        result.as_ref().err().map(|error| error.message.as_str()).unwrap_or("none"));
    {
        let mut s = lock_state(state);
        let stages = std::mem::take(&mut s.observation.current_stages);
        s.observation.record(CommandObservation {
            command_id: operation.id.clone(),
            command: operation.name.into(),
            source: operation.source.as_str().into(),
            occurred_at_ms: operation.started_at_ms,
            track_id,
            listen_id,
            queue_wait_ms,
            execution_ms,
            outcome: outcome.into(),
            failure: result.as_ref().err().cloned(),
            stages,
            first_progress_ms: None,
        });
        s.observation.current_operation = None;
        s.observation.active_stage = None;
    }
    result.map(|outcome| CommandReply {
        command_id: operation.id,
        outcome,
        state: build_playback_state(state),
    })
}

fn timed_stage<T>(
    state: &Arc<Mutex<SharedState>>,
    stage: &str,
    track_id: Option<i64>,
    work: impl FnOnce() -> Result<T, PlaybackFailure>,
) -> Result<T, PlaybackFailure> {
    let started = Instant::now();
    lock_state(state).observation.active_stage = Some(crate::playback_observation::ActiveStage {
        stage: stage.into(),
        started_at_ms: now_epoch_ms(),
    });
    let result = work();
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let command_id = {
        let mut s = lock_state(state);
        s.observation.active_stage = None;
        if s.observation.current_stages.len() == 16 {
            s.observation.current_stages.remove(0);
        }
        s.observation
            .current_stages
            .push(crate::playback_observation::StageTiming {
                stage: stage.into(),
                elapsed_ms,
                success: result.is_ok(),
            });
        s.observation
            .current_operation
            .as_ref()
            .map(|op| op.id.clone())
    };
    log::debug!(target: "sparkle::playback", "event=stage_completed command_id={} track_id={track_id:?} stage={stage} elapsed_ms={elapsed_ms} success={}",
        command_id.as_deref().unwrap_or("internal"), result.is_ok());
    result
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
    pending_start_reason: ListenStartReason,
    revision: u64,
    progress_sequence: u64,
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
    observation: PlaybackObservation,
}

#[derive(Serialize, Clone)]
struct ProgressEvent {
    revision: u64,
    sequence: u64,
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
    event: PlaybackEvent,
    source: PlaybackSource,
) -> PlaybackEventRecord {
    PlaybackEventRecord {
        id: new_trace_id("event"),
        run_id: state.observation.run_id.clone(),
        command_id: state
            .observation
            .current_operation
            .as_ref()
            .map(|op| op.id.clone()),
        listen_id: state.active_listen_id.clone(),
        session_id: state.active_session_id.clone(),
        occurred_at_ms: now_epoch_ms(),
        event,
        source,
        track_id: state.current_track.as_ref().map(|track| track.id),
        position_ms: state
            .current_track
            .as_ref()
            .map(|_| state.position_ms.max(0)),
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
    event: PlaybackEvent,
    source: PlaybackSource,
) {
    let event = event_record_locked(&lock_state(state), event, source);
    writer.record_event(event);
}

fn record_command_failure(
    state: &Arc<Mutex<SharedState>>,
    writer: &DbWriter,
    operation: &Operation,
    failure: &PlaybackFailure,
) {
    let mut event = event_record_locked(
        &lock_state(state),
        PlaybackEvent::CommandFailed {
            command: operation.name.into(),
            stage: failure.stage.clone(),
            target_track_id: failure.track_id.or(operation.target_track_id),
        },
        operation.source,
    );
    event.command_id = Some(operation.id.clone());
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
        if s.observation.current_operation.is_some() {
            s.observation.awaiting_progress = s.observation.current_operation.clone();
        }
        let record = listen_record_locked(&s, false, None, None)
            .expect("a listen is complete immediately after it starts");
        let event = event_record_locked(&s, PlaybackEvent::ListenStarted(reason), source);
        (record, event, needs_session)
    };
    writer.upsert_listen(record.clone());
    writer.record_event(event);
    log::debug!(
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
        let event = event_record_locked(&s, PlaybackEvent::ListenEnded(reason), source);
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
    log::debug!(
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
    let (has_active, timed_out, pending_reason) = {
        let s = lock_state(state);
        (
            s.active_listen_id.is_some(),
            s.active_listen_id.is_some()
                && s.session_last_active_at_ms.is_some_and(|last| {
                    now_epoch_ms().saturating_sub(last) > LISTENING_SESSION_GAP_MS
                }),
            s.pending_start_reason,
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
        record_event(state, writer, PlaybackEvent::PlaybackResumed, source);
        let (listen_id, track_id) = {
            let s = lock_state(state);
            (
                s.active_listen_id.clone(),
                s.current_track.as_ref().map(|track| track.id),
            )
        };
        log::debug!(
            target: "sparkle::playback",
            "event=playback_resumed listen_id={} track_id={} source={}",
            listen_id.as_deref().unwrap_or("none"),
            track_id.map(|id| id.to_string()).unwrap_or_else(|| "none".to_string()),
            source.as_str()
        );
    } else {
        let reason = if pending_reason == ListenStartReason::RestoredResume {
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
    if let Err(error) = load_track_at_index_with_autoplay(
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
        log::warn!(target: "sparkle::playback", "event=session_restore_failed stage={} error={error}", error.stage);
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
        let target_ms = clamp_seek_position(snapshot.position_ms, lock_state(state).duration_ms);
        let position_ms = if let Some(player) = player {
            match seek_player(player, state, target_ms) {
                Ok(()) => target_ms,
                Err(error) => {
                    log::warn!(target: "sparkle::playback", "event=session_seek_restore_failed stage={} error={error}", error.stage);
                    lock_state(state).observation.last_failure = Some(error);
                    player.get_pos().as_millis() as i64
                }
            }
        } else {
            target_ms
        };
        let mut s = lock_state(state);
        s.position_ms = position_ms;
        s.seek_target = Some((position_ms, Instant::now()));
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

fn decode_playback_file(
    file: File,
) -> Result<Decoder<BufReader<File>>, rodio::decoder::DecoderError> {
    // The File conversion supplies byte length and enables random access. The
    // generic BufReader constructor defaults to an unseekable stream.
    Decoder::try_from(file)
}

fn decoded_duration_ms(source: &impl Source, fallback_ms: i64) -> i64 {
    source
        .total_duration()
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .filter(|duration_ms| *duration_ms > 0)
        .unwrap_or(fallback_ms.max(0))
}

fn load_source_into_player(player: &Player, track: &Track) -> Result<i64, PlaybackFailure> {
    let file = File::open(&track.file_path)
        .map_err(|error| PlaybackFailure::new("file_open", Some(track.id), error))?;
    let decoded_source = decode_playback_file(file)
        .map_err(|error| PlaybackFailure::new("decode", Some(track.id), error))?;
    let duration_ms = decoded_duration_ms(&decoded_source, track.duration_ms.unwrap_or(0));
    player.stop();
    player.clear();
    player.append(decoded_source);
    Ok(duration_ms)
}

fn reload_current_for_device(
    player: &Player,
    state: &Arc<Mutex<SharedState>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
) {
    let operation = {
        let mut s = lock_state(state);
        let operation = s.observation.awaiting_progress.clone().unwrap_or_else(|| {
            Operation::new(
                None,
                "output_recovery",
                PlaybackSource::Internal,
                s.current_track.as_ref().map(|track| track.id),
            )
        });
        s.observation.current_operation = Some(operation.clone());
        operation
    };
    if let Err(failure) = reload_current_for_device_inner(player, state, writer, app_handle) {
        record_command_failure(state, writer, &operation, &failure);
        lock_state(state).observation.last_failure =
            Some(failure.for_command(&operation.id, operation.name));
    }
    lock_state(state).observation.current_operation = None;
}

fn reload_current_for_device_inner(
    player: &Player,
    state: &Arc<Mutex<SharedState>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
) -> Result<(), PlaybackFailure> {
    let (
        track,
        was_playing,
        position_ms,
        pending_source,
        pending_reason,
        had_active_listen,
        session_timed_out,
    ) = {
        let s = lock_state(state);
        (
            s.current_track.clone(),
            s.is_playing || s.play_when_device_ready,
            s.position_ms,
            s.pending_play_source,
            s.pending_start_reason,
            s.active_listen_id.is_some(),
            s.active_listen_id.is_some()
                && s.session_last_active_at_ms.is_some_and(|last| {
                    now_epoch_ms().saturating_sub(last) > LISTENING_SESSION_GAP_MS
                }),
        )
    };
    let track = match track {
        Some(t) => t,
        None => return Ok(()),
    };

    player.pause();
    let duration_ms = match timed_stage(state, "recovery_source", Some(track.id), || {
        load_source_into_player(player, &track)
    }) {
        Ok(duration_ms) => duration_ms,
        Err(error) => {
            log::error!(target: "sparkle::audio", "event=output_restore_failed stage={} track_id={} error={error}", error.stage, track.id);
            player.stop();
            player.clear();
            update_state_for_stop(
                state,
                writer,
                ListenEndReason::PlaybackError,
                PlaybackSource::Internal,
            );
            emit_state_changed(app_handle, state);
            return Err(error);
        }
    };
    lock_state(state).duration_ms = duration_ms;
    let position_ms = clamp_seek_position(position_ms, duration_ms);

    apply_player_volume(player, state);

    if position_ms > 0 {
        if let Err(error) = timed_stage(state, "recovery_seek", Some(track.id), || {
            seek_player(player, state, position_ms)
        }) {
            log::error!(target: "sparkle::audio", "event=output_restore_failed stage={} track_id={} error={error}", error.stage, track.id);
            player.stop();
            player.clear();
            update_state_for_stop(
                state,
                writer,
                ListenEndReason::PlaybackError,
                PlaybackSource::Internal,
            );
            emit_state_changed(app_handle, state);
            return Err(error);
        }
    }

    if was_playing {
        player.play();
    } else {
        player.pause();
    }

    {
        let mut s = lock_state(state);
        s.position_ms = position_ms;
        s.seek_target = Some((position_ms, Instant::now()));
        s.play_when_device_ready = false;
        s.is_playing = was_playing;
    }

    if was_playing {
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
                ListenStartReason::ResumeAfterInactivity,
            );
        } else if !had_active_listen {
            begin_active_listen(state, writer, pending_source, pending_reason);
        } else {
            let mut s = lock_state(state);
            s.session_last_active_at_ms = Some(now_epoch_ms());
        }
        log::info!(
            target: "sparkle::audio",
            "event=playback_recovered track_id={} resumed=true session_rotated={session_timed_out}",
            track.id
        );
    }

    emit_state_changed(app_handle, state);
    Ok(())
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
                        match dispatch_command(
                            cmd,
                            Some(&player),
                            &state,
                            &db,
                            &writer,
                            &app_handle,
                        ) {
                            CommandFlow::Continue(_) => {}
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
                    mark_output_unavailable(
                        &state,
                        &writer,
                        &app_handle,
                        OutputUnavailableReason::DeviceChanged,
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

            let first_progress = {
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
                let mut first_progress = None;
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
                            first_progress = s.observation.awaiting_progress.take();
                        }
                    }
                    s.last_counted_position_ms = Some(pos_ms);
                } else {
                    s.last_counted_position_ms = None;
                }
                first_progress
            };
            if let Some(operation) = first_progress {
                let elapsed_ms = operation.queued_at.elapsed().as_millis() as u64;
                {
                    let mut s = lock_state(&state);
                    if let Some(record) = s
                        .observation
                        .recent_commands
                        .iter_mut()
                        .find(|record| record.command_id == operation.id)
                    {
                        record.first_progress_ms = Some(elapsed_ms);
                    }
                }
                log::debug!(target: "sparkle::playback", "event=first_playback_progress command_id={} command={} elapsed_ms={elapsed_ms} position_ms={pos_ms}", operation.id, operation.name);
            }

            // Detect stuck playback (device unplugged / audio endpoint changed)
            if !is_paused && !player_empty {
                if pos == last_pos {
                    if stuck_since.is_none() {
                        stuck_since = Some(Instant::now());
                    } else if stuck_since.unwrap().elapsed() >= stuck_timeout {
                        log::warn!(
                            target: "sparkle::audio",
                            "event=playback_stalled position_ms={pos_ms} stall_ms=3000 recovery=recreate_pipeline suspected_cause=output_unavailable"
                        );
                        mark_output_unavailable(
                            &state,
                            &writer,
                            &app_handle,
                            OutputUnavailableReason::ClockStalled,
                        );
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
                    let (position_ms, duration_ms, revision, sequence) = {
                        let mut s = lock_state(&state);
                        s.progress_sequence += 1;
                        (
                            s.position_ms,
                            s.duration_ms,
                            s.revision,
                            s.progress_sequence,
                        )
                    };
                    publish_event(
                        &app_handle,
                        &state,
                        "playback-progress",
                        ProgressEvent {
                            revision,
                            sequence,
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
                    dispatch_command(
                        AudioCommand::AutoAdvance,
                        Some(&player),
                        &state,
                        &db,
                        &writer,
                        &app_handle,
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
    let mut open_started = Instant::now();
    let mut slow_open_logged = false;
    loop {
        if pending_device_open.is_none() && Instant::now() >= retry_at {
            open_started = Instant::now();
            slow_open_logged = false;
            let attempts = {
                let mut s = lock_state(state);
                let observation = &mut s.observation;
                observation
                    .recovery_started_at_ms
                    .get_or_insert(now_epoch_ms());
                observation.recovery_attempts += 1;
                observation.device_open_started_at_ms = Some(now_epoch_ms());
                observation.recovery_attempts
            };
            log::debug!(target: "sparkle::audio", "event=output_open_started attempt={attempts}");
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

        if pending_device_open.is_some()
            && !slow_open_logged
            && open_started.elapsed() >= COMMAND_REPLY_TIMEOUT
        {
            log::warn!(target: "sparkle::audio", "event=output_open_slow elapsed_ms={} status=pending", open_started.elapsed().as_millis());
            slow_open_logged = true;
        }
        if let Some(result) = open_result {
            lock_state(state).observation.device_open_started_at_ms = None;
            pending_device_open.take();
            match result {
                Ok(handle) => {
                    let (attempts, elapsed_ms) = {
                        let mut s = lock_state(state);
                        let observation = &mut s.observation;
                        observation.output_available = true;
                        observation.output_unavailable_reason = None;
                        observation.output_config = Some(format!("{:?}", handle.config()));
                        let elapsed = observation
                            .recovery_started_at_ms
                            .take()
                            .map(|started| now_epoch_ms().saturating_sub(started).max(0) as u64)
                            .unwrap_or(0);
                        observation.last_recovery_ms = Some(elapsed);
                        (observation.recovery_attempts, elapsed)
                    };
                    record_event(
                        state,
                        writer,
                        PlaybackEvent::OutputRestored,
                        PlaybackSource::Internal,
                    );
                    log::info!(target: "sparkle::audio", "event=output_recovery_completed attempts={attempts} elapsed_ms={elapsed_ms} open_ms={}", open_started.elapsed().as_millis());
                    if *device_error_logged {
                        log::info!(target: "sparkle::audio", "event=output_device_available");
                        *device_error_logged = false;
                    }
                    return DeviceWaitFlow::Ready(handle);
                }
                Err(error) => {
                    {
                        let mut s = lock_state(state);
                        let track_id = s.current_track.as_ref().map(|track| track.id);
                        s.observation.last_failure =
                            Some(PlaybackFailure::new("output_open", track_id, &error));
                    }
                    mark_output_unavailable(
                        state,
                        writer,
                        app_handle,
                        OutputUnavailableReason::OpenFailed,
                    );
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
            Ok(cmd) => match dispatch_command(cmd, None, state, db, writer, app_handle) {
                CommandFlow::Continue(_) => {}
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
    reason: OutputUnavailableReason,
) {
    let (newly_unavailable, should_resume, track_id) = {
        let mut s = lock_state(state);
        let newly_unavailable = s.observation.output_unavailable(reason);
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
            PlaybackEvent::OutputUnavailable(reason),
            PlaybackSource::Internal,
        );
        log::warn!(
            target: "sparkle::audio",
            "event=output_unavailable track_id={} resume_pending={should_resume} reason={}",
            track_id.map(|id| id.to_string()).unwrap_or_else(|| "none".to_string()), reason.as_str()
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
    Continue(CommandOutcome),
    Shutdown(mpsc::Sender<Result<(), String>>),
}

fn handle_command(
    cmd: AudioCommand,
    player: Option<&Player>,
    state: &Arc<Mutex<SharedState>>,
    db: &Arc<Mutex<rusqlite::Connection>>,
    writer: &DbWriter,
    app_handle: &AppHandle,
) -> Result<CommandFlow, PlaybackFailure> {
    let may_defer = matches!(
        &cmd,
        AudioCommand::LoadQueue(..)
            | AudioCommand::PlayTrack(..)
            | AudioCommand::Play(_)
            | AudioCommand::PlayAt(..)
            | AudioCommand::PlayNext(..)
            | AudioCommand::Previous(_)
            | AudioCommand::Next(_)
            | AudioCommand::AutoAdvance
    );
    let mut outcome = CommandOutcome::Applied;
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
                PlaybackEvent::QueueLoaded(QueueLoadReason::QueueReplaced),
                source,
            );
            // Snapshot once and release the state lock before logging.
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
                if let Err(error) = load_track_at_index(
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
                    if let Some(player) = player {
                        player.stop();
                        player.clear();
                    }
                    update_state_for_stop(state, writer, ListenEndReason::PlaybackError, source);
                    emit_state_changed(app_handle, state);

                    return Err(error);
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
                s.current_track = None;
                s.first_lyric_line = None;
                s.album_art = None;
                s.position_ms = 0;
                s.duration_ms = 0;
                s.is_playing = false;
                s.play_when_device_ready = false;
            }
            record_event(
                state,
                writer,
                PlaybackEvent::QueueLoaded(QueueLoadReason::SingleTrack),
                source,
            );
            if let Err(error) = load_track_at_index(
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
                if let Some(player) = player {
                    player.stop();
                    player.clear();
                }
                update_state_for_stop(state, writer, ListenEndReason::PlaybackError, source);
                emit_state_changed(app_handle, state);

                return Err(error);
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
            if was_playing {
                return Ok(CommandFlow::Continue(CommandOutcome::Noop));
            }
            if has_track {
                {
                    let mut s = lock_state(state);
                    if s.pending_start_reason != ListenStartReason::RestoredResume {
                        s.pending_start_reason = ListenStartReason::Replay;
                    }
                }
                if let Some(player) = player {
                    player.play();
                    {
                        let mut s = lock_state(state);
                        s.is_playing = true;
                        s.play_when_device_ready = false;
                        s.pending_play_source = source;
                    }
                    if !was_playing {
                        {
                            let mut s = lock_state(state);
                            s.observation.awaiting_progress =
                                s.observation.current_operation.clone();
                        }
                        resume_or_begin_listen(state, writer, source);
                    }
                } else {
                    // Commands still resolve when no output exists, but the
                    // logical state must never claim inaudible playback.
                    let mut s = lock_state(state);
                    s.is_playing = false;
                    s.play_when_device_ready = true;
                    s.observation.awaiting_progress = s.observation.current_operation.clone();
                    outcome = CommandOutcome::Deferred;
                    s.pending_play_source = source;
                    s.last_counted_position_ms = None;
                }
                emit_state_changed(app_handle, state);
                save_session_to_db(state, writer);
            } else {
                let idx = lock_state(state).queue_index;
                if idx.is_none() {
                    outcome = CommandOutcome::Noop;
                }
                if let Some(i) = idx {
                    if let Err(error) = load_track_at_index(
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
                        if let Some(player) = player {
                            player.stop();
                            player.clear();
                        }
                        update_state_for_stop(
                            state,
                            writer,
                            ListenEndReason::PlaybackError,
                            source,
                        );
                        emit_state_changed(app_handle, state);

                        return Err(error);
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
            if !was_active {
                outcome = CommandOutcome::Noop;
            }
            lock_state(state).observation.awaiting_progress = None;
            if was_active {
                record_event(state, writer, PlaybackEvent::PlaybackPaused, source);
                let (listen_id, track_id, position_ms) = {
                    let s = lock_state(state);
                    (
                        s.active_listen_id.clone(),
                        s.current_track.as_ref().map(|track| track.id),
                        s.position_ms,
                    )
                };
                log::debug!(
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
            if lock_state(state).current_track.is_none() {
                return Ok(CommandFlow::Continue(CommandOutcome::Noop));
            }
            record_event(state, writer, PlaybackEvent::PlaybackStopped, source);
            if let Some(player) = player {
                player.stop();
                player.clear();
            }
            update_state_for_stop(state, writer, ListenEndReason::Stopped, source);
            end_active_session(state);
            emit_state_changed(app_handle, state);
            save_session_to_db(state, writer);
            log::debug!(target: "sparkle::playback", "event=playback_stopped source={}", source.as_str());
        }
        AudioCommand::Seek(position_ms, source) => {
            let (track_id, duration_ms) = {
                let s = lock_state(state);
                (
                    s.current_track.as_ref().map(|track| track.id),
                    s.duration_ms,
                )
            };
            if track_id.is_none() {
                return Ok(CommandFlow::Continue(CommandOutcome::Noop));
            }
            let position_ms = clamp_seek_position(position_ms, duration_ms);
            if let Some(player) = player {
                if let Err(error) = timed_stage(state, "seek", track_id, || {
                    seek_player(player, state, position_ms)
                }) {
                    let mut s = lock_state(state);
                    s.position_ms = player.get_pos().as_millis() as i64;
                    s.seek_target = None;
                    s.last_counted_position_ms = None;
                    drop(s);
                    emit_state_changed(app_handle, state);
                    return Err(error);
                }
                record_event(
                    state,
                    writer,
                    PlaybackEvent::Seeked {
                        reason: SeekReason::Absolute,
                        target_position_ms: position_ms,
                    },
                    source,
                );
            } else {
                outcome = CommandOutcome::Deferred;
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
        AudioCommand::SeekLyrics(track_id, position_ms) => {
            let target = {
                let s = lock_state(state);
                lyric_seek_target(
                    s.current_track.as_ref().map(|track| track.id),
                    track_id,
                    position_ms,
                    s.duration_ms,
                )
            };
            if let Some((position_ms, pause_at_end)) = target {
                if pause_at_end {
                    handle_command(
                        AudioCommand::Pause(PlaybackSource::Ui),
                        player,
                        state,
                        db,
                        writer,
                        app_handle,
                    )?;
                }
                return handle_command(
                    AudioCommand::Seek(position_ms, PlaybackSource::Ui),
                    player,
                    state,
                    db,
                    writer,
                    app_handle,
                );
            } else {
                outcome = CommandOutcome::Noop;
            }
        }
        AudioCommand::RefreshLyrics(track_id) => {
            let is_current = lock_state(state)
                .current_track
                .as_ref()
                .map(|track| track.id)
                == Some(track_id);
            if is_current {
                let track = load_track_from_db(db, track_id)
                    .map_err(|error| PlaybackFailure::new("metadata", Some(track_id), error))?;
                let first_line = known_first_lyric_line(db, &track).map_err(|error| {
                    PlaybackFailure::new("lyrics_metadata", Some(track_id), error)
                })?;
                {
                    let mut s = lock_state(state);
                    s.current_track = Some(track);
                    s.first_lyric_line = first_line;
                }
                emit_state_changed(app_handle, state);
            } else {
                outcome = CommandOutcome::Noop;
            }
        }
        AudioCommand::AutoAdvance => {
            outcome = advance(
                player,
                state,
                db,
                writer,
                app_handle,
                true,
                PlaybackSource::Automatic,
            )?;
        }
        AudioCommand::Next(source) => {
            outcome = advance(player, state, db, writer, app_handle, false, source)?;
            save_session_to_db(state, writer);
        }
        AudioCommand::Previous(source) => {
            let (pos, order_pos) = {
                let s = lock_state(state);
                (s.position_ms, s.order_pos)
            };
            match previous_target(pos, order_pos) {
                PreviousTarget::Restart => {
                    let event = event_record_locked(
                        &lock_state(state),
                        PlaybackEvent::Seeked {
                            reason: SeekReason::PreviousRestart,
                            target_position_ms: 0,
                        },
                        source,
                    );
                    seek_to_start(player, state)?;
                    if player.is_some() {
                        writer.record_event(event);
                    } else {
                        outcome = CommandOutcome::Deferred;
                    }
                    emit_state_changed(app_handle, state);
                }
                PreviousTarget::Position(prev_pos) => {
                    finalize_active_listen(state, writer, ListenEndReason::ManualPrevious, source);
                    let prev_idx = {
                        let mut s = lock_state(state);
                        s.order_pos = Some(prev_pos);
                        s.queue_index = s.play_order.get(prev_pos).copied();
                        s.queue_index
                    };
                    if let Some(i) = prev_idx {
                        if let Err(error) = load_track_at_index(
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
                            if let Some(player) = player {
                                player.stop();
                                player.clear();
                            }
                            update_state_for_stop(
                                state,
                                writer,
                                ListenEndReason::PlaybackError,
                                source,
                            );
                            emit_state_changed(app_handle, state);

                            return Err(error);
                        }
                    }
                }
                PreviousTarget::Noop => outcome = CommandOutcome::Noop,
            }
            emit_queue_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::SetShuffle(shuffle, source) => {
            let changed = {
                let mut s = lock_state(state);
                let SharedState {
                    shuffle: previous,
                    queue,
                    queue_index,
                    play_order,
                    order_pos,
                    ..
                } = &mut *s;
                change_shuffle(
                    previous,
                    shuffle,
                    queue.len(),
                    *queue_index,
                    play_order,
                    order_pos,
                )
            };
            if !changed {
                return Ok(CommandFlow::Continue(CommandOutcome::Noop));
            }
            {
                record_event(state, writer, PlaybackEvent::ShuffleChanged, source);
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
            record_event(state, writer, PlaybackEvent::RepeatChanged, source);
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
            let start_immediately = match current {
                (Some(cur_idx), Some(pos)) => {
                    let mut s = lock_state(state);
                    let SharedState {
                        queue, play_order, ..
                    } = &mut *s;
                    let Some((current_index, order_pos)) =
                        queue_track_next(queue, play_order, cur_idx, pos, track_id)
                    else {
                        return Ok(CommandFlow::Continue(CommandOutcome::Noop));
                    };
                    s.queue_index = Some(current_index);
                    s.order_pos = Some(order_pos);
                    false
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
                    true
                }
            };
            record_event(
                state,
                writer,
                PlaybackEvent::QueuedNext {
                    target_track_id: track_id,
                },
                source,
            );
            log::debug!(
                target: "sparkle::playback",
                "event=queued_next source={} target_track_id={track_id}",
                source.as_str()
            );
            if start_immediately {
                if let Err(error) = load_track_at_index(
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
                    if let Some(player) = player {
                        player.stop();
                        player.clear();
                    }
                    update_state_for_stop(state, writer, ListenEndReason::PlaybackError, source);
                    emit_state_changed(app_handle, state);

                    return Err(error);
                }
            }
            emit_queue_changed(app_handle, state);
            save_session_to_db(state, writer);
        }
        AudioCommand::GetQueue(reply) => {
            let view = build_queue_view(state, db);
            let _ = reply.send(view);
        }
        AudioCommand::PlayAt(order_pos, source) => {
            let valid = order_pos < lock_state(state).play_order.len();
            if !valid {
                return Err(PlaybackFailure::new(
                    "queue_lookup",
                    None,
                    "The requested queue entry is unavailable.",
                ));
            }
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
                if let Err(error) = load_track_at_index(
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
                    if let Some(player) = player {
                        player.stop();
                        player.clear();
                    }
                    update_state_for_stop(state, writer, ListenEndReason::PlaybackError, source);
                    emit_state_changed(app_handle, state);

                    return Err(error);
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
        AudioCommand::Shutdown(reply) => return Ok(CommandFlow::Shutdown(reply)),
        AudioCommand::Execute(_) => unreachable!("requests are unwrapped by dispatch_command"),
    }
    if may_defer
        && outcome == CommandOutcome::Applied
        && player.is_none()
        && lock_state(state).play_when_device_ready
    {
        outcome = CommandOutcome::Deferred;
    }
    Ok(CommandFlow::Continue(outcome))
}

fn clamp_seek_position(position_ms: i64, duration_ms: i64) -> i64 {
    if duration_ms > 0 {
        // EOF has no audio frame to seek to. Stay inside the stream at the
        // millisecond precision used by playback state and command replies.
        position_ms.clamp(0, duration_ms - 1)
    } else {
        position_ms.max(0)
    }
}

fn seek_player(
    player: &Player,
    state: &Arc<Mutex<SharedState>>,
    position_ms: i64,
) -> Result<(), PlaybackFailure> {
    let was_playing = !player.is_paused();
    player.pause();
    let result = match player.try_seek(Duration::from_millis(position_ms.max(0) as u64)) {
        Ok(()) => Ok(()),
        Err(error) => {
            log::debug!(target: "sparkle::audio", "event=seek_fallback_started target_position_ms={position_ms} error={error:?}");
            let (track, gain) = {
                let s = lock_state(state);
                (
                    s.current_track.clone(),
                    combined_gain(s.volume, s.latched_sound_check_gain_db),
                )
            };
            let track =
                track.ok_or_else(|| PlaybackFailure::new("seek", None, "No track is loaded."))?;
            reload_source_at_position(player, &track, gain, position_ms, |player, position| {
                player
                    .try_seek(position)
                    .map_err(|error| format!("{error:?}"))
            })
        }
    };
    if was_playing {
        player.play();
    }
    result
}

fn seek_to_start(
    player: Option<&Player>,
    state: &Arc<Mutex<SharedState>>,
) -> Result<(), PlaybackFailure> {
    if let Some(player) = player {
        seek_player(player, state, 0)?;
    }
    let mut s = lock_state(state);
    s.position_ms = 0;
    s.seek_target = Some((0, Instant::now()));
    s.last_counted_position_ms = None;
    Ok(())
}

fn lyric_seek_target(
    current_track_id: Option<i64>,
    requested_track_id: i64,
    position_ms: i64,
    duration_ms: i64,
) -> Option<(i64, bool)> {
    if current_track_id != Some(requested_track_id) {
        return None;
    }
    if duration_ms > 0 && position_ms >= duration_ms {
        // Keep the decoder inside this track and pause before seeking. An
        // invalid LRC timestamp must not exhaust the source and advance queue.
        Some((duration_ms.saturating_sub(1), true))
    } else {
        Some((position_ms.max(0), false))
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
) -> Result<CommandOutcome, PlaybackFailure> {
    let (order_pos, order_len, repeat_mode, queue_index) = {
        let s = lock_state(state);
        (
            s.order_pos,
            s.play_order.len(),
            s.repeat_mode,
            s.queue_index,
        )
    };

    let target = advance_target(order_pos, order_len, queue_index, repeat_mode, auto);
    // Repeat-one replays the current track on automatic advance; a manual
    // skip still moves to the next track.
    if let AdvanceTarget::RepeatCurrent(i) = target {
        if let Err(error) = load_track_at_index(
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

            return Err(error);
        }
        save_session_to_db(state, writer);
        return Ok(if player.is_some() {
            CommandOutcome::Applied
        } else {
            CommandOutcome::Deferred
        });
    }

    let next_pos = match target {
        AdvanceTarget::NextPosition(pos) => Some(pos),
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
            if let Err(error) = load_track_at_index(
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
                if let Some(player) = player {
                    player.stop();
                    player.clear();
                }
                update_state_for_stop(state, writer, ListenEndReason::PlaybackError, source);
                emit_state_changed(app_handle, state);

                return Err(error);
            }
        } else {
            if let Some(player) = player {
                player.stop();
                player.clear();
            }
            update_state_for_stop(state, writer, ListenEndReason::PlaybackError, source);
            emit_state_changed(app_handle, state);
            return Err(PlaybackFailure::new(
                "queue_lookup",
                None,
                "The next queue entry is unavailable.",
            ));
        }
    } else if let AdvanceTarget::Finish(queue_index) = target {
        // End of the queue with repeat off: pause and keep the last track
        // loaded at position 0 so pressing play starts it again. The track
        // stays visible instead of the player emptying out.
        if let Some(i) = queue_index {
            if let Err(error) = load_track_at_index_with_autoplay(
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

                return Err(error);
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
            return Err(PlaybackFailure::new(
                "queue_lookup",
                None,
                "The current queue entry is unavailable.",
            ));
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
    Ok(if !auto && next_pos.is_none() {
        CommandOutcome::Noop
    } else if player.is_none() {
        CommandOutcome::Deferred
    } else {
        CommandOutcome::Applied
    })
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
    s.pending_start_reason = ListenStartReason::Unknown;
    s.current_track = None;
    s.observation.awaiting_progress = None;
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
) -> Result<(), PlaybackFailure> {
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
) -> Result<(), PlaybackFailure> {
    // A track transition finalizes the previous listen before metadata swaps.
    finalize_active_listen(state, writer, end_reason, source);
    let track_id = {
        let s = lock_state(state);
        s.queue.get(index).copied()
    };
    let track_id = match track_id {
        Some(id) => id,
        None => {
            return Err(PlaybackFailure::new(
                "queue_lookup",
                None,
                "The requested queue entry is unavailable.",
            ))
        }
    };

    // Stop the previous source before opening another file; failed loads must
    // not leave sound playing behind a stopped UI.
    if let Some(player) = player {
        player.pause();
        player.stop();
        player.clear();
    }
    let track = timed_stage(state, "metadata", Some(track_id), || {
        load_track_from_db(db, track_id)
            .map_err(|error| PlaybackFailure::new("metadata", Some(track_id), error))
    })?;
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
        s.is_playing = false;
        s.play_when_device_ready = play_when_device_ready;
        if autoplay || play_when_device_ready {
            s.observation.awaiting_progress = s.observation.current_operation.clone();
        }
        s.pending_play_source = source;
        s.pending_start_reason = start_reason;
        s.latched_sound_check_gain_db = latched_gain_db;
        s.position_ms = 0;
        s.duration_ms = track.duration_ms.unwrap_or(0);
        s.seek_target = None;
        s.listened_ms = 0;
        s.last_counted_position_ms = None;
    }
    emit_state_changed(app_handle, state);

    let Some(player) = player else {
        return Ok(());
    };

    let file = timed_stage(state, "file_open", Some(track.id), || {
        File::open(&track.file_path)
            .map_err(|error| PlaybackFailure::new("file_open", Some(track.id), error))
    })?;
    let decoded_source = timed_stage(state, "decode", Some(track.id), || {
        decode_playback_file(file)
            .map_err(|error| PlaybackFailure::new("decode", Some(track.id), error))
    })?;
    lock_state(state).duration_ms =
        decoded_duration_ms(&decoded_source, track.duration_ms.unwrap_or(0));

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
        lock_state(state).is_playing = true;
        begin_active_listen(state, writer, source, start_reason);
    } else {
        player.pause();
    }

    if analysis_pending {
        log::debug!(
            target: "sparkle::loudness",
            "event=playback_started_unscanned track_id={} fallback=unity gain_latched=true",
            track.id
        );
    }

    emit_state_changed(app_handle, state);
    Ok(())
}

fn reload_source_at_position(
    player: &Player,
    track: &Track,
    gain: Float,
    position_ms: i64,
    seek: impl FnOnce(&Player, Duration) -> Result<(), String>,
) -> Result<(), PlaybackFailure> {
    let file = File::open(&track.file_path)
        .map_err(|error| PlaybackFailure::new("seek_reopen", Some(track.id), error))?;
    let source = decode_playback_file(file)
        .map_err(|error| PlaybackFailure::new("seek_decode", Some(track.id), error))?;

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
    // A newly opened decoder is already at zero; asking it to seek there again
    // can repeat a format-specific failure that caused this reload.
    let result = if pos.is_zero() {
        Ok(())
    } else {
        seek(player, pos)
            .map_err(|error| PlaybackFailure::new("seek_after_reload", Some(track.id), error))
    };

    player.set_volume(gain);

    result
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
    Ok(lyrics::known_lyrics(&conn, track)?
        .and_then(|lyrics| lyrics.synced_text)
        .as_deref()
        .and_then(lyrics::first_synced_line))
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
             ORDER BY ta.position, a.name",
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

fn publish_event<T: Serialize + Clone>(
    app_handle: &AppHandle,
    state: &Arc<Mutex<SharedState>>,
    name: &str,
    payload: T,
) {
    if let Err(error) = app_handle.emit(name, payload) {
        let count = {
            let mut s = lock_state(state);
            s.observation.state_delivery_failures += 1;
            s.observation.state_delivery_failures
        };
        log::log!(target: "sparkle::playback", if count == 1 { log::Level::Warn } else { log::Level::Trace },
            "event=frontend_event_delivery_failed channel={name} failures={count} error={error}");
    }
}

fn emit_state_changed(app_handle: &AppHandle, state: &Arc<Mutex<SharedState>>) {
    let ps = {
        let mut s = lock_state(state);
        s.revision += 1;
        playback_state_locked(&s)
    };
    log::trace!(
        target: "sparkle::playback",
        "event=state_publish_requested playing={} track_id={:?} position_ms={} duration_ms={}",
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
    publish_event(app_handle, state, "playback-state-changed", ps);
}

fn emit_queue_changed(app_handle: &AppHandle, state: &Arc<Mutex<SharedState>>) {
    refresh_loudness_priorities(state);
    publish_event(app_handle, state, "queue-changed", ());
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
    playback_state_locked(&lock_state(state))
}

fn playback_state_locked(s: &SharedState) -> PlaybackState {
    PlaybackState {
        revision: s.revision,
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
