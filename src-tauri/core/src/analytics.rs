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

// One vocabulary is used by live producers, recovery, and backup import.
// Aliases are accepted on input only; new records always use canonical names.
macro_rules! vocabulary {
    ($name:ident { $($variant:ident => $label:literal $(| $alias:literal)*,)* }) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub enum $name { $($variant,)* #[default] Unknown }
        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $label,)* Self::Unknown => "unknown" }
            }
            pub fn parse(value: &str) -> Self {
                match value.trim().to_ascii_lowercase().as_str() {
                    $($label $(| $alias)* => Self::$variant,)*
                    _ => Self::Unknown,
                }
            }
        }
    };
}

vocabulary!(PlaybackEventKind {
    QueueLoaded => "queue_loaded",
    ListenStarted => "listen_started" | "track_started",
    PlaybackResumed => "playback_resumed",
    PlaybackPaused => "playback_paused",
    Seeked => "seeked",
    ListenEnded => "listen_ended",
    PlaybackStopped => "playback_stopped",
    ShuffleChanged => "shuffle_changed",
    RepeatChanged => "repeat_changed",
    QueuedNext => "queued_next",
    OutputUnavailable => "output_unavailable",
    OutputRestored => "output_restored",
    CommandFailed => "command_failed",
});

vocabulary!(ListenStartReason {
    QueueStarted => "queue_started",
    TrackSelected => "track_selected",
    ManualNext => "manual_next",
    ManualPrevious => "manual_previous",
    QueueJump => "queue_jump",
    AutoAdvance => "auto_advance",
    RepeatOne => "repeat_one",
    PlayNext => "play_next",
    Replay => "replay",
    ResumeAfterInactivity => "resume_after_inactivity",
    RestoredResume => "restored_resume",
    OutputRestored => "output_restored",
    LegacyMigration => "legacy_migration",
    LegacyImport => "legacy_import",
});

vocabulary!(ListenEndReason {
    Completed => "completed",
    ManualNext => "manual_next",
    ManualPrevious => "manual_previous",
    QueueJump => "queue_jump",
    QueueReplaced => "queue_replaced",
    TrackSelected => "track_selected",
    Stopped => "stopped",
    AppShutdown => "app_shutdown",
    RepeatOne => "repeat_one",
    SessionTimeout => "session_timeout",
    PlaybackError => "playback_error",
    Interrupted => "interrupted",
    LegacyMigration => "legacy_migration",
    LegacyImport => "legacy_import",
});

impl PlaybackEventKind {
    /// Imported records use the same vocabulary as live producers. Old
    /// redundant reasons are discarded, and unproven output causes stay unknown.
    pub fn canonical_reason(self, value: Option<&str>) -> Option<&'static str> {
        let value = value.unwrap_or("").trim().to_ascii_lowercase();
        match self {
            Self::ListenStarted => Some(ListenStartReason::parse(&value).as_str()),
            Self::ListenEnded => Some(ListenEndReason::parse(&value).as_str()),
            Self::QueueLoaded => Some(match value.as_str() {
                "queue_replaced" => "queue_replaced",
                "single_track" => "single_track",
                _ => "unknown",
            }),
            Self::Seeked => Some(match value.as_str() {
                "absolute" => "absolute",
                "previous_restart" => "previous_restart",
                _ => "unknown",
            }),
            Self::OutputUnavailable => Some(match value.as_str() {
                "device_changed" => "device_changed",
                "clock_stalled" => "clock_stalled",
                "open_failed" => "open_failed",
                _ => "unknown",
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueLoadReason {
    QueueReplaced,
    SingleTrack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeekReason {
    Absolute,
    PreviousRestart,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputUnavailableReason {
    DeviceChanged,
    ClockStalled,
    OpenFailed,
}

impl OutputUnavailableReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeviceChanged => "device_changed",
            Self::ClockStalled => "clock_stalled",
            Self::OpenFailed => "open_failed",
        }
    }
}

/// Variant-specific data prevents a queue target from replacing the current
/// listen's track, and prevents unrelated reasons from being mixed together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaybackEvent {
    QueueLoaded(QueueLoadReason),
    ListenStarted(ListenStartReason),
    PlaybackResumed,
    PlaybackPaused,
    Seeked {
        reason: SeekReason,
        target_position_ms: i64,
    },
    ListenEnded(ListenEndReason),
    PlaybackStopped,
    ShuffleChanged,
    RepeatChanged,
    QueuedNext {
        target_track_id: i64,
    },
    OutputUnavailable(OutputUnavailableReason),
    OutputRestored,
    CommandFailed {
        command: String,
        stage: String,
        target_track_id: Option<i64>,
    },
}

impl PlaybackEvent {
    pub fn kind(&self) -> PlaybackEventKind {
        match self {
            Self::QueueLoaded(_) => PlaybackEventKind::QueueLoaded,
            Self::ListenStarted(_) => PlaybackEventKind::ListenStarted,
            Self::PlaybackResumed => PlaybackEventKind::PlaybackResumed,
            Self::PlaybackPaused => PlaybackEventKind::PlaybackPaused,
            Self::Seeked { .. } => PlaybackEventKind::Seeked,
            Self::ListenEnded(_) => PlaybackEventKind::ListenEnded,
            Self::PlaybackStopped => PlaybackEventKind::PlaybackStopped,
            Self::ShuffleChanged => PlaybackEventKind::ShuffleChanged,
            Self::RepeatChanged => PlaybackEventKind::RepeatChanged,
            Self::QueuedNext { .. } => PlaybackEventKind::QueuedNext,
            Self::OutputUnavailable(_) => PlaybackEventKind::OutputUnavailable,
            Self::OutputRestored => PlaybackEventKind::OutputRestored,
            Self::CommandFailed { .. } => PlaybackEventKind::CommandFailed,
        }
    }

    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::QueueLoaded(QueueLoadReason::QueueReplaced) => Some("queue_replaced"),
            Self::QueueLoaded(QueueLoadReason::SingleTrack) => Some("single_track"),
            Self::ListenStarted(reason) => Some(reason.as_str()),
            Self::ListenEnded(reason) => Some(reason.as_str()),
            Self::Seeked {
                reason: SeekReason::Absolute,
                ..
            } => Some("absolute"),
            Self::Seeked {
                reason: SeekReason::PreviousRestart,
                ..
            } => Some("previous_restart"),
            Self::OutputUnavailable(reason) => Some(reason.as_str()),
            _ => None,
        }
    }

    pub fn target_track_id(&self) -> Option<i64> {
        match self {
            Self::QueuedNext { target_track_id } => Some(*target_track_id),
            Self::CommandFailed {
                target_track_id, ..
            } => *target_track_id,
            _ => None,
        }
    }

    pub fn target_position_ms(&self) -> Option<i64> {
        match self {
            Self::Seeked {
                target_position_ms, ..
            } => Some(*target_position_ms),
            _ => None,
        }
    }

    pub fn command(&self) -> Option<&str> {
        match self {
            Self::CommandFailed { command, .. } => Some(command),
            _ => None,
        }
    }

    pub fn failure_stage(&self) -> Option<&str> {
        match self {
            Self::CommandFailed { stage, .. } => Some(stage),
            _ => None,
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
    pub run_id: String,
    pub command_id: Option<String>,
    pub listen_id: Option<String>,
    pub session_id: Option<String>,
    pub occurred_at_ms: i64,
    pub event: PlaybackEvent,
    pub source: PlaybackSource,
    pub track_id: Option<i64>,
    pub position_ms: Option<i64>,
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
