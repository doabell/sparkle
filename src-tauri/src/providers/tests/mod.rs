use super::*;
use crate::test_support::{http_client, HttpFixture};

#[test]
fn image_downloads_accept_bounded_bodies_and_reject_declared_or_streamed_overflow() {
    let peer = HttpFixture::response(200, "image/png", b"image bytes");
    let response = http_client().get(&peer.url).send().unwrap();
    assert_eq!(read_image_response(response).unwrap(), b"image bytes");
    assert!(peer.request().starts_with("GET / HTTP/1.1"));
    let peer = HttpFixture::new(
        b"HTTP/1.1 200 OK\r\nContent-Length: 20971521\r\nConnection: close\r\n\r\n".to_vec(),
    );
    assert!(
        read_image_response(http_client().get(&peer.url).send().unwrap())
            .unwrap_err()
            .contains("too large")
    );
    let mut response = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
    response.resize(response.len() + 20 * 1024 * 1024 + 1, 0);
    let peer = HttpFixture::new(response);
    assert!(
        read_image_response(http_client().get(&peer.url).send().unwrap())
            .unwrap_err()
            .contains("too large")
    );
}

#[test]
fn image_download_reports_truncation_instead_of_caching_partial_bytes() {
    let peer = HttpFixture::new(
        b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nshort".to_vec(),
    );
    assert!(read_image_response(http_client().get(&peer.url).send().unwrap()).is_err());
}
