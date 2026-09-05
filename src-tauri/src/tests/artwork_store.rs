use super::*;
use object_store::memory::InMemory;

fn config(prefix: &str) -> S3Config {
    S3Config {
        public_url: Url::parse("https://cdn.example.test").unwrap(),
        prefix: normalize_prefix(prefix),
    }
}

fn memory_store(prefix: &str) -> S3ArtworkStore {
    S3ArtworkStore {
        config: config(prefix),
        store: Arc::new(InMemory::new()),
        runtime: tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap(),
        known_keys: HashSet::new(),
    }
}

#[test]
fn content_hashes_map_to_deterministic_jpeg_keys() {
    let config = config("artwork");
    assert_eq!(
        config.object_key("0123456789abcdef0123456789abcdef"),
        "artwork/0123456789abcdef0123456789abcdef.jpg"
    );
}

#[test]
fn candidate_lookup_ignores_album_keys_and_deduplicates_content_hashes() {
    let config = config("");
    let hashes = vec![
        "album:42".to_string(),
        "abcdefabcdefabcdefabcdefabcdefab".to_string(),
        "abcdefabcdefabcdefabcdefabcdefab".to_string(),
    ];
    assert_eq!(
        candidate_object_keys(&config, &hashes),
        vec!["abcdefabcdefabcdefabcdefabcdefab.jpg".to_string()]
    );
}

#[test]
fn prefix_is_normalized_for_stable_object_keys() {
    assert_eq!(normalize_prefix("/artwork//"), "artwork/");
    assert_eq!(normalize_prefix("///"), "");
}

#[test]
fn public_urls_include_the_object_prefix_and_jpeg_name() {
    let config = config("artwork");
    let object_key = config.object_key("0123456789abcdef0123456789abcdef");
    assert_eq!(
        append_path(&config.public_url, &encode_path(&object_key)).as_str(),
        "https://cdn.example.test/artwork/0123456789abcdef0123456789abcdef.jpg"
    );
}

#[test]
fn invalid_cache_keys_are_not_treated_as_content_hashes() {
    assert!(!is_content_hash("album:42"));
    assert!(!is_content_hash("short"));
    assert!(is_content_hash("abcdefabcdefabcdefabcdefabcdefab"));
}

#[test]
fn head_probe_reuses_an_existing_content_object_without_overwriting_it() {
    let hash = "abcdefabcdefabcdefabcdefabcdefab".to_string();
    let mut store = memory_store("artwork");
    let object_key = store.config.object_key(&hash);
    store
        .put_object(&object_key, b"existing-object".to_vec())
        .unwrap();

    let url = store
        .find_or_upload(b"replacement".to_vec(), &[hash])
        .unwrap();

    assert_eq!(
        url,
        "https://cdn.example.test/artwork/abcdefabcdefabcdefabcdefabcdefab.jpg"
    );
    let backend = Arc::clone(&store.store);
    let location = Path::from(object_key);
    let stored = store
        .runtime
        .block_on(async move { backend.get(&location).await.unwrap().bytes().await.unwrap() });
    assert_eq!(stored.as_ref(), b"existing-object");
}

#[test]
fn settings_test_upload_is_verified_and_deleted() {
    let mut store = memory_store("artwork");
    let url = store
        .test_access_and_upload(b"test-object".to_vec())
        .unwrap();

    assert_eq!(url, "https://cdn.example.test/artwork/sparkle-test.jpg");
    assert!(!store.known_keys.contains("artwork/sparkle-test.jpg"));
    assert!(!store
        .object_exists("artwork/sparkle-test.jpg")
        .expect("check that the S3 test object was deleted"));
}

#[test]
fn cleanup_failure_is_actionable_after_a_successful_probe() {
    let error = finish_test_with_cleanup(
        Ok("https://cdn.example.test/artwork/sparkle-test.jpg"),
        Err("S3 DELETE failed: access denied".to_string()),
        "artwork/sparkle-test.jpg",
    )
    .unwrap_err();

    assert!(error.starts_with("S3 access test succeeded, but cleanup failed"));
    assert!(error.contains("artwork/sparkle-test.jpg"));
    assert!(error.contains("S3 DELETE failed: access denied"));
    assert!(error.contains("Delete it manually from the configured bucket"));
}

#[test]
fn cleanup_failure_does_not_mask_the_probe_failure() {
    let error = finish_test_with_cleanup::<String>(
        Err("S3 HEAD failed: timed out".to_string()),
        Err("S3 DELETE failed: access denied".to_string()),
        "artwork/sparkle-test.jpg",
    )
    .unwrap_err();

    assert!(error.starts_with("S3 HEAD failed: timed out"));
    assert!(error.contains("cleanup also failed"));
    assert!(error.contains("artwork/sparkle-test.jpg"));
    assert!(error.contains("S3 DELETE failed: access denied"));
}

#[test]
fn successful_cleanup_preserves_the_probe_failure() {
    let error = finish_test_with_cleanup::<String>(
        Err("S3 test object was not readable after upload".to_string()),
        Ok(()),
        "artwork/sparkle-test.jpg",
    )
    .unwrap_err();

    assert_eq!(
        error,
        "S3 test object was not readable after upload".to_string()
    );
}

#[test]
fn public_path_style_store_can_be_built_without_network_access() {
    let store = S3ArtworkStore::new(S3BuildConfig {
        endpoint: Url::parse("http://minio.example.test:9000").unwrap(),
        bucket: "sparkle".to_string(),
        public_url: Url::parse("https://cdn.example.test").unwrap(),
        access_key: None,
        secret_key: None,
        session_token: None,
        region: DEFAULT_REGION.to_string(),
        prefix: normalize_prefix("artwork"),
    })
    .unwrap();
    assert_eq!(store.config.prefix, "artwork/");
}

#[test]
fn persisted_settings_build_an_s3_store_without_network_access() {
    let settings = Settings {
        discord_artwork_s3_endpoint: "http://minio.example.test:9000".to_string(),
        discord_artwork_s3_bucket: "sparkle".to_string(),
        discord_artwork_s3_access_key: "access".to_string(),
        discord_artwork_s3_secret_key: "secret".to_string(),
        discord_artwork_s3_prefix: "artwork".to_string(),
        ..Default::default()
    };

    let store = S3ArtworkStore::from_settings(&settings)
        .unwrap()
        .expect("settings should enable S3");
    assert_eq!(store.config.prefix, "artwork/");
}

#[test]
fn public_url_ownership_is_scoped_to_the_configured_base() {
    let store = S3ArtworkStore::new(S3BuildConfig {
        endpoint: Url::parse("http://minio.example.test:9000").unwrap(),
        bucket: "sparkle".to_string(),
        public_url: Url::parse("https://cdn.example.test/artwork").unwrap(),
        access_key: None,
        secret_key: None,
        session_token: None,
        region: DEFAULT_REGION.to_string(),
        prefix: normalize_prefix("images"),
    })
    .unwrap();
    assert!(store.owns_public_url("https://cdn.example.test/artwork/images/a.jpg"));
    assert!(!store.owns_public_url("https://cdn.example.test/other/images/a.jpg"));
}
