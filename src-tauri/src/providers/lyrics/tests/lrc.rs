use super::*;

#[test]
fn sidecar_lyrics_handle_missing_empty_valid_and_unreadable_content() {
    let root = std::env::temp_dir().join(crate::analytics::new_trace_id("sparkle-lrc-test"));
    fs::create_dir(&root).unwrap();
    let audio = root.join("song.live.flac");
    let lrc = root.join("song.live.lrc");
    let metadata = TrackMetadata {
        file_path: Some(audio.to_string_lossy().into()),
        ..Default::default()
    };
    assert!(fetch(&metadata).unwrap().is_none());
    fs::write(&lrc, " \r\n").unwrap();
    assert!(fetch(&metadata).unwrap().is_none());
    fs::write(&lrc, "[00:01.00]Hello\r\n[00:02.00]World").unwrap();
    let lyrics = fetch(&metadata).unwrap().unwrap();
    assert_eq!(lyrics.source, "lrc");
    assert_eq!(lyrics.plain_text.as_deref(), Some("Hello\nWorld"));
    assert!(lyrics.synced_text.unwrap().contains("[00:01.00]"));
    fs::write(&lrc, [0xff, 0xfe]).unwrap();
    assert!(fetch(&metadata).is_err());
    fs::remove_dir_all(&root).unwrap();
}
