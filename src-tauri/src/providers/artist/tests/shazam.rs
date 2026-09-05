use super::*;

#[test]
fn converts_apple_artwork_template_to_webp() {
    assert_eq!(
        artwork_url("https://example.test/{w}x{h}bb.jpg").as_deref(),
        Some("https://example.test/800x800vb.webp")
    );
}
