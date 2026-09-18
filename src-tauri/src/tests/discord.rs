use super::*;
use rusqlite::Connection;
use std::fs;

fn example_playback() -> PlaybackState {
    PlaybackState {
        is_playing: true,
        current_track: Some(Track {
            id: 1,
            file_path: "song.flac".into(),
            title: Some("Song".into()),
            artist_names: vec!["Artist".into()],
            album_title: Some("Album".into()),
            album_id: None,
            artist_ids: vec![],
            track_number: None,
            disc_number: None,
            duration_ms: Some(180_000),
            year: None,
            genre: None,
            embedded_lyrics: None,
            lrc_offset_ms: 0,
            lyrics_source: None,
        }),
        first_lyric_line: None,
        album_art: None,
        position_ms: 1_000,
        duration_ms: 180_000,
        volume: 1.0,
        shuffle: false,
        repeat_mode: crate::models::RepeatMode::Off,
    }
}

#[test]
fn activity_explicitly_names_sparkle_and_defaults_to_app_status() {
    let playback = example_playback();
    let fields = presence_fields(
        &playback,
        playback.current_track.as_ref().unwrap(),
        None,
        &DiscordLayout::default(),
        None,
    );
    let payload = serde_json::to_value(build_activity(fields)).unwrap();
    assert_eq!(payload["name"], "Sparkle");
    assert_eq!(payload["type"], 2);
    assert_eq!(payload["status_display_type"], 0);
    assert_eq!(payload["details"], "Song");
    assert_eq!(payload["state"], "Artist");
    assert_eq!(payload["assets"]["large_text"], "Album");
    assert_eq!(
        payload["timestamps"]["end"].as_i64().unwrap()
            - payload["timestamps"]["start"].as_i64().unwrap(),
        180_000
    );
}

#[test]
fn templates_support_lyrics_fallback_hidden_fields_and_unicode_limits() {
    let playback = example_playback();
    let track = playback.current_track.as_ref().unwrap();
    let mut layout = DiscordLayout {
        name: " ".into(),
        details: "{title} — {artist}".into(),
        state: "{lyrics}".into(),
        image_text: "{lyrics}".into(),
        status_display: "state".into(),
        show_artwork: false,
        show_progress: false,
    };
    let fields = presence_fields(
        &playback,
        track,
        Some("https://example.test/cover.jpg".into()),
        &layout,
        Some("Current line"),
    );
    let payload = serde_json::to_value(build_activity(fields)).unwrap();
    assert_eq!(payload["name"], "Sparkle");
    assert_eq!(payload["details"], "Song — Artist");
    assert_eq!(payload["state"], "Current line");
    assert_eq!(payload["status_display_type"], 1);
    assert!(payload.get("assets").is_none());
    assert!(payload.get("timestamps").is_none());
    assert_eq!(
        presence_fields(&playback, track, None, &layout, None).artist,
        "Album"
    );
    layout.state.clear();
    layout.details = "  ".into();
    let payload = serde_json::to_value(build_activity(presence_fields(
        &playback, track, None, &layout, None,
    )))
    .unwrap();
    assert!(payload.get("state").is_none());
    assert!(payload.get("details").is_none());
    assert_eq!(payload["status_display_type"], 0);
    assert_eq!(
        render_template("{title} {unknown}", "{lyrics}", "", "", Some("Line")),
        "{lyrics} {unknown}"
    );
    assert_eq!(
        render_template("unfinished {", "", "", "", None),
        "unfinished {"
    );
    assert_eq!(
        render_template("{lyrics}", "", "", "Album", Some("  ")),
        "Album"
    );
    assert_eq!(
        render_template("{lyrics}", "", "", "", Some(&"🎵".repeat(40))),
        "🎵".repeat(32)
    );
    assert_eq!(render_template("{title}", "A", "", "", None), "A\u{180e}");
}

