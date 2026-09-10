use crate::models::{detect_image_mime_type, ImageData};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::collections::HashSet;
use std::time::Duration;

const SEARCH_URL: &str = "https://api.deezer.com/search/artist";
const USER_AGENT: &str = concat!(
    "SparkleMusicPlayer/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/doabell/sparkle)"
);

#[derive(Deserialize)]
struct SearchResponse {
    data: Option<Vec<Artist>>,
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    code: i64,
    message: String,
}

#[derive(Deserialize)]
struct Artist {
    picture_xl: Option<String>,
    picture_big: Option<String>,
    picture_medium: Option<String>,
    picture_small: Option<String>,
}

fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(6))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())
}

fn usable_image_url(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "https" | "http")
            // Deezer returns a generic silhouette when the artist image hash
            // is empty. Let another result or provider supply a real image.
            && !url.path().contains("/artist//")
    })
}

fn search_with_client(
    client: &Client,
    endpoint: &str,
    title: &str,
    count: usize,
) -> Result<Vec<String>, String> {
    if title.trim().is_empty() || count == 0 {
        return Ok(Vec::new());
    }
    let limit = count.min(25);
    let response = client
        .get(endpoint)
        .header("Accept", "application/json")
        .query(&[
            ("q", title.trim().to_string()),
            ("limit", limit.to_string()),
        ])
        .send()
        .map_err(|e| e.to_string())?;
    let data: SearchResponse = crate::providers::checked_search_response(response)?
        .json()
        .map_err(|e| format!("Deezer returned invalid search JSON: {e}"))?;
    // Deezer can report errors inside an HTTP 200 response.
    if let Some(error) = data.error {
        return Err(format!("Deezer: {} (code {})", error.message, error.code));
    }
    let artists = data
        .data
        .ok_or("Deezer returned an invalid search response")?;
    let mut seen = HashSet::new();
    Ok(artists
        .into_iter()
        .filter_map(|artist| {
            [
                artist.picture_xl,
                artist.picture_big,
                artist.picture_medium,
                artist.picture_small,
            ]
            .into_iter()
            .flatten()
            .map(|url| url.trim().to_string())
            .find(|url| usable_image_url(url))
        })
        .filter(|url| seen.insert(url.clone()))
        .take(limit)
        .collect())
}

/// Public artist search, including Deezer's name and language aliases.
/// No API key is needed. Call without holding the database lock.
pub fn search_image_urls(title: &str, count: usize) -> Result<Vec<String>, String> {
    search_with_client(&client()?, SEARCH_URL, title, count)
}

fn download_image(client: &Client, url: &str) -> Result<Option<ImageData>, String> {
    let response = client.get(url).send().map_err(|e| e.to_string())?;
    let data = crate::providers::read_image_response(crate::providers::checked_search_response(
        response,
    )?)?;
    // Reject non-image responses before they enter the cache.
    if image::guess_format(&data).is_err() {
        return Ok(None);
    }
    Ok(Some(ImageData {
        source: "deezer".to_string(),
        mime_type: detect_image_mime_type(&data),
        data: Some(data),
    }))
}

/// Uses the highest ranked artist with a portrait. The chooser exposes more
/// matches for users who want to select and keep a different image as Custom.
pub fn fetch_image_by_title(title: &str) -> Result<Option<ImageData>, String> {
    let client = client()?;
    let urls = search_with_client(&client, SEARCH_URL, title, 5)?;
    match urls.first() {
        Some(url) => download_image(&client, url),
        None => Ok(None),
    }
}

#[cfg(test)]
#[path = "tests/deezer.rs"]
mod tests;
