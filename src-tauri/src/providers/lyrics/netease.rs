// Portions adapted from MusicBee-NeteaseLyrics and its NetEase implementation:
// https://github.com/cqjjjzr/MusicBee-NeteaseLyrics
// The adapted portions are licensed under Apache-2.0 and have been modified
// substantially for Sparkle. See THIRD_PARTY_NOTICES.md.

use crate::models::Lyrics;
use aes::cipher::block_padding::Pkcs7;
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockEncryptMut, KeyIvInit};
use base64::Engine;
use num_bigint::BigUint;
use num_traits::Num;
use regex::Regex;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())
}

const NONCE: &str = "0CoJUm6Qyw8W8jud";
const IV: &str = "0102030405060708";
const MODULUS: &str = "00e0b509f6259df8642dbc35662901477df22677ec152b5ff68ace615bb7b725152b3ab17a876aea8a5aa76d2e417629ec4ee341f56135fccf695280104e0312ecbda92557c93870114af6c9d05c4f7f0c3685b7a46bee255932575cce10b424d813cfe4875d3e82047b97ddef52741d546b8e289dc6935b3ece0462db0a22b8e7";
const SEARCH_URL: &str = "https://music.163.com/weapi/search/get";
const LYRIC_URL: &str = "https://music.163.com/weapi/song/lyric?csrf_token=";
const LEGACY_SEARCH_URL: &str = "https://music.163.com/api/search/get/";
const LEGACY_LYRIC_URL: &str = "https://music.163.com/api/song/lyric";

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

#[derive(Deserialize, Debug, Default)]
struct SearchResponse {
    #[serde(default)]
    result: SearchResult,
    code: i32,
}

#[derive(Deserialize, Debug, Default)]
struct SearchResult {
    #[serde(default, rename = "songCount")]
    song_count: i32,
    #[serde(default)]
    songs: Vec<SearchResultSong>,
}

#[derive(Deserialize, Debug, Clone)]
struct SearchResultSong {
    id: i64,
    name: String,
    #[serde(default)]
    duration: i64,
    #[serde(default)]
    album: SearchResultAlbum,
    #[serde(default)]
    artists: Vec<SearchResultArtist>,
}

#[derive(Deserialize, Debug, Default, Clone)]
struct SearchResultAlbum {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize, Debug, Clone)]
struct SearchResultArtist {
    name: String,
}

#[derive(Deserialize, Debug)]
struct LyricResponse {
    #[serde(default)]
    lrc: Option<LyricInner>,
    code: i32,
}

#[derive(Deserialize, Debug, Default)]
struct LyricInner {
    #[serde(default)]
    lyric: Option<String>,
}

fn aes_encrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<String, String> {
    let key = GenericArray::from_slice(key);
    let iv = GenericArray::from_slice(iv);
    let cipher = cbc::Encryptor::<aes::Aes128>::new(key, iv);
    let mut buf = vec![0u8; data.len() + 16];
    buf[..data.len()].copy_from_slice(data);
    let ct = cipher
        .encrypt_padded_mut::<Pkcs7>(&mut buf, data.len())
        .map_err(|_| "aes encrypt failed".to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(ct))
}

fn random_secret_key(length: usize) -> String {
    let chars = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut state = nanos;
    let mut out = String::with_capacity(length);
    for _ in 0..length {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let idx = ((state >> 32) as usize) % chars.len();
        out.push(chars[idx] as char);
    }
    out
}

fn rsa_encrypt(secret_key: &str) -> Result<String, String> {
    let reversed: String = secret_key.chars().rev().collect();
    let m = BigUint::from_bytes_be(reversed.as_bytes());
    let e = BigUint::from(0x010001u32);
    let n = BigUint::from_str_radix(MODULUS, 16).map_err(|_| "invalid modulus")?;
    let c = m.modpow(&e, &n);
    let hex = format!("{:x}", c);
    let key = if hex.len() > 256 {
        hex[hex.len() - 256..].to_string()
    } else {
        format!("{:0>256}", hex)
    };
    Ok(key)
}

fn encrypt_request(params: &str) -> Result<HashMap<&'static str, String>, String> {
    let secret_key = random_secret_key(16);
    let first = aes_encrypt(params.as_bytes(), NONCE.as_bytes(), IV.as_bytes())?;
    let params_b64 = aes_encrypt(first.as_bytes(), secret_key.as_bytes(), IV.as_bytes())?;
    let enc_sec_key = rsa_encrypt(&secret_key)?;
    let mut map = HashMap::new();
    map.insert("params", params_b64);
    map.insert("encSecKey", enc_sec_key);
    Ok(map)
}

