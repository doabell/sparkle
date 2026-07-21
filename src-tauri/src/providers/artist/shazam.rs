use crate::models::{detect_image_mime_type, ImageData};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::time::Duration;

const SEARCH_URL: &str = "https://www.shazam.com/services/amapi/v1/catalog/jp/search";
const USER_AGENT: &str = "SparkleMusicPlayer/0.1.0 (local desktop music player)";

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: SearchResults,
}

#[derive(Deserialize, Default)]
struct SearchResults {
    #[serde(default)]
    artists: ArtistResults,
}

#[derive(Deserialize, Default)]
struct ArtistResults {
    #[serde(default)]
    data: Vec<ArtistResult>,
}

#[derive(Deserialize)]
struct ArtistResult {
    attributes: ArtistAttributes,
}

#[derive(Deserialize)]
struct ArtistAttributes {
    artwork: Option<Artwork>,
}

#[derive(Deserialize)]
struct Artwork {
    url: String,
}

fn artwork_url(template: &str) -> Option<String> {
    if template.trim().is_empty() {
        return None;
    }
    Some(
        template
            .replace("{w}x{h}bb.jpg", "800x800vb.webp")
            .replace("{w}", "800")
            .replace("{h}", "800"),
    )
}

pub fn search_image_urls(title: &str, count: usize) -> Result<Vec<String>, String> {
    if title.trim().is_empty() || count == 0 {
        return Ok(Vec::new());
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(SEARCH_URL)
        .query(&[
            ("term", title),
            ("types", "artists"),
            ("limit", &count.to_string()),
        ])
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Ok(Vec::new());
    }
    let data: SearchResponse = response.json().map_err(|e| e.to_string())?;
    Ok(data
        .results
        .artists
        .data
        .into_iter()
        .filter_map(|artist| artwork_url(&artist.attributes.artwork?.url))
        .take(count)
        .collect())
}

fn download_image(url: &str) -> Result<Option<ImageData>, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client.get(url).send().map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let data = match crate::providers::read_image_response(response) {
        Ok(data) => data,
        Err(_) => return Ok(None),
    };
    if data.len() < 1024 {
        return Ok(None);
    }
    Ok(Some(ImageData {
        source: "shazam".to_string(),
        mime_type: detect_image_mime_type(&data),
        data: Some(data),
    }))
}

pub fn fetch_image_by_title(title: &str) -> Result<Option<ImageData>, String> {
    let Some(url) = search_image_urls(title, 1)?.into_iter().next() else {
        return Ok(None);
    };
    download_image(&url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_apple_artwork_template_to_webp() {
        assert_eq!(
            artwork_url("https://example.test/{w}x{h}bb.jpg").as_deref(),
            Some("https://example.test/800x800vb.webp")
        );
    }
}
