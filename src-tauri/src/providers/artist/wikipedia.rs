use crate::models::{detect_image_mime_type, ArtistInfo, ImageData};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::time::Duration;

// Wikimedia rejects requests without a descriptive User-Agent with 403.
const USER_AGENT: &str = "SparkleMusicPlayer/0.4.0 (https://github.com/doabell/sparkle)";

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

#[derive(Deserialize, Default)]
struct PageImageSearch {
    #[serde(default)]
    query: PageImageQuery,
    error: Option<PageImageError>,
}

#[derive(Deserialize, Default)]
struct PageImageQuery {
    #[serde(default)]
    pages: Vec<PageImage>,
}

#[derive(Deserialize)]
struct PageImage {
    index: Option<u32>,
    thumbnail: Option<WikipediaThumbnail>,
}

#[derive(Deserialize)]
struct PageImageError {
    code: String,
}

fn image_url(src: &str) -> Option<String> {
    let normalized = if src.starts_with("//") {
        format!("https:{src}")
    } else {
        src.to_string()
    };
    let url = reqwest::Url::parse(&normalized).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.path().to_lowercase().ends_with(".svg") {
        return None;
    }
    Some(normalized)
}

/// Keep exact article galleries when available, then search page titles and
/// aliases when the query is not an exact title or the article has no images.
/// The manual chooser can therefore accept names such as "Ikuta Lilas" whose
/// English article is titled "Lilas Ikuta".
pub fn image_urls_by_title(title: &str, lang: &str, count: usize) -> Result<Vec<String>, String> {
    if title.trim().is_empty() || lang.trim().is_empty() || count == 0 {
        return Ok(Vec::new());
    }
    let client = Client::builder()
        // Both requests fit within the chooser's eight-second search budget.
        .timeout(Duration::from_secs(4))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let media_url = format!(
        "https://{}.wikipedia.org/api/rest_v1/page/media-list/{}",
        lang,
        percent_encode(title)
    );
    let search_url = format!("https://{lang}.wikipedia.org/w/api.php");
    image_urls_with_client(&client, &media_url, &search_url, title, count)
}

fn image_urls_with_client(
    client: &Client,
    media_url: &str,
    search_url: &str,
    title: &str,
    count: usize,
) -> Result<Vec<String>, String> {
    // An uncached or unavailable REST article must not prevent the independent
    // Action API name search from returning results.
    let gallery = (|| -> Result<MediaList, String> {
        let response = client
            .get(media_url)
            .timeout(Duration::from_secs(3))
            .send()
            .map_err(|e| e.to_string())?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(MediaList { items: Vec::new() });
        }
        crate::providers::checked_search_response(response)?
            .json::<MediaList>()
            .map_err(|e| e.to_string())
    })();
    let (list, gallery_error) = match gallery {
        Ok(list) => (list, None),
        Err(error) => (MediaList { items: Vec::new() }, Some(error)),
    };
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
        if let Some(url) = image_url(&src) {
            if !urls.contains(&url) {
                urls.push(url);
            }
        }
    }
    if !urls.is_empty() {
        return Ok(urls);
    }

    let response = client
        .get(search_url)
        .query(&[
            ("action", "query"),
            ("format", "json"),
            ("formatversion", "2"),
            ("generator", "search"),
            ("gsrsearch", title),
            ("gsrnamespace", "0"),
            ("gsrlimit", &count.clamp(1, 10).to_string()),
            ("gsrenablerewrites", "1"),
            ("prop", "pageimages"),
            ("piprop", "thumbnail"),
            ("pithumbsize", "800"),
            ("pilicense", "any"),
        ])
        .send()
        .map_err(|e| e.to_string())?;
    let mut result: PageImageSearch = crate::providers::checked_search_response(response)?
        .json()
        .map_err(|e| e.to_string())?;
    if let Some(error) = result.error {
        return Err(format!("Wikipedia search failed ({})", error.code));
    }
    result
        .query
        .pages
        .sort_by_key(|page| page.index.unwrap_or(u32::MAX));
    for page in result.query.pages {
        if let Some(url) = page
            .thumbnail
            .and_then(|thumbnail| image_url(&thumbnail.source))
        {
            if !urls.contains(&url) {
                urls.push(url);
            }
            if urls.len() >= count {
                break;
            }
        }
    }
    if urls.is_empty() {
        if let Some(error) = gallery_error {
            return Err(error);
        }
    }
    Ok(urls)
}

#[cfg(test)]
#[path = "tests/wikipedia.rs"]
mod tests;
