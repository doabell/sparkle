use crate::models::Lyrics;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

mod document;
pub use document::{
    apply_lrc_offset, first_synced_line, inject_translation, parse_lrc, strip_lrc_timestamps,
};

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
             WHERE t.id = ? ORDER BY ta.position, ta.artist_id LIMIT 1",
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
pub fn fetch_lyrics_from_sources_with_cache(
    sources: &[String],
    metadata: &TrackMetadata,
    custom: Option<&Lyrics>,
    cached: &[Lyrics],
) -> Result<Option<Lyrics>, String> {
    fetch_from_sources(sources, |source| {
        if source == "custom" {
            return Ok(custom.cloned());
        }
        // Local text is cheap to re-read, and its lifetime follows the file,
        // not the remote cache. Consult each remote cache only at its own
        // position in the configured order.
        if !matches!(source, "embedded" | "lrc" | "none") {
            if let Some(lyrics) = cached.iter().find(|lyrics| lyrics.source == source) {
                return Ok(Some(lyrics.clone()));
            }
        }
        fetch_lyrics_from_source(source, metadata)
    })
}

pub fn no_lyrics() -> Lyrics {
    Lyrics {
        source: "none".to_string(),
        synced_text: None,
        plain_text: None,
    }
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
        "none" => Ok(Some(no_lyrics())),
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

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
