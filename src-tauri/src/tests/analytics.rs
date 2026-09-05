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
