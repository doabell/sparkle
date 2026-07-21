use crate::models::{ArtistInfo, CachedImage, ImageData, Lyrics};
use rusqlite::{Connection, OptionalExtension};
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

// Online-metadata payloads: lyrics and artist info are small text kept in
// the database (lyrics must be searchable); image bytes live as files under
// `<app data>/cache/` so the database stays small.
//
//   cache/artist-info/{hash}.txt         artist bio summary
//   cache/images/artist/{hash}.{ext}
//   cache/images/album/{hash}.{ext}

// Cache entries persist until the user explicitly clears them. The expiry
// column remains in the schema for compatibility with older databases, but
// it is no longer part of cache lookup behavior.
const NEVER_EXPIRES: i64 = i64::MAX;

// Image bytes remain on disk and never cross the normal UI IPC boundary. A
// bounded encoded size prevents a malformed or unexpectedly huge file from
// consuming arbitrary memory without adding a CPU-heavy resize step.
pub const MAX_CACHED_IMAGE_BYTES: usize = 20 * 1024 * 1024;

pub fn artist_info_dir(root: &Path) -> PathBuf {
    root.join("artist-info")
}

pub fn lyrics_dir(root: &Path) -> PathBuf {
    root.join("lyrics")
}

pub fn images_dir(root: &Path, entity_type: &str) -> PathBuf {
    root.join("images").join(entity_type)
}

pub fn ensure_dirs(root: &Path) {
    let _ = std::fs::create_dir_all(artist_info_dir(root));
    let _ = std::fs::create_dir_all(lyrics_dir(root));
    let _ = std::fs::create_dir_all(images_dir(root, "artist"));
    let _ = std::fs::create_dir_all(images_dir(root, "album"));
}

fn write_file(dir: &Path, name: &str, contents: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(name), contents).map_err(|e| e.to_string())
}

fn read_file(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name)).ok()
}

fn write_bytes(dir: &Path, name: &str, contents: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(name), contents).map_err(|e| e.to_string())
}

fn remove_file(dir: &Path, name: &str) {
    let _ = std::fs::remove_file(dir.join(name));
}

// ---------------------------------------------------------------- lyrics

pub fn get_lyrics_from_source(
    conn: &Connection,
    track_id: i64,
    source: &str,
) -> Result<Option<Lyrics>, String> {
    let row: Option<(String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT source, synced_text, plain_text FROM lyrics \
             WHERE track_id = ? AND source = ?",
            rusqlite::params![track_id, source],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(row.map(|(source, synced_text, plain_text)| Lyrics {
        source,
        synced_text,
        plain_text,
    }))
}

pub fn get_non_custom_lyrics(conn: &Connection, track_id: i64) -> Result<Vec<Lyrics>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT source, synced_text, plain_text FROM lyrics \
             WHERE track_id = ? AND source != 'custom' \
             ORDER BY fetched_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([track_id], |row| {
        Ok(Lyrics {
            source: row.get(0)?,
            synced_text: row.get(1)?,
            plain_text: row.get(2)?,
        })
    });
    let result = rows
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string());
    result
}

