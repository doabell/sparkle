use super::*;
use rusqlite::Connection;
use std::fs;

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
