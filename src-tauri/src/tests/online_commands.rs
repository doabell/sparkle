use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn manual_artwork_search_uses_all_enabled_online_providers() {
    let settings = Settings {
        artist_image_sources: vec![
            "custom".to_string(),
            "wikipedia:ja".to_string(),
            "shazam".to_string(),
            "brave".to_string(),
            "duckduckgo".to_string(),
        ],
        ..Settings::default()
    };

    assert_eq!(
        manual_image_search_sources(&settings),
        vec![
            "wikipedia:ja".to_string(),
            "shazam".to_string(),
            "brave".to_string(),
            "duckduckgo".to_string(),
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
                source: "shazam".to_string(),
                url: "https://images.example/shared.jpg".to_string(),
            },
            ImageCandidate {
                source: "wikipedia:ja".to_string(),
                url: "https://images.example/shared.jpg".to_string(),
            },
            ImageCandidate {
                source: "duckduckgo".to_string(),
                url: "https://images.example/other.jpg".to_string(),
            },
        ],
        24,
    );

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].source, "shazam");
    assert_eq!(candidates[1].source, "duckduckgo");
}

#[test]
fn manual_lyrics_choice_is_persisted_as_custom_content() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE tracks (id INTEGER PRIMARY KEY, lyrics_source TEXT); \
         CREATE TABLE lyrics (track_id INTEGER NOT NULL, source TEXT NOT NULL, synced_text TEXT, plain_text TEXT, fetched_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, PRIMARY KEY (track_id, source));",
    )
    .unwrap();
    conn.execute("INSERT INTO tracks (id) VALUES (1)", [])
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
