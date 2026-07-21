// Portions adapted from MusicBee-QQLyrics, which is derived from
// MusicBee-NeteaseLyrics:
// https://github.com/mslxl/MusicBee-QQLyrics
// The adapted portions are licensed under Apache-2.0 and have been modified
// substantially for Sparkle. See THIRD_PARTY_NOTICES.md.

use crate::models::Lyrics;
use base64::Engine;
use regex::Regex;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())
}

const SEARCH_URL: &str = "https://c.y.qq.com/splcloud/fcgi-bin/smartbox_new.fcg";
const LYRIC_URL: &str = "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg";

#[derive(Deserialize, Debug)]
struct SearchResponse {
    code: i32,
    #[serde(default)]
    data: Option<SearchData>,
}

#[derive(Deserialize, Debug)]
struct SearchData {
    #[serde(default)]
    song: Option<SearchSong>,
}

#[derive(Deserialize, Debug)]
struct SearchSong {
    #[serde(default)]
    itemlist: Option<Vec<SearchItem>>,
}

#[derive(Deserialize, Debug, Clone)]
struct SearchItem {
    name: String,
    mid: String,
}

#[derive(Deserialize, Debug, Default)]
struct LyricResult {
    #[serde(default)]
    lyric: String,
    #[serde(default)]
    trans: Option<String>,
    code: i32,
}

fn query(s: &str) -> Result<Vec<SearchItem>, String> {
    let client = http_client()?;
    let response = client
        .get(SEARCH_URL)
        .query(&[
            ("format", "json"),
            ("inCharset", "utf-8"),
            ("outCharset", "utf-8"),
            ("platform", "yqq.json"),
            ("key", s),
        ])
        .header("Referer", "https://c.y.qq.com/")
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Ok(Vec::new());
    }
    let data: SearchResponse = response.json().map_err(|e| e.to_string())?;
    if data.code != 0 {
        return Ok(Vec::new());
    }
    Ok(data
        .data
        .and_then(|d| d.song)
        .and_then(|s| s.itemlist)
        .unwrap_or_default())
}

fn decode_qq_lyric(s: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| e.to_string())?;
    String::from_utf8(bytes.clone())
        .map_err(|e| e.to_string())
        .or_else(|_| {
            let (cow, _, _) = encoding_rs::GB18030.decode(&bytes);
            Ok(cow.into_owned())
        })
}

fn request_lyric(mid: &str) -> Result<LyricResult, String> {
    let client = http_client()?;
    let response = client
        .get(LYRIC_URL)
        .query(&[
            ("format", "json"),
            ("inCharset", "utf-8"),
            ("outCharset", "utf-8"),
            ("notice", "0"),
            ("platform", "yqq.json"),
            ("needNewCode", "1"),
            ("uin", "0"),
            ("loginUin", "0"),
            ("songmid", mid),
        ])
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:104.0) Gecko/20100101 Firefox/104.0",
        )
        .header("Accept", "application/json")
        .header("Referer", "https://y.qq.com/")
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err("lyric request failed".to_string());
    }
    let mut data: LyricResult = response.json().map_err(|e| e.to_string())?;
    if data.code != 0 {
        return Err("lyric request failed".to_string());
    }
    data.lyric = decode_qq_lyric(&data.lyric)?;
    if let Some(ref mut t) = data.trans {
        *t = decode_qq_lyric(t)?;
    }
    Ok(data)
}

fn remove_feat(name: &str) -> String {
    Regex::new(r"\s*\(feat.+\)")
        .unwrap()
        .replace_all(name, "")
        .to_string()
}

fn remove_leading_number(name: &str) -> String {
    Regex::new(r"^\d+\.?\s*")
        .unwrap()
        .replace_all(name, "")
        .to_string()
}

fn get_first_seq(s: &str) -> String {
    let s = s.replace('\u{00A0}', " ");
    s.split_whitespace().next().unwrap_or("").to_string()
}