fn search(s: &str) -> Result<Vec<SearchResultSong>, String> {
    let params = serde_json::json!({
        "csrf_token": "",
        "s": s,
        "offset": 0,
        "type": 1,
        "limit": 20
    });
    let form = encrypt_request(&params.to_string())?;
    let client = http_client()?;
    let response = client
        .post(SEARCH_URL)
        .header("Referer", "https://music.163.com")
        .header("User-Agent", USER_AGENT)
        .form(&form)
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return search_legacy(s);
    }
    let data: SearchResponse = response.json().map_err(|e| e.to_string())?;
    if data.code != 200 || data.result.song_count <= 0 {
        return search_legacy(s);
    }
    Ok(data.result.songs)
}

fn search_legacy(s: &str) -> Result<Vec<SearchResultSong>, String> {
    let client = http_client()?;
    let response = client
        .get(LEGACY_SEARCH_URL)
        .query(&[
            ("csrf_token", ""),
            ("hlpretag", ""),
            ("hlposttag", ""),
            ("s", s),
            ("type", "1"),
            ("offset", "0"),
            ("total", "true"),
            ("limit", "6"),
        ])
        .header("Referer", "https://music.163.com/")
        .header("User-Agent", USER_AGENT)
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Ok(Vec::new());
    }
    let data: SearchResponse = response.json().map_err(|e| e.to_string())?;
    if data.code != 200 || data.result.song_count <= 0 {
        return Ok(Vec::new());
    }
    Ok(data.result.songs)
}

fn request_lyric(id: i64) -> Result<LyricResponse, String> {
    let params = serde_json::json!({
        "OS": "pc",
        "id": id,
        "lv": -1,
        "kv": -1,
        "tv": -1,
        "rv": -1
    });
    let form = encrypt_request(&params.to_string())?;
    let client = http_client()?;
    let response = client
        .post(LYRIC_URL)
        .header("Referer", "https://music.163.com")
        .header("User-Agent", USER_AGENT)
        .form(&form)
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return request_lyric_legacy(id);
    }
    let data: LyricResponse = response.json().map_err(|e| e.to_string())?;
    if data.code != 200 {
        return request_lyric_legacy(id);
    }
    Ok(data)
}

fn request_lyric_legacy(id: i64) -> Result<LyricResponse, String> {
    let client = http_client()?;
    let response = client
        .get(LEGACY_LYRIC_URL)
        .query(&[
            ("os", "pc"),
            ("id", &id.to_string()),
            ("lv", "-1"),
            ("kv", "-1"),
            ("tv", "-1"),
        ])
        .header("Referer", "https://music.163.com/")
        .header("Cookie", "appver=1.5.0.75771;")
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err("lyric request failed".to_string());
    }
    let data: LyricResponse = response.json().map_err(|e| e.to_string())?;
    if data.code != 200 {
        return Err("lyric request failed".to_string());
    }
    Ok(data)
}

fn sanitize(s: &str) -> String {
    s.replace('（', "(")
        .replace('）', ")")
        .replace('\u{00A0}', " ")
}

fn split_by_delimiters(s: &str) -> Vec<String> {
    let re = Regex::new(r"/|\u0026|,|，| x | \* |×|·").unwrap();
    re.split(s)
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn extract_feat(s: &str) -> (String, Vec<String>) {
    let paren = Regex::new(r"\s*\(feat(.+)\)").unwrap();
    let bare = Regex::new(r"\s+feat(.+)").unwrap();
    let mut text = s.to_string();
    let mut clause = String::new();
    if let Some(caps) = paren.captures(s) {
        if let Some(m) = caps.get(0) {
            text = text.replacen(m.as_str(), "", 1);
        }
        if let Some(g) = caps.get(1) {
            clause = g.as_str().to_string();
        }
    } else if let Some(caps) = bare.captures(s) {
        if let Some(m) = caps.get(0) {
            text = text.replacen(m.as_str(), "", 1);
        }
        if let Some(g) = caps.get(1) {
            clause = g.as_str().to_string();
        }
    }
    text = text.trim().to_string();
    if clause.is_empty() {
        return (text, Vec::new());
    }
    let clause = clause
        .trim_start_matches(|c| c == '.' || c == ' ')
        .trim_end_matches(')');
    let artists = split_by_delimiters(&clause);
    (text, artists)
}

fn split_title_artist(title: &str, artist: &str) -> (String, Vec<String>) {
    let (artists_without_feat, mut feat_artists) = extract_feat(&sanitize(artist));
    let mut artists = split_by_delimiters(&artists_without_feat);
    artists.append(&mut feat_artists);
    let (title_without_feat, mut feat_artists2) = extract_feat(&sanitize(title));
    artists.append(&mut feat_artists2);
    artists.sort();
    (title_without_feat, artists)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a_len = a.chars().count();
    let b_len = b.chars().count();
    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];
    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (curr[j] + 1).min(prev[j + 1] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_len]
}

