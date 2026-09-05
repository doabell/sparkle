use super::*;

#[test]
fn playback_metadata_keeps_all_main_artists_and_missing_tracks_are_errors() {
    let conn = crate::db::test_connection();
    conn.execute_batch("INSERT INTO artists (id,name) VALUES (1,'Bob'),(2,'Alice'),(3,'Composer');
        INSERT INTO albums (id,title) VALUES (1,'Album');
        INSERT INTO tracks (id,file_path,title,album_id,duration_ms,lrc_offset_ms,embedded_lyrics) VALUES (1,'unused.flac','Song',1,180000,-50,'[00:02.00]Second\n[00:01.00]First');
        INSERT INTO track_artists (track_id,artist_id,role) VALUES (1,1,'main'),(1,2,'main'),(1,3,'composer');").unwrap();
    let db = Arc::new(Mutex::new(conn));
    let track = load_track_from_db(&db, 1).unwrap();
    assert_eq!(track.artist_ids, vec![2, 1]);
    assert_eq!(track.artist_names, vec!["Alice", "Bob"]);
    assert_eq!(track.album_title.as_deref(), Some("Album"));
    assert_eq!(track.lrc_offset_ms, -50);
    assert!(load_track_from_db(&db, 99)
        .unwrap_err()
        .contains("track not found"));
    assert_eq!(
        known_first_lyric_line(&db, &track).unwrap().as_deref(),
        Some("First")
    );
    {
        let conn = lock_db(&db);
        cache::set_lyrics(&conn, 1, "custom", Some("[00:01.00]Mine"), None).unwrap();
        conn.execute("UPDATE tracks SET lyrics_source='custom' WHERE id=1", [])
            .unwrap();
    }
    assert_eq!(
        known_first_lyric_line(&db, &track).unwrap().as_deref(),
        Some("Mine")
    );
    lock_db(&db)
        .execute("UPDATE tracks SET lyrics_source='qq' WHERE id=1", [])
        .unwrap();
    assert!(known_first_lyric_line(&db, &track).unwrap().is_none());
}

#[test]
fn volume_curve_and_command_channel_success_are_stable() {
    assert_eq!(slider_to_gain(0.0), 0.0);
    assert_eq!(slider_to_gain(1.0), 1.0);
    assert!(slider_to_gain(0.5) < 0.5);
    assert!((db_to_gain(-6.0) - 0.5011872336).abs() < 1e-6);
    assert!(combined_gain(0.5, -6.0) < slider_to_gain(0.5));
    assert_eq!(
        immediate_start_gain(GainAvailability::Ready(-4.0)),
        (-4.0, false)
    );
    let (tx, rx) = mpsc::channel();
    tx.send(42).unwrap();
    assert_eq!(
        receive_audio_reply(rx, Duration::from_secs(1), "reply").unwrap(),
        42
    );
    assert!(join_audio_worker(None).is_ok());
    assert!(join_audio_worker(Some(std::thread::spawn(|| {}))).is_ok());
}

#[test]
fn identity_order_when_not_shuffled() {
    let (order, pos) = build_play_order(5, 2, false);
    assert_eq!(order, vec![0, 1, 2, 3, 4]);
    assert_eq!(pos, 2);
}

#[test]
fn shuffled_order_keeps_start_first_and_is_permutation() {
    let (order, pos) = build_play_order(10, 3, true);
    assert_eq!(pos, 0);
    assert_eq!(order[0], 3);
    assert!(is_valid_play_order(&order, 10));
}

#[test]
fn empty_queue_builds_empty_order() {
    let (order, pos) = build_play_order(0, 0, true);
    assert!(order.is_empty());
    assert_eq!(pos, 0);
}

#[test]
fn play_order_validation() {
    assert!(is_valid_play_order(&[2, 0, 1], 3));
    assert!(!is_valid_play_order(&[0, 0, 1], 3));
    assert!(!is_valid_play_order(&[0, 1], 3));
    assert!(!is_valid_play_order(&[0, 1, 5], 3));
    assert!(!is_valid_play_order(&[], 1));
}

#[test]
fn dedup_keeps_first_occurrence_order() {
    let (deduped, start) = dedup_queue(vec![10, 20, 10, 30, 20], 0);
    assert_eq!(deduped, vec![10, 20, 30]);
    assert_eq!(start, 0);
}

#[test]
fn dedup_remaps_start_index_to_same_track() {
    // start_index 2 is track id 10; after dedup it lives at index 0.
    let (deduped, start) = dedup_queue(vec![20, 20, 10, 30], 2);
    assert_eq!(deduped, vec![20, 10, 30]);
    assert_eq!(start, 1);
}

#[test]
fn dedup_empty_queue() {
    let (deduped, start) = dedup_queue(Vec::new(), 0);
    assert!(deduped.is_empty());
    assert_eq!(start, 0);
}

#[test]
fn meaningful_listens_ignore_previews_but_keep_short_tracks() {
    assert!(!is_meaningful_listen(4_999, 8_000));
    assert!(is_meaningful_listen(5_000, 8_000));
    assert!(!is_meaningful_listen(29_999, 180_000));
    assert!(is_meaningful_listen(30_000, 180_000));
}

#[test]
fn pending_loudness_starts_immediately_at_unity() {
    let (gain_db, analysis_pending) = immediate_start_gain(GainAvailability::Pending);

    assert_eq!(gain_db, 0.0);
    assert!(analysis_pending);
}

#[test]
fn command_reply_wait_is_bounded() {
    let (_reply_tx, reply_rx) = mpsc::channel::<PlaybackState>();

    let error = receive_audio_reply(reply_rx, Duration::from_millis(1), "test reply").unwrap_err();

    assert!(error.contains("timed out waiting for test reply"));
}

#[test]
fn command_reply_reports_a_stopped_worker() {
    let (reply_tx, reply_rx) = mpsc::channel::<PlaybackState>();
    drop(reply_tx);

    let error = receive_audio_reply(reply_rx, Duration::from_secs(1), "test reply").unwrap_err();

    assert!(error.contains("stopped before replying with test reply"));
}
