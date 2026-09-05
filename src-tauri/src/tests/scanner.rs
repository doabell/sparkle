use super::*;
use crate::test_support::TestDir;
use lofty::config::WriteOptions;
use lofty::tag::TagExt;

fn monitored_library(root: &Path) -> Connection {
    let conn = crate::db::test_connection();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    conn.execute(
        "INSERT INTO folders (path) VALUES (?)",
        [root.to_string_lossy().as_ref()],
    )
    .unwrap();
    conn
}

#[test]
fn real_scan_indexes_tags_rescans_unchanged_files_and_forces_changed_artist_rules() {
    let root = TestDir::new();
    let music = root.join("music");
    std::fs::create_dir(&music).unwrap();
    let path = music.join("tone.flac");
    std::fs::copy(root.audio("source.flac"), &path).unwrap();
    let mut conn = monitored_library(&music);
    let settings = Settings {
        artist_split_regex: ";".into(),
        ..Default::default()
    };
    let mut progress = Vec::new();
    let result =
        scan_library_with_progress(&mut conn, &settings, false, &root.join("cache"), |event| {
            progress.push(event)
        })
        .unwrap();
    assert_eq!(
        (result.scanned, result.added, result.updated, result.errors),
        (1, 1, 0, 0)
    );
    assert_eq!(progress.first().unwrap().phase, "scanning");
    assert_eq!(progress.first().unwrap().total, 1);
    assert_eq!(progress.last().unwrap().phase, "cleaning");
    assert_eq!(progress.last().unwrap().scanned, 1);
    let row = conn.query_row(
        "SELECT title,track_number,disc_number,year,genre,audio_format,sample_rate_hz,channels,duration_ms FROM tracks",
        [],
        |r| Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, i64>(8)?,
        )),
    ).unwrap();
    assert_eq!(
        row,
        (
            "Fixture song".into(),
            3,
            1,
            2024,
            "Test".into(),
            "flac".into(),
            44100,
            1,
            250
        )
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM track_artists", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        conn.query_row(
            "SELECT track_count FROM artists WHERE name='Alice'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT album_count FROM artists WHERE name='Ensemble'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    let second = scan_library(&mut conn, &settings, false, &root.join("cache")).unwrap();
    assert_eq!((second.scanned, second.added, second.updated), (1, 0, 0));
    conn.execute(
        "UPDATE tracks SET lrc_offset_ms=-50, lyrics_source='custom'",
        [],
    )
    .unwrap();
    let merged = Settings {
        artist_split_exceptions: vec!["Alice; Bob".into()],
        ..settings
    };
    let forced = scan_library(&mut conn, &merged, true, &root.join("cache")).unwrap();
    assert_eq!((forced.added, forced.updated), (0, 1));
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM track_artists", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT lrc_offset_ms,lyrics_source FROM tracks",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        )
        .unwrap(),
        (-50, "custom".into())
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM artists WHERE name IN ('Alice','Bob')",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert!(conn
        .query_row("SELECT scanned_at FROM folders", [], |r| r
            .get::<_, Option<i64>>(0))
        .unwrap()
        .is_some());
}

#[test]
fn scanning_isolates_bad_files_skips_disabled_folders_and_prunes_deleted_content() {
    let root = TestDir::new();
    let music = root.join("music");
    std::fs::create_dir(&music).unwrap();
    let nested = music.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let path = nested.join("tone.FLAC");
    std::fs::copy(root.audio("original.flac"), &path).unwrap();
    std::fs::write(music.join("bad.flac"), b"not audio").unwrap();
    std::fs::write(music.join("readme.txt"), b"ignore").unwrap();
    let mut conn = monitored_library(&music);
    conn.execute(
        "INSERT INTO folders (path,enabled) VALUES (?,0)",
        [root.join("disabled").to_string_lossy().as_ref()],
    )
    .unwrap();
    let first = scan_library(&mut conn, &Settings::default(), false, &root.join("cache")).unwrap();
    assert_eq!((first.scanned, first.added, first.errors), (1, 1, 1));
    let track_id = conn
        .query_row("SELECT id FROM tracks", [], |r| r.get::<_, i64>(0))
        .unwrap();
    cache::set_lyrics(&conn, track_id, "custom", None, Some("Mine")).unwrap();
    conn.execute("INSERT INTO playlists (id,name) VALUES (1,'Keep')", [])
        .unwrap();
    conn.execute(
        "INSERT INTO playlist_tracks (playlist_id,track_id,position) VALUES (1,?,0)",
        [track_id],
    )
    .unwrap();
    std::fs::remove_file(&path).unwrap();
    let second = scan_library(&mut conn, &Settings::default(), false, &root.join("cache")).unwrap();
    assert_eq!((second.removed, second.errors), (1, 1));
    for table in [
        "tracks",
        "albums",
        "artists",
        "lyrics",
        "playlist_tracks",
        "artist_albums",
    ] {
        assert_eq!(
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0,
            "{table}"
        );
    }
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM playlists", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn tag_changes_replace_metadata_and_missing_technical_fields_force_a_rescan() {
    let root = TestDir::new();
    let path = root.audio("tone.flac");
    let mut conn = monitored_library(root.path());
    scan_library(&mut conn, &Settings::default(), false, &root.join("cache")).unwrap();
    let mut file = Probe::open(&path).unwrap().read().unwrap();
    let tag = file.primary_tag_mut().unwrap();
    tag.set_title("Changed".into());
    tag.remove_key(ItemKey::AlbumTitle);
    tag.remove_key(ItemKey::AlbumArtist);
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    // Simulate an old library row without technical properties, even if the
    // filesystem timestamp has only one-second precision.
    conn.execute("UPDATE tracks SET sample_rate_hz=NULL", [])
        .unwrap();
    let result = scan_library(&mut conn, &Settings::default(), false, &root.join("cache")).unwrap();
    assert_eq!(result.updated, 1);
    assert_eq!(
        conn.query_row("SELECT title,album_id FROM tracks", [], |r| Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<i64>>(1)?
        )))
        .unwrap(),
        ("Changed".into(), None)
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM albums", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn scan_rejects_invalid_configuration_before_mutating_the_database() {
    let root = TestDir::new();
    let mut conn = monitored_library(root.path());
    let bad = Settings {
        artist_split_regex: "[".into(),
        ..Default::default()
    };
    assert!(scan_library(&mut conn, &bad, false, &root.join("cache")).is_err());
    conn.execute(
        "UPDATE folders SET path=?",
        [root.join("missing").to_string_lossy().as_ref()],
    )
    .unwrap();
    assert!(scan_library(&mut conn, &Settings::default(), false, &root.join("cache")).is_err());
    assert!(conn
        .query_row("SELECT scanned_at FROM folders", [], |r| r
            .get::<_, Option<i64>>(0))
        .unwrap()
        .is_none());
}

