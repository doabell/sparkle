use crate::models::{detect_image_mime_type, ArtistInfo, ImageData};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::time::Duration;

// Wikimedia rejects requests without a descriptive User-Agent with 403.
const USER_AGENT: &str = "SparkleMusicPlayer/0.1.0 (local desktop music player)";

#[derive(Deserialize, Debug)]
struct WikipediaSummary {
    #[serde(default)]
    extract: Option<String>,
    #[serde(default)]
    thumbnail: Option<WikipediaThumbnail>,
}

#[derive(Deserialize, Debug)]
struct WikipediaThumbnail {
    source: String,
}

fn fetch_summary_raw(title: &str, lang: &str) -> Result<Option<WikipediaSummary>, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!(
        "https://{}.wikipedia.org/api/rest_v1/page/summary/{}",
        lang,
        percent_encode(title)
    );
    let response = client.get(&url).send().map_err(|e| e.to_string())?;
    if response.status().is_success() {
        let data: WikipediaSummary = response.json().map_err(|e| e.to_string())?;
        Ok(Some(data))
    } else {
        Ok(None)
    }
}

fn percent_encode(input: &str) -> String {
    let mut out = String::new();
    for c in input.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            ' ' => out.push('_'),
            _ => {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    out
}

/// The first language edition (in list order) that has a page for the title.
/// Artist info and image both derive from this page so they always describe
/// the same subject.
fn fetch_page(title: &str, langs: &[String]) -> Result<Option<(String, WikipediaSummary)>, String> {
    for lang in langs {
        let lang = lang.trim();
        if lang.is_empty() {
            continue;
        }
        if let Some(summary) = fetch_summary_raw(title, lang)? {
            return Ok(Some((lang.to_string(), summary)));
        }
    }
    Ok(None)
}

/// Tries each language in order and returns the summary from the first page
/// that exists. The returned source records which language matched.
pub fn fetch_summary_by_title(title: &str, langs: &[String]) -> Result<Option<ArtistInfo>, String> {
    if title.trim().is_empty() {
        return Ok(None);
    }

    match fetch_page(title, langs)? {
        Some((lang, summary)) => Ok(Some(ArtistInfo {
            source: format!("wikipedia:{}", lang),
            summary: summary.extract.filter(|s| !s.trim().is_empty()),
        })),
        None => Ok(None),
    }
}

/// Tries each language in order and downloads the thumbnail from the first
/// page that exists (the same page the bio comes from).
pub fn fetch_image_by_title(title: &str, langs: &[String]) -> Result<Option<ImageData>, String> {
    if title.trim().is_empty() {
        return Ok(None);
    }

    let (lang, summary) = match fetch_page(title, langs)? {
        Some(found) => found,
        None => return Ok(None),
    };

    let thumbnail_url = match summary.thumbnail {
        Some(t) if !t.source.trim().is_empty() => t.source,
        _ => return Ok(None),
    };

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(&thumbnail_url)
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Ok(None);
    }

    let data = match crate::providers::read_image_response(response) {
        Ok(data) => data,
        Err(_) => return Ok(None),
    };
    let mime_type = detect_image_mime_type(&data);
    Ok(Some(ImageData {
        source: format!("wikipedia:{}", lang),
        data: Some(data),
        mime_type,
    }))
}

#[derive(Deserialize, Debug)]
struct MediaList {
    #[serde(default)]
    items: Vec<MediaListItem>,
}

#[derive(Deserialize, Debug)]
struct MediaListItem {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    srcset: Vec<MediaSrc>,
}

#[derive(Deserialize, Debug)]
struct MediaSrc {
    src: String,
}

/// Lists up to `count` image URLs from the article's media list without
/// downloading anything — for the chooser, where the webview renders the
/// candidates itself. Skips SVGs (icons, logos, maps) in favor of photos.
pub fn image_urls_by_title(title: &str, lang: &str, count: usize) -> Result<Vec<String>, String> {
    if title.trim().is_empty() || lang.trim().is_empty() {
        return Ok(Vec::new());
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!(
        "https://{}.wikipedia.org/api/rest_v1/page/media-list/{}",
        lang,
        percent_encode(title)
    );
    let response = client.get(&url).send().map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Ok(Vec::new());
    }
    let list: MediaList = response.json().map_err(|e| e.to_string())?;
    let mut urls = Vec::new();
    for item in &list.items {
        if urls.len() >= count {
            break;
        }
        if item.kind != "image" {
            continue;
        }
        // Largest src in the srcset is last.
        let Some(src) = item.srcset.last().map(|s| s.src.clone()) else {
            continue;
        };
        if src.to_lowercase().ends_with(".svg") {
            continue;
        }
        urls.push(if src.starts_with("//") {
            format!("https:{src}")
        } else {
            src
        });
    }
    Ok(urls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encode_ascii_and_spaces() {
        assert_eq!(percent_encode("Tyler, The Creator"), "Tyler%2C_The_Creator");
        assert_eq!(percent_encode("AC/DC"), "AC%2FDC");
    }

    #[test]
    fn percent_encode_utf8() {
        // ö = U+00F6 = 0xC3 0xB6 in UTF-8
        assert_eq!(percent_encode("Björk"), "Bj%C3%B6rk");
        // 中 = U+4E2D = 0xE4 0xB8 0xAD
        assert_eq!(percent_encode("中"), "%E4%B8%AD");
    }
}
