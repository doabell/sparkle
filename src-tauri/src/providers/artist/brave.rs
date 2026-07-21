use crate::models::{detect_image_mime_type, ImageData};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::time::Duration;

const USER_AGENT: &str = "SparkleMusicPlayer/0.1.0 (local desktop music player)";

#[derive(Deserialize, Debug)]
struct BraveImageResponse {
    #[serde(default)]
    results: Vec<BraveImageResult>,
}

#[derive(Deserialize, Debug)]
struct BraveImageResult {
    #[serde(default)]
    properties: Option<BraveImageProperties>,
    #[serde(default)]
    thumbnail: Option<BraveThumbnail>,
}

#[derive(Deserialize, Debug)]
struct BraveImageProperties {
    url: String,
}

#[derive(Deserialize, Debug)]
struct BraveThumbnail {
    src: String,
}

/// Maps a Wikipedia-style language code to a Brave search_lang value, so
/// image results come from the same locale as the artist's Wikipedia page.
fn brave_search_lang(lang: &str) -> &str {
    match lang {
        "zh" => "zh-hans",
        "zh-hant" | "zh-tw" | "zh-hk" => "zh-hant",
        "pt-br" => "pt-br",
        other => other,
    }
}

/// Searches Brave Image Search for "{title} artist" and returns candidate
/// image URLs. Requires a Brave Search API key; returns an empty vec when
/// the key is missing or the search fails. Network I/O — call WITHOUT
/// holding the DB lock.
pub fn search_image_urls(
    title: &str,
    api_key: &str,
    count: usize,
    lang: &str,
) -> Result<Vec<String>, String> {
    if title.trim().is_empty() || api_key.trim().is_empty() {
        return Ok(Vec::new());
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get("https://api.search.brave.com/res/v1/images/search")
        .header("X-Subscription-Token", api_key.trim())
        .header("Accept", "application/json")
        .query(&[
            ("q", format!("{} artist", title)),
            ("count", count.to_string()),
            ("safesearch", "strict".to_string()),
            ("search_lang", brave_search_lang(lang).to_string()),
        ])
        .send()
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Ok(Vec::new());
    }

    let data: BraveImageResponse = response.json().map_err(|e| e.to_string())?;
    Ok(data
        .results
        .iter()
        .filter_map(|r| {
            r.properties
                .as_ref()
                .map(|p| p.url.clone())
                .or_else(|| r.thumbnail.as_ref().map(|t| t.src.clone()))
        })
        .filter(|u| !u.trim().is_empty())
        .collect())
}

fn download_image(url: &str) -> Result<Option<ImageData>, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let response = match client.get(url).send() {
        Ok(response) if response.status().is_success() => response,
        _ => return Ok(None),
    };
    let data = match crate::providers::read_image_response(response) {
        Ok(data) => data,
        Err(_) => return Ok(None),
    };
    if data.len() < 1024 {
        return Ok(None);
    }
    let mime_type = detect_image_mime_type(&data);
    Ok(Some(ImageData {
        source: "brave".to_string(),
        data: Some(data),
        mime_type,
    }))
}

/// Searches Brave Image Search for "{title} artist" and downloads the first
/// usable result. Requires a Brave Search API key; returns Ok(None) when the
/// key is missing or nothing usable is found. Network I/O — call WITHOUT
/// holding the DB lock.
pub fn fetch_image_by_title(
    title: &str,
    api_key: &str,
    lang: &str,
) -> Result<Option<ImageData>, String> {
    let urls = search_image_urls(title, api_key, 5, lang)?;
    // Try up to three candidates — some hosts block hotlinking.
    for url in urls.iter().take(3) {
        if let Some(image) = download_image(url)? {
            return Ok(Some(image));
        }
    }
    Ok(None)
}