fn unique_test_directory(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "sparkle-scanner-{label}-{}-{nonce}",
        std::process::id()
    ))
}

#[test]
fn folder_membership_requires_a_path_component_boundary() {
    let separator = std::path::MAIN_SEPARATOR;
    let folder = format!("library{separator}Music");
    let child = format!("{folder}{separator}Artist{separator}song.flac");
    let sibling = format!("library{separator}Music Backup{separator}song.flac");

    assert!(path_is_within_folder(&child, &folder));
    assert!(!path_is_within_folder(&sibling, &folder));
}

#[test]
fn wav_is_not_advertised_as_a_supported_scan_format() {
    assert!(is_audio_file(Path::new("song.flac")));
    assert!(!is_audio_file(Path::new("song.wav")));
}

#[test]
fn stale_pruning_does_not_delete_from_a_prefix_sibling() {
    let mut conn = Connection::open_in_memory().expect("open scanner test database");
    conn.execute_batch(
        "
        CREATE TABLE tracks (id INTEGER PRIMARY KEY, file_path TEXT NOT NULL);
        CREATE TABLE track_artists (track_id INTEGER NOT NULL);
        CREATE TABLE playlist_tracks (track_id INTEGER NOT NULL);
        CREATE TABLE play_queue (track_id INTEGER NOT NULL);
        CREATE TABLE lyrics (track_id INTEGER NOT NULL);
        ",
    )
    .expect("create scanner test tables");

    let separator = std::path::MAIN_SEPARATOR;
    let folder_path = format!("library{separator}Music");
    let stale_path = format!("{folder_path}{separator}stale.flac");
    let sibling_path = format!("library{separator}Music Backup{separator}outside.flac");
    conn.execute(
        "INSERT INTO tracks (id, file_path) VALUES (1, ?1), (2, ?2)",
        rusqlite::params![stale_path, sibling_path],
    )
    .expect("insert scanner test tracks");

    let tx = conn.transaction().expect("start scanner test transaction");
    let folders = [Folder {
        id: 1,
        path: folder_path,
        enabled: true,
        scanned_at: None,
    }];
    let cache_root = unique_test_directory("prune");
    std::fs::create_dir_all(&cache_root).expect("create scanner cache test directory");
    let removed = prune_stale_tracks(&tx, &folders, &HashSet::new(), &cache_root)
        .expect("prune stale scanner tracks");

    assert_eq!(removed, 1);
    assert_eq!(
        tx.query_row("SELECT file_path FROM tracks WHERE id = 2", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("prefix sibling track remains"),
        sibling_path
    );
    drop(tx);
    std::fs::remove_dir(&cache_root).expect("remove scanner cache test directory");
}

#[cfg(windows)]
#[test]
fn folder_membership_handles_windows_case_and_separator_variants() {
    assert!(path_is_within_folder(
        "C:/MUSIC/Artist/song.flac",
        r"c:\music"
    ));
    assert!(!path_is_within_folder(
        r"C:\Music Backup\song.flac",
        r"C:\Music"
    ));
}

#[test]
fn canonical_directory_identity_deduplicates_alias_paths() {
    let root = unique_test_directory("directory-identity");
    std::fs::create_dir_all(&root).expect("create temporary scanner directory");

    let mut visited = HashSet::new();

    assert!(mark_directory_visited(&root, &mut visited).expect("visit temporary directory"));
    assert!(
        !mark_directory_visited(&root.join("."), &mut visited).expect("visit dot-directory alias")
    );

    std::fs::remove_dir(&root).expect("remove temporary scanner directory");
}
