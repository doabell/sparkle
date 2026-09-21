use crate::analytics::{new_trace_id, now_epoch_ms, PlaybackSource};
use crate::models::PlaybackState;
use serde::Serialize;
use std::collections::VecDeque;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommandOutcome {
    Applied,
    Deferred,
    Noop,
}

impl CommandOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Deferred => "deferred",
            Self::Noop => "noop",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PlaybackFailure {
    pub command_id: Option<String>,
    pub command: Option<String>,
    pub stage: String,
    pub track_id: Option<i64>,
    pub message: String,
}

impl PlaybackFailure {
    pub fn new(stage: &str, track_id: Option<i64>, message: impl ToString) -> Self {
        Self {
            command_id: None,
            command: None,
            stage: stage.into(),
            track_id,
            message: sparkle_core::logging::sanitize(&message.to_string()),
        }
    }

    pub fn for_command(mut self, id: &str, name: &str) -> Self {
        self.command_id = Some(id.into());
        self.command = Some(name.into());
        self
    }
}

impl std::fmt::Display for PlaybackFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct CommandReply {
    pub command_id: String,
    pub outcome: CommandOutcome,
    pub state: PlaybackState,
}

#[derive(Clone)]
pub(crate) struct Operation {
    pub id: String,
    pub name: &'static str,
    pub source: PlaybackSource,
    pub queued_at: Instant,
    pub started_at: Instant,
    pub started_at_ms: i64,
    pub target_track_id: Option<i64>,
}

impl Operation {
    pub fn new(
        id: Option<String>,
        name: &'static str,
        source: PlaybackSource,
        target_track_id: Option<i64>,
    ) -> Self {
        let now = Instant::now();
        // Only accept generated identifiers, never arbitrary caller content in logs.
        let id = id
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 80
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            })
            .unwrap_or_else(|| new_trace_id("command"));
        Self {
            id,
            name,
            source,
            queued_at: now,
            started_at: now,
            started_at_ms: now_epoch_ms(),
            target_track_id,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CommandObservation {
    pub command_id: String,
    pub command: String,
    pub source: String,
    pub occurred_at_ms: i64,
    pub track_id: Option<i64>,
    pub listen_id: Option<String>,
    pub queue_wait_ms: u64,
    pub execution_ms: u64,
    pub first_progress_ms: Option<u64>,
    pub outcome: String,
    pub failure: Option<PlaybackFailure>,
    pub stages: Vec<StageTiming>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct StageTiming {
    pub stage: String,
    pub elapsed_ms: u64,
    pub success: bool,
}

#[derive(Serialize)]
pub(crate) struct ActiveStage {
    pub stage: String,
    pub started_at_ms: i64,
}

#[derive(Serialize)]
pub(crate) struct PlaybackObservation {
    pub run_id: String,
    pub output_available: bool,
    pub output_config: Option<String>,
    pub recovery_started_at_ms: Option<i64>,
    pub recovery_attempts: u64,
    pub device_open_started_at_ms: Option<i64>,
    pub last_recovery_ms: Option<u64>,
    pub state_delivery_failures: u64,
    pub last_failure: Option<PlaybackFailure>,
    pub recent_commands: VecDeque<CommandObservation>,
    pub current_stages: Vec<StageTiming>,
    pub active_stage: Option<ActiveStage>,
    #[serde(skip)]
    pub current_operation: Option<Operation>,
    #[serde(skip)]
    pub awaiting_progress: Option<Operation>,
}

impl Default for PlaybackObservation {
    fn default() -> Self {
        Self {
            run_id: new_trace_id("run"),
            output_available: false,
            output_config: None,
            recovery_started_at_ms: None,
            recovery_attempts: 0,
            device_open_started_at_ms: None,
            last_recovery_ms: None,
            state_delivery_failures: 0,
            last_failure: None,
            recent_commands: VecDeque::new(),
            current_stages: Vec::new(),
            active_stage: None,
            current_operation: None,
            awaiting_progress: None,
        }
    }
}

impl PlaybackObservation {
    pub fn record(&mut self, record: CommandObservation) {
        if let Some(failure) = &record.failure {
            self.last_failure = Some(failure.clone());
        }
        if record.command == "set_volume"
            && record.failure.is_none()
            && self
                .recent_commands
                .back()
                .is_some_and(|last| last.command == "set_volume" && last.failure.is_none())
        {
            self.recent_commands.pop_back();
        }
        if self.recent_commands.len() == 200 {
            self.recent_commands.pop_front();
        }
        self.recent_commands.push_back(record);
    }
}

#[cfg(test)]
#[path = "tests/playback_observation.rs"]
mod tests;
