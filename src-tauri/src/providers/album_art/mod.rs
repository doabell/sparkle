use crate::cache;
use crate::models::{detect_image_mime_type, CachedImage, ImageData};
use crate::settings::Settings;
use lofty::file::TaggedFileExt;
use lofty::picture::PictureType;
use lofty::probe::Probe;
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;

pub mod cover_art_archive;

pub fn get_cached_album_art(
    conn: &Connection,
    root: &Path,
    album_id: i64,
) -> Result<Option<CachedImage>, String> {
    cache::get_non_custom_image(conn, root, "album", album_id)
}

/// The data online sources need to fetch art for an album: a file path for
/// embedded art and the MusicBrainz release id for Cover Art Archive.
pub struct AlbumArtLookup {
    pub file_path: Option<String>,
    pub mbid: Option<String>,
}

pub fn album_art_lookup(conn: &Connection, album_id: i64) -> Result<AlbumArtLookup, String> {
    let file_path: Option<String> = conn
        .query_row(
            "SELECT t.file_path FROM tracks t WHERE t.album_id = ? LIMIT 1",
            [album_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let mbid: Option<String> = conn
        .query_row(
            "SELECT NULLIF(mbid, '') FROM albums WHERE id = ?",
            [album_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    Ok(AlbumArtLookup { file_path, mbid })
}

/// Extracts embedded art from an audio file. Performs file I/O — call
/// WITHOUT holding the DB lock.
pub fn fetch_embedded_from_path(file_path: &str) -> Result<Option<ImageData>, String> {
    let tagged_file = Probe::open(file_path)
        .and_then(|p| p.read())
        .map_err(|e| e.to_string())?;

    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
        .ok_or_else(|| "no tags found".to_string())?;

    let pictures = tag.pictures();
    if pictures.is_empty() {
        return Ok(None);
    }

    let picture = pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())
        .expect("pictures not empty");

    let data = picture.data().to_vec();
    let mime_type = detect_image_mime_type(&data);
    Ok(Some(ImageData {
        source: "embedded".to_string(),
        data: Some(data),
        mime_type,
    }))
}

/// Fetches album art from the network/filesystem, trying the configured
/// sources in order. "custom" is skipped — the caller checks for a
/// user-provided image first. Must be called WITHOUT holding the DB lock.
pub fn fetch_album_art_online(
    lookup: &AlbumArtLookup,
    settings: &Settings,
) -> Result<Option<ImageData>, String> {
    for source in &settings.album_art_sources {
        let fetched = match source.as_str() {
            "custom" => Ok(None),
            "embedded" => match &lookup.file_path {
                Some(path) => fetch_embedded_from_path(path),
                None => Ok(None),
            },
            "cover_art_archive" => match &lookup.mbid {
                Some(mbid) => cover_art_archive::fetch_by_mbid(mbid),
                None => Ok(None),
            },
            _ => Ok(None),
        };

        match fetched {
            Ok(Some(image)) => return Ok(Some(image)),
            Ok(None) => {}
            Err(error) => {
                // A broken tag or a temporary network failure should not make
                // every card retry forever, nor prevent later providers from
                // supplying a cover.
                log::debug!(target: "sparkle::album_art", "source={source} fetch_failed error={error}");
            }
        }
    }

    Ok(None)
}

pub fn cache_album_art(
    conn: &Connection,
    root: &Path,
    album_id: i64,
    image: &ImageData,
) -> Result<CachedImage, String> {
    cache::set_image(
        conn,
        root,
        "album",
        album_id,
        &image.source,
        None,
        image.data.as_deref(),
    )
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
