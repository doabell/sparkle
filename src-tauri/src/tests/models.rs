use super::*;

#[test]
fn repeat_modes_cycle_and_match_the_frontend_wire_contract() {
    let mut mode = RepeatMode::default();
    for expected in ["off", "all", "one", "off"] {
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, format!("\"{expected}\""));
        assert_eq!(serde_json::from_str::<RepeatMode>(&json).unwrap(), mode);
        mode = mode.next();
    }
}

#[test]
fn image_signatures_handle_short_unknown_and_supported_formats() {
    for (bytes, mime) in [
        (&b"\x89PNG\r\n\x1a\n"[..], "image/png"),
        (&b"\xff\xd8\xff"[..], "image/jpeg"),
        (&b"GIF87a"[..], "image/gif"),
        (&b"GIF89a"[..], "image/gif"),
        (&b"RIFF1234WEBP"[..], "image/webp"),
        (&b"BM"[..], "image/bmp"),
        (&b"RIFF"[..], "image/jpeg"),
        (&b"RIFF1234WAVE"[..], "image/jpeg"),
        (&b""[..], "image/jpeg"),
    ] {
        assert_eq!(detect_image_mime_type(bytes), mime);
    }
    let missing = CachedImage::none();
    assert_eq!(missing.source, "none");
    assert_eq!(missing.mime_type, "image/jpeg");
    assert!(missing.file_path.is_none());
}
