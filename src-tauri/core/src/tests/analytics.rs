use super::*;

#[test]
fn context_validation_checks_boundaries_and_never_persists_search_text() {
    for (kind, id, expected) in [
        (" Artist ", " 42 ", Some("42")),
        ("playlist", "", None),
        ("album", "123456789012345678901", None),
        ("album", "１２", None),
        ("health", "missing_titles", Some("missing_titles")),
        ("health", "My Files", None),
        ("search", "private words", None),
        ("unknown", "123", None),
    ] {
        let context = PlaybackContext {
            kind: kind.into(),
            id: Some(id.into()),
        }
        .sanitized();
        assert_eq!(context.id.as_deref(), expected);
    }
    assert_eq!(PlaybackContext::default().sanitized().id, None);
    assert!(now_epoch_ms() > 0);
    assert!(!is_meaningful_listen(-1, 180000));
    assert!(is_meaningful_listen(i64::MAX, i64::MAX));
    assert!(!is_completed(-1, 100));
}

#[test]
fn playback_source_wire_values_stay_stable_and_unknown_values_are_safe() {
    for (source, label) in [
        (PlaybackSource::Ui, "ui"),
        (PlaybackSource::Keyboard, "keyboard"),
        (PlaybackSource::SystemMedia, "system_media"),
        (PlaybackSource::Automatic, "automatic"),
        (PlaybackSource::Restore, "restore"),
        (PlaybackSource::Internal, "internal"),
        (PlaybackSource::Legacy, "legacy"),
        (PlaybackSource::Unknown, "unknown"),
    ] {
        assert_eq!(source.as_str(), label);
        assert_eq!(
            serde_json::to_string(&source).unwrap(),
            format!("\"{label}\"")
        );
    }
    assert_eq!(
        serde_json::from_str::<PlaybackSource>("\"new_source\"").unwrap(),
        PlaybackSource::Unknown
    );
}

#[test]
fn meaningful_listens_ignore_previews_but_keep_short_tracks() {
    assert!(!is_meaningful_listen(4_999, 8_000));
    assert!(is_meaningful_listen(5_000, 8_000));
    assert!(!is_meaningful_listen(29_999, 180_000));
    assert!(is_meaningful_listen(30_000, 180_000));
}

#[test]
fn completion_uses_the_final_ten_percent() {
    assert!(!is_completed(89_999, 100_000));
    assert!(is_completed(90_000, 100_000));
    assert!(!is_completed(90_000, 0));
}

#[test]
fn context_rejects_free_form_kinds_and_content_ids() {
    let context = PlaybackContext {
        kind: " Filesystem ".to_string(),
        id: Some("x".repeat(200)),
    }
    .sanitized();
    assert_eq!(context.kind, "unknown");
    assert_eq!(context.id, None);

    assert_eq!(
        PlaybackContext {
            kind: "album".to_string(),
            id: Some("42".to_string()),
        }
        .sanitized()
        .id
        .as_deref(),
        Some("42")
    );
    assert_eq!(
        PlaybackContext {
            kind: "search".to_string(),
            id: Some("private query".to_string()),
        }
        .sanitized()
        .id,
        None
    );
}

#[test]
fn generated_trace_ids_are_distinct() {
    assert_ne!(new_trace_id("listen"), new_trace_id("listen"));
}

