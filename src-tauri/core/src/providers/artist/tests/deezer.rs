use super::*;
use crate::test_support::{http_client, HttpFixture};

#[test]
fn search_keeps_ranking_prefers_large_images_and_skips_placeholders_and_duplicates() {
    let search = HttpFixture::json(
        200,
        r#"{"data":[
        {"name":"Nana Mizuki","picture_xl":"https://cdn.test/artist/portrait/1000.jpg","picture_big":"https://cdn.test/small.jpg"},
        {"picture_xl":"https://cdn.test/artist//1000.jpg","picture_big":"https://cdn.test/artist//500.jpg"},
        {"picture_xl":"https://cdn.test/artist/portrait/1000.jpg"},
        {"picture_xl":" ","picture_big":"https://cdn.test/fallback.jpg"},
        {"picture_xl":"file:///tmp/private.jpg"},
        {"picture_medium":"https://cdn.test/medium.jpg"},
        {"picture_small":"https://cdn.test/last.jpg"}
    ]}"#,
    );
    assert_eq!(
        search_with_client(&http_client(), &search.url, " 水樹 奈々 ", 3).unwrap(),
        vec![
            "https://cdn.test/artist/portrait/1000.jpg",
            "https://cdn.test/fallback.jpg",
            "https://cdn.test/medium.jpg"
        ]
    );
    let request = search.request();
    assert!(request.contains("q=%E6%B0%B4%E6%A8%B9+%E5%A5%88%E3%80%85"));
    assert!(request.contains("limit=3"));
    assert!(!request.contains("api_key"));
}

#[test]
fn search_distinguishes_api_errors_http_errors_and_empty_matches() {
    for (status, body, expected_error) in [
        (200, r#"{"data":[]}"#, None),
        (
            200,
            r#"{"error":{"code":4,"message":"Quota limit exceeded"}}"#,
            Some("Quota limit exceeded"),
        ),
        (429, "{}", Some("429")),
        (503, "{}", Some("503")),
        (200, "{}", Some("invalid search response")),
        (200, "not json", Some("invalid search JSON")),
    ] {
        let search = HttpFixture::json(status, body);
        let result = search_with_client(&http_client(), &search.url, "Artist", 100);
        match expected_error {
            Some(expected) => {
                let error = result.unwrap_err();
                assert!(error.contains(expected), "expected {expected}: {error}");
            }
            None => assert!(result.unwrap().is_empty()),
        }
        assert!(search.request().contains("limit=25"));
    }
    assert!(search_image_urls(" ", 5).unwrap().is_empty());
    assert!(search_image_urls("Artist", 0).unwrap().is_empty());
    assert!(fetch_image_by_title(" ").unwrap().is_none());
}

#[test]
fn downloads_preserve_image_bytes_and_reject_error_pages() {
    let mut data = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 2)
        .write_to(&mut data, image::ImageFormat::Png)
        .unwrap();
    let data = data.into_inner();
    let fixture = HttpFixture::response(200, "image/png", &data);
    let image = download_image(&http_client(), &fixture.url)
        .unwrap()
        .unwrap();
    assert_eq!(image.source, "deezer");
    assert_eq!(image.mime_type, "image/png");
    assert_eq!(image.data.unwrap(), data);
    let fixture = HttpFixture::response(200, "text/html", b"<html>temporarily unavailable</html>");
    assert!(download_image(&http_client(), &fixture.url)
        .unwrap()
        .is_none());
    let fixture = HttpFixture::response(403, "text/plain", b"Forbidden");
    assert!(download_image(&http_client(), &fixture.url)
        .unwrap_err()
        .contains("403"));
}
