use super::*;
use crate::test_support::TestDir;

fn audio_candidate(path: &Path) -> Candidate {
    let (file_mtime, size) = file_revision(path).unwrap();
    Candidate {
        track_id: 1,
        file_path: path.to_string_lossy().into_owned(),
        file_mtime,
        file_size_bytes: Some(size),
        previous_attempts: 0,
    }
}

#[test]
fn real_decoder_analyzes_short_audio_without_amplification() {
    let root = TestDir::new();
    let path = root.audio("tone.flac");
    match analyze_candidate(&audio_candidate(&path), || false).unwrap() {
        AnalysisResult::PeakOnly {
            true_peak_dbtp,
            gain_db,
        } => {
            assert!(true_peak_dbtp.is_finite());
            assert!(gain_db <= 0.0);
        }
        other => panic!("a 250ms tone should use peak-only analysis: {other:?}"),
    }
}

// Two seconds of stereo 16-bit PCM, decoded through the same path as user files.
fn write_wave(path: &Path, amplitude: f64) {
    let sample_rate = 48_000_u32;
    let frames = sample_rate * 2;
    let data_size = frames * 4;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&2_u16.to_le_bytes()); // stereo
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 4).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes()); // block alignment
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    for frame in 0..frames {
        let phase = std::f64::consts::TAU * 1_000.0 * f64::from(frame) / f64::from(sample_rate);
        let sample = (phase.sin() * amplitude * f64::from(i16::MAX)) as i16;
        bytes.extend_from_slice(&sample.to_le_bytes());
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn real_decoder_distinguishes_silence_and_attenuates_a_loud_stereo_signal() {
    let root = TestDir::new();
    let path = root.join("signal.wav");
    write_wave(&path, 0.0);
    assert!(matches!(
        analyze_candidate(&audio_candidate(&path), || false).unwrap(),
        AnalysisResult::Silent
    ));
    write_wave(&path, 0.5);
    let AnalysisResult::Complete {
        integrated_lufs,
        true_peak_dbtp,
        gain_db,
    } = analyze_candidate(&audio_candidate(&path), || false).unwrap()
    else {
        panic!("a two-second tone should have integrated loudness");
    };
    assert!(integrated_lufs > TARGET_LUFS);
    assert!(
        (true_peak_dbtp - -6.02).abs() < 0.2,
        "half-scale sine peak: {true_peak_dbtp}"
    );
    assert!(gain_db < 0.0);
    assert!((integrated_lufs + gain_db - TARGET_LUFS).abs() < 0.01);
    assert!(true_peak_dbtp + gain_db <= TRUE_PEAK_CEILING_DBTP);
}

#[test]
fn a_file_changed_mid_analysis_is_never_accepted() {
    use std::io::Write;
    let root = TestDir::new();
    let path = root.audio("changing.flac");
    let candidate = audio_candidate(&path);
    let mut calls = 0;
    let result = analyze_candidate(&candidate, || {
        calls += 1;
        if calls == 2 {
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap()
                .write_all(&[0; 64])
                .unwrap();
        }
        false
    });
    assert!(matches!(
        result,
        Err(AnalysisFailure::Failed {
            code: "file_changed",
            ..
        })
    ));
}

#[test]
fn analysis_cancellation_and_corrupt_files_do_not_become_measurements() {
    let root = TestDir::new();
    let path = root.audio("tone.flac");
    let candidate = audio_candidate(&path);
    assert!(matches!(
        analyze_candidate(&candidate, || true),
        Err(AnalysisFailure::Cancelled)
    ));
    let mut calls = 0;
    assert!(matches!(
        analyze_candidate(&candidate, || {
            calls += 1;
            calls > 1
        }),
        Err(AnalysisFailure::Cancelled)
    ));
    std::fs::write(&path, b"corrupt audio").unwrap();
    assert!(matches!(
        analyze_candidate(&candidate, || false),
        Err(AnalysisFailure::Failed {
            code: "file_changed",
            ..
        })
    ));
    let changed = audio_candidate(&path);
    assert!(matches!(
        analyze_candidate(&changed, || false),
        Err(AnalysisFailure::Failed { code: "decode", .. })
    ));
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        analyze_candidate(&changed, || false),
        Err(AnalysisFailure::Failed {
            code: "file_metadata",
            ..
        })
    ));
}

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
