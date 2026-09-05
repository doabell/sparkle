use super::*;
use rusqlite::Connection;

#[test]
fn artist_info_replacement_and_deletion_remove_obsolete_files() {
    let conn = in_memory_db();
    let root = test_root();
    assert!(get_artist_info(&conn, &root, 7).unwrap().is_none());
    set_artist_info(&conn, &root, 7, "wiki", Some("First bio")).unwrap();
    assert_eq!(
        get_artist_info(&conn, &root, 7)
            .unwrap()
            .unwrap()
            .summary
            .as_deref(),
        Some("First bio")
    );
    set_artist_info(&conn, &root, 7, "brave", Some("New bio")).unwrap();
    assert_eq!(dir_stats(&artist_info_dir(&root)).0, 1);
    assert_eq!(
        get_artist_info(&conn, &root, 7).unwrap().unwrap().source,
        "brave"
    );
    set_artist_info(&conn, &root, 7, "brave", None).unwrap();
    assert_eq!(dir_stats(&artist_info_dir(&root)), (0, 0));
    assert!(get_artist_info(&conn, &root, 7)
        .unwrap()
        .unwrap()
        .summary
        .is_none());
    set_artist_info(&conn, &root, 7, "wiki", Some("Again")).unwrap();
    delete_artist_info(&conn, &root, 7).unwrap();
    delete_artist_info(&conn, &root, 7).unwrap();
    assert!(get_artist_info(&conn, &root, 7).unwrap().is_none());
    set_artist_info(&conn, &root, 8, "wiki", Some("Other")).unwrap();
    clear_artist_info(&conn, &root).unwrap();
    assert_eq!(dir_stats(&artist_info_dir(&root)), (0, 0));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn custom_lyric_files_replace_extensions_and_allow_copying_the_managed_file() {
    let root = test_root();
    let source = root.join("input.txt");
    std::fs::write(&source, "Words").unwrap();
    copy_custom_lyrics_file(&root, 7, &source, "txt").unwrap();
    let txt = custom_lyrics_path(&root, 7, "txt");
    assert_eq!(std::fs::read_to_string(&txt).unwrap(), "Words");
    copy_custom_lyrics_file(&root, 7, &txt, "TXT").unwrap();
    copy_custom_lyrics_file(&root, 7, &source, "LRC").unwrap();
    assert!(!txt.exists());
    assert!(custom_lyrics_path(&root, 7, "lrc").exists());
    delete_custom_lyrics_file(&root, 7);
    assert!(!custom_lyrics_path(&root, 7, "lrc").exists());
    assert!(copy_custom_lyrics_file(&root, 7, &root.join("missing"), "txt").is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cache_stats_and_source_deletion_preserve_unrelated_content() {
    let conn = in_memory_db();
    let root = test_root();
    set_lyrics(&conn, 7, "custom", None, Some("Mine")).unwrap();
    set_lyrics(&conn, 7, "online", None, Some("Words")).unwrap();
    assert_eq!(get_non_custom_lyrics(&conn, 7).unwrap().len(), 1);
    assert_eq!(cache_stats(&conn, &root)[0], ("Lyrics", 2, 9));
    delete_lyrics_from_source(&conn, 7, "online").unwrap();
    assert!(get_non_custom_lyrics(&conn, 7).unwrap().is_empty());
    assert!(get_lyrics_from_source(&conn, 7, "custom")
        .unwrap()
        .is_some());
    set_artist_info(&conn, &root, 7, "wiki", Some("Biography")).unwrap();
    assert_eq!(cache_stats(&conn, &root)[1], ("Artist info", 1, 9));
    assert!(get_non_custom_image(&conn, &root, "artist", 7)
        .unwrap()
        .is_none());
    assert!(read_cached_image(&CachedImage::none())
        .unwrap()
        .data
        .is_none());
    std::fs::remove_dir_all(root).unwrap();
}

fn test_root() -> PathBuf {
    let unique = format!(
        "sparkle-cache-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let dir = std::env::temp_dir().join(unique);
    ensure_dirs(&dir);
    dir
}

fn in_memory_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE lyrics (track_id INTEGER NOT NULL, source TEXT NOT NULL, synced_text TEXT, plain_text TEXT, fetched_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, PRIMARY KEY (track_id, source)); \
         CREATE TABLE artist_info (artist_id INTEGER PRIMARY KEY, source TEXT NOT NULL, file_path TEXT, fetched_at INTEGER NOT NULL, expires_at INTEGER NOT NULL); \
         CREATE TABLE images (entity_type TEXT NOT NULL, entity_id INTEGER NOT NULL, source TEXT NOT NULL, url TEXT, file_path TEXT, mime_type TEXT, fetched_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, PRIMARY KEY (entity_type, entity_id, source));"
    ).unwrap();
    conn
}

fn jpeg_bytes(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_pixel(width, height, image::Rgb([32, 96, 160]));
    let mut data = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, 90)
        .encode_image(&image)
        .unwrap();
    data
}

#[test]
fn lyrics_roundtrip() {
    let conn = in_memory_db();
    set_lyrics(&conn, 1, "lrc", Some("[00:00] hello"), Some("hello")).unwrap();
    let lyrics = get_lyrics_from_source(&conn, 1, "lrc")
        .unwrap()
        .expect("lyrics found");
    assert_eq!(lyrics.source, "lrc");
    assert_eq!(lyrics.synced_text.as_deref(), Some("[00:00] hello"));
    assert_eq!(lyrics.plain_text.as_deref(), Some("hello"));
    delete_lyrics(&conn, 1).unwrap();
    assert!(get_lyrics_from_source(&conn, 1, "lrc").unwrap().is_none());
}

#[test]
fn cached_lyrics_persist_until_cleared() {
    let conn = in_memory_db();
    set_lyrics(&conn, 1, "lrc", Some("text"), None).unwrap();
    assert!(get_lyrics_from_source(&conn, 1, "lrc").unwrap().is_some());
}

#[test]
fn clearing_lyrics_keeps_custom_content() {
    let conn = in_memory_db();
    set_lyrics(&conn, 1, "custom", Some("user"), None).unwrap();
    set_lyrics(&conn, 1, "lrclib", Some("online"), None).unwrap();
    clear_lyrics(&conn).unwrap();
    assert!(get_lyrics_from_source(&conn, 1, "custom")
        .unwrap()
        .is_some());
    assert!(get_lyrics_from_source(&conn, 1, "lrclib")
        .unwrap()
        .is_none());
}

#[test]
fn custom_lyrics_file_is_copied_and_removed_with_track() {
    let root = test_root();
    let source = root.join("picked.lrc");
    std::fs::write(&source, "[00:01.00]hello").unwrap();
    copy_custom_lyrics_file(&root, 12, &source, "lrc").unwrap();
    let managed = lyrics_dir(&root).join("custom-12.lrc");
    assert_eq!(
        std::fs::read_to_string(&managed).unwrap(),
        "[00:01.00]hello"
    );
    std::fs::remove_file(source).unwrap();
    assert!(managed.exists());
    delete_custom_lyrics_file(&root, 12);
    assert!(!managed.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn image_roundtrip_via_files() {
    let conn = in_memory_db();
    let root = test_root();
    let input = jpeg_bytes(8, 8);
    set_image(
        &conn,
        &root,
        "artist",
        1,
        "wikipedia:en",
        Some("http://x"),
        Some(&input),
    )
    .unwrap();
    let image = get_image(&conn, &root, "artist", 1)
        .unwrap()
        .expect("image found");
    assert_eq!(image.source, "wikipedia:en");
    let path = PathBuf::from(image.file_path.expect("cache file path"));
    assert_eq!(std::fs::read(&path).unwrap(), input);
    // File names are opaque hashes, not raw ids.
    let files: Vec<_> = std::fs::read_dir(images_dir(&root, "artist"))
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(files.len(), 1);
    let name = files[0].file_name().to_string_lossy().to_string();
    assert!(name.ends_with(".jpg"));
    assert!(
        name.len() == 20 && name[..16].chars().all(|c| c.is_ascii_hexdigit()),
        "expected a 16-char hex hash file name, got {name}"
    );
    delete_images(&conn, &root, "artist", 1, false).unwrap();
    assert!(get_image(&conn, &root, "artist", 1).unwrap().is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn image_validation_keeps_original_bytes() {
    let original = jpeg_bytes(2048, 1024);
    assert_eq!(
        validate_image_for_cache(original.clone()).unwrap(),
        original
    );
}

#[test]
fn image_file_reads_are_size_limited() {
    let root = test_root();
    let path = root.join("too-large-image.jpg");
    std::fs::File::create(&path)
        .unwrap()
        .set_len(MAX_CACHED_IMAGE_BYTES as u64 + 1)
        .unwrap();
    assert!(read_image_file(&path)
        .unwrap_err()
        .contains("image is too large"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn missing_image_file_becomes_a_cache_miss() {
    let conn = in_memory_db();
    let root = test_root();
    let image = set_image(
        &conn,
        &root,
        "album",
        4,
        "embedded",
        None,
        Some(&jpeg_bytes(8, 8)),
    )
    .unwrap();
    std::fs::remove_file(image.file_path.expect("cache file path")).unwrap();
    assert!(get_image(&conn, &root, "album", 4).unwrap().is_none());
    let remaining: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM images WHERE entity_type = 'album' AND entity_id = 4",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 0);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn delete_images_can_keep_custom() {
    let conn = in_memory_db();
    let root = test_root();
    set_image(&conn, &root, "album", 7, "custom", None, Some(&[9, 9])).unwrap();
    set_image(&conn, &root, "album", 7, "embedded", None, Some(&[1])).unwrap();
    delete_images(&conn, &root, "album", 7, true).unwrap();
    let remaining = get_custom_image(&conn, &root, "album", 7).unwrap();
    assert!(remaining.is_some());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn clearing_images_keeps_custom() {
    let conn = in_memory_db();
    let root = test_root();
    set_image(&conn, &root, "artist", 7, "custom", None, Some(&[9, 9])).unwrap();
    set_image(&conn, &root, "artist", 7, "wikipedia:en", None, Some(&[1])).unwrap();
    clear_images(&conn, &root).unwrap();
    assert!(get_custom_image(&conn, &root, "artist", 7)
        .unwrap()
        .is_some());
    assert!(get_image(&conn, &root, "artist", 7).unwrap().is_some());
    let _ = std::fs::remove_dir_all(&root);
}