#[test]
fn lyric_lookup_tracks_source_changes_offsets_and_missing_sync() {
    let conn = crate::db::test_connection();
    conn.execute("INSERT INTO tracks (id, file_path, lyrics_source, lrc_offset_ms) VALUES (1, 'song.flac', 'custom', 500)", []).unwrap();
    cache::set_lyrics(
        &conn,
        1,
        "custom",
        Some("[offset:100]\n[00:01]First\n[00:02]Second"),
        None,
    )
    .unwrap();
    let mut playback = example_playback();
    let layout = DiscordLayout {
        state: "{lyrics}".into(),
        ..Default::default()
    };
    playback.position_ms = 1399;
    assert_eq!(current_lyric(&conn, &playback, &layout), None);
    playback.position_ms = 1400;
    assert_eq!(
        current_lyric(&conn, &playback, &layout).as_deref(),
        Some("First")
    );
    playback.position_ms = 2400;
    assert_eq!(
        current_lyric(&conn, &playback, &layout).as_deref(),
        Some("Second")
    );
    // A backwards seek selects the current line, never a queued lyric.
    playback.position_ms = 1400;
    assert_eq!(
        current_lyric(&conn, &playback, &layout).as_deref(),
        Some("First")
    );
    conn.execute("UPDATE tracks SET lrc_offset_ms=0 WHERE id=1", [])
        .unwrap();
    playback.position_ms = 1900;
    assert_eq!(
        current_lyric(&conn, &playback, &layout).as_deref(),
        Some("Second")
    );
    conn.execute("UPDATE tracks SET lyrics_source='none' WHERE id=1", [])
        .unwrap();
    assert_eq!(current_lyric(&conn, &playback, &layout), None);
    conn.execute("UPDATE tracks SET lyrics_source='qq' WHERE id=1", [])
        .unwrap();
    assert_eq!(current_lyric(&conn, &playback, &layout), None);
    cache::set_lyrics(&conn, 1, "qq", None, Some("Plain lyrics")).unwrap();
    assert_eq!(current_lyric(&conn, &playback, &layout), None);
    cache::set_lyrics(&conn, 1, "qq", Some("[00:00]New source"), None).unwrap();
    assert_eq!(
        current_lyric(&conn, &playback, &layout).as_deref(),
        Some("New source")
    );
    assert_eq!(
        current_lyric(&conn, &playback, &DiscordLayout::default()),
        None
    );
    playback.current_track.as_mut().unwrap().id = 2;
    assert_eq!(current_lyric(&conn, &playback, &layout), None);
}

#[test]
fn playback_clock_advances_only_while_playing_and_clamps_at_track_end() {
    let now = Instant::now();
    let mut snapshot = PlaybackSnapshot {
        playback: Box::new(example_playback()),
        received_at: now,
    };
    assert_eq!(
        snapshot.current(now + Duration::from_secs(15)).position_ms,
        16_000
    );
    assert_eq!(
        snapshot.current(now + Duration::from_secs(200)).position_ms,
        180_000
    );
    snapshot.playback.is_playing = false;
    assert_eq!(
        snapshot.current(now + Duration::from_secs(15)).position_ms,
        1_000
    );
    // A new event anchors both forward and backward seeks to its position.
    snapshot.playback.is_playing = true;
    snapshot.playback.position_ms = 60_000;
    snapshot.received_at = now + Duration::from_secs(15);
    assert_eq!(
        snapshot.current(now + Duration::from_secs(16)).position_ms,
        61_000
    );
}

#[test]
fn preserves_md5_base64_cache_key() {
    let keys = unique_cache_keys([md5_hex(b""), md5_hex(b""), md5_hex(b"")]);
    assert_eq!(keys, vec!["d41d8cd98f00b204e9800998ecf8427e"]);
}

#[test]
fn artwork_store_modes_are_explicit() {
    assert_eq!(
        ArtworkStoreKind::from_setting("disabled"),
        ArtworkStoreKind::Disabled
    );
    assert_eq!(
        ArtworkStoreKind::from_setting("catbox"),
        ArtworkStoreKind::Catbox
    );
    assert_eq!(ArtworkStoreKind::from_setting("s3"), ArtworkStoreKind::S3);
    assert_eq!(
        ArtworkStoreKind::from_setting("unknown"),
        ArtworkStoreKind::Catbox
    );
}

#[test]
fn disabled_artwork_storage_does_not_upload() {
    let settings = crate::settings::Settings {
        discord_artwork_store: "disabled".to_string(),
        ..Default::default()
    };
    assert_eq!(
        test_artwork_storage(&settings),
        Err("artwork storage is disabled".to_string())
    );
}

#[test]
fn cache_urls_are_scoped_to_the_selected_store() {
    let catbox_url = "https://files.catbox.moe/existing.jpg".to_string();
    let s3_url = "https://cdn.example.test/artwork/existing.jpg".to_string();
    let cache = ArtworkCache {
        entries: HashMap::from([(
            "hash".to_string(),
            ArtworkUrls {
                catbox_url: Some(catbox_url.clone()),
                s3_url: Some(s3_url.clone()),
            },
        )]),
    };
    let catbox = ArtworkStoreState {
        kind: ArtworkStoreKind::Catbox,
        s3_store: None,
    };
    let s3_settings = crate::settings::Settings {
        discord_artwork_s3_endpoint: "http://minio.example.test:9000".to_string(),
        discord_artwork_s3_bucket: "sparkle".to_string(),
        discord_artwork_s3_public_url: "https://cdn.example.test".to_string(),
        discord_artwork_s3_prefix: "artwork".to_string(),
        ..Default::default()
    };
    let s3 = ArtworkStoreState {
        kind: ArtworkStoreKind::S3,
        s3_store: Some(
            S3ArtworkStore::from_settings(&s3_settings)
                .unwrap()
                .expect("S3 settings should build a store"),
        ),
    };
    let disabled = ArtworkStoreState {
        kind: ArtworkStoreKind::Disabled,
        s3_store: None,
    };
    assert_eq!(
        cache.lookup_for_store(&["hash".to_string()], &catbox),
        Some(catbox_url)
    );
    assert_eq!(
        cache.lookup_for_store(&["hash".to_string()], &s3),
        Some(s3_url)
    );
    assert_eq!(
        cache.lookup_for_store(&["hash".to_string()], &disabled),
        None
    );
}

