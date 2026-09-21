use crate::models::RepeatMode;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MIN_MEANINGFUL_LISTEN_MS: i64 = 30_000;
pub const MIN_SHORT_TRACK_LISTEN_MS: i64 = 5_000;
pub const LISTENING_SESSION_GAP_MS: i64 = 20 * 60 * 1_000;

static TRACE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The entry point that caused a playback transition. Sources describe intent,
/// not the platform that happened to execute it, and are intentionally stable
/// analytics vocabulary rather than display strings.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackSource {
    Ui,
    Keyboard,
    SystemMedia,
    Automatic,
    Restore,
    Internal,
    Legacy,
    #[default]
    #[serde(other)]
    Unknown,
}

impl PlaybackSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ui => "ui",
            Self::Keyboard => "keyboard",
            Self::SystemMedia => "system_media",
            Self::Automatic => "automatic",
            Self::Restore => "restore",
            Self::Internal => "internal",
            Self::Legacy => "legacy",
            Self::Unknown => "unknown",
        }
    }
}

/// Where a queue originated. `id` is deliberately an opaque local identifier:
/// callers must not put paths, search text, titles, or other user content here.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PlaybackContext {
    pub kind: String,
    #[serde(default)]
    pub id: Option<String>,
}

impl Default for PlaybackContext {
    fn default() -> Self {
        Self {
            kind: "unknown".to_string(),
            id: None,
        }
    }
}

impl PlaybackContext {
    pub fn sanitized(self) -> Self {
        const KINDS: &[&str] = &[
            "album", "artist", "genre", "health", "home", "playlist", "queue", "search", "single",
            "songs", "unknown",
        ];
        let kind = self.kind.trim().to_ascii_lowercase();
        let kind = if KINDS.contains(&kind.as_str()) {
            kind
        } else {
            "unknown".to_string()
        };
        // Context IDs are deliberately stricter than arbitrary opaque text.
        // Entity IDs are local numeric primary keys; health IDs are stable
        // vocabulary tokens. Contexts that do not need an ID discard one so
        // a future caller cannot accidentally persist a title or search term.
        let candidate = self.id.map(|value| value.trim().to_string());
        let id = match kind.as_str() {
            "album" | "artist" | "playlist" => candidate.filter(|value| {
                !value.is_empty()
                    && value.len() <= 20
                    && value.bytes().all(|byte| byte.is_ascii_digit())
            }),
            "health" => candidate.filter(|value| {
                !value.is_empty()
                    && value.len() <= 64
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
            }),
            _ => None,
        };
        Self { kind, id }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackEventKind {
    QueueLoaded,
    TrackStarted,
    PlaybackResumed,
    PlaybackPaused,
    Seeked,
    ListenEnded,
    PlaybackStopped,
    ShuffleChanged,
    RepeatChanged,
    QueuedNext,
    OutputUnavailable,
    OutputRestored,
}

impl PlaybackEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::QueueLoaded => "queue_loaded",
            Self::TrackStarted => "track_started",
            Self::PlaybackResumed => "playback_resumed",
            Self::PlaybackPaused => "playback_paused",
            Self::Seeked => "seeked",
            Self::ListenEnded => "listen_ended",
            Self::PlaybackStopped => "playback_stopped",
            Self::ShuffleChanged => "shuffle_changed",
            Self::RepeatChanged => "repeat_changed",
            Self::QueuedNext => "queued_next",
            Self::OutputUnavailable => "output_unavailable",
            Self::OutputRestored => "output_restored",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListenStartReason {
    QueueStarted,
    TrackSelected,
    ManualNext,
    ManualPrevious,
    QueueJump,
    AutoAdvance,
    RepeatOne,
    PlayNext,
    Replay,
    ResumeAfterInactivity,
    RestoredResume,
    OutputRestored,
    #[default]
    Unknown,
}

impl ListenStartReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::QueueStarted => "queue_started",
            Self::TrackSelected => "track_selected",
            Self::ManualNext => "manual_next",
            Self::ManualPrevious => "manual_previous",
            Self::QueueJump => "queue_jump",
            Self::AutoAdvance => "auto_advance",
            Self::RepeatOne => "repeat_one",
            Self::PlayNext => "play_next",
            Self::Replay => "replay",
            Self::ResumeAfterInactivity => "resume_after_inactivity",
            Self::RestoredResume => "restored_resume",
            Self::OutputRestored => "output_restored",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListenEndReason {
    Completed,
    ManualNext,
    ManualPrevious,
    QueueJump,
    QueueReplaced,
    TrackSelected,
    Stopped,
    AppShutdown,
    RepeatOne,
    SessionTimeout,
    PlaybackError,
}

impl ListenEndReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::ManualNext => "manual_next",
            Self::ManualPrevious => "manual_previous",
            Self::QueueJump => "queue_jump",
            Self::QueueReplaced => "queue_replaced",
            Self::TrackSelected => "track_selected",
            Self::Stopped => "stopped",
            Self::AppShutdown => "app_shutdown",
            Self::RepeatOne => "repeat_one",
            Self::SessionTimeout => "session_timeout",
            Self::PlaybackError => "playback_error",
        }
    }
}

/// A durable, query-friendly materialization of one attempt to listen to a
/// track. Open records are periodically checkpointed and finalized on a
/// semantic boundary. Tiny listens remain available for skip analytics while
/// `meaningful` preserves the existing public statistics definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListenRecord {
    pub id: String,
    pub session_id: String,
    pub track_id: i64,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub last_activity_at_ms: i64,
    pub start_position_ms: i64,
    pub end_position_ms: i64,
    pub duration_ms: i64,
    pub listened_ms: i64,
    pub meaningful: bool,
    pub completed: bool,
    pub finalized: bool,
    pub start_source: PlaybackSource,
    pub start_reason: ListenStartReason,
    pub end_reason: Option<ListenEndReason>,
    pub context: PlaybackContext,
    pub queue_index: Option<usize>,
    pub play_order_index: Option<usize>,
    pub queue_length: usize,
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,
}

/// An immutable semantic transition. Events intentionally contain only local
/// IDs and numeric playback state; display metadata and filesystem paths do not
/// belong in analytics or diagnostic correlation records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaybackEventRecord {
    pub id: String,
    pub listen_id: Option<String>,
    pub session_id: Option<String>,
    pub occurred_at_ms: i64,
    pub kind: PlaybackEventKind,
    pub source: PlaybackSource,
    pub reason: Option<String>,
    pub track_id: Option<i64>,
    pub position_ms: Option<i64>,
    pub target_position_ms: Option<i64>,
    pub context: PlaybackContext,
    pub queue_index: Option<usize>,
    pub play_order_index: Option<usize>,
    pub queue_length: usize,
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,
}

pub fn is_meaningful_listen(listened_ms: i64, duration_ms: i64) -> bool {
    listened_ms >= MIN_MEANINGFUL_LISTEN_MS
        || (duration_ms > 0
            && listened_ms >= MIN_SHORT_TRACK_LISTEN_MS
            && listened_ms.saturating_mul(2) >= duration_ms)
}

pub fn is_completed(position_ms: i64, duration_ms: i64) -> bool {
    duration_ms > 0 && position_ms.saturating_mul(10) >= duration_ms.saturating_mul(9)
}

pub fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

pub fn new_trace_id(prefix: &str) -> String {
    let counter = TRACE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}-{:x}-{:x}-{counter:x}", std::process::id(), nanos)
}

#[cfg(test)]
#[path = "tests/analytics.rs"]
mod tests;
