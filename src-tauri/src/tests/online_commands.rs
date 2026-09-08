use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn manual_artwork_search_uses_all_enabled_online_providers() {
    let settings = Settings {
        artist_image_sources: vec![
            "custom".to_string(),
            "wikipedia:ja".to_string(),
            "deezer".to_string(),
            "brave".to_string(),
            "unknown".to_string(),
        ],
        ..Settings::default()
    };

    assert_eq!(
        manual_image_search_sources(&settings),
        vec![
            "wikipedia:ja".to_string(),
            "deezer".to_string(),
            "brave".to_string()
        ]
    );
}

#[test]
fn manual_lyrics_search_uses_all_enabled_online_providers() {
    let settings = Settings {
        lyrics_sources: vec![
            "embedded".to_string(),
            "lrc".to_string(),
            "lrclib".to_string(),
            "netease".to_string(),
            "kashinavi".to_string(),
            "qq".to_string(),
        ],
        ..Settings::default()
    };

    assert_eq!(
        manual_lyrics_search_sources(&settings),
        vec![
            "embedded".to_string(),
            "lrc".to_string(),
            "lrclib".to_string(),
            "netease".to_string(),
            "kashinavi".to_string(),
            "qq".to_string(),
        ]
    );
}

#[test]
fn manual_lyrics_search_keeps_enabled_provider_failures_visible() {
    let outcome = collect_manual_image_search(
        vec![
            "embedded".to_string(),
            "lrclib".to_string(),
            "qq".to_string(),
        ],
        Duration::from_millis(200),
        |source| {
            if source == "lrclib" {
                Err("provider unavailable".to_string())
            } else {
                Ok(vec![source])
            }
        },
    );
    assert_eq!(
        outcome.candidates,
        vec!["embedded".to_string(), "qq".to_string()]
    );
    assert_eq!(outcome.failed_sources[0].0, "lrclib");
    assert!(outcome.timed_out_sources.is_empty());
}

#[test]
fn manual_artwork_query_skips_the_database_fallback() {
    let fallback_called = AtomicBool::new(false);
    let title = manual_image_search_title(Some("Björk".to_string()), || {
        fallback_called.store(true, Ordering::SeqCst);
        Ok(Some("fallback".to_string()))
    })
    .unwrap();

    assert_eq!(title.as_deref(), Some("Björk"));
    assert!(!fallback_called.load(Ordering::SeqCst));
}

#[test]
fn manual_image_search_returns_partial_results_within_its_budget() {
    let started = std::time::Instant::now();
    let outcome = collect_manual_image_search(
        vec!["fast".to_string(), "slow".to_string()],
        Duration::from_millis(100),
        |source| {
            if source == "slow" {
                std::thread::sleep(Duration::from_millis(500));
                Ok(Vec::new())
            } else {
                Ok(vec![source])
            }
        },
    );

    assert_eq!(outcome.candidates, vec!["fast"]);
    assert_eq!(outcome.timed_out_sources, vec!["slow"]);
    assert!(
        started.elapsed() < Duration::from_millis(300),
        "manual search waited for the slow provider"
    );
}

#[test]
fn manual_image_search_keeps_configured_provider_order() {
    let outcome = collect_manual_image_search(
        vec!["first".to_string(), "second".to_string()],
        Duration::from_millis(200),
        |source| {
            if source == "first" {
                // Complete second first to prove collection order is not
                // accidentally determined by network timing.
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(vec![source])
        },
    );

    assert_eq!(outcome.candidates, vec!["first", "second"]);
}

#[test]
fn manual_image_search_deduplicates_urls_preserving_first_provider() {
    let candidates = unique_image_candidates(
        vec![
            ImageCandidate {
                source: "brave".to_string(),
                url: "https://images.example/shared.jpg".to_string(),
            },
            ImageCandidate {
                source: "wikipedia:ja".to_string(),
                url: "https://images.example/shared.jpg".to_string(),
            },
            ImageCandidate {
                source: "wikipedia:en".to_string(),
                url: "https://images.example/other.jpg".to_string(),
            },
        ],
        24,
    );

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].source, "brave");
    assert_eq!(candidates[1].source, "wikipedia:en");
}