#[test]
fn changed_artwork_does_not_reuse_a_stale_album_pointer() {
    let first = artwork_content_keys(b"normalized-one", b"original-one");
    let second = artwork_content_keys(b"normalized-two", b"original-two");
    let persistent_key = album_artwork_key(42);
    let url = "https://files.catbox.moe/existing.jpg".to_string();
    let mut entries = HashMap::new();
    for key in first {
        entries.insert(
            key,
            ArtworkUrls {
                catbox_url: Some(url.clone()),
                ..Default::default()
            },
        );
    }
    entries.insert(
        persistent_key.clone(),
        ArtworkUrls {
            catbox_url: Some(url.clone()),
            ..Default::default()
        },
    );
    let cache = ArtworkCache { entries };

    assert_eq!(cache.lookup(&second), None);
    assert_eq!(cache.lookup(&[persistent_key]), Some(url));
}

#[test]
fn catbox_artwork_can_be_reused_without_a_local_image() {
    let url = "https://files.catbox.moe/existing.jpg".to_string();
    let cache = ArtworkCache {
        entries: HashMap::from([(
            album_artwork_key(42),
            ArtworkUrls {
                catbox_url: Some(url.clone()),
                ..Default::default()
            },
        )]),
    };

    assert_eq!(persistent_artwork_url(&cache, Some(42)), Some(url));
    assert_eq!(persistent_artwork_url(&cache, None), None);
}

#[test]
fn cache_cleanup_keeps_persisted_catbox_urls() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "
        CREATE TABLE tracks (id INTEGER PRIMARY KEY, lyrics_revision INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE lyrics (track_id INTEGER PRIMARY KEY, source TEXT NOT NULL);
        CREATE TABLE artist_info (artist_id INTEGER PRIMARY KEY, source TEXT NOT NULL);
        CREATE TABLE images (
            entity_type TEXT NOT NULL,
            entity_id INTEGER NOT NULL,
            source TEXT NOT NULL,
            file_path TEXT,
            PRIMARY KEY (entity_type, entity_id, source)
        );
        CREATE TABLE discord_artwork_cache (
            cache_key TEXT NOT NULL PRIMARY KEY,
            catbox_url TEXT,
            s3_url TEXT,
            updated_at INTEGER NOT NULL DEFAULT 0,
            CHECK (catbox_url IS NOT NULL OR s3_url IS NOT NULL)
        );
        ",
    )
    .unwrap();
    let persistent_key = album_artwork_key(42);
    let url = "https://files.catbox.moe/existing.jpg";
    conn.execute(
        "INSERT INTO discord_artwork_cache (cache_key, catbox_url) VALUES (?1, ?2)",
        [&persistent_key, url],
    )
    .unwrap();
    let root = std::env::temp_dir().join(format!(
        "sparkle-catbox-persistence-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();

    cache::clear_lyrics(&conn).unwrap();
    cache::clear_artist_info(&conn, &root).unwrap();
    cache::clear_images(&conn, &root).unwrap();

    let store = ArtworkCache::load(&conn).unwrap();
    assert_eq!(store.lookup(&[persistent_key]), Some(url.to_string()));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn artwork_cache_store_preserves_catbox_and_s3_urls() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "
        CREATE TABLE discord_artwork_cache (
            cache_key TEXT NOT NULL PRIMARY KEY,
            catbox_url TEXT,
            s3_url TEXT,
            updated_at INTEGER NOT NULL DEFAULT 0,
            CHECK (catbox_url IS NOT NULL OR s3_url IS NOT NULL)
        );
        ",
    )
    .unwrap();
    let key = "artwork-hash".to_string();
    let catbox_url = "https://files.catbox.moe/catbox.jpg".to_string();
    let s3_url = "https://cdn.example.test/artwork/hash.jpg".to_string();
    let mut cache = ArtworkCache {
        entries: HashMap::new(),
    };

    cache
        .store(
            &conn,
            std::slice::from_ref(&key),
            ArtworkStoreKind::Catbox,
            catbox_url.clone(),
        )
        .unwrap();
    cache
        .store(
            &conn,
            std::slice::from_ref(&key),
            ArtworkStoreKind::S3,
            s3_url.clone(),
        )
        .unwrap();

    let loaded = ArtworkCache::load(&conn).unwrap();
    let urls = loaded.entries.get(&key).unwrap();
    assert_eq!(urls.catbox_url, Some(catbox_url));
    assert_eq!(urls.s3_url, Some(s3_url));
}

