use super::*;

#[test]
fn measurement_persistence_resets_failures_and_scheduler_respects_retry_budget() {
    let conn = crate::db::test_connection();
    conn.execute_batch("INSERT INTO tracks (id,file_path,file_mtime,file_size_bytes) VALUES (1,'one.flac',100,2048),(2,'two.flac',100,2048)").unwrap();
    assert_eq!(next_candidate(&conn, &[2, 1]).unwrap().unwrap().track_id, 2);
    let mut candidate = candidate_for_track(&conn, 1).unwrap().unwrap();
    persist_failure(&conn, &candidate, "decode").unwrap();
    assert!(candidate_for_track(&conn, 1).unwrap().is_none());
    assert_eq!(
        gain_for_track(&conn, 1).unwrap(),
        GainAvailability::Ready(0.0)
    );
    conn.execute("UPDATE track_loudness SET retry_after=0", [])
        .unwrap();
    candidate = candidate_for_track(&conn, 1).unwrap().unwrap();
    assert_eq!(candidate.previous_attempts, 1);
    candidate.previous_attempts = MAX_ATTEMPTS - 1;
    persist_failure(&conn, &candidate, "decode").unwrap();
    conn.execute("UPDATE track_loudness SET retry_after=0", [])
        .unwrap();
    assert!(candidate_for_track(&conn, 1).unwrap().is_none());
    for result in [
        AnalysisResult::Complete {
            integrated_lufs: -12.0,
            true_peak_dbtp: -2.0,
            gain_db: -6.0,
        },
        AnalysisResult::PeakOnly {
            true_peak_dbtp: 1.0,
            gain_db: -2.0,
        },
        AnalysisResult::Silent,
    ] {
        persist_success(&conn, &candidate, result).unwrap();
        assert!(candidate_for_track(&conn, 1).unwrap().is_none());
        let row = conn
            .query_row(
                "SELECT attempt_count,retry_after,error_code FROM track_loudness WHERE track_id=1",
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row, (0, None, None));
    }
    assert_eq!(
        gain_for_track(&conn, 1).unwrap(),
        GainAvailability::Ready(0.0)
    );
    conn.execute("UPDATE tracks SET file_size_bytes=2049 WHERE id=1", [])
        .unwrap();
    assert_eq!(
        candidate_for_track(&conn, 1)
            .unwrap()
            .unwrap()
            .previous_attempts,
        0
    );
    assert_eq!(next_candidate(&conn, &[]).unwrap().unwrap().track_id, 1);
}

#[test]
fn analysis_revision_validation_handles_legacy_metadata_and_rejects_changed_files() {
    let mut candidate = Candidate {
        track_id: 1,
        file_path: "unused".into(),
        file_mtime: 100,
        file_size_bytes: Some(2048),
        previous_attempts: 0,
    };
    assert!(verify_revision(&candidate, (100, 2048)).is_ok());
    assert!(verify_revision(&candidate, (101, 2048)).is_err());
    assert!(verify_revision(&candidate, (100, 2049)).is_err());
    candidate.file_mtime = 0;
    candidate.file_size_bytes = None;
    assert!(verify_revision(&candidate, (101, 2049)).is_ok());
}

fn measurement_connection() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE tracks (
            id INTEGER PRIMARY KEY,
            file_path TEXT NOT NULL,
            file_mtime INTEGER NOT NULL,
            file_size_bytes INTEGER
         );
         CREATE TABLE track_loudness (
            track_id INTEGER PRIMARY KEY,
            status TEXT NOT NULL,
            gain_db REAL,
            analyzer_version INTEGER NOT NULL,
            analyzed_file_mtime INTEGER NOT NULL,
            analyzed_file_size_bytes INTEGER,
            attempt_count INTEGER NOT NULL,
            retry_after INTEGER
         );",
    )
    .unwrap();
    conn
}

#[test]
fn loud_tracks_are_attenuated_to_target() {
    assert!((effective_gain_db(-10.0, -2.0) - -8.0).abs() < 1e-9);
}

#[test]
fn true_peak_ceiling_wins_when_it_is_stricter() {
    assert!((effective_gain_db(-16.0, 1.5) - -2.5).abs() < 1e-9);
}

#[test]
fn attenuation_only_never_amplifies_quiet_tracks() {
    assert_eq!(effective_gain_db(-30.0, -12.0), 0.0);
}

#[test]
fn playback_uses_only_measurements_for_the_exact_file_revision() {
    let conn = measurement_connection();
    conn.execute("INSERT INTO tracks VALUES (1, 'song.flac', 100, 2048)", [])
        .unwrap();
    conn.execute(
        "INSERT INTO track_loudness VALUES
         (1, 'complete', -6.0, ?1, 100, 2048, 0, NULL)",
        [ANALYZER_VERSION],
    )
    .unwrap();
    assert_eq!(
        gain_for_track(&conn, 1).unwrap(),
        GainAvailability::Ready(-6.0)
    );

    conn.execute("UPDATE tracks SET file_mtime = 101 WHERE id = 1", [])
        .unwrap();
    assert_eq!(gain_for_track(&conn, 1).unwrap(), GainAvailability::Pending);
}

#[test]
fn a_new_file_revision_gets_a_fresh_retry_budget() {
    let conn = measurement_connection();
    conn.execute("INSERT INTO tracks VALUES (1, 'song.flac', 101, 2048)", [])
        .unwrap();
    conn.execute(
        "INSERT INTO track_loudness VALUES
         (1, 'failed', NULL, ?1, 100, 2048, 3, 0)",
        [ANALYZER_VERSION],
    )
    .unwrap();

    let candidate = candidate_for_track(&conn, 1).unwrap().unwrap();
    assert_eq!(candidate.previous_attempts, 0);
}
