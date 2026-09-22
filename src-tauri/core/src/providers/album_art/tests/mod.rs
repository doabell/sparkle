use super::*;
use crate::test_support::TestDir;
use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture};
use lofty::tag::TagExt;

#[test]
fn album_lookup_handles_missing_tracks_and_empty_release_ids() {
    let conn = crate::db::test_connection();
    let missing = album_art_lookup(&conn, 99).unwrap();
    assert!(missing.file_path.is_none() && missing.mbid.is_none());
    conn.execute_batch("INSERT INTO albums (id,title,mbid) VALUES (1,'Album',''); INSERT INTO tracks (file_path,album_id) VALUES ('song.flac',1)").unwrap();
    let lookup = album_art_lookup(&conn, 1).unwrap();
    assert_eq!(lookup.file_path.as_deref(), Some("song.flac"));
    assert!(lookup.mbid.is_none());
    conn.execute("UPDATE albums SET mbid='release-id' WHERE id=1", [])
        .unwrap();
    assert_eq!(
        album_art_lookup(&conn, 1).unwrap().mbid.as_deref(),
        Some("release-id")
    );
}

#[test]
fn embedded_art_prefers_front_cover_then_falls_back_to_first_picture() {
    let root = TestDir::new();
    let path = root.audio("tone.flac");
    assert!(fetch_embedded_from_path(path.to_str().unwrap())
        .unwrap()
        .is_none());
    let mut file = Probe::open(&path).unwrap().read().unwrap();
    let tag = file.primary_tag_mut().unwrap();
    let jpeg = |color| {
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut bytes)
            .encode_image(&image::RgbImage::from_pixel(2, 2, image::Rgb(color)))
            .unwrap();
        bytes
    };
    let back = jpeg([20, 40, 60]);
    let front = jpeg([90, 10, 20]);
    tag.push_picture(
        Picture::unchecked(back.clone())
            .pic_type(PictureType::CoverBack)
            .mime_type(MimeType::Jpeg)
            .build(),
    );
    tag.push_picture(
        Picture::unchecked(front.clone())
            .pic_type(PictureType::CoverFront)
            .mime_type(MimeType::Jpeg)
            .build(),
    );
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    let art = fetch_embedded_from_path(path.to_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(art.data, Some(front));
    assert_eq!(art.mime_type, "image/jpeg");
    tag.remove_picture_type(PictureType::CoverFront);
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    assert_eq!(
        fetch_embedded_from_path(path.to_str().unwrap())
            .unwrap()
            .unwrap()
            .data,
        Some(back)
    );
    let settings = Settings {
        album_art_sources: vec!["custom".into(), "unknown".into(), "embedded".into()],
        ..Default::default()
    };
    let lookup = AlbumArtLookup {
        file_path: Some(path.to_string_lossy().into_owned()),
        mbid: None,
    };
    assert_eq!(
        fetch_album_art_online(&lookup, &settings)
            .unwrap()
            .unwrap()
            .source,
        "embedded"
    );
    let missing = AlbumArtLookup {
        file_path: Some(root.join("missing.flac").to_string_lossy().into_owned()),
        mbid: None,
    };
    assert!(fetch_album_art_online(&missing, &settings)
        .unwrap()
        .is_none());
    assert!(fetch_album_art_online(
        &AlbumArtLookup {
            file_path: None,
            mbid: None
        },
        &Settings::default()
    )
    .unwrap()
    .is_none());
}

#[test]
fn album_cache_returns_file_references_and_never_mistakes_custom_for_online_art() {
    let root = TestDir::new();
    let conn = crate::db::test_connection();
    assert!(get_cached_album_art(&conn, root.path(), 7)
        .unwrap()
        .is_none());
    cache_album_art(
        &conn,
        root.path(),
        7,
        &ImageData {
            source: "custom".into(),
            data: Some(vec![1, 2, 3]),
            mime_type: "image/jpeg".into(),
        },
    )
    .unwrap();
    assert!(get_cached_album_art(&conn, root.path(), 7)
        .unwrap()
        .is_none());
    let image = ImageData {
        source: "embedded".into(),
        data: Some(vec![4, 5, 6]),
        mime_type: "image/jpeg".into(),
    };
    let reference = cache_album_art(&conn, root.path(), 7, &image).unwrap();
    assert_eq!(
        get_cached_album_art(&conn, root.path(), 7)
            .unwrap()
            .unwrap()
            .file_path,
        reference.file_path
    );
}
