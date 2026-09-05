use super::*;

#[test]
fn settings_and_custom_metadata_roundtrip_preserves_local_secrets_and_folders() {
    let root = std::env::temp_dir().join(crate::analytics::new_trace_id("sparkle-backup-metadata"));
    std::fs::create_dir(&root).unwrap();
    let source = crate::db::test_connection();
    insert_test_track(&source, 1, "C:/Music/one.flac");
    source
        .execute("INSERT INTO album_artists VALUES (1,1)", [])
        .unwrap();
    source
        .execute("UPDATE artists SET bio='Custom biography' WHERE id=1", [])
        .unwrap();
    source
        .execute("UPDATE tracks SET lyrics_source='custom' WHERE id=1", [])
        .unwrap();
    cache::set_lyrics(&source, 1, "custom", Some("[00:01.00]Mine"), Some("Mine")).unwrap();
    let settings = settings::Settings {
        theme_mode: crate::models::ThemeMode::Light,
        ui_font: "Georgia".into(),
        brave_api_key: "source-secret".into(),
        monitored_folders: vec!["C:/Source".into()],
        ..Default::default()
    };
    settings::save_settings(&source, &settings).unwrap();
    let mut pixels = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut pixels)
        .encode_image(&image::RgbImage::from_pixel(2, 2, image::Rgb([20, 40, 60])))
        .unwrap();
    for kind in ["artist", "album"] {
        cache::set_image(
            &source,
            &root.join("source"),
            kind,
            1,
            "custom",
            None,
            Some(&pixels),
        )
        .unwrap();
    }
    let path = root.join("metadata.sparklebackup");
    let sections = BackupSections {
        settings: true,
        playlists: false,
        custom_metadata: true,
        history: false,
    };
    let manifest = export(&source, &root.join("source"), &path, sections).unwrap();
    assert!(manifest.settings);
    assert_eq!(manifest.lyrics, 1);
    assert_eq!(manifest.artist_bios, 1);
    assert_eq!(manifest.artwork, 2);
    let (data, _) = read(&path).unwrap();
    assert!(data.settings.unwrap().brave_api_key.is_empty());
    let target = crate::db::test_connection();
    insert_test_track(&target, 99, "D:/Moved/one.flac");
    target
        .execute("INSERT INTO album_artists VALUES (99,99)", [])
        .unwrap();
    let local = settings::Settings {
        brave_api_key: "local-secret".into(),
        discord_artwork_s3_secret_key: "local-s3-secret".into(),
        monitored_folders: vec!["D:/Local".into()],
        ..Default::default()
    };
    settings::save_settings(&target, &local).unwrap();
    let summary = import(&target, &root.join("target"), &path, sections).unwrap();
    assert_eq!(summary.lyrics, 1);
    assert_eq!(summary.artist_bios, 1);
    assert_eq!(summary.artwork, 2);
    assert_eq!(summary.unmatched_tracks, 0);
    let restored = settings::load_settings(&target).unwrap();
    assert_eq!(restored.theme_mode, crate::models::ThemeMode::Light);
    assert_eq!(restored.ui_font, "Georgia");
    assert_eq!(restored.brave_api_key, "local-secret");
    assert_eq!(restored.discord_artwork_s3_secret_key, "local-s3-secret");
    assert_eq!(restored.monitored_folders, vec!["D:/Local"]);
    assert_eq!(
        cache::get_lyrics_from_source(&target, 99, "custom")
            .unwrap()
            .unwrap()
            .plain_text
            .as_deref(),
        Some("Mine")
    );
    for kind in ["artist", "album"] {
        assert!(
            cache::get_custom_image(&target, &root.join("target"), kind, 99)
                .unwrap()
                .unwrap()
                .file_path
                .is_some()
        );
    }
    let empty = crate::db::test_connection();
    let unmatched = import(&empty, &root.join("unmatched"), &path, sections).unwrap();
    assert_eq!(unmatched.unmatched_tracks, 1);
    assert_eq!(unmatched.unmatched_artwork, 2);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_unsupported_and_unselected_backups_fail_without_mutation() {
    let root = std::env::temp_dir().join(crate::analytics::new_trace_id("sparkle-backup-invalid"));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("bad.sparklebackup");
    assert!(inspect(&path).is_err());
    std::fs::write(&path, b"not gzip").unwrap();
    assert!(inspect(&path).is_err());
    let mut data = sample_backup();
    data.version = BACKUP_VERSION + 1;
    std::fs::write(&path, encode(&data).unwrap()).unwrap();
    assert!(inspect(&path).unwrap_err().contains("unsupported"));
    let conn = crate::db::test_connection();
    let none = BackupSections {
        settings: false,
        playlists: false,
        custom_metadata: false,
        history: false,
    };
    assert!(export(&conn, &root, &path, none).is_err());
    assert!(import(&conn, &root, &path, none).is_err());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn track_resolution_rejects_ambiguous_metadata_and_out_of_tolerance_durations() {
    let mut track = sample_backup().tracks.remove(0);
    let local = |id, duration| LocalTrack {
        id,
        file_path: format!("D:/{id}.flac"),
        title: Some(" One ".into()),
        album_title: Some("album".into()),
        artist_names: vec!["ARTIST".into()],
        duration_ms: Some(duration),
    };
    assert_eq!(resolve_track(&track, &[local(1, 182000)]), Some(1));
    assert_eq!(resolve_track(&track, &[local(1, 182001)]), None);
    assert_eq!(
        resolve_track(&track, &[local(1, 180000), local(2, 180000)]),
        None
    );
    track.title = Some(" ".into());
    assert_eq!(resolve_track(&track, &[local(1, 180000)]), None);
}

fn sample_backup() -> BackupData {
    BackupData {
        format: BACKUP_FORMAT.to_string(),
        version: BACKUP_VERSION,
        created_at: 1_700_000_000,
        app_version: "0.1.0".to_string(),
        settings: None,
        tracks: vec![BackupTrackRef {
            file_path: "C:/Music/one.flac".to_string(),
            title: Some("One".to_string()),
            album_title: Some("Album".to_string()),
            artist_names: vec!["Artist".to_string()],
            duration_ms: Some(180_000),
        }],
        playlists: Vec::new(),
        lyrics: Vec::new(),
        artist_bios: Vec::new(),
        artwork: Vec::new(),
        listening_history: Vec::new(),
        playback_events: Vec::new(),
    }
}

#[test]
fn backup_payload_is_gzipped() {
    let bytes = encode(&sample_backup()).unwrap();
    assert_eq!(&bytes[..2], &[0x1f, 0x8b]);
    let mut decoder = GzDecoder::new(bytes.as_slice());
    let mut json = Vec::new();
    decoder.read_to_end(&mut json).unwrap();
    let restored: BackupData = serde_json::from_slice(&json).unwrap();
    assert_eq!(restored.version, BACKUP_VERSION);
    assert_eq!(restored.tracks[0].title.as_deref(), Some("One"));
}

#[test]
fn version_three_history_deserializes_without_trace_fields() {
    let json = serde_json::json!({
        "format": BACKUP_FORMAT,
        "version": 3,
        "created_at": 1_700_000_000,
        "app_version": "0.1.0",
        "settings": null,
        "tracks": [{
            "file_path": "C:/Music/one.flac",
            "title": "One",
            "album_title": "Album",
            "artist_names": ["Artist"],
            "duration_ms": 180000
        }],
        "playlists": [],
        "lyrics": [],
        "artist_bios": [],
        "artwork": [],
        "listening_history": [{
            "track_key": 0,
            "started_at": 1700000000,
            "played_ms": 60000,
            "completed": false
        }]
    });
    let restored: BackupData = serde_json::from_value(json).unwrap();
    assert_eq!(restored.version, 3);
    assert_eq!(restored.listening_history.len(), 1);
    assert!(restored.playback_events.is_empty());
    assert!(restored.listening_history[0].id.is_none());
}

#[test]
fn track_resolution_prefers_path_then_unique_metadata() {
    let local = vec![LocalTrack {
        id: 42,
        file_path: "D:/Moved/one.flac".to_string(),
        title: Some("One".to_string()),
        album_title: Some("Album".to_string()),
        artist_names: vec!["Artist".to_string()],
        duration_ms: Some(181_000),
    }];
    assert_eq!(resolve_track(&sample_backup().tracks[0], &local), Some(42));
}

fn create_playlist_test_schema(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE artists (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
         CREATE TABLE albums (id INTEGER PRIMARY KEY, title TEXT NOT NULL, year INTEGER);
         CREATE TABLE tracks (
            id INTEGER PRIMARY KEY,
            file_path TEXT NOT NULL UNIQUE,
            title TEXT,
            album_id INTEGER,
            duration_ms INTEGER
         );
         CREATE TABLE track_artists (
            track_id INTEGER NOT NULL,
            artist_id INTEGER NOT NULL,
            role TEXT NOT NULL
         );
         CREATE TABLE playlists (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            smart_query TEXT
         );
         CREATE TABLE playlist_tracks (
            playlist_id INTEGER NOT NULL,
            track_id INTEGER NOT NULL,
            position INTEGER NOT NULL,
            PRIMARY KEY (playlist_id, track_id)
         );",
    )
    .unwrap();
}

fn create_analytics_test_schema(conn: &Connection) {
    create_playlist_test_schema(conn);
    conn.execute_batch(
        "CREATE TABLE listens (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            started_at_ms INTEGER NOT NULL,
            ended_at_ms INTEGER,
            last_activity_at_ms INTEGER NOT NULL,
            start_position_ms INTEGER NOT NULL,
            end_position_ms INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            listened_ms INTEGER NOT NULL,
            meaningful INTEGER NOT NULL,
            completed INTEGER NOT NULL,
            finalized INTEGER NOT NULL,
            start_source TEXT NOT NULL,
            start_reason TEXT NOT NULL,
            end_reason TEXT,
            context_type TEXT NOT NULL,
            context_id TEXT,
            queue_index INTEGER,
            play_order_index INTEGER,
            queue_length INTEGER NOT NULL,
            shuffle INTEGER NOT NULL,
            repeat_mode TEXT NOT NULL
         );
         CREATE TABLE playback_events (
            id TEXT PRIMARY KEY,
            listen_id TEXT REFERENCES listens(id) ON DELETE CASCADE,
            session_id TEXT,
            occurred_at_ms INTEGER NOT NULL,
            event_type TEXT NOT NULL,
            source TEXT NOT NULL,
            reason TEXT,
            track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE,
            position_ms INTEGER,
            target_position_ms INTEGER,
            context_type TEXT NOT NULL,
            context_id TEXT,
            queue_index INTEGER,
            play_order_index INTEGER,
            queue_length INTEGER NOT NULL,
            shuffle INTEGER NOT NULL,
            repeat_mode TEXT NOT NULL
         );",
    )
    .unwrap();
}