#[test]
fn content_hit_repairs_a_stale_album_pointer_without_an_upload() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "
        CREATE TABLE discord_artwork_cache (
            cache_key TEXT NOT NULL PRIMARY KEY,
            catbox_url TEXT,
            s3_url TEXT,
            updated_at INTEGER NOT NULL DEFAULT 0,
            CHECK (catbox_url IS NOT NULL OR s3_url IS NOT NULL)
        );
        ",
    )
    .unwrap();
    let content_key = artwork_content_keys(b"normalized", b"original")
        .into_iter()
        .next()
        .unwrap();
    let persistent_key = album_artwork_key(42);
    let current_url = "https://files.catbox.moe/current.jpg".to_string();
    let stale_url = "https://files.catbox.moe/stale.jpg".to_string();
    let mut cache = ArtworkCache {
        entries: HashMap::from([
            (
                content_key.clone(),
                ArtworkUrls {
                    catbox_url: Some(current_url.clone()),
                    ..Default::default()
                },
            ),
            (
                persistent_key.clone(),
                ArtworkUrls {
                    catbox_url: Some(stale_url),
                    ..Default::default()
                },
            ),
        ]),
    };
    let keys = vec![content_key, persistent_key];

    assert!(!cache.keys_match_url_for_store(&keys, ArtworkStoreKind::Catbox, &current_url));
    cache
        .store(&conn, &keys, ArtworkStoreKind::Catbox, current_url.clone())
        .unwrap();
    assert!(cache.keys_match_url_for_store(&keys, ArtworkStoreKind::Catbox, &current_url));
}

#[test]
fn artwork_invalidation_removes_only_the_album_pointer() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "
        CREATE TABLE discord_artwork_cache (
            cache_key TEXT NOT NULL PRIMARY KEY,
            catbox_url TEXT,
            s3_url TEXT,
            updated_at INTEGER NOT NULL DEFAULT 0,
            CHECK (catbox_url IS NOT NULL OR s3_url IS NOT NULL)
        );
        ",
    )
    .unwrap();
    let content_key = artwork_content_keys(b"normalized", b"original")
        .into_iter()
        .next()
        .unwrap();
    let album_key = album_artwork_key(42);
    let url = "https://files.catbox.moe/current.jpg";
    conn.execute(
        "INSERT INTO discord_artwork_cache (cache_key, catbox_url) VALUES (?1, ?2), (?3, ?2)",
        rusqlite::params![content_key, url, album_key],
    )
    .unwrap();

    invalidate_album_artwork(&conn, 42).unwrap();

    let cache = ArtworkCache::load(&conn).unwrap();
    assert_eq!(cache.lookup(&[content_key]), Some(url.to_string()));
    assert_eq!(persistent_artwork_url(&cache, Some(42)), None);
}

#[test]
fn truncates_without_splitting_utf8_characters() {
    assert_eq!(truncate_utf8("hello\u{1f30d}", 6), "hello");
}

#[cfg(windows)]
#[test]
fn uses_gdiplus_output_for_cache_keys() {
    const SOURCE: &str = "iVBORw0KGgoAAAANSUhEUgAAAAMAAAACCAYAAACddGYaAAAAAXNSR0IArs4c6QAAAARnQU1BAACxjwv8YQUAAAAJcEhZcwAADsMAAA7DAcdvqGQAAAAYSURBVBhXY/jPAEQgyPAfSIKZQMDw/z8Aqm8O8p3BH9oAAAAASUVORK5CYII=";

    let work_dir = std::env::temp_dir().join(format!(
        "sparkle-discord-gdiplus-test-{}",
        std::process::id()
    ));
    let source = STANDARD.decode(SOURCE).unwrap();
    let normalized = resize_to_cache_jpeg(&source, &work_dir);
    let _ = fs::remove_dir_all(&work_dir);
    let normalized = normalized.unwrap();
    let normalized_base64 = STANDARD.encode(&normalized);
    let cache_key = md5_hex(normalized_base64.as_bytes());
    let cache_keys = artwork_content_keys(&normalized, &source);

    assert_eq!(cache_keys.first(), Some(&cache_key));
    let cache = ArtworkCache {
        entries: HashMap::from([(
            cache_key,
            ArtworkUrls {
                catbox_url: Some("https://files.catbox.moe/existing.jpg".to_string()),
                ..Default::default()
            },
        )]),
    };
    assert_eq!(
        cache.lookup(&cache_keys),
        Some("https://files.catbox.moe/existing.jpg".to_string())
    );
}
