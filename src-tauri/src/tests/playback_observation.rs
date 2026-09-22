use super::*;

fn record(index: usize, command: &str) -> CommandObservation {
    CommandObservation {
        command_id: format!("command-{index}"),
        command: command.into(),
        source: "ui".into(),
        occurred_at_ms: 1,
        track_id: Some(7),
        listen_id: None,
        queue_wait_ms: 0,
        execution_ms: 1,
        first_progress_ms: None,
        outcome: "applied".into(),
        failure: None,
        stages: Vec::new(),
    }
}

#[test]
fn command_ids_are_preserved_or_safely_replaced_and_errors_keep_correlation() {
    let valid = Operation::new(
        Some("request-123".into()),
        "seek",
        PlaybackSource::Keyboard,
        Some(7),
    );
    assert_eq!(valid.id, "request-123");
    let invalid = Operation::new(
        Some("private title\ncommand=other".into()),
        "seek",
        PlaybackSource::Ui,
        None,
    );
    assert!(invalid.id.starts_with("command-"));
    let error = PlaybackFailure::new(
        "decode",
        Some(7),
        "failed https://secret.example/path\ninjected",
    )
    .for_command(&valid.id, valid.name);
    let json = serde_json::to_value(error).unwrap();
    assert_eq!(json["command_id"], "request-123");
    assert_eq!(json["command"], "seek");
    assert_eq!(json["stage"], "decode");
    assert_eq!(json["track_id"], 7);
    assert_eq!(json["message"], "failed [url]\\ninjected");
}

#[test]
fn recent_commands_are_bounded_and_volume_drags_do_not_evict_failure_context() {
    let mut observation = PlaybackObservation::default();
    let mut failure = record(0, "seek");
    failure.failure = Some(PlaybackFailure::new(
        "seek_after_reload",
        Some(7),
        "unsupported",
    ));
    observation.record(failure);
    for index in 1..500 {
        observation.record(record(index, "set_volume"));
    }
    assert_eq!(observation.recent_commands.len(), 2);
    assert_eq!(
        observation.recent_commands.back().unwrap().command_id,
        "command-499"
    );
    assert_eq!(
        observation.last_failure.as_ref().unwrap().stage,
        "seek_after_reload"
    );
    for index in 500..800 {
        observation.record(record(index, "play"));
    }
    assert_eq!(observation.recent_commands.len(), 200);
    assert_eq!(
        observation.recent_commands.front().unwrap().command_id,
        "command-600"
    );
    assert_eq!(CommandOutcome::Applied.as_str(), "applied");
    assert_eq!(CommandOutcome::Deferred.as_str(), "deferred");
    assert_eq!(CommandOutcome::Noop.as_str(), "noop");
}

#[test]
fn output_events_follow_availability_and_observed_causes_without_retry_spam() {
    let mut observation = PlaybackObservation::default();
    assert!(observation.output_unavailable(OutputUnavailableReason::OpenFailed));
    observation.recovery_attempts = 4;
    let started = observation.recovery_started_at_ms;
    assert!(!observation.output_unavailable(OutputUnavailableReason::OpenFailed));
    assert_eq!(observation.recovery_attempts, 4);
    assert_eq!(observation.recovery_started_at_ms, started);
    observation.output_available = true;
    assert!(observation.output_unavailable(OutputUnavailableReason::DeviceChanged));
    assert_eq!(observation.recovery_attempts, 0);
    assert!(observation.output_unavailable(OutputUnavailableReason::ClockStalled));
    assert_eq!(
        observation.output_unavailable_reason,
        Some(OutputUnavailableReason::ClockStalled)
    );
}