#[test]
fn manual_lyrics_choice_is_persisted_as_custom_content() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE tracks (id INTEGER PRIMARY KEY, lyrics_source TEXT, lrc_offset_ms INTEGER NOT NULL DEFAULT 0, lyrics_revision INTEGER NOT NULL DEFAULT 0); \
         CREATE TABLE lyrics (track_id INTEGER NOT NULL, source TEXT NOT NULL, synced_text TEXT, plain_text TEXT, fetched_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, PRIMARY KEY (track_id, source));",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tracks (id, lrc_offset_ms) VALUES (1, -200), (2, 350)",
        [],
    )
    .unwrap();

    store_manual_lyrics_choice(&conn, 1, "netease", Some("[00:00.00]Manual lyric"), None).unwrap();

    let lyrics = cache::get_lyrics_from_source(&conn, 1, "custom")
        .unwrap()
        .unwrap();
    let source: Option<String> = conn
        .query_row("SELECT lyrics_source FROM tracks WHERE id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(lyrics.source, "custom");
    assert_eq!(source.as_deref(), Some("custom"));
    assert_eq!(
        conn.query_row("SELECT lrc_offset_ms FROM tracks WHERE id=1", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT lrc_offset_ms FROM tracks WHERE id=2", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        350
    );
    assert_eq!(
        lyrics.synced_text.as_deref(),
        Some("[00:00.00]Manual lyric")
    );
    assert!(
        store_manual_lyrics_choice(&conn, 999, "custom", Some("[00:01]Missing"), None).is_err()
    );
    assert!(cache::get_lyrics_from_source(&conn, 999, "custom")
        .unwrap()
        .is_none());
}

#[test]
fn stale_automatic_result_cannot_restore_replaced_lyrics() {
    let automatic = None;
    let netease = Some("netease".to_string());

    assert!(!can_cache_lyrics_result(&automatic, &netease));
    assert!(!can_cache_lyrics_result(
        &automatic,
        &Some("custom".to_string()),
    ));
    assert!(can_cache_lyrics_result(&netease, &netease));
}

fn lyric_library(sources: &[&str]) -> rusqlite::Connection {
    let conn = crate::db::test_connection();
    conn.execute("INSERT INTO tracks(id,file_path,title,embedded_lyrics) VALUES(1,'missing.flac','Song','[00:02]Current embedded')",[]).unwrap();
    settings::save_settings(
        &conn,
        &Settings {
            lyrics_sources: sources.iter().map(|source| source.to_string()).collect(),
            ..Settings::default()
        },
    )
    .unwrap();
    cache::set_lyrics(&conn, 1, "lrclib", Some("[00:01]Cached remote"), None).unwrap();
    conn
}

#[test]
fn ordered_lookup_refreshes_local_lyrics_and_never_promotes_a_lower_cached_provider() {
    let root = crate::test_support::TestDir::new();
    let conn = lyric_library(&["lrc", "embedded", "lrclib"]);
    conn.execute(
        "UPDATE tracks SET file_path=? WHERE id=1",
        [root.join("song.flac").to_str().unwrap()],
    )
    .unwrap();
    cache::set_lyrics(&conn, 1, "embedded", Some("[00:01]Stale embedded"), None).unwrap();
    let db = Arc::new(Mutex::new(conn));
    let first = lookup_track_lyrics(&db, 1).unwrap();
    assert_eq!(first.source, "embedded");
    assert_eq!(first.plain_text.as_deref(), Some("Current embedded"));
    let sidecar = root.join("song.lrc");
    std::fs::write(&sidecar, "[00:03]New sidecar").unwrap();
    assert_eq!(lookup_track_lyrics(&db, 1).unwrap().source, "lrc");
    std::fs::write(&sidecar, "[00:04]Edited sidecar").unwrap();
    assert_eq!(
        lookup_track_lyrics(&db, 1).unwrap().plain_text.as_deref(),
        Some("Edited sidecar")
    );
    std::fs::remove_file(sidecar).unwrap();
    assert_eq!(lookup_track_lyrics(&db, 1).unwrap().source, "embedded");
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "UPDATE tracks SET embedded_lyrics=NULL, lyrics_revision=lyrics_revision+1",
            [],
        )
        .unwrap();
    }
    assert_eq!(lookup_track_lyrics(&db, 1).unwrap().source, "lrclib");
    {
        let conn = db.lock().unwrap();
        settings::save_settings(
            &conn,
            &Settings {
                lyrics_sources: vec!["none".into(), "lrclib".into()],
                ..Settings::default()
            },
        )
        .unwrap();
    }
    assert_eq!(lookup_track_lyrics(&db, 1).unwrap().source, "none");
}

