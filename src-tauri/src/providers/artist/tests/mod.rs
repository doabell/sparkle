use super::*;
use crate::test_support::TestDir;

#[test]
fn query_overrides_follow_image_then_info_then_name_precedence() {
    let conn = crate::db::test_connection();
    assert!(artist_query_title(&conn, 99).unwrap().is_none());
    assert!(artist_image_query_title(&conn, 99).unwrap().is_none());
    conn.execute("INSERT INTO artists (id,name) VALUES (1,'Alice')", [])
        .unwrap();
    for (info, image, expected_info, expected_image) in [
        (None, None, "Alice", "Alice"),
        (Some("Biography"), None, "Biography", "Biography"),
        (Some("Biography"), Some("Portrait"), "Biography", "Portrait"),
        (Some(" "), Some("  "), "Alice", "Alice"),
    ] {
        conn.execute(
            "UPDATE artists SET info_term=?1,image_term=?2 WHERE id=1",
            rusqlite::params![info, image],
        )
        .unwrap();
        assert_eq!(
            artist_query_title(&conn, 1).unwrap().as_deref(),
            Some(expected_info)
        );
        assert_eq!(
            artist_image_query_title(&conn, 1).unwrap().as_deref(),
            Some(expected_image)
        );
    }
    let settings = Settings {
        artist_image_sources: vec![
            "custom".into(),
            "wikipedia:ja".into(),
            "wikipedia:fr".into(),
        ],
        ..Default::default()
    };
    assert_eq!(brave_lang_hint(Some("wikipedia:zh"), &settings), "zh");
    assert_eq!(brave_lang_hint(Some("brave"), &settings), "ja");
    assert_eq!(
        brave_lang_hint(
            None,
            &Settings {
                artist_image_sources: vec![],
                ..settings
            }
        ),
        "en"
    );
}

#[test]
fn provider_configuration_without_online_sources_does_not_fetch() {
    let settings = Settings {
        artist_info_sources: vec!["custom".into(), "unknown".into()],
        artist_image_sources: vec!["custom".into(), "unknown".into(), "brave".into()],
        brave_api_key: String::new(),
        ..Default::default()
    };
    assert!(fetch_artist_info_online("Alice", &settings)
        .unwrap()
        .is_none());
    assert!(fetch_artist_image_online("Alice", &settings)
        .unwrap()
        .is_none());
    assert!(fetch_artist_info_from_provider("custom", "Alice")
        .unwrap()
        .is_none());
    assert!(
        fetch_artist_image_from_provider("custom", "Alice", "en", &settings)
            .unwrap()
            .is_none()
    );
    assert!(
        fetch_artist_image_from_provider("brave", "Alice", "en", &settings)
            .unwrap()
            .is_none()
    );
}

#[test]
fn artist_metadata_cache_roundtrips_bios_and_distinguishes_custom_images() {
    let root = TestDir::new();
    let conn = crate::db::test_connection();
    conn.execute("INSERT INTO artists (id,name) VALUES (1,'Alice')", [])
        .unwrap();
    assert!(get_cached_artist_info(&conn, root.path(), 1)
        .unwrap()
        .is_none());
    cache_artist_info(
        &conn,
        root.path(),
        1,
        &ArtistInfo {
            source: "wikipedia:en".into(),
            summary: Some("Biography".into()),
        },
    )
    .unwrap();
    assert_eq!(
        get_cached_artist_info(&conn, root.path(), 1)
            .unwrap()
            .unwrap()
            .summary
            .as_deref(),
        Some("Biography")
    );
    assert!(get_cached_artist_image(&conn, root.path(), 1)
        .unwrap()
        .is_none());
    let mut image = ImageData {
        source: "custom".into(),
        data: Some(vec![1, 2]),
        mime_type: "image/jpeg".into(),
    };
    cache_artist_image(&conn, root.path(), 1, &image).unwrap();
    assert!(get_cached_artist_image(&conn, root.path(), 1)
        .unwrap()
        .is_none());
    image.source = "brave".into();
    let reference = cache_artist_image(&conn, root.path(), 1, &image).unwrap();
    assert_eq!(
        get_cached_artist_image(&conn, root.path(), 1)
            .unwrap()
            .unwrap()
            .file_path,
        reference.file_path
    );
}