#[test]
fn semantic_payloads_have_canonical_names_and_only_applicable_reasons() {
    let cases = [
        (
            PlaybackEvent::QueueLoaded(QueueLoadReason::QueueReplaced),
            "queue_loaded",
            Some("queue_replaced"),
        ),
        (
            PlaybackEvent::QueueLoaded(QueueLoadReason::SingleTrack),
            "queue_loaded",
            Some("single_track"),
        ),
        (
            PlaybackEvent::ListenStarted(ListenStartReason::ResumeAfterInactivity),
            "listen_started",
            Some("resume_after_inactivity"),
        ),
        (PlaybackEvent::PlaybackResumed, "playback_resumed", None),
        (PlaybackEvent::PlaybackPaused, "playback_paused", None),
        (
            PlaybackEvent::Seeked {
                reason: SeekReason::Absolute,
                target_position_ms: 80_000,
            },
            "seeked",
            Some("absolute"),
        ),
        (
            PlaybackEvent::Seeked {
                reason: SeekReason::PreviousRestart,
                target_position_ms: 0,
            },
            "seeked",
            Some("previous_restart"),
        ),
        (
            PlaybackEvent::ListenEnded(ListenEndReason::Interrupted),
            "listen_ended",
            Some("interrupted"),
        ),
        (PlaybackEvent::PlaybackStopped, "playback_stopped", None),
        (PlaybackEvent::ShuffleChanged, "shuffle_changed", None),
        (PlaybackEvent::RepeatChanged, "repeat_changed", None),
        (
            PlaybackEvent::QueuedNext {
                target_track_id: 42,
            },
            "queued_next",
            None,
        ),
        (
            PlaybackEvent::OutputUnavailable(OutputUnavailableReason::ClockStalled),
            "output_unavailable",
            Some("clock_stalled"),
        ),
        (
            PlaybackEvent::OutputUnavailable(OutputUnavailableReason::DeviceChanged),
            "output_unavailable",
            Some("device_changed"),
        ),
        (
            PlaybackEvent::OutputUnavailable(OutputUnavailableReason::OpenFailed),
            "output_unavailable",
            Some("open_failed"),
        ),
        (PlaybackEvent::OutputRestored, "output_restored", None),
        (
            PlaybackEvent::CommandFailed {
                command: "seek".into(),
                stage: "seek_after_reload".into(),
                target_track_id: Some(42),
            },
            "command_failed",
            None,
        ),
    ];
    for (event, name, reason) in cases {
        assert_eq!(event.kind().as_str(), name);
        assert_eq!(PlaybackEventKind::parse(name), event.kind());
        assert_eq!(event.reason(), reason);
        assert_eq!(event.kind().canonical_reason(reason), reason);
    }
    let seek = PlaybackEvent::Seeked {
        reason: SeekReason::Absolute,
        target_position_ms: 80_000,
    };
    assert_eq!(seek.target_position_ms(), Some(80_000));
    assert_eq!(seek.target_track_id(), None);
    assert_eq!(seek.command(), None);
    assert_eq!(seek.failure_stage(), None);
}

#[test]
fn imports_and_recovery_share_the_live_vocabulary() {
    assert_eq!(
        PlaybackEventKind::parse("track_started"),
        PlaybackEventKind::ListenStarted
    );
    for reason in [
        "interrupted",
        "legacy_import",
        "legacy_migration",
        "completed",
        "manual_next",
        "playback_error",
    ] {
        assert_eq!(ListenEndReason::parse(reason).as_str(), reason);
        assert_eq!(
            PlaybackEventKind::ListenEnded.canonical_reason(Some(reason)),
            Some(reason)
        );
    }
    for reason in [
        "legacy_import",
        "legacy_migration",
        "queue_started",
        "output_restored",
        "restored_resume",
        "play_next",
    ] {
        assert_eq!(ListenStartReason::parse(reason).as_str(), reason);
    }
    assert_eq!(
        PlaybackEventKind::parse("future_event"),
        PlaybackEventKind::Unknown
    );
    assert_eq!(PlaybackEventKind::Unknown.as_str(), "unknown");
    for kind in [
        PlaybackEventKind::ListenStarted,
        PlaybackEventKind::ListenEnded,
        PlaybackEventKind::QueueLoaded,
        PlaybackEventKind::Seeked,
        PlaybackEventKind::OutputUnavailable,
    ] {
        assert_eq!(
            kind.canonical_reason(Some("private arbitrary text")),
            Some("unknown")
        );
    }
    assert_eq!(
        PlaybackEventKind::OutputUnavailable.canonical_reason(Some("device_unavailable")),
        Some("unknown")
    );
    assert_eq!(
        PlaybackEventKind::PlaybackPaused.canonical_reason(Some("user_pause")),
        None
    );
}
