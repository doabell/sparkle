use crate::models::{detect_image_mime_type, ImageData};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::time::Duration;

// DuckDuckGo has no official image API; this uses the same i.js endpoint
// the web frontend calls, keyed by a vqd token from the search page. No API
// key required, but the endpoint is undocumented and may rate-limit.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

#[derive(Deserialize, Debug)]
struct DdgImageResponse {
    #[serde(default)]
    results: Vec<DdgImageResult>,
}

#[derive(Deserialize, Debug)]
struct DdgImageResult {
    #[serde(default)]
    image: String,
}

fn extract_vqd(html: &str) -> Option<String> {
    for needle in ["vqd=\"", "vqd='"] {
        if let Some(start) = html.find(needle) {
            let rest = &html[start + needle.len()..];
            let end = rest.find(['"', '\''])?;
            return Some(rest[..end].to_string());
        }
    }
    None
}

pub fn search_image_urls(title: &str, count: usize) -> Result<Vec<String>, String> {
    if title.trim().is_empty() {
        return Ok(Vec::new());
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;

    let query = format!("{} artist", title);
    let html = client
        .get("https://duckduckgo.com/")
        .query(&[("q", query.as_str())])
        .send()
        .and_then(|r| r.text())
        .map_err(|e| e.to_string())?;
    let Some(vqd) = extract_vqd(&html) else {
        return Ok(Vec::new());
    };

    let response = client
        .get("https://duckduckgo.com/i.js")
        .header("Referer", "https://duckduckgo.com/")
        .query(&[
            ("l", "us-en"),
            ("o", "json"),
            ("q", query.as_str()),
            ("vqd", vqd.as_str()),
            ("p", "1"),
        ])
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Ok(Vec::new());
    }
    let data: DdgImageResponse = response.json().map_err(|e| e.to_string())?;
    Ok(data
        .results
        .into_iter()
        .map(|r| r.image)
        .filter(|u| !u.trim().is_empty())
        .take(count * 2)
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
        source: "duckduckgo".to_string(),
        data: Some(data),
        mime_type,
    }))
}

/// Downloads the first usable DuckDuckGo image for the provider flow.
pub fn fetch_image_by_title(title: &str) -> Result<Option<ImageData>, String> {
    let urls = search_image_urls(title, 5)?;
    for url in urls.iter().take(3) {
        if let Some(image) = download_image(url)? {
            return Ok(Some(image));
        }
    }
    Ok(None)
}
