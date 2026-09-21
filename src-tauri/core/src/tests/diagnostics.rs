use super::*;
use crate::test_support::TestDir;

fn fixture(root: &TestDir) -> Connection {
    let (conn, _) = crate::db::init_db(&root.join("library.db")).unwrap();
    conn.execute("INSERT INTO tracks(id,file_path,title,embedded_lyrics) VALUES(1,'private-path','private-title','private-lyrics')", []).unwrap();
    conn
}

#[test]
fn capture_marks_the_incident_and_exports_only_recent_diagnostic_fields() {
    let root = TestDir::new();
    let conn = fixture(&root);
    let incident = 10_000_000;
    for (id, time) in [
        ("old", incident - EVENT_WINDOW_MS - 1),
        ("recent", incident - 1),
        ("future", incident + 1),
    ] {
        conn.execute("INSERT INTO playback_events(id,occurred_at_ms,event_type,source,track_id) VALUES(?1,?2,'seeked','ui',1)", rusqlite::params![id, time]).unwrap();
    }
    std::fs::write(
        root.join("sparkle-dev.log"),
        "event=failed error=https://user:secret@example.test/token\n",
    )
    .unwrap();
    std::fs::write(root.join("sparkle.log"), "release-profile-data").unwrap();
    let capture = capture(
        &root.join("library.db"),
        root.path(),
        "sparkle-dev",
        CaptureInfo {
            id: "incident".into(),
            incident_at_ms: incident,
        },
        "test",
        json!({ "is_playing": false }),
    );
    assert_eq!(capture.capture.incident_at_ms, incident);
    assert_eq!(capture.recent_events.len(), 1);
    assert_eq!(capture.recent_events[0]["id"], "recent");
    assert!(!capture.events_truncated);
    assert_eq!(capture.logs.len(), 1);
    assert!(capture.logs[0].text.contains("[url]"));
    let serialized = serde_json::to_string(&capture).unwrap();
    for private in [
        "private-path",
        "private-title",
        "private-lyrics",
        "secret",
        "release-profile-data",
    ] {
        assert!(
            !serialized.contains(private),
            "unexpected content: {private}"
        );
    }
}

#[test]
fn event_capture_keeps_the_latest_bounded_window_in_chronological_order() {
    let root = TestDir::new();
    let conn = fixture(&root);
    let tx = conn.unchecked_transaction().unwrap();
    for index in 1..=EVENT_LIMIT + 5 {
        tx.execute("INSERT INTO playback_events(id,occurred_at_ms,event_type,source) VALUES(?1,?2,'seeked','ui')",
            rusqlite::params![format!("event-{index}"), index as i64]).unwrap();
    }
    tx.commit().unwrap();
    let (events, truncated) = recent_events(&conn, 100_000).unwrap();
    assert!(truncated);
    assert_eq!(events.len(), EVENT_LIMIT);
    assert_eq!(events[0]["occurred_at_ms"], 6);
    assert_eq!(events.last().unwrap()["occurred_at_ms"], EVENT_LIMIT + 5);
}

#[test]
fn log_capture_bounds_files_and_bytes_without_mixing_profiles() {
    let root = TestDir::new();
    for index in 0..5 {
        std::fs::write(root.join(&format!("sparkle_{index}.log")), "archive").unwrap();
    }
    let large = format!("{}\nlast complete line\n", "x".repeat(LOG_BYTES as usize));
    std::fs::write(root.join("sparkle.log"), large).unwrap();
    std::fs::write(root.join("sparkle-dev.log"), "private dev profile").unwrap();
    let mut warnings = Vec::new();
    let logs = collect_logs(root.path(), "sparkle", &mut warnings);
    assert_eq!(logs.len(), LOG_FILES);
    assert_eq!(logs[0].name, "sparkle.log");
    assert!(logs[0].truncated);
    assert_eq!(logs[0].text, "last complete line");
    assert!(warnings.is_empty());
    assert!(logs.iter().all(|log| log.name != "sparkle-dev.log"));
}

#[test]
fn unavailable_database_and_logs_preserve_the_runtime_snapshot() {
    let root = TestDir::new();
    let bundle = capture(
        &root.join("missing.db"),
        &root.join("missing-logs"),
        "sparkle",
        CaptureInfo {
            id: "incident".into(),
            incident_at_ms: 123,
        },
        "test",
        json!({ "writer": { "running": false } }),
    );
    assert_eq!(bundle.warnings.len(), 2);
    assert_eq!(bundle.runtime["writer"]["running"], false);
    assert!(bundle.logs.is_empty());
    assert!(bundle.recent_events.is_empty());
    assert!(!root.join("missing.db").exists());
}