fn match_score(
    song: &SearchResultSong,
    title: &str,
    artists: &[String],
    album: Option<&str>,
    duration_sec: Option<u64>,
) -> f64 {
    let mut result_artists: Vec<String> =
        song.artists.iter().map(|a| a.name.to_lowercase()).collect();
    result_artists.sort();
    let result_artists_str = result_artists.join(" ");
    let artists_str = artists.join(" ").to_lowercase();
    let title_distance = levenshtein(&title.to_lowercase(), &song.name.to_lowercase());
    let artist_distance = levenshtein(&artists_str, &result_artists_str);
    let album_distance = album
        .map(|a| levenshtein(&a.to_lowercase(), &song.album.name.to_lowercase()))
        .unwrap_or(0);
    let duration_diff = duration_sec
        .map(|d| d as f64 - (song.duration as f64 / 1000.0))
        .unwrap_or(0.0);
    -(duration_diff * duration_diff)
        - 2.0 * title_distance as f64
        - 0.7 * artist_distance as f64
        - album_distance as f64
}

/// Searches with the artist-qualified strategy and returns candidates
/// best-first (score = title/artist/album/duration match).
fn scored_candidates(
    title_clean: &str,
    artists: &[String],
    album: Option<&str>,
    duration_sec: Option<u64>,
) -> Vec<SearchResultSong> {
    let artists_str = artists.join(" ");
    let queries = [
        format!("{} {}", title_clean, artists_str),
        title_clean.to_string(),
        format!("{} {} {}", title_clean, artists_str, album.unwrap_or("")),
    ];
    let mut results = Vec::new();
    for query in &queries {
        let found = search(query).unwrap_or_default();
        results.extend(found);
    }
    let mut seen = std::collections::HashSet::new();
    results.retain(|s| seen.insert(s.id));
    let mut scored: Vec<(f64, SearchResultSong)> = results
        .into_iter()
        .map(|song| {
            (
                match_score(&song, title_clean, artists, album, duration_sec),
                song,
            )
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().map(|(_, song)| song).collect()
}

/// Downloads the original lyric for a song. Netease translations are ignored
/// so this provider stays in original-only mode.
fn lyric_for_song(song: &SearchResultSong) -> Option<String> {
    let lyric_resp = request_lyric(song.id).ok()?;
    lyric_resp
        .lrc
        .and_then(|l| l.lyric)
        .filter(|l| !l.trim().is_empty())
}

/// Up to `count` lyric-bearing candidates for an explicit free-text query.
pub fn fetch_candidates_for_query(query: &str, count: usize) -> Result<Vec<Lyrics>, String> {
    let mut out = Vec::new();
    for song in search(query)?.into_iter().take(count + 3) {
        if out.len() >= count {
            break;
        }
        if let Some(synced_text) = lyric_for_song(&song) {
            let plain_text = super::strip_lrc_timestamps(&synced_text);
            out.push(Lyrics {
                source: "netease".to_string(),
                synced_text: Some(synced_text),
                plain_text: Some(plain_text),
            });
        }
    }
    Ok(out)
}

pub fn fetch_netease_lyrics_blocking(
    title: &str,
    artist: &str,
    duration_sec: Option<u64>,
    album: Option<&str>,
) -> Result<Option<Lyrics>, String> {
    let (title_clean, artists) = split_title_artist(title, artist);
    // Walk candidates best-first: the top hit often has no (usable) lyric
    // while the runner-up does.
    for song in scored_candidates(&title_clean, &artists, album, duration_sec)
        .into_iter()
        .take(5)
    {
        if let Some(synced_text) = lyric_for_song(&song) {
            let plain_text = super::strip_lrc_timestamps(&synced_text);
            return Ok(Some(Lyrics {
                source: "netease".to_string(),
                synced_text: Some(synced_text),
                plain_text: Some(plain_text),
            }));
        }
    }
    Ok(None)
}

/// Up to `count` lyric-bearing candidates for the manual lyrics picker.
pub fn fetch_candidates(
    title: &str,
    artist: &str,
    album: Option<&str>,
    duration_sec: Option<u64>,
    count: usize,
) -> Result<Vec<Lyrics>, String> {
    let (title_clean, artists) = split_title_artist(title, artist);
    let mut out = Vec::new();
    for song in scored_candidates(&title_clean, &artists, album, duration_sec)
        .into_iter()
        .take(count + 3)
    {
        if out.len() >= count {
            break;
        }
        if let Some(synced_text) = lyric_for_song(&song) {
            let plain_text = super::strip_lrc_timestamps(&synced_text);
            out.push(Lyrics {
                source: "netease".to_string(),
                synced_text: Some(synced_text),
                plain_text: Some(plain_text),
            });
        }
    }
    Ok(out)
}

#[allow(dead_code)]
pub async fn fetch_netease_lyrics(
    title: &str,
    artist: &str,
    duration_sec: Option<u64>,
) -> Result<Option<Lyrics>, String> {
    let title = title.to_string();
    let artist = artist.to_string();
    tokio::task::spawn_blocking(move || {
        fetch_netease_lyrics_blocking(&title, &artist, duration_sec, None)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_netease_camel_case_song_count() {
        let response: SearchResponse =
            serde_json::from_str(r#"{"code":200,"result":{"songCount":1,"songs":[]}}"#).unwrap();
        assert_eq!(response.result.song_count, 1);
    }
}
