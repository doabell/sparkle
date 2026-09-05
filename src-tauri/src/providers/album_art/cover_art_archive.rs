use crate::models::{detect_image_mime_type, ImageData};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::time::Duration;

// Cover Art Archive serves MusicBrainz release art; a descriptive UA keeps
// them happy.
const USER_AGENT: &str = "SparkleMusicPlayer/0.1.0 (local desktop music player)";

#[derive(Deserialize, Debug)]
struct CoverArtArchiveResponse {
    #[serde(default)]
    images: Vec<CoverArtImage>,
}

#[derive(Deserialize, Debug)]
struct CoverArtImage {
    image: String,
    #[serde(default)]
    front: bool,
}

/// Fetches the front cover for a MusicBrainz release id. Performs network
/// I/O — call WITHOUT holding the DB lock.
pub fn fetch_by_mbid(mbid: &str) -> Result<Option<ImageData>, String> {
    if mbid.trim().is_empty() {
        return Ok(None);
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("https://coverartarchive.org/release/{}", mbid);
    fetch_from_url(&client, &url)
}

fn fetch_from_url(client: &Client, url: &str) -> Result<Option<ImageData>, String> {
    let response = client.get(url).send().map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Ok(None);
    }

    let data: CoverArtArchiveResponse = response.json().map_err(|e| e.to_string())?;
    let image_url = data
        .images
        .iter()
        .find(|img| img.front)
        .or_else(|| data.images.first())
        .map(|img| img.image.clone())
        .ok_or_else(|| "no images in cover art archive response".to_string())?;

    let image_response = client.get(&image_url).send().map_err(|e| e.to_string())?;
    if !image_response.status().is_success() {
        return Ok(None);
    }

    let data = crate::providers::read_image_response(image_response)?;
    let mime_type = detect_image_mime_type(&data);
    Ok(Some(ImageData {
        source: "cover_art_archive".to_string(),
        data: Some(data),
        mime_type,
    }))
}

#[cfg(test)]
#[path = "tests/cover_art_archive.rs"]
mod tests;
