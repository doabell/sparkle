use super::*;
use crate::test_support::TestDir;
use lofty::config::WriteOptions;
use lofty::tag::TagExt;

#[test]
fn id3v24_nul_separated_frames_supply_all_artists_in_order() {
    use lofty::id3::v2::Id3v2Tag;
    let mut id3 = Id3v2Tag::default();
    id3.set_artist("AC/DC\0Tyler, The Creator\0宇多田ヒカル".into());
    let mut tag: Tag = id3.into();
    tag.insert_text(ItemKey::AlbumArtist, "Group One\0Group Two".into());
    // Old custom fields must not supersede the newly corrected standard frame.
    tag.insert_text(ItemKey::TrackArtists, "Old; combined; field".into());
    assert_eq!(
        collect_artists(&tag, ItemKey::TrackArtist, ItemKey::TrackArtists),
        vec!["AC/DC", "Tyler, The Creator", "宇多田ヒカル"]
    );
    assert_eq!(
        collect_artists(&tag, ItemKey::AlbumArtist, ItemKey::AlbumArtists),
        vec!["Group One", "Group Two"]
    );
}

#[test]
fn repeated_artist_tags_and_legacy_custom_values_preserve_punctuation() {
    use lofty::tag::{ItemValue, TagItem, TagType};
    let mut tag = Tag::new(TagType::VorbisComments);
    for name in ["Alice; Bob", "Group / Ensemble", "Alice; Bob"] {
        tag.push(TagItem::new(
            ItemKey::TrackArtist,
            ItemValue::Text(name.into()),
        ));
    }
    assert_eq!(
        collect_artists(&tag, ItemKey::TrackArtist, ItemKey::TrackArtists),
        vec!["Alice; Bob", "Group / Ensemble"]
    );
    tag.remove_key(ItemKey::TrackArtist);
    tag.insert_text(ItemKey::TrackArtists, "Fallback\0Other".into());
    assert_eq!(
        collect_artists(&tag, ItemKey::TrackArtist, ItemKey::TrackArtists),
        vec!["Fallback", "Other"]
    );
}

