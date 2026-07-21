use crate::models::Lyrics;
use crate::providers::lyrics::TrackMetadata;
use std::fs;

pub fn fetch(metadata: &TrackMetadata) -> Result<Option<Lyrics>, String> {
    let file_path = match metadata.file_path.as_deref() {
        Some(p) => p,
        None => return Ok(None),
    };

    let lrc_path = super::lrc_path_for_track(file_path);
    if !lrc_path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(&lrc_path).map_err(|e| e.to_string())?;
    if text.trim().is_empty() {
        return Ok(None);
    }

    let synced_text = Some(text.clone());
    let plain_text = Some(super::strip_lrc_timestamps(&text));

    Ok(Some(Lyrics {
        source: "lrc".to_string(),
        synced_text,
        plain_text,
    }))
}