#[test]
fn revision_checks_reject_edits_rescans_cache_clears_and_provider_reordering() {
    let conn = lyric_library(&["custom", "embedded", "lrclib"]);
    cache::set_custom_lyrics(&conn, 1, Some("[00:01]Old"), None).unwrap();
    let snapshot = lyrics_snapshot(&conn, 1).unwrap();
    assert!(snapshot_is_current(&conn, 1, &snapshot).unwrap());
    cache::set_custom_lyrics(&conn, 1, Some("[00:01]New"), None).unwrap();
    assert!(!snapshot_is_current(&conn, 1, &snapshot).unwrap());
    let snapshot = lyrics_snapshot(&conn, 1).unwrap();
    conn.execute("UPDATE tracks SET lyrics_revision=lyrics_revision+1", [])
        .unwrap();
    assert!(!snapshot_is_current(&conn, 1, &snapshot).unwrap());
    let snapshot = lyrics_snapshot(&conn, 1).unwrap();
    cache::clear_lyrics(&conn).unwrap();
    assert!(!snapshot_is_current(&conn, 1, &snapshot).unwrap());
    conn.execute("UPDATE tracks SET lyrics_source=NULL", [])
        .unwrap();
    let snapshot = lyrics_snapshot(&conn, 1).unwrap();
    settings::save_settings(
        &conn,
        &Settings {
            lyrics_sources: vec!["lrclib".into(), "custom".into()],
            ..Settings::default()
        },
    )
    .unwrap();
    assert!(!snapshot_is_current(&conn, 1, &snapshot).unwrap());
}

#[test]
fn only_explicit_adjustment_bakes_timing_and_save_and_export_preserve_written_timestamps() {
    let root = crate::test_support::TestDir::new();
    let conn = lyric_library(&["embedded"]);
    conn.execute("UPDATE tracks SET lrc_offset_ms=700 WHERE id=1", [])
        .unwrap();
    let text = "[offset:200]\n[00:01]Edited words";
    let exported = root.join("song.lrc");
    export_lyrics_file(&conn, 1, text, &exported).unwrap();
    assert_eq!(
        std::fs::read_to_string(exported).unwrap(),
        "[offset:200]\n[00:01]Edited words\n"
    );
    assert_eq!(
        conn.query_row("SELECT lrc_offset_ms FROM tracks WHERE id=1", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        700
    );
    assert!(cache::get_lyrics_from_source(&conn, 1, "custom")
        .unwrap()
        .is_none());
    let audio = root.audio("keep.flac");
    let original = std::fs::read(&audio).unwrap();
    assert!(export_lyrics_file(&conn, 1, text, &audio).is_err());
    assert_eq!(std::fs::read(audio).unwrap(), original);
    let unadjusted = save_edited_lyrics(&conn, 1, text, false).unwrap();
    assert_eq!(unadjusted.synced_text.as_deref(), Some(text));
    assert_eq!(
        conn.query_row("SELECT lrc_offset_ms FROM tracks WHERE id=1", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        700
    );
    let saved = save_edited_lyrics(&conn, 1, text, true).unwrap();
    assert_eq!(
        saved.synced_text.as_deref(),
        Some("[offset:0]\n[00:01.500]Edited words")
    );
    assert_eq!(
        conn.query_row("SELECT lrc_offset_ms FROM tracks WHERE id=1", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let again = save_edited_lyrics(&conn, 1, saved.synced_text.as_deref().unwrap(), true).unwrap();
    assert_eq!(again.synced_text, saved.synced_text);
    export_lyrics_file(
        &conn,
        1,
        again.synced_text.as_deref().unwrap(),
        &root.join("adjusted.lrc"),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("adjusted.lrc")).unwrap(),
        "[offset:0]\n[00:01.500]Edited words\n"
    );
    assert!(cache::get_lyrics_from_source(&conn, 1, "lrclib")
        .unwrap()
        .is_some());
    assert!(save_edited_lyrics(&conn, 1, "[ar:Metadata only]", false).is_err());
    assert!(save_edited_lyrics(&conn, 999, "[00:01]Missing", false).is_err());
    assert_eq!(
        cache::get_lyrics_from_source(&conn, 1, "custom")
            .unwrap()
            .unwrap()
            .synced_text,
        saved.synced_text
    );
}