pub fn fetch_qq_lyrics_blocking(
    title: &str,
    artist: &str,
    _duration_sec: Option<u64>,
) -> Result<Option<Lyrics>, String> {
    let combined = format!("{} {}", title, artist);
    let mut ret = query(&combined)?;
    if ret.is_empty() {
        ret = query(title)?;
    }
    if ret.is_empty() {
        return Ok(None);
    }
    let expected_first = get_first_seq(&remove_leading_number(&remove_feat(title))).to_lowercase();
    let filtered: Vec<&SearchItem> = ret
        .iter()
        .filter(|item| {
            get_first_seq(&remove_leading_number(&item.name)).to_lowercase() == expected_first
        })
        .collect();
    let chosen = if !filtered.is_empty() {
        filtered[0]
    } else {
        return Ok(None);
    };
    let lyric = request_lyric(&chosen.mid)?;
    if lyric.lyric.trim().is_empty() {
        return Ok(None);
    }
    let synced_text = if let Some(trans) = lyric.trans.filter(|t| !t.trim().is_empty()) {
        super::inject_translation(&lyric.lyric, &trans)
    } else {
        lyric.lyric
    };
    let plain_text = super::strip_lrc_timestamps(&synced_text);
    Ok(Some(Lyrics {
        source: "qq".to_string(),
        synced_text: Some(synced_text),
        plain_text: Some(plain_text),
    }))
}

/// Up to `count` lyric-bearing candidates for the manual lyrics picker.
pub fn fetch_candidates(title: &str, artist: &str, count: usize) -> Result<Vec<Lyrics>, String> {
    let combined = format!("{} {}", title, artist);
    let mut ret = query(&combined).unwrap_or_default();
    if ret.is_empty() {
        ret = query(title).unwrap_or_default();
    }
    let expected_first = get_first_seq(&remove_leading_number(&remove_feat(title))).to_lowercase();
    let mut out = Vec::new();
    for item in ret
        .iter()
        .filter(|item| {
            get_first_seq(&remove_leading_number(&item.name)).to_lowercase() == expected_first
        })
        .take(count + 2)
    {
        if out.len() >= count {
            break;
        }
        let Ok(lyric) = request_lyric(&item.mid) else {
            continue;
        };
        if lyric.lyric.trim().is_empty() {
            continue;
        }
        let synced_text = if let Some(trans) = lyric.trans.filter(|t| !t.trim().is_empty()) {
            super::inject_translation(&lyric.lyric, &trans)
        } else {
            lyric.lyric
        };
        let plain_text = super::strip_lrc_timestamps(&synced_text);
        out.push(Lyrics {
            source: "qq".to_string(),
            synced_text: Some(synced_text),
            plain_text: Some(plain_text),
        });
    }
    Ok(out)
}

/// Up to `count` lyric-bearing candidates for an explicit free-text query.
pub fn fetch_candidates_for_query(query_text: &str, count: usize) -> Result<Vec<Lyrics>, String> {
    let mut out = Vec::new();
    for item in query(query_text)?.into_iter().take(count + 3) {
        if out.len() >= count {
            break;
        }
        let Ok(lyric) = request_lyric(&item.mid) else {
            continue;
        };
        if lyric.lyric.trim().is_empty() {
            continue;
        }
        let synced_text = if let Some(trans) = lyric.trans.filter(|t| !t.trim().is_empty()) {
            super::inject_translation(&lyric.lyric, &trans)
        } else {
            lyric.lyric
        };
        let plain_text = super::strip_lrc_timestamps(&synced_text);
        out.push(Lyrics {
            source: "qq".to_string(),
            synced_text: Some(synced_text),
            plain_text: Some(plain_text),
        });
    }
    Ok(out)
}

#[allow(dead_code)]
pub async fn fetch_qq_lyrics(
    title: &str,
    artist: &str,
    duration_sec: Option<u64>,
) -> Result<Option<Lyrics>, String> {
    let title = title.to_string();
    let artist = artist.to_string();
    tokio::task::spawn_blocking(move || fetch_qq_lyrics_blocking(&title, &artist, duration_sec))
        .await
        .map_err(|e| e.to_string())?
}
