use super::*;
use crate::test_support::{http_client, HttpFixture};

#[test]
fn front_cover_wins_and_missing_front_falls_back_to_first_image() {
    let client = http_client();
    for front in [true, false] {
        // The image signature, not the server's Content-Type, determines MIME.
        let bytes = b"\x89PNG\r\n\x1a\nfixture";
        let image = HttpFixture::response(200, "application/octet-stream", bytes);
        let images = if front {
            serde_json::json!([
                {"image":"invalid URL", "front":false},
                {"image":image.url, "front":true}
            ])
        } else {
            serde_json::json!([{"image":image.url}, {"image":"invalid URL"}])
        };
        let release = HttpFixture::json(200, &serde_json::json!({"images":images}).to_string());
        let art = fetch_from_url(&client, &release.url).unwrap().unwrap();
        assert_eq!(art.source, "cover_art_archive");
        assert_eq!(art.mime_type, "image/png");
        assert_eq!(art.data.as_deref(), Some(bytes.as_slice()));
        assert!(release.request().starts_with("GET / "));
        assert!(image.request().starts_with("GET / "));
    }
}

#[test]
fn unavailable_releases_and_images_return_none_but_malformed_payloads_report_errors() {
    let client = http_client();
    assert!(fetch_by_mbid("  ").unwrap().is_none());
    let release = HttpFixture::json(404, "not found");
    assert!(fetch_from_url(&client, &release.url).unwrap().is_none());
    for body in ["not JSON", "{}", r#"{"images":[]}"#, r#"{"images":[{}]}"#] {
        let release = HttpFixture::json(200, body);
        assert!(fetch_from_url(&client, &release.url).is_err(), "{body}");
    }
    let image = HttpFixture::response(403, "text/plain", b"forbidden");
    let release = HttpFixture::json(
        200,
        &serde_json::json!({"images":[{"image":image.url}]}).to_string(),
    );
    assert!(fetch_from_url(&client, &release.url).unwrap().is_none());
    assert!(image.request().starts_with("GET / "));
    let image = HttpFixture::new(
        b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nshort".to_vec(),
    );
    let release = HttpFixture::json(
        200,
        &serde_json::json!({"images":[{"image":image.url}]}).to_string(),
    );
    assert!(fetch_from_url(&client, &release.url).is_err());
    assert!(image.request().starts_with("GET / "));
}