#[test]
fn renaming_and_retagging_keep_track_identity_and_user_data_across_missing_scans() {
    let root = TestDir::new();
    let original = root.audio("original.flac");
    let mut conn = monitored_library(root.path());
    scan_library(&mut conn, &Settings::default(), false, &root.join("cache")).unwrap();
    let (id, fingerprint): (i64, String) = conn
        .query_row("SELECT id,audio_fingerprint FROM tracks", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    cache::set_custom_lyrics(&conn, id, Some("[00:15]Saved lyrics"), Some("Saved lyrics")).unwrap();
    conn.execute_batch(&format!("UPDATE tracks SET lrc_offset_ms=375;
        INSERT INTO playlists(id,name) VALUES(1,'Keep');
        INSERT INTO playlist_tracks VALUES(1,{id},0);
        INSERT INTO play_queue(track_id,position) VALUES({id},0);
        INSERT INTO listens(id,session_id,track_id,started_at_ms,last_activity_at_ms,start_source,start_reason)
        VALUES('listen','session',{id},1000,1000,'ui','play');")).unwrap();
    let offline = root.join("temporarily.offline");
    std::fs::rename(&original, &offline).unwrap();
    let missing =
        scan_library(&mut conn, &Settings::default(), false, &root.join("cache")).unwrap();
    assert_eq!(missing.removed, 1);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM available_tracks", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(cache::get_lyrics_from_source(&conn, id, "custom")
        .unwrap()
        .is_some());
    let renamed = root.join("renamed.flac");
    std::fs::rename(offline, &renamed).unwrap();
    let mut file = Probe::open(&renamed).unwrap().read().unwrap();
    let tag = file.primary_tag_mut().unwrap();
    tag.set_title("A different label".into());
    tag.set_artist("New credit".into());
    tag.save_to_path(&renamed, WriteOptions::default()).unwrap();
    let result = scan_library(&mut conn, &Settings::default(), false, &root.join("cache")).unwrap();
    assert_eq!((result.added, result.updated, result.removed), (0, 1, 0));
    let row: (i64, String, String, String, i64) = conn
        .query_row(
            "SELECT id,file_path,title,audio_fingerprint,lrc_offset_ms FROM available_tracks",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(
        row,
        (
            id,
            renamed.to_string_lossy().into(),
            "A different label".into(),
            fingerprint,
            375
        )
    );
    for table in ["lyrics", "playlist_tracks", "play_queue", "listens"] {
        assert_eq!(
            conn.query_row(&format!("SELECT track_id FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            id
        );
    }
}

#[test]
fn identical_copies_are_separate_and_ambiguous_moves_preserve_the_missing_record() {
    let root = TestDir::new();
    let original = root.audio("original.flac");
    let mut conn = monitored_library(root.path());
    scan_library(&mut conn, &Settings::default(), false, &root.join("cache")).unwrap();
    let id: i64 = conn
        .query_row("SELECT id FROM tracks", [], |r| r.get(0))
        .unwrap();
    cache::set_custom_lyrics(&conn, id, Some("[00:01]Mine"), None).unwrap();
    std::fs::copy(&original, root.join("copy.flac")).unwrap();
    assert_eq!(
        scan_library(&mut conn, &Settings::default(), false, &root.join("cache"))
            .unwrap()
            .added,
        1
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    std::fs::rename(&original, root.join("renamed.flac")).unwrap();
    std::fs::copy(root.join("renamed.flac"), root.join("another.flac")).unwrap();
    let result = scan_library(&mut conn, &Settings::default(), false, &root.join("cache")).unwrap();
    assert_eq!((result.added, result.removed), (2, 1));
    assert!(cache::get_lyrics_from_source(&conn, id, "custom")
        .unwrap()
        .is_some());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM available_tracks", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[test]
fn album_identity_stays_title_year_and_credits_do_not_depend_on_scan_order() {
    let root = TestDir::new();
    let mut conn = monitored_library(root.path());
    for (name, number, artists) in [("z.flac", 1, "Zebra\0Alpha"), ("a.flac", 2, "Beta")] {
        let path = root.audio(name);
        let mut file = Probe::open(&path).unwrap().read().unwrap();
        let tag = file.primary_tag_mut().unwrap();
        tag.set_track(number);
        tag.set_artist(artists.into());
        tag.insert_text(ItemKey::AlbumArtist, artists.into());
        tag.save_to_path(&path, WriteOptions::default()).unwrap();
    }
    for force in [false, true] {
        scan_library(&mut conn, &Settings::default(), force, &root.join("cache")).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM albums", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        let credits=conn.prepare("SELECT a.name FROM album_artists aa JOIN artists a ON a.id=aa.artist_id ORDER BY aa.position").unwrap()
            .query_map([],|r|r.get::<_,String>(0)).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
        assert_eq!(credits, vec!["Zebra", "Alpha", "Beta"]);
        let credits=conn.prepare("SELECT a.name FROM track_artists ta JOIN artists a ON a.id=ta.artist_id JOIN tracks t ON t.id=ta.track_id WHERE t.track_number=1 ORDER BY ta.position").unwrap()
            .query_map([],|r|r.get::<_,String>(0)).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
        assert_eq!(credits, vec!["Zebra", "Alpha"]);
    }
}

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
fn real_scan_indexes_all_artist_values_and_forces_metadata_refresh() {
    let root = TestDir::new();
    let music = root.join("music");
    std::fs::create_dir(&music).unwrap();
    let path = music.join("tone.flac");
    std::fs::copy(root.audio("source.flac"), &path).unwrap();
    let mut conn = monitored_library(&music);
    let mut file = Probe::open(&path).unwrap().read().unwrap();
    let tag = file.primary_tag_mut().unwrap();
    tag.set_artist("Alice".into());
    tag.push(lofty::tag::TagItem::new(
        ItemKey::TrackArtist,
        lofty::tag::ItemValue::Text("Bob".into()),
    ));
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    let settings = Settings::default();
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
    let mut file = Probe::open(&path).unwrap().read().unwrap();
    let tag = file.primary_tag_mut().unwrap();
    tag.set_artist("Alice; Bob".into());
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    let forced = scan_library(&mut conn, &settings, true, &root.join("cache")).unwrap();
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
fn scanning_isolates_bad_files_and_archives_missing_content() {
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
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM available_tracks", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    for table in ["tracks", "albums", "lyrics", "playlist_tracks"] {
        assert_eq!(
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1,
            "retained {table}"
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
        CREATE TABLE tracks (id INTEGER PRIMARY KEY, file_path TEXT NOT NULL, missing_since INTEGER, lyrics_revision INTEGER NOT NULL DEFAULT 0);
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
