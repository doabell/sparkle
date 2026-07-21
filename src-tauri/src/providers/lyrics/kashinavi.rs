// Portions adapted from mb_KashiNaviLyricsPlugin:
// https://github.com/noriokun4649/mb_KashiNaviLyricsPlugin
// MIT-licensed source, modified substantially for Sparkle. See
// THIRD_PARTY_NOTICES.md.

use crate::models::Lyrics;
use regex::Regex;
use reqwest::blocking::Client;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const SEARCH_URL: &str = "https://kashinavi.com/search.php";
const LYRICS_URL_PREFIX: &str = "https://kashinavi.com/lyrics/";

fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        )
        .build()
        .map_err(|e| e.to_string())
}

fn decode_shift_jis(bytes: &[u8]) -> String {
    let (text, _, _) = encoding_rs::SHIFT_JIS.decode(bytes);
    text.into_owned()
}

fn get_text(client: &Client, url: &str) -> Result<String, String> {
    let response = client
        .get(url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let bytes = response.bytes().map_err(|e| e.to_string())?;
    Ok(decode_shift_jis(&bytes))
}

#[derive(Clone, Debug)]
struct SearchResult {
    id: String,
    title: Option<String>,
    artist: Option<String>,
}

fn search_results(client: &Client, query: &str) -> Result<Vec<SearchResult>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let response = client
        .get(SEARCH_URL)
        .query(&[
            ("r", "kyoku"),
            ("search", query),
            ("m", "bubun"),
            ("start", "1"),
        ])
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let bytes = response.bytes().map_err(|e| e.to_string())?;
    let html = decode_shift_jis(&bytes);
    if html.contains("該当データがありませんでした。") {
        return Ok(Vec::new());
    }

    let result_pattern = Regex::new(
        r#"(?is)<a\s+href=["']/lyrics/(\d+)/["'][^>]*>\s*([^<]*?)\s*</a>\s*</td>\s*<td[^>]*>\s*<a\s+href=["']/artist/[^"']+["'][^>]*>\s*([^<]*?)\s*</a>"#,
    )
    .map_err(|e| e.to_string())?;
    let id_pattern = Regex::new(r#"href=["']/lyrics/(\d+)"#).map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    for capture in result_pattern.captures_iter(&html) {
        let result = SearchResult {
            id: capture[1].to_string(),
            title: Some(clean_html_fragment(&capture[2])),
            artist: Some(clean_html_fragment(&capture[3])),
        };
        if !results
            .iter()
            .any(|existing: &SearchResult| existing.id == result.id)
        {
            results.push(result);
        }
    }
    for capture in id_pattern.captures_iter(&html) {
        let id = capture[1].to_string();
        if !results.iter().any(|existing| existing.id == id) {
            results.push(SearchResult {
                id,
                title: None,
                artist: None,
            });
        }
    }
    Ok(results)
}

fn decode_html_entities(text: &str) -> String {
    let named = text
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'");
    let numeric = Regex::new(r"&#(?:x([0-9a-fA-F]+)|([0-9]+));").unwrap();
    numeric
        .replace_all(&named, |captures: &regex::Captures<'_>| {
            let value = captures
                .get(1)
                .and_then(|hex| u32::from_str_radix(hex.as_str(), 16).ok())
                .or_else(|| {
                    captures
                        .get(2)
                        .and_then(|decimal| decimal.as_str().parse().ok())
                });
            value
                .and_then(char::from_u32)
                .map(|character| character.to_string())
                .unwrap_or_else(|| captures[0].to_string())
        })
        .into_owned()
}

fn clean_html_fragment(fragment: &str) -> String {
    let with_newlines = Regex::new(r"(?i)<br\s*/?>|</p>")
        .unwrap()
        .replace_all(fragment, "\n");
    let without_tags = Regex::new(r"(?s)<[^>]+>")
        .unwrap()
        .replace_all(&with_newlines, "");
    decode_html_entities(&without_tags)
        .replace('\r', "")
        .trim()
        .to_string()
}

fn extract_lyrics(html: &str) -> Option<String> {
    let line_pattern =
        Regex::new(r#"(?is)<div[^>]*class=[\"']line-jp[\"'][^>]*>(.*?)</div>"#).unwrap();
    let lines: Vec<String> = line_pattern
        .captures_iter(html)
        .map(|capture| clean_html_fragment(&capture[1]))
        .filter(|line| !line.is_empty())
        .collect();
    if !lines.is_empty() {
        return Some(lines.join("\n"));
    }

    None
}

fn fetch_by_id(client: &Client, id: &str) -> Result<Option<Lyrics>, String> {
    let html = get_text(client, &format!("{}{}/", LYRICS_URL_PREFIX, id))?;
    let Some(text) = extract_lyrics(&html) else {
        return Ok(None);
    };
    Ok(Some(Lyrics {
        source: "kashinavi".to_string(),
        synced_text: Some(text.clone()),
        plain_text: Some(text),
    }))
}

fn normalized_match(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn fetch_kashinavi_lyrics_blocking(
    title: &str,
    artist: &str,
) -> Result<Option<Lyrics>, String> {
    let client = client()?;
    let results = search_results(&client, title)?;
    let title_match = normalized_match(title);
    let artist_match = normalized_match(artist);
    let mut ordered = results.clone();
    ordered.sort_by_key(|result| {
        let exact_title = result
            .title
            .as_deref()
            .map(normalized_match)
            .is_some_and(|value| value == title_match);
        let exact_artist = result
            .artist
            .as_deref()
            .map(normalized_match)
            .is_some_and(|value| value == artist_match);
        if exact_title && exact_artist {
            0
        } else if exact_title {
            1
        } else {
            2
        }
    });
    for result in ordered.into_iter().take(5) {
        if let Some(lyrics) = fetch_by_id(&client, &result.id)? {
            return Ok(Some(lyrics));
        }
    }
    Ok(None)
}

pub fn fetch_candidates_for_query(query: &str, count: usize) -> Result<Vec<Lyrics>, String> {
    let client = client()?;
    let mut candidates = Vec::new();
    for result in search_results(&client, query)?.into_iter().take(count + 3) {
        if candidates.len() >= count {
            break;
        }
        if let Some(lyrics) = fetch_by_id(&client, &result.id)? {
            candidates.push(lyrics);
        }
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_current_line_markup() {
        let html = r#"
            <div class="line-jp">夢ならばどれほどよかったでしょう</div>
            <div class="line-ro">yume naraba</div>
            <div class="line-jp">未だにあなたのことを夢にみる</div>
        "#;
        assert_eq!(
            extract_lyrics(html).as_deref(),
            Some("夢ならばどれほどよかったでしょう\n未だにあなたのことを夢にみる")
        );
    }

    #[test]
    fn extracts_current_search_ids() {
        let html = r#"
            <a href="/lyrics/108265/">current</a>
            <a href="/lyrics/108265/">duplicate</a>
        "#;
        let pattern = Regex::new(r#"href=["']/lyrics/(\d+)"#).unwrap();
        let ids: Vec<_> = pattern
            .captures_iter(html)
            .map(|capture| capture[1].to_string())
            .collect();
        assert_eq!(ids, vec!["108265", "108265"]);
    }
}
