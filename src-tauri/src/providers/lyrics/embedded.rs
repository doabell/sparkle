use crate::models::Lyrics;
use crate::providers::lyrics::TrackMetadata;

pub fn fetch(metadata: &TrackMetadata) -> Result<Option<Lyrics>, String> {
    match metadata.embedded_lyrics.as_deref() {
        Some(text) if !text.trim().is_empty() => {
            let synced_text = Some(text.to_string());
            let plain_text = Some(super::strip_lrc_timestamps(text));
            Ok(Some(Lyrics {
                source: "embedded".to_string(),
                synced_text,
                plain_text,
            }))
        }
        _ => Ok(None),
    }
}
