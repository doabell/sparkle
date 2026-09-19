use super::*;

#[test]
fn verbosity_is_cumulative_and_dependency_noise_stays_suppressed() {
    let levels = [
        LogLevel::Error,
        LogLevel::Warn,
        LogLevel::Info,
        LogLevel::Debug,
        LogLevel::Trace,
    ];
    for configured in levels {
        for event in levels {
            for target in [
                "sparkle",
                "sparkle::audio",
                "sparkle::frontend",
                "sparkle_lib::logging",
            ] {
                assert_eq!(
                    should_emit(event.level(), target, configured as u8),
                    event as u8 <= configured as u8
                );
            }
            for target in [
                "reqwest",
                "symphonia::decode",
                "sparkle_other",
                "sparkle_lib_extra",
            ] {
                assert_eq!(
                    should_emit(event.level(), target, configured as u8),
                    event as u8 <= configured as u8 && event.level() <= Level::Warn
                );
            }
        }
    }
}

#[test]
fn records_redact_urls_and_cannot_inject_lines_or_terminal_escapes() {
    let output = sanitize(
        "event=failed url=HTTPS://user:secret@example.test/path?token=secret\nnext\r\n\x1b[31m",
    );
    assert_eq!(output, "event=failed url=[url]\\nnext\\r\\n [31m");
    assert!(!output.contains("secret"));
    assert_eq!(
        sanitize("provider=deezer status=429"),
        "provider=deezer status=429"
    );
    let long = sanitize(&"音".repeat(5000));
    assert!(long.ends_with("…[truncated]"));
    assert_eq!(long.chars().count(), 4096 + "…[truncated]".chars().count());
}

#[test]
fn frontend_records_validate_labels_and_bound_payloads() {
    assert!(log_frontend(
        LogLevel::Error,
        "playback".into(),
        "load_failed".into(),
        Some("decoder unavailable".into())
    )
    .is_ok());
    assert!(log_frontend(LogLevel::Trace, "runtime".into(), "ready".into(), None).is_ok());
    for label in [
        "",
        "new\nrecord",
        "scope=other",
        "Uppercase",
        &"x".repeat(65),
    ] {
        assert!(log_frontend(LogLevel::Info, label.into(), "ready".into(), None).is_err());
        assert!(log_frontend(LogLevel::Info, "runtime".into(), label.into(), None).is_err());
    }
    assert!(log_frontend(
        LogLevel::Error,
        "runtime".into(),
        "failed".into(),
        Some("x".repeat(8193))
    )
    .is_err());
    assert!(serde_json::from_str::<LogLevel>("\"verbose\"").is_err());
}

#[test]
fn live_level_changes_update_call_site_filtering() {
    let previous = log::max_level();
    set_level(LogLevel::Debug);
    assert_eq!(log::max_level(), LevelFilter::Debug);
    assert!(!should_emit(
        Level::Trace,
        "sparkle::playback",
        LOG_LEVEL.load(Ordering::Relaxed)
    ));
    set_level(LogLevel::Trace);
    assert_eq!(log::max_level(), LevelFilter::Trace);
    set_level(LogLevel::Info);
    log::set_max_level(previous);
}
