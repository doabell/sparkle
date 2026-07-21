use crate::cache;
use crate::models::{ArtistInfo, CachedImage, ImageData};
use crate::settings::Settings;
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;

pub mod brave;
pub mod duckduckgo;
pub mod shazam;
pub mod wikipedia;

pub fn get_cached_artist_info(
    conn: &Connection,
    root: &Path,
    artist_id: i64,
) -> Result<Option<ArtistInfo>, String> {
    cache::get_artist_info(conn, root, artist_id)
}

pub fn get_cached_artist_image(
    conn: &Connection,
    root: &Path,
    artist_id: i64,
) -> Result<Option<CachedImage>, String> {
    cache::get_non_custom_image(conn, root, "artist", artist_id)
}

/// The title to query online sources with: the artist's info_term override,
/// falling back to their library name.
pub fn artist_query_title(conn: &Connection, artist_id: i64) -> Result<Option<String>, String> {
    let row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT name, info_term FROM artists WHERE id = ?",
            [artist_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(row.map(|(name, term)| term.filter(|t| !t.trim().is_empty()).unwrap_or(name)))
}

/// The artist's image search term override, falling back to info_term/name.
pub fn artist_image_query_title(
    conn: &Connection,
    artist_id: i64,
) -> Result<Option<String>, String> {
    let row: Option<(String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT name, info_term, image_term FROM artists WHERE id = ?",
            [artist_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(row.map(|(name, info_term, image_term)| {
        image_term
            .filter(|t| !t.trim().is_empty())
            .or_else(|| info_term.filter(|t| !t.trim().is_empty()))
            .unwrap_or(name)
    }))
}

/// Extracts the language code from a `wikipedia:{lang}` provider id.
fn wikipedia_lang(source: &str) -> Option<&str> {
    source.strip_prefix("wikipedia:")
}

/// The Brave locale hint: the artist's Wikipedia edition if one is chosen,
/// otherwise the first Wikipedia edition in the settings list, else "en".
pub fn brave_lang_hint<'a>(explicit_provider: Option<&'a str>, settings: &'a Settings) -> &'a str {
    if let Some(lang) = explicit_provider.and_then(wikipedia_lang) {
        return lang;
    }
    settings
        .artist_image_sources
        .iter()
        .find_map(|s| wikipedia_lang(s))
        .unwrap_or("en")
}

/// Fetches artist info from the network, trying the configured sources in
/// order. "custom" is skipped here — the caller checks user-provided content
/// before any network call. Must be called WITHOUT holding the DB lock —
/// it performs blocking HTTP.
pub fn fetch_artist_info_online(
    title: &str,
    settings: &Settings,
) -> Result<Option<ArtistInfo>, String> {
    for source in &settings.artist_info_sources {
        let fetched = match source.as_str() {
            "custom" => Ok(None),
            s if wikipedia_lang(s).is_some() => {
                wikipedia::fetch_summary_by_title(title, &[wikipedia_lang(s).unwrap().to_string()])
            }
            _ => Ok(None),
        }?;
        if let Some(info) = fetched {
            return Ok(Some(info));
        }
    }
    Ok(None)
}

/// Fetches an artist image from the network, trying the configured sources
/// in order. Must be called WITHOUT holding the DB lock — it performs
/// blocking HTTP.
pub fn fetch_artist_image_online(
    title: &str,
    settings: &Settings,
) -> Result<Option<ImageData>, String> {
    for source in &settings.artist_image_sources {
        let fetched = match source.as_str() {
            "custom" => Ok(None),
            "brave" => brave::fetch_image_by_title(
                title,
                &settings.brave_api_key,
                brave_lang_hint(None, settings),
            ),
            "duckduckgo" => duckduckgo::fetch_image_by_title(title),
            "shazam" => shazam::fetch_image_by_title(title),
            s if wikipedia_lang(s).is_some() => {
                wikipedia::fetch_image_by_title(title, &[wikipedia_lang(s).unwrap().to_string()])
            }
            _ => Ok(None),
        };
        match fetched {
            Ok(Some(image)) => return Ok(Some(image)),
            Ok(None) => {}
            Err(error) => {
                log::debug!(target: "sparkle::artist_image", "source={source} fetch_failed error={error}");
            }
        }
    }
    Ok(None)
}

/// Fetches from a single explicit provider (per-artist mix & match
/// override). Returns None for providers that cannot serve info.
pub fn fetch_artist_info_from_provider(
    provider: &str,
    title: &str,
) -> Result<Option<ArtistInfo>, String> {
    match wikipedia_lang(provider) {
        Some(lang) => wikipedia::fetch_summary_by_title(title, &[lang.to_string()]),
        None => Ok(None),
    }
}

/// Fetches an image from a single explicit provider.
pub fn fetch_artist_image_from_provider(
    provider: &str,
    title: &str,
    lang_hint: &str,
    settings: &Settings,
) -> Result<Option<ImageData>, String> {
    match provider {
        "brave" => brave::fetch_image_by_title(title, &settings.brave_api_key, lang_hint),
        "duckduckgo" => duckduckgo::fetch_image_by_title(title),
        "shazam" => shazam::fetch_image_by_title(title),
        s if wikipedia_lang(s).is_some() => {
            wikipedia::fetch_image_by_title(title, &[wikipedia_lang(s).unwrap().to_string()])
        }
        _ => Ok(None),
    }
}

pub fn cache_artist_info(
    conn: &Connection,
    root: &Path,
    artist_id: i64,
    info: &ArtistInfo,
) -> Result<(), String> {
    cache::set_artist_info(conn, root, artist_id, &info.source, info.summary.as_deref())
}

pub fn cache_artist_image(
    conn: &Connection,
    root: &Path,
    artist_id: i64,
    image: &ImageData,
) -> Result<CachedImage, String> {
    cache::set_image(
        conn,
        root,
        "artist",
        artist_id,
        &image.source,
        None,
        image.data.as_deref(),
    )
}
