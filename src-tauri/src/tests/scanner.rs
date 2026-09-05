use super::*;

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
