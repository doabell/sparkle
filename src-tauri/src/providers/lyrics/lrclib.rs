use crate::models::Lyrics;
use crate::providers::lyrics::TrackMetadata;
use reqwest::blocking::Client;
use reqwest::Url;
use serde::Deserialize;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct LrclibResult {
    #[serde(default)]
    synced_lyrics: Option<String>,
    #[serde(default)]
    plain_lyrics: Option<String>,
}

pub fn fetch(metadata: &TrackMetadata) -> Result<Option<Lyrics>, String> {
    if metadata.title.is_none() {
        return Ok(None);
    }
    fetch_with_client(metadata, &http_client()?, "https://lrclib.net/api/get")
}

fn fetch_with_client(
    metadata: &TrackMetadata,
    client: &Client,
    endpoint: &str,
) -> Result<Option<Lyrics>, String> {
    let title = match metadata.title.as_deref() {
        Some(t) => t,
        None => return Ok(None),
    };
    let artist = metadata.artist.as_deref().unwrap_or("");

    let url = Url::parse_with_params(endpoint, &[("artist_name", artist), ("track_name", title)])
        .map_err(|e| e.to_string())?;

    let response = client.get(url.as_str()).send().map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let data: LrclibResult = response.json().map_err(|e| e.to_string())?;

    match data.synced_lyrics {
        Some(synced) if !synced.trim().is_empty() => {
            let plain_text = data.plain_lyrics.filter(|p| !p.trim().is_empty());
            Ok(Some(Lyrics {
                source: "lrclib".to_string(),
                synced_text: Some(synced),
                plain_text,
            }))
        }
        _ => Ok(None),
    }
}

/// Free-text search for the manual lyrics picker.
pub fn fetch_candidates(query: &str, count: usize) -> Result<Vec<Lyrics>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    candidates_with_client(
        query,
        count,
        &http_client()?,
        "https://lrclib.net/api/search",
    )
}

fn candidates_with_client(
    query: &str,
    count: usize,
    client: &Client,
    endpoint: &str,
) -> Result<Vec<Lyrics>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let url =
        Url::parse_with_params(endpoint, &[("q", query.trim())]).map_err(|e| e.to_string())?;
    let response = client.get(url.as_str()).send().map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Ok(Vec::new());
    }
    let data: Vec<LrclibResult> = response.json().map_err(|e| e.to_string())?;
    Ok(data
        .into_iter()
        .filter(|item| {
            item.synced_lyrics
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        })
        .take(count)
        .map(|item| Lyrics {
            source: "lrclib".to_string(),
            synced_text: item.synced_lyrics,
            plain_text: item.plain_lyrics,
        })
        .collect())
}

#[cfg(test)]
#[path = "tests/lrclib.rs"]
mod tests;
