use crate::models::Lyrics;
use regex::Regex;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

pub mod embedded;
pub mod kashinavi;
pub mod lrc;
pub mod lrclib;
pub mod netease;
pub mod qq;

#[derive(Debug, Clone, Default)]
pub struct TrackMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub file_path: Option<String>,
    pub embedded_lyrics: Option<String>,
}

type TrackMetadataRow = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
);

pub fn fetch_track_metadata(conn: &Connection, track_id: i64) -> Result<TrackMetadata, String> {
    let row: TrackMetadataRow = conn
        .query_row(
            "SELECT t.title, al.title AS album_title, a.name, t.duration_ms, t.file_path, t.embedded_lyrics \
             FROM tracks t \
             LEFT JOIN albums al ON al.id = t.album_id \
             LEFT JOIN track_artists ta ON ta.track_id = t.id AND ta.role = 'main' \
             LEFT JOIN artists a ON a.id = ta.artist_id \
             WHERE t.id = ? LIMIT 1",
            [track_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;
    Ok(TrackMetadata {
        title: row.0,
        artist: row.2,
        album: row.1,
        duration_ms: row.3,
        file_path: row.4,
        embedded_lyrics: row.5,
    })
}

/// Searches automatic lyric providers in configured order and stops after the
/// first usable result. That keeps the settings order meaningful and avoids
/// calling NetEase, QQ, and other later providers when an earlier source has
/// already supplied lyrics.
pub fn fetch_lyrics_from_sources_with_custom(
    sources: &[String],
    metadata: &TrackMetadata,
    custom: Option<&Lyrics>,
) -> Result<Option<Lyrics>, String> {
    fetch_from_sources(sources, |source| {
        if source == "custom" {
            return Ok(custom.cloned());
        }
        fetch_lyrics_from_source(source, metadata)
    })
}

fn fetch_from_sources<T, F>(sources: &[String], mut fetch: F) -> Result<Option<T>, String>
where
    F: FnMut(&str) -> Result<Option<T>, String>,
{
    for source in sources {
        match fetch(source) {
            Ok(Some(lyrics)) => return Ok(Some(lyrics)),
            Ok(None) => {}
            Err(e) => {
                log::debug!(
                    target: "sparkle::lyrics",
                    "event=provider_failed provider={source} error={e}"
                );
            }
        }
    }

    Ok(None)
}

fn fetch_lyrics_from_source(
    source: &str,
    metadata: &TrackMetadata,
) -> Result<Option<Lyrics>, String> {
    match source {
        "embedded" => embedded::fetch(metadata),
        "lrc" => lrc::fetch(metadata),
        "lrclib" => lrclib::fetch(metadata),
        "netease" => {
            if let (Some(title), Some(artist)) =
                (metadata.title.as_deref(), metadata.artist.as_deref())
            {
                let duration_sec = metadata.duration_ms.map(|d| (d / 1000) as u64);
                netease::fetch_netease_lyrics_blocking(
                    title,
                    artist,
                    duration_sec,
                    metadata.album.as_deref(),
                )
            } else {
                Ok(None)
            }
        }
        "kashinavi" => {
            if let (Some(title), Some(artist)) =
                (metadata.title.as_deref(), metadata.artist.as_deref())
            {
                kashinavi::fetch_kashinavi_lyrics_blocking(title, artist)
            } else {
                Ok(None)
            }
        }
        "qq" => {
            if let (Some(title), Some(artist)) =
                (metadata.title.as_deref(), metadata.artist.as_deref())
            {
                let duration_sec = metadata.duration_ms.map(|d| (d / 1000) as u64);
                qq::fetch_qq_lyrics_blocking(title, artist, duration_sec)
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

pub fn lrc_path_for_track(file_path: &str) -> PathBuf {
    let path = Path::new(file_path);
    let mut lrc = path.file_stem().unwrap_or_default().to_os_string();
    lrc.push(".lrc");
    path.with_file_name(lrc)
}

pub fn strip_lrc_timestamps(synced: &str) -> String {
    let re = Regex::new(r"\[\d{2}:\d{2}(?:\.\d{2,3})?\]").unwrap();
    re.replace_all(synced, "")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn first_synced_line(text: &str) -> Option<String> {
    let timestamp = Regex::new(r"\[(\d+):(\d+(?:\.\d+)?)\]").unwrap();
    text.lines()
        .filter_map(|line| {
            let first_time_ms = timestamp
                .captures_iter(line)
                .filter_map(|captures| {
                    let minutes = captures.get(1)?.as_str().parse::<f64>().ok()?;
                    let seconds = captures.get(2)?.as_str().parse::<f64>().ok()?;
                    Some(((minutes * 60.0 + seconds) * 1000.0).round() as i64)
                })
                .min()?;
            let lyric = timestamp.replace_all(line.trim(), "").trim().to_string();
            (!lyric.is_empty()).then_some((first_time_ms, lyric))
        })
        .min_by_key(|(time_ms, _)| *time_ms)
        .map(|(_, lyric)| lyric)
}

pub fn inject_translation(original: &str, translation: &str) -> String {
    let re = Regex::new(r"((?:\[.+?\])+)(.*)").unwrap();
    let time_re = Regex::new(r"\[.+?\]").unwrap();
    let mut entries: Vec<(String, String)> = Vec::new();
    for line in original.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(caps) = re.captures(line) {
            let timestamps = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let content = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
            for t in time_re.find_iter(timestamps) {
                entries.push((t.as_str().to_string(), content.to_string()));
            }
        }
    }
    let mut trans_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for line in translation.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(caps) = re.captures(line) {
            let timestamps = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let content = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
            for t in time_re.find_iter(timestamps) {
                trans_map.insert(t.as_str().to_string(), content.to_string());
            }
        }
    }
    let mut lines = Vec::new();
    for (time, content) in entries {
        if let Some(trans) = trans_map.get(&time) {
            lines.push((time.clone(), format!("{}{}/{}", time, content, trans)));
        } else {
            lines.push((time.clone(), format!("{}{}", time, content)));
        }
    }
    lines.sort_by(|a, b| a.0.cmp(&b.0));
    lines
        .into_iter()
        .map(|(_, line)| line)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn automatic_lookup_stops_at_the_first_configured_match() {
        let sources = vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
        ];
        let calls = Mutex::new(Vec::new());

        let result = fetch_from_sources(&sources, |source| {
            calls.lock().unwrap().push(source.to_string());
            Ok((source == "first").then(|| source.to_string()))
        })
        .unwrap();

        assert_eq!(result.as_deref(), Some("first"));
        assert_eq!(calls.into_inner().unwrap(), vec!["first"]);
    }

    #[test]
    fn custom_content_participates_as_an_ordered_provider() {
        let sources = vec!["custom".to_string(), "lrclib".to_string()];
        let custom = Lyrics {
            source: "custom".to_string(),
            synced_text: Some("[00:00.00]saved".to_string()),
            plain_text: Some("saved".to_string()),
        };

        let result = fetch_lyrics_from_sources_with_custom(
            &sources,
            &TrackMetadata::default(),
            Some(&custom),
        )
        .unwrap();

        assert_eq!(result.unwrap().source, "custom");
    }

    #[test]
    fn first_synced_line_requires_a_timestamp_and_text() {
        assert_eq!(
            first_synced_line("[00:01.25]hello").as_deref(),
            Some("hello")
        );
        assert!(first_synced_line("plain lyrics").is_none());
        assert!(first_synced_line("[00:01.25]").is_none());
    }

    #[test]
    fn first_synced_line_uses_timestamp_order() {
        assert_eq!(
            first_synced_line("[00:10.00]second\n[00:02.50]first").as_deref(),
            Some("first")
        );
    }
}
