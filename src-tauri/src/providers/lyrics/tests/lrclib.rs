use super::*;
use crate::test_support::{http_client as test_client, HttpFixture};

#[test]
fn decodes_provider_camel_case_fields_and_encodes_search_terms() {
    let peer = HttpFixture::json(
        200,
        r#"{"syncedLyrics":"[00:01.00]Hello","plainLyrics":"Hello"}"#,
    );
    let metadata = TrackMetadata {
        title: Some("A & B?".into()),
        artist: Some("Alice/Bob".into()),
        ..Default::default()
    };
    let lyrics = fetch_with_client(&metadata, &test_client(), &peer.url)
        .unwrap()
        .expect("synced result");
    assert_eq!(lyrics.source, "lrclib");
    assert_eq!(lyrics.synced_text.as_deref(), Some("[00:01.00]Hello"));
    assert_eq!(lyrics.plain_text.as_deref(), Some("Hello"));
    let request = peer.request();
    assert!(request.contains("artist_name=Alice%2FBob"));
    assert!(request.contains("track_name=A+%26+B%3F"));
}

#[test]
fn handles_missing_metadata_empty_lyrics_http_errors_and_malformed_json() {
    let client = test_client();
    assert!(fetch(&TrackMetadata::default()).unwrap().is_none());
    assert!(
        fetch_with_client(&TrackMetadata::default(), &client, "not a URL")
            .unwrap()
            .is_none()
    );
    let metadata = TrackMetadata {
        title: Some("Song".into()),
        ..Default::default()
    };
    for (status, body) in [
        (404, "not found"),
        (200, r#"{"plainLyrics":"Plain only"}"#),
        (200, r#"{"syncedLyrics":"  "}"#),
    ] {
        let peer = HttpFixture::json(status, body);
        assert!(fetch_with_client(&metadata, &client, &peer.url)
            .unwrap()
            .is_none());
    }
    let peer = HttpFixture::json(200, "{");
    assert!(fetch_with_client(&metadata, &client, &peer.url).is_err());
    assert!(fetch_with_client(&metadata, &client, "invalid URL").is_err());
}

#[test]
fn manual_candidates_filter_empty_lyrics_limit_results_and_preserve_order() {
    let client = test_client();
    assert!(fetch_candidates("  ", 5).unwrap().is_empty());
    assert!(candidates_with_client("  ", 5, &client, "invalid URL")
        .unwrap()
        .is_empty());
    let peer = HttpFixture::json(
        200,
        r#"[{"syncedLyrics":null},{"syncedLyrics":" "},{"syncedLyrics":"[00:01]First"},{"syncedLyrics":"[00:02]Second"}]"#,
    );
    let lyrics = candidates_with_client(" Query ", 1, &client, &peer.url).unwrap();
    assert_eq!(lyrics.len(), 1);
    assert_eq!(lyrics[0].synced_text.as_deref(), Some("[00:01]First"));
    assert!(peer.request().contains("q=Query"));
    let peer = HttpFixture::json(503, "unavailable");
    assert!(candidates_with_client("Query", 5, &client, &peer.url)
        .unwrap()
        .is_empty());
    let peer = HttpFixture::json(200, "not json");
    assert!(candidates_with_client("Query", 5, &client, &peer.url).is_err());
}