fn insert_test_track(conn: &Connection, id: i64, file_path: &str) {
    conn.execute("INSERT INTO artists (id, name) VALUES (?, 'Artist')", [id])
        .unwrap();
    conn.execute(
        "INSERT INTO albums (id, title, year) VALUES (?, 'Album', 2024)",
        [id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tracks (id, file_path, title, album_id, duration_ms) \
         VALUES (?, ?, 'One', ?, 180000)",
        rusqlite::params![id, file_path, id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO track_artists (track_id, artist_id, role) VALUES (?, ?, 'main')",
        [id, id],
    )
    .unwrap();
}

#[test]
fn playlist_restore_matches_a_moved_reindexed_track() {
    let source = Connection::open_in_memory().unwrap();
    create_playlist_test_schema(&source);
    insert_test_track(&source, 1, "C:/Music/one.flac");
    source
        .execute(
            "INSERT INTO playlists (id, name, description) VALUES (1, 'Keepers', 'Test')",
            [],
        )
        .unwrap();
    source
        .execute(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (1, 1, 0)",
            [],
        )
        .unwrap();

    let path = std::env::temp_dir().join(format!(
        "sparkle-backup-test-{}-{}.sparklebackup",
        std::process::id(),
        unix_timestamp()
    ));
    let sections = BackupSections {
        settings: false,
        playlists: true,
        custom_metadata: false,
        history: false,
    };
    export(&source, Path::new("."), &path, sections).unwrap();
    assert_eq!(inspect(&path).unwrap().playlist_tracks, 1);

    let target = Connection::open_in_memory().unwrap();
    create_playlist_test_schema(&target);
    insert_test_track(&target, 99, "D:/Moved/one.flac");
    let summary = import(&target, Path::new("."), &path, sections).unwrap();
    let restored_track: i64 = target
        .query_row(
            "SELECT pt.track_id FROM playlist_tracks pt \
             JOIN playlists p ON p.id = pt.playlist_id WHERE p.name = 'Keepers'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(summary.playlists, 1);
    assert_eq!(summary.unmatched_tracks, 0);
    assert_eq!(restored_track, 99);
    let _ = std::fs::remove_file(path);
}

#[test]
fn analytics_backup_roundtrip_preserves_trace_and_is_idempotent() {
    let source = Connection::open_in_memory().unwrap();
    source.execute("PRAGMA foreign_keys = ON", []).unwrap();
    create_analytics_test_schema(&source);
    insert_test_track(&source, 1, "C:/Music/one.flac");
    source
        .execute(
            "INSERT INTO listens VALUES (
                'listen-1', 'session-1', 1, 1700000000123, 1700000060123,
                1700000060123, 0, 60000, 180000, 60000, 1, 0, 1,
                'keyboard', 'queue_started', 'manual_next', 'album', '1',
                0, 0, 3, 0, 'off'
             )",
            [],
        )
        .unwrap();
    source
        .execute(
            "INSERT INTO playback_events VALUES (
                'event-1', 'listen-1', 'session-1', 1700000060123,
                'listen_ended', 'keyboard', 'manual_next', 1, 60000, NULL,
                'album', '1', 0, 0, 3, 0, 'off'
             )",
            [],
        )
        .unwrap();

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "sparkle-analytics-backup-{}-{unique}.sparklebackup",
        std::process::id()
    ));
    let sections = BackupSections {
        settings: false,
        playlists: false,
        custom_metadata: false,
        history: true,
    };
    let manifest = export(&source, Path::new("."), &path, sections).unwrap();
    assert_eq!(manifest.history, 1);
    assert_eq!(manifest.file_version, 4);

    let target = Connection::open_in_memory().unwrap();
    target.execute("PRAGMA foreign_keys = ON", []).unwrap();
    create_analytics_test_schema(&target);
    insert_test_track(&target, 99, "D:/Moved/one.flac");

    let first = import(&target, Path::new("."), &path, sections).unwrap();
    let second = import(&target, Path::new("."), &path, sections).unwrap();
    assert_eq!(first.history, 1);
    assert_eq!(second.history, 0);
    assert_eq!(
        target
            .query_row("SELECT COUNT(*) FROM playback_events", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let restored: (i64, String, String, i64, i64, i64) = target
        .query_row(
            "SELECT track_id, start_source, context_type, listened_ms, \
                    queue_index, play_order_index FROM listens",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        restored,
        (99, "keyboard".into(), "album".into(), 60_000, 0, 0)
    );
    let _ = std::fs::remove_file(path);
}
