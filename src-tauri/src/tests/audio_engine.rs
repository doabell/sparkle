use super::*;

#[test]
fn automatic_repeat_one_replays_but_manual_next_advances() {
    for repeat in [RepeatMode::Off, RepeatMode::All, RepeatMode::One] {
        assert_eq!(
            advance_target(Some(1), 3, Some(0), repeat, false),
            AdvanceTarget::NextPosition(2)
        );
        assert_eq!(
            advance_target(Some(1), 3, Some(0), repeat, true),
            if repeat == RepeatMode::One {
                AdvanceTarget::RepeatCurrent(0)
            } else {
                AdvanceTarget::NextPosition(2)
            }
        );
    }
}

#[test]
fn queue_end_distinguishes_replay_wrap_finish_and_manual_noop() {
    for len in [1, 4] {
        let last = Some(len - 1);
        for repeat in [RepeatMode::Off, RepeatMode::All, RepeatMode::One] {
            assert_eq!(
                advance_target(last, len, last, repeat, false),
                if repeat == RepeatMode::All {
                    AdvanceTarget::NextPosition(0)
                } else {
                    AdvanceTarget::Noop
                }
            );
            assert_eq!(
                advance_target(last, len, last, repeat, true),
                match repeat {
                    RepeatMode::Off => AdvanceTarget::Finish(last),
                    RepeatMode::All => AdvanceTarget::NextPosition(0),
                    RepeatMode::One => AdvanceTarget::RepeatCurrent(len - 1),
                }
            );
        }
    }
    for repeat in [RepeatMode::Off, RepeatMode::All, RepeatMode::One] {
        assert_eq!(
            advance_target(None, 0, None, repeat, false),
            AdvanceTarget::Noop
        );
        assert_eq!(
            advance_target(None, 0, None, repeat, true),
            AdvanceTarget::Finish(None)
        );
    }
}

#[test]
fn previous_restarts_after_three_seconds_or_at_the_first_song() {
    for position in [0, 2_999, 3_000] {
        assert_eq!(
            previous_target(position, Some(2)),
            PreviousTarget::Position(1)
        );
        assert_eq!(previous_target(position, Some(0)), PreviousTarget::Restart);
        assert_eq!(previous_target(position, None), PreviousTarget::Noop);
    }
    assert_eq!(previous_target(3_001, Some(2)), PreviousTarget::Restart);
    assert_eq!(previous_target(3_001, Some(0)), PreviousTarget::Restart);
}

#[test]
fn shuffle_changes_traversal_without_changing_the_current_song() {
    for current in 0..8 {
        let (order, mut pos) = build_play_order(8, current, true);
        assert_eq!(order[pos], current);
        let mut visited = vec![order[pos]];
        loop {
            match advance_target(
                Some(pos),
                order.len(),
                Some(order[pos]),
                RepeatMode::Off,
                true,
            ) {
                AdvanceTarget::NextPosition(next) => {
                    assert_eq!(
                        previous_target(0, Some(next)),
                        PreviousTarget::Position(pos)
                    );
                    pos = next;
                    visited.push(order[pos]);
                }
                AdvanceTarget::Finish(index) => {
                    assert_eq!(index, Some(order[pos]));
                    break;
                }
                other => panic!("unexpected traversal: {other:?}"),
            }
        }
        visited.sort_unstable();
        assert_eq!(visited, (0..8).collect::<Vec<_>>());
        // Disabling shuffle keeps the current song and resumes its library order.
        let (natural, natural_pos) = build_play_order(8, order[pos], false);
        assert_eq!(natural[natural_pos], order[pos]);
        assert_eq!(natural, (0..8).collect::<Vec<_>>());
    }
}

#[test]
fn play_next_moves_existing_entries_without_losing_current_song_or_other_ordering() {
    for order in [vec![0, 1, 2, 3], vec![3, 1, 0, 2], vec![2, 0, 3, 1]] {
        for current_index in 0..4 {
            for requested in [10, 20, 30, 40, 50] {
                let mut queue = vec![10, 20, 30, 40];
                let current_id = queue[current_index];
                let pos = order.iter().position(|&i| i == current_index).unwrap();
                let mut expected = order.iter().map(|&i| queue[i]).collect::<Vec<_>>();
                if requested != current_id {
                    expected.retain(|&id| id != requested);
                    let current = expected.iter().position(|&id| id == current_id).unwrap();
                    expected.insert(current + 1, requested);
                }
                let mut next_order = order.clone();
                let (index, next_pos) =
                    queue_track_next(&mut queue, &mut next_order, current_index, pos, requested);
                assert_eq!(queue[index], current_id);
                assert_eq!(next_order[next_pos], index);
                assert!(is_valid_play_order(&next_order, queue.len()));
                assert_eq!(
                    next_order.iter().map(|&i| queue[i]).collect::<Vec<_>>(),
                    expected
                );
                assert_eq!(queue.iter().filter(|&&id| id == requested).count(), 1);
                // Repeating Play Next has no further effect on queue or cursors.
                let snapshot = (queue.clone(), next_order.clone(), index, next_pos);
                let cursors =
                    queue_track_next(&mut queue, &mut next_order, index, next_pos, requested);
                assert_eq!((queue, next_order, cursors.0, cursors.1), snapshot);
            }
        }
    }
}

#[test]
fn a_failed_source_load_can_recover_without_opening_an_output_device() {
    let root = crate::test_support::TestDir::new();
    let path = root.audio("tone.flac");
    let corrupt = root.join("corrupt.flac");
    std::fs::write(&corrupt, b"not audio").unwrap();
    let conn = crate::db::test_connection();
    conn.execute(
        "INSERT INTO tracks (id,file_path) VALUES (1,?)",
        [path.to_string_lossy().as_ref()],
    )
    .unwrap();
    let mut track = load_track_from_db(&Arc::new(Mutex::new(conn)), 1).unwrap();
    // Player::new exposes a sample iterator, with no OS mixer or audio endpoint.
    let (player, mut samples) = Player::new();
    for missing_or_corrupt in [root.join("missing.flac"), corrupt] {
        track.file_path = missing_or_corrupt.to_string_lossy().into_owned();
        assert!(!load_source_into_player(&player, &track));
        assert!(player.empty());
    }
    track.file_path = path.to_string_lossy().into_owned();
    player.set_volume(0.4);
    assert!(load_source_into_player(&player, &track));
    assert_eq!(player.len(), 1);
    assert_eq!(player.volume(), 0.4);
    assert!(player.is_paused());
    assert!(samples.by_ref().take(1024).all(|sample| sample == 0.0));
    player.play();
    let decoded: Vec<_> = samples.by_ref().take(32_000).collect();
    assert!(decoded.iter().all(|sample| sample.is_finite()));
    assert!(decoded.iter().any(|sample| sample.abs() > 0.001));
    assert!(player.empty());
    // Loading again after completion is reusable and remains silent until Play.
    assert!(load_source_into_player(&player, &track));
    assert!(player.is_paused());
    assert_eq!(player.len(), 1);
}

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