pub fn set_lyrics(
    conn: &Connection,
    track_id: i64,
    source: &str,
    synced_text: Option<&str>,
    plain_text: Option<&str>,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO lyrics (track_id, source, synced_text, plain_text, fetched_at, expires_at) \
         VALUES (?1, ?2, ?3, ?4, unixepoch(), ?5) \
         ON CONFLICT(track_id, source) DO UPDATE SET \
           synced_text = excluded.synced_text, \
           plain_text = excluded.plain_text, \
           fetched_at = excluded.fetched_at, \
           expires_at = excluded.expires_at",
        rusqlite::params![track_id, source, synced_text, plain_text, NEVER_EXPIRES],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_lyrics(conn: &Connection, track_id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM lyrics WHERE track_id = ?", [track_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_lyrics_from_source(
    conn: &Connection,
    track_id: i64,
    source: &str,
) -> Result<(), String> {
    conn.execute(
        "DELETE FROM lyrics WHERE track_id = ? AND source = ?",
        rusqlite::params![track_id, source],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn custom_lyrics_path(root: &Path, track_id: i64, extension: &str) -> PathBuf {
    lyrics_dir(root).join(format!("custom-{track_id}.{extension}"))
}

/// Copies a user-selected lyrics file into Sparkle's managed cache. The
/// database still stores the parsed text so playback never depends on the
/// original path remaining available.
pub fn copy_custom_lyrics_file(
    root: &Path,
    track_id: i64,
    source: &Path,
    extension: &str,
) -> Result<(), String> {
    let extension = if extension.eq_ignore_ascii_case("lrc") {
        "lrc"
    } else {
        "txt"
    };
    let dir = lyrics_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = custom_lyrics_path(root, track_id, extension);
    if source != target {
        std::fs::copy(source, &target).map_err(|e| format!("failed to copy lyrics file: {e}"))?;
    }
    let other_extension = if extension == "lrc" { "txt" } else { "lrc" };
    let _ = std::fs::remove_file(custom_lyrics_path(root, track_id, other_extension));
    Ok(())
}

pub fn delete_custom_lyrics_file(root: &Path, track_id: i64) {
    for extension in ["lrc", "txt"] {
        let _ = std::fs::remove_file(custom_lyrics_path(root, track_id, extension));
    }
}

// ------------------------------------------------------------ artist info

pub fn get_artist_info(
    conn: &Connection,
    root: &Path,
    artist_id: i64,
) -> Result<Option<ArtistInfo>, String> {
    let row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT source, file_path FROM artist_info WHERE artist_id = ?",
            [artist_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(row.map(|(source, file_path)| {
        let summary = file_path.and_then(|p| read_file(&artist_info_dir(root), &p));
        ArtistInfo { source, summary }
    }))
}

pub fn set_artist_info(
    conn: &Connection,
    root: &Path,
    artist_id: i64,
    source: &str,
    summary: Option<&str>,
) -> Result<(), String> {
    let dir = artist_info_dir(root);
    // Remove the previous file for this artist if the name changed.
    let old: Option<String> = conn
        .query_row(
            "SELECT file_path FROM artist_info WHERE artist_id = ?",
            [artist_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    let name = artist_info_file_name(artist_id, source);
    match summary {
        Some(text) => write_file(&dir, &name, text)?,
        None => {}
    }
    if let Some(old) = old {
        if old != name || summary.is_none() {
            remove_file(&dir, &old);
        }
    }
    if summary.is_none() {
        remove_file(&dir, &name);
    }
    conn.execute(
        "INSERT OR REPLACE INTO artist_info (artist_id, source, file_path, fetched_at, expires_at) \
         VALUES (?1, ?2, ?3, unixepoch(), ?4)",
        rusqlite::params![artist_id, source, summary.map(|_| &name), NEVER_EXPIRES],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_artist_info(conn: &Connection, root: &Path, artist_id: i64) -> Result<(), String> {
    let path: Option<String> = conn
        .query_row(
            "SELECT file_path FROM artist_info WHERE artist_id = ?",
            [artist_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    conn.execute("DELETE FROM artist_info WHERE artist_id = ?", [artist_id])
        .map_err(|e| e.to_string())?;
    if let Some(p) = path {
        remove_file(&artist_info_dir(root), &p);
    }
    Ok(())
}

// ----------------------------------------------------------------- images

fn sanitize_source(source: &str) -> String {
    source
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Stable FNV-1a (64-bit) hash — cache file names are opaque hashes instead
/// of raw entity ids, so the directory is not enumerable by id.
fn name_hash(parts: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in parts.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn ext_for_mime(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        _ => "jpg",
    }
}

fn image_file_name(entity_type: &str, entity_id: i64, source: &str, mime: &str) -> String {
    let key = format!("{entity_type}:{entity_id}:{}", sanitize_source(source));
    format!("{}.{}", name_hash(&key), ext_for_mime(mime))
}

fn artist_info_file_name(artist_id: i64, source: &str) -> String {
    let key = format!("artist-info:{artist_id}:{}", sanitize_source(source));
    format!("{}.txt", name_hash(&key))
}

/// Validates the byte budget for an image copied into Sparkle's managed
/// cache. Deliberately does not decode or resize: the webview loads the file
/// directly, so avoiding Rust-to-JavaScript copies is the main memory win.
pub fn validate_image_for_cache(data: Vec<u8>) -> Result<Vec<u8>, String> {
    if data.len() > MAX_CACHED_IMAGE_BYTES {
        return Err(format!(
            "image is too large (max {} MB)",
            MAX_CACHED_IMAGE_BYTES / (1024 * 1024)
        ));
    }
    Ok(data)
}

/// Reads a user-selected image without allocating more than the cache's
/// accepted input budget, even if the file grows after its metadata is read.
pub fn read_image_file(path: &Path) -> Result<Vec<u8>, String> {
    let declared_length = std::fs::metadata(path)
        .map_err(|e| format!("failed to inspect image: {e}"))?
        .len();
    if declared_length > MAX_CACHED_IMAGE_BYTES as u64 {
        return Err(format!(
            "image is too large (max {} MB)",
            MAX_CACHED_IMAGE_BYTES / (1024 * 1024)
        ));
    }

    let mut data = Vec::with_capacity(declared_length as usize);
    let file = File::open(path).map_err(|e| format!("failed to read image: {e}"))?;
    let mut reader = file.take(MAX_CACHED_IMAGE_BYTES as u64 + 1);
    reader
        .read_to_end(&mut data)
        .map_err(|e| format!("failed to read image: {e}"))?;
    if data.len() > MAX_CACHED_IMAGE_BYTES {
        return Err(format!(
            "image is too large (max {} MB)",
            MAX_CACHED_IMAGE_BYTES / (1024 * 1024)
        ));
    }
    Ok(data)
}

fn safe_cached_path(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Some(dir.join(name)),
        _ => None,
    }
}

fn reference_from_row(
    conn: &Connection,
    root: &Path,
    entity_type: &str,
    entity_id: i64,
    source: String,
    file_name: Option<String>,
    mime_type: Option<String>,
) -> Result<Option<CachedImage>, String> {
    let Some(file_name) = file_name else {
        return if source == "none" {
            Ok(Some(CachedImage::none()))
        } else {
            conn.execute(
                "DELETE FROM images WHERE entity_type = ? AND entity_id = ? AND source = ?",
                rusqlite::params![entity_type, entity_id, source],
            )
            .map_err(|e| e.to_string())?;
            Ok(None)
        };
    };

    let dir = images_dir(root, entity_type);
    let Some(path) = safe_cached_path(&dir, &file_name) else {
        conn.execute(
            "DELETE FROM images WHERE entity_type = ? AND entity_id = ? AND source = ?",
            rusqlite::params![entity_type, entity_id, source],
        )
        .map_err(|e| e.to_string())?;
        return Ok(None);
    };
    if !path.is_file() {
        conn.execute(
            "DELETE FROM images WHERE entity_type = ? AND entity_id = ? AND source = ?",
            rusqlite::params![entity_type, entity_id, source],
        )
        .map_err(|e| e.to_string())?;
        return Ok(None);
    }

    Ok(Some(CachedImage {
        source,
        file_path: Some(path.to_string_lossy().to_string()),
        mime_type: mime_type.unwrap_or_else(|| "image/jpeg".to_string()),
    }))
}

/// The most recently fetched non-expired image for an entity, any source.
pub fn get_image(
    conn: &Connection,
    root: &Path,
    entity_type: &str,
    entity_id: i64,
) -> Result<Option<CachedImage>, String> {
    let row: Option<(String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT source, file_path, mime_type FROM images \
             WHERE entity_type = ? AND entity_id = ? \
             ORDER BY fetched_at DESC, rowid DESC LIMIT 1",
            rusqlite::params![entity_type, entity_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match row {
        Some((source, file_name, mime_type)) => reference_from_row(
            conn,
            root,
            entity_type,
            entity_id,
            source,
            file_name,
            mime_type,
        ),
        None => Ok(None),
    }
}

/// The cached non-custom image for an entity. Providers use this so a user
/// image only wins when the corresponding custom provider is enabled.
pub fn get_non_custom_image(
    conn: &Connection,
    root: &Path,
    entity_type: &str,
    entity_id: i64,
) -> Result<Option<CachedImage>, String> {
    let row: Option<(String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT source, file_path, mime_type FROM images \
             WHERE entity_type = ? AND entity_id = ? AND source != 'custom' \
             ORDER BY fetched_at DESC, rowid DESC LIMIT 1",
            rusqlite::params![entity_type, entity_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match row {
        Some((source, file_name, mime_type)) => reference_from_row(
            conn,
            root,
            entity_type,
            entity_id,
            source,
            file_name,
            mime_type,
        ),
        None => Ok(None),
    }
}

pub fn get_image_from_source(
    conn: &Connection,
    root: &Path,
    entity_type: &str,
    entity_id: i64,
    source: &str,
) -> Result<Option<CachedImage>, String> {
    let row: Option<(String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT source, file_path, mime_type FROM images \
             WHERE entity_type = ? AND entity_id = ? AND source = ?",
            rusqlite::params![entity_type, entity_id, source],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match row {
        Some((source, file_name, mime_type)) => reference_from_row(
            conn,
            root,
            entity_type,
            entity_id,
            source,
            file_name,
            mime_type,
        ),
        None => Ok(None),
    }
}

/// The user-provided custom image for an entity, if any. Custom images do
/// not expire and always win over online sources.
pub fn get_custom_image(
    conn: &Connection,
    root: &Path,
    entity_type: &str,
    entity_id: i64,
) -> Result<Option<CachedImage>, String> {
    get_image_from_source(conn, root, entity_type, entity_id, "custom")
}

/// Reads image bytes only for consumers that truly require them (for example,
/// Windows' native media-session API). Webview rendering should use the
/// `CachedImage.file_path` asset URL instead.
pub fn read_cached_image(image: &CachedImage) -> Result<ImageData, String> {
    let data = image
        .file_path
        .as_deref()
        .map(std::fs::read)
        .transpose()
        .map_err(|e| e.to_string())?;
    Ok(ImageData {
        source: image.source.clone(),
        data,
        mime_type: image.mime_type.clone(),
    })
}

pub fn set_image(
    conn: &Connection,
    root: &Path,
    entity_type: &str,
    entity_id: i64,
    source: &str,
    url: Option<&str>,
    image_data: Option<&[u8]>,
) -> Result<CachedImage, String> {
    // Remove the previous file for this (entity, source) if the name changed.
    let old: Option<String> = conn
        .query_row(
            "SELECT file_path FROM images WHERE entity_type = ? AND entity_id = ? AND source = ?",
            rusqlite::params![entity_type, entity_id, source],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();

    let dir = images_dir(root, entity_type);
    let (file_name, mime) = match image_data {
        Some(data) => {
            let mime = crate::models::detect_image_mime_type(data);
            let name = image_file_name(entity_type, entity_id, source, &mime);
            write_bytes(&dir, &name, data)?;
            (Some(name), Some(mime))
        }
        None => (None, None),
    };
    if let Some(old) = old {
        if Some(&old) != file_name.as_ref() {
            remove_file(&dir, &old);
        }
    }
    conn.execute(
        "INSERT OR REPLACE INTO images (entity_type, entity_id, source, url, file_path, mime_type, fetched_at, expires_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, unixepoch(), ?7)",
        rusqlite::params![
            entity_type,
            entity_id,
            source,
            url,
            file_name.as_deref(),
            mime.as_deref(),
            NEVER_EXPIRES
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(CachedImage {
        source: source.to_string(),
        file_path: file_name
            .as_deref()
            .map(|name| dir.join(name).to_string_lossy().to_string()),
        mime_type: mime.unwrap_or_else(|| "image/jpeg".to_string()),
    })
}

/// Deletes cached images for an entity. With `keep_custom`, the user's own
/// image survives (used when refetching online data); otherwise everything
/// goes (used when the entity itself is deleted).
pub fn delete_images(
    conn: &Connection,
    root: &Path,
    entity_type: &str,
    entity_id: i64,
    keep_custom: bool,
) -> Result<(), String> {
    let paths: Vec<String> = if keep_custom {
        let mut stmt = conn
            .prepare(
                "SELECT file_path FROM images WHERE entity_type = ? AND entity_id = ? AND source != 'custom' AND file_path IS NOT NULL",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![entity_type, entity_id], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT file_path FROM images WHERE entity_type = ? AND entity_id = ? AND file_path IS NOT NULL",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![entity_type, entity_id], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    let dir = images_dir(root, entity_type);
    for path in &paths {
        remove_file(&dir, path);
    }
    if keep_custom {
        conn.execute(
            "DELETE FROM images WHERE entity_type = ? AND entity_id = ? AND source != 'custom'",
            rusqlite::params![entity_type, entity_id],
        )
        .map_err(|e| e.to_string())?;
    } else {
        conn.execute(
            "DELETE FROM images WHERE entity_type = ? AND entity_id = ?",
            rusqlite::params![entity_type, entity_id],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ------------------------------------------------------- clearing & stats

pub fn clear_lyrics(conn: &Connection) -> Result<(), String> {
    // Custom lyrics are user content, not online cache data.
    conn.execute("DELETE FROM lyrics WHERE source != 'custom'", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn clear_artist_info(conn: &Connection, root: &Path) -> Result<(), String> {
    conn.execute("DELETE FROM artist_info", [])
        .map_err(|e| e.to_string())?;
    remove_dir_contents(&artist_info_dir(root));
    Ok(())
}

/// Clears online images. Custom user images are kept.
pub fn clear_images(conn: &Connection, root: &Path) -> Result<(), String> {
    let paths: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT entity_type, file_path FROM images WHERE source != 'custom' AND file_path IS NOT NULL")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    for (entity_type, path) in paths {
        remove_file(&images_dir(root, &entity_type), &path);
    }
    conn.execute("DELETE FROM images WHERE source != 'custom'", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn remove_dir_contents(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn dir_stats(dir: &Path) -> (i64, i64) {
    let mut items = 0;
    let mut bytes = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    items += 1;
                    bytes += meta.len() as i64;
                }
            }
        }
    }
    (items, bytes)
}

/// (items, bytes) per cache category: lyrics measured in the database,
/// artist info and images on disk.
pub fn cache_stats(conn: &Connection, root: &Path) -> Vec<(&'static str, i64, i64)> {
    let (li, lb): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(LENGTH(COALESCE(synced_text, '')) + LENGTH(COALESCE(plain_text, ''))), 0) FROM lyrics",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or((0, 0));
    let (ai, ab) = dir_stats(&artist_info_dir(root));
    let (ia, ib) = dir_stats(&images_dir(root, "artist"));
    let (la, lb2) = dir_stats(&images_dir(root, "album"));
    vec![
        ("Lyrics", li, lb),
        ("Artist info", ai, ab),
        ("Images", ia + la, ib + lb2),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn test_root() -> PathBuf {
        let unique = format!(
            "sparkle-cache-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(unique);
        ensure_dirs(&dir);
        dir
    }

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE lyrics (track_id INTEGER NOT NULL, source TEXT NOT NULL, synced_text TEXT, plain_text TEXT, fetched_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, PRIMARY KEY (track_id, source)); \
             CREATE TABLE artist_info (artist_id INTEGER PRIMARY KEY, source TEXT NOT NULL, file_path TEXT, fetched_at INTEGER NOT NULL, expires_at INTEGER NOT NULL); \
             CREATE TABLE images (entity_type TEXT NOT NULL, entity_id INTEGER NOT NULL, source TEXT NOT NULL, url TEXT, file_path TEXT, mime_type TEXT, fetched_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, PRIMARY KEY (entity_type, entity_id, source));"
        ).unwrap();
        conn
    }

    fn jpeg_bytes(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbImage::from_pixel(width, height, image::Rgb([32, 96, 160]));
        let mut data = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, 90)
            .encode_image(&image)
            .unwrap();
        data
    }

    #[test]
    fn lyrics_roundtrip() {
        let conn = in_memory_db();
        set_lyrics(&conn, 1, "lrc", Some("[00:00] hello"), Some("hello")).unwrap();
        let lyrics = get_lyrics_from_source(&conn, 1, "lrc")
            .unwrap()
            .expect("lyrics found");
        assert_eq!(lyrics.source, "lrc");
        assert_eq!(lyrics.synced_text.as_deref(), Some("[00:00] hello"));
        assert_eq!(lyrics.plain_text.as_deref(), Some("hello"));
        delete_lyrics(&conn, 1).unwrap();
        assert!(get_lyrics_from_source(&conn, 1, "lrc").unwrap().is_none());
    }

    #[test]
    fn cached_lyrics_persist_until_cleared() {
        let conn = in_memory_db();
        set_lyrics(&conn, 1, "lrc", Some("text"), None).unwrap();
        assert!(get_lyrics_from_source(&conn, 1, "lrc").unwrap().is_some());
    }

    #[test]
    fn clearing_lyrics_keeps_custom_content() {
        let conn = in_memory_db();
        set_lyrics(&conn, 1, "custom", Some("user"), None).unwrap();
        set_lyrics(&conn, 1, "lrclib", Some("online"), None).unwrap();
        clear_lyrics(&conn).unwrap();
        assert!(get_lyrics_from_source(&conn, 1, "custom")
            .unwrap()
            .is_some());
        assert!(get_lyrics_from_source(&conn, 1, "lrclib")
            .unwrap()
            .is_none());
    }

    #[test]
    fn custom_lyrics_file_is_copied_and_removed_with_track() {
        let root = test_root();
        let source = root.join("picked.lrc");
        std::fs::write(&source, "[00:01.00]hello").unwrap();
        copy_custom_lyrics_file(&root, 12, &source, "lrc").unwrap();
        let managed = lyrics_dir(&root).join("custom-12.lrc");
        assert_eq!(
            std::fs::read_to_string(&managed).unwrap(),
            "[00:01.00]hello"
        );
        std::fs::remove_file(source).unwrap();
        assert!(managed.exists());
        delete_custom_lyrics_file(&root, 12);
        assert!(!managed.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn image_roundtrip_via_files() {
        let conn = in_memory_db();
        let root = test_root();
        let input = jpeg_bytes(8, 8);
        set_image(
            &conn,
            &root,
            "artist",
            1,
            "wikipedia:en",
            Some("http://x"),
            Some(&input),
        )
        .unwrap();
        let image = get_image(&conn, &root, "artist", 1)
            .unwrap()
            .expect("image found");
        assert_eq!(image.source, "wikipedia:en");
        let path = PathBuf::from(image.file_path.expect("cache file path"));
        assert_eq!(std::fs::read(&path).unwrap(), input);
        // File names are opaque hashes, not raw ids.
        let files: Vec<_> = std::fs::read_dir(images_dir(&root, "artist"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1);
        let name = files[0].file_name().to_string_lossy().to_string();
        assert!(name.ends_with(".jpg"));
        assert!(
            name.len() == 20 && name[..16].chars().all(|c| c.is_ascii_hexdigit()),
            "expected a 16-char hex hash file name, got {name}"
        );
        delete_images(&conn, &root, "artist", 1, false).unwrap();
        assert!(get_image(&conn, &root, "artist", 1).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn image_validation_keeps_original_bytes() {
        let original = jpeg_bytes(2048, 1024);
        assert_eq!(
            validate_image_for_cache(original.clone()).unwrap(),
            original
        );
    }

    #[test]
    fn image_file_reads_are_size_limited() {
        let root = test_root();
        let path = root.join("too-large-image.jpg");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_CACHED_IMAGE_BYTES as u64 + 1)
            .unwrap();
        assert!(read_image_file(&path)
            .unwrap_err()
            .contains("image is too large"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_image_file_becomes_a_cache_miss() {
        let conn = in_memory_db();
        let root = test_root();
        let image = set_image(
            &conn,
            &root,
            "album",
            4,
            "embedded",
            None,
            Some(&jpeg_bytes(8, 8)),
        )
        .unwrap();
        std::fs::remove_file(image.file_path.expect("cache file path")).unwrap();
        assert!(get_image(&conn, &root, "album", 4).unwrap().is_none());
        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM images WHERE entity_type = 'album' AND entity_id = 4",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_images_can_keep_custom() {
        let conn = in_memory_db();
        let root = test_root();
        set_image(&conn, &root, "album", 7, "custom", None, Some(&[9, 9])).unwrap();
        set_image(&conn, &root, "album", 7, "embedded", None, Some(&[1])).unwrap();
        delete_images(&conn, &root, "album", 7, true).unwrap();
        let remaining = get_custom_image(&conn, &root, "album", 7).unwrap();
        assert!(remaining.is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn clearing_images_keeps_custom() {
        let conn = in_memory_db();
        let root = test_root();
        set_image(&conn, &root, "artist", 7, "custom", None, Some(&[9, 9])).unwrap();
        set_image(&conn, &root, "artist", 7, "wikipedia:en", None, Some(&[1])).unwrap();
        clear_images(&conn, &root).unwrap();
        assert!(get_custom_image(&conn, &root, "artist", 7)
            .unwrap()
            .is_some());
        assert!(get_image(&conn, &root, "artist", 7).unwrap().is_some());
        let _ = std::fs::remove_dir_all(&root);
    }
}
