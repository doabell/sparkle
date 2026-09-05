use super::*;

#[test]
fn local_provider_dispatch_respects_custom_none_and_missing_metadata() {
    let metadata = TrackMetadata {
        embedded_lyrics: Some("[00:01.00]Hello".into()),
        ..Default::default()
    };
    let custom = Lyrics {
        source: "custom".into(),
        plain_text: Some("Mine".into()),
        synced_text: None,
    };
    let fetched = fetch_lyrics_from_sources_with_custom(
        &["unknown".into(), "custom".into(), "embedded".into()],
        &metadata,
        Some(&custom),
    )
    .unwrap()
    .unwrap();
    assert_eq!(fetched.plain_text.as_deref(), Some("Mine"));
    let fetched = fetch_lyrics_from_sources_with_custom(
        &["custom".into(), "embedded".into()],
        &metadata,
        None,
    )
    .unwrap()
    .unwrap();
    assert_eq!(fetched.source, "embedded");
    assert_eq!(fetched.plain_text.as_deref(), Some("Hello"));
    let fetched =
        fetch_lyrics_from_sources_with_custom(&["none".into(), "embedded".into()], &metadata, None)
            .unwrap()
            .unwrap();
    assert_eq!(fetched.source, "none");
    assert!(fetched.synced_text.is_none());
    for source in [
        "embedded",
        "lrc",
        "lrclib",
        "netease",
        "kashinavi",
        "qq",
        "unknown",
    ] {
        assert!(fetch_lyrics_from_source(source, &TrackMetadata::default())
            .unwrap()
            .is_none());
    }
    assert!(embedded::fetch(&TrackMetadata {
        embedded_lyrics: Some(" \n".into()),
        ..Default::default()
    })
    .unwrap()
    .is_none());
}

#[test]
fn lyric_metadata_uses_main_artist_and_preserves_missing_tags() {
    let conn = crate::db::test_connection();
    conn.execute_batch("INSERT INTO artists (id,name) VALUES (1,'Alice'); INSERT INTO albums (id,title) VALUES (1,'Album'); INSERT INTO tracks (id,file_path,title,album_id,duration_ms,embedded_lyrics) VALUES (1,'song.flac','Song',1,123000,'Words'),(2,'untagged.flac',NULL,NULL,NULL,NULL); INSERT INTO track_artists (track_id,artist_id,role) VALUES (1,1,'main');").unwrap();
    let tagged = fetch_track_metadata(&conn, 1).unwrap();
    assert_eq!(tagged.title.as_deref(), Some("Song"));
    assert_eq!(tagged.artist.as_deref(), Some("Alice"));
    assert_eq!(tagged.album.as_deref(), Some("Album"));
    assert_eq!(tagged.duration_ms, Some(123000));
    assert_eq!(tagged.embedded_lyrics.as_deref(), Some("Words"));
    assert!(fetch_track_metadata(&conn, 2).unwrap().artist.is_none());
    assert!(fetch_track_metadata(&conn, 99).is_err());
}

#[test]
fn translations_expand_repeated_timestamps_and_leave_unmatched_lines_alone() {
    assert_eq!(
        inject_translation(
            "\n[00:03.00]Last\n[00:01.00][00:02.00]Hello\nuntimed",
            "\n[00:01.00][00:02.00]你好\nuntimed"
        ),
        "[00:01.00]Hello/你好\n[00:02.00]Hello/你好\n[00:03.00]Last"
    );
    assert_eq!(inject_translation("", ""), "");
}
use std::sync::Mutex;

#[test]
fn automatic_lookup_stops_at_the_first_configured_match() {
    let sources = vec![
        "first".to_string(),
        "second".to_string(),
        "third".to_string(),
    ];
    let calls = Mutex::new(Vec::new());

    let result = fetch_from_sources(&sources, |source| {
        calls.lock().unwrap().push(source.to_string());
        Ok((source == "first").then(|| source.to_string()))
    })
    .unwrap();

    assert_eq!(result.as_deref(), Some("first"));
    assert_eq!(calls.into_inner().unwrap(), vec!["first"]);
}

#[test]
fn custom_content_participates_as_an_ordered_provider() {
    let sources = vec!["custom".to_string(), "lrclib".to_string()];
    let custom = Lyrics {
        source: "custom".to_string(),
        synced_text: Some("[00:00.00]saved".to_string()),
        plain_text: Some("saved".to_string()),
    };

    let result =
        fetch_lyrics_from_sources_with_custom(&sources, &TrackMetadata::default(), Some(&custom))
            .unwrap();

    assert_eq!(result.unwrap().source, "custom");
}

#[test]
fn no_lyrics_provider_returns_an_explicit_empty_result() {
    let result = fetch_lyrics_from_sources_with_custom(
        &["none".to_string()],
        &TrackMetadata::default(),
        None,
    )
    .unwrap()
    .unwrap();

    assert_eq!(result.source, "none");
    assert!(result.synced_text.is_none());
    assert!(result.plain_text.is_none());
}

#[test]
fn first_synced_line_requires_a_timestamp_and_text() {
    assert_eq!(
        first_synced_line("[00:01.25]hello").as_deref(),
        Some("hello")
    );
    assert!(first_synced_line("plain lyrics").is_none());
    assert!(first_synced_line("[00:01.25]").is_none());
}

#[test]
fn first_synced_line_uses_timestamp_order() {
    assert_eq!(
        first_synced_line("[00:10.00]second\n[00:02.50]first").as_deref(),
        Some("first")
    );
}
