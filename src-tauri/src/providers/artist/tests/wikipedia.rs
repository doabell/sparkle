use super::*;
use crate::test_support::{http_client, HttpFixture};

#[test]
fn manual_images_keep_exact_galleries_and_skip_vector_urls_with_query_strings() {
    let media = HttpFixture::json(
        200,
        r#"{"items":[
        {"type":"image","srcset":[{"src":"//images.test/logo.svg?utm_source=wiki"}]},
        {"type":"audio","srcset":[{"src":"https://images.test/song.ogg"}]},
        {"type":"image","srcset":[{"src":"//images.test/small.jpg"},{"src":"//images.test/portrait.jpg"}]},
        {"type":"image","srcset":[{"src":"//images.test/portrait.jpg"}]}
    ]}"#,
    );
    assert_eq!(
        image_urls_with_client(
            &http_client(),
            &media.url,
            "http://unused.invalid",
            "Artist",
            4
        )
        .unwrap(),
        vec!["https://images.test/portrait.jpg"]
    );
}

#[test]
fn manual_images_search_names_when_the_exact_article_is_missing_or_has_no_images() {
    for (status, body) in [(404, "{}"), (200, r#"{"items":[]}"#), (503, "{}")] {
        let media = HttpFixture::json(status, body);
        let search = HttpFixture::json(
            200,
            r#"{"query":{"pages":[
            {"index":3,"thumbnail":{"source":"https://images.test/related.jpg"}},
            {"index":1,"thumbnail":{"source":"https://images.test/artist.jpg"}},
            {"index":2},
            {"index":4,"thumbnail":{"source":"https://images.test/artist.jpg"}}
        ]}}"#,
        );
        assert_eq!(
            image_urls_with_client(&http_client(), &media.url, &search.url, "Ikuta Lilas", 4)
                .unwrap(),
            vec![
                "https://images.test/artist.jpg",
                "https://images.test/related.jpg"
            ]
        );
        let request = search.request();
        assert!(request.contains("generator=search"));
        assert!(request.contains("gsrsearch=Ikuta+Lilas"));
        assert!(request.contains("prop=pageimages"));
    }
}

#[test]
fn manual_images_distinguish_provider_errors_from_no_matching_images() {
    let media = HttpFixture::json(503, "{}");
    let search = HttpFixture::json(200, r#"{"batchcomplete":true}"#);
    assert!(
        image_urls_with_client(&http_client(), &media.url, &search.url, "Artist", 4)
            .unwrap_err()
            .contains("503")
    );
    for (body, is_error) in [
        (r#"{"error":{"code":"ratelimited"}}"#, true),
        (r#"{"batchcomplete":true}"#, false),
    ] {
        let media = HttpFixture::json(404, "{}");
        let search = HttpFixture::json(200, body);
        let result = image_urls_with_client(&http_client(), &media.url, &search.url, "Artist", 4);
        if is_error {
            assert!(result.unwrap_err().contains("ratelimited"));
        } else {
            assert!(result.unwrap().is_empty());
        }
    }
}

#[test]
fn percent_encode_ascii_and_spaces() {
    assert_eq!(percent_encode("Tyler, The Creator"), "Tyler%2C_The_Creator");
    assert_eq!(percent_encode("AC/DC"), "AC%2FDC");
}

#[test]
fn percent_encode_utf8() {
    // ö = U+00F6 = 0xC3 0xB6 in UTF-8
    assert_eq!(percent_encode("Björk"), "Bj%C3%B6rk");
    // 中 = U+4E2D = 0xE4 0xB8 0xAD
    assert_eq!(percent_encode("中"), "%E4%B8%AD");
}
