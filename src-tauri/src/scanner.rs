use crate::cache;
use crate::models::{Folder, ScanProgress, ScanResult};
use crate::normalizer::{normalize_artist_name, split_artists};
use crate::settings::Settings;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, Tag};
use regex::Regex;
use rusqlite::{Connection, OptionalExtension, Transaction};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SUPPORTED_EXTENSIONS: [&str; 7] = ["mp3", "flac", "ogg", "m4a", "aac", "alac", "opus"];

struct FileResult {
    added: bool,
    updated: bool,
}

/// Scans all enabled folders. With `force`, every file is re-parsed even if
/// its mtime is unchanged — needed when artist-splitting rules change, since
/// the derived artists must be rebuilt from the same files. Tracks whose
/// files disappeared, albums with no tracks, and artists with no tracks or
/// albums are pruned so the library reflects the folders' current state.
pub fn scan_library(
    conn: &mut Connection,
    settings: &Settings,
    force: bool,
    cache_root: &Path,
) -> Result<ScanResult, String> {
    scan_library_with_progress(conn, settings, force, cache_root, |_| {})
}

pub fn scan_library_with_progress<F>(
    conn: &mut Connection,
    settings: &Settings,
    force: bool,
    cache_root: &Path,
    mut on_progress: F,
) -> Result<ScanResult, String>
where
    F: FnMut(ScanProgress),
{
    let regex = Regex::new(&settings.artist_split_regex).map_err(|e| e.to_string())?;
    let folders = list_enabled_folders(conn)?;
    let mut result = ScanResult {
        scanned: 0,
        added: 0,
        updated: 0,
        removed: 0,
        errors: 0,
    };
    let mut artist_cache: HashMap<String, i64> = HashMap::new();
    let mut album_cache: HashMap<(String, Option<i64>), i64> = HashMap::new();

    let mut files = Vec::new();
    for folder in &folders {
        for path in collect_audio_files(&folder.path)? {
            files.push(path);
        }
    }
    let total = files.len();
    on_progress(ScanProgress {
        phase: "scanning".to_string(),
        current_path: None,
        scanned: 0,
        total,
        added: 0,
        updated: 0,
        removed: 0,
        errors: 0,
    });

    // One transaction for the entire scan instead of one per file — this is
    // the single biggest scan-speed win (500 files = 1 fsync instead of 500).
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    let mut seen_paths: HashSet<String> = HashSet::new();

    for path in files {
        seen_paths.insert(path.clone());
        match process_file(
            &tx,
            &path,
            &regex,
            &settings.artist_split_exceptions,
            force,
            &mut artist_cache,
            &mut album_cache,
        ) {
            Ok(file_result) => {
                result.scanned += 1;
                if file_result.added {
                    result.added += 1;
                }
                if file_result.updated {
                    result.updated += 1;
                }
            }
            Err(_e) => {
                result.errors += 1;
            }
        }
        on_progress(ScanProgress {
            phase: "scanning".to_string(),
            current_path: Some(path),
            scanned: result.scanned,
            total,
            added: result.added,
            updated: result.updated,
            removed: result.removed,
            errors: result.errors,
        });
    }

    for folder in &folders {
        tx.execute(
            "UPDATE folders SET scanned_at = ? WHERE id = ?",
            rusqlite::params![now_seconds(), folder.id],
        )
        .map_err(|e| e.to_string())?;
    }

    result.removed = prune_stale_tracks(&tx, &folders, &seen_paths, cache_root)?;
    on_progress(ScanProgress {
        phase: "cleaning".to_string(),
        current_path: None,
        scanned: result.scanned,
        total,
        added: result.added,
        updated: result.updated,
        removed: result.removed,
        errors: result.errors,
    });
    prune_orphan_albums(&tx, cache_root)?;
    prune_orphan_artists(&tx, cache_root)?;

    tx.commit().map_err(|e| e.to_string())?;

    recompute_artist_stats(conn)?;

    Ok(result)
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Deletes tracks under monitored folders whose files no longer exist.
fn prune_stale_tracks(
    tx: &Transaction,
    folders: &[Folder],
    seen_paths: &HashSet<String>,
    cache_root: &Path,
) -> Result<usize, String> {
    let mut stale: HashSet<i64> = HashSet::new();
    for folder in folders {
        let pattern = format!("{}%", escape_like(&folder.path));
        let mut stmt = tx
            .prepare("SELECT id, file_path FROM tracks WHERE file_path LIKE ?1 ESCAPE '\\'")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (id, path) = row.map_err(|e| e.to_string())?;
            // The SQL prefix is only a coarse filter. Requiring a path
            // separator boundary prevents a folder such as `C:\Music` from
            // claiming tracks under `C:\Music Backup`.
            if path_is_within_folder(&path, &folder.path) && !seen_paths.contains(&path) {
                stale.insert(id);
            }
        }
    }
    let removed = stale.len();
    let mut stale: Vec<i64> = stale.into_iter().collect();
    stale.sort_unstable();
    for id in stale {
        tx.execute("DELETE FROM track_artists WHERE track_id = ?", [id])
            .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM playlist_tracks WHERE track_id = ?", [id])
            .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM play_queue WHERE track_id = ?", [id])
            .map_err(|e| e.to_string())?;
        cache::delete_lyrics(tx, id)?;
        cache::delete_custom_lyrics_file(cache_root, id);
        tx.execute("DELETE FROM tracks WHERE id = ?", [id])
            .map_err(|e| e.to_string())?;
    }
    Ok(removed)
}

/// Deletes albums that no longer have any tracks, with their cached art.
fn prune_orphan_albums(tx: &Transaction, cache_root: &Path) -> Result<(), String> {
    let mut stmt = tx
        .prepare("SELECT id FROM albums al WHERE NOT EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = al.id)")
        .map_err(|e| e.to_string())?;
    let ids: Vec<i64> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for id in ids {
        tx.execute("DELETE FROM album_artists WHERE album_id = ?", [id])
            .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM artist_albums WHERE album_id = ?", [id])
            .map_err(|e| e.to_string())?;
        cache::delete_images(tx, cache_root, "album", id, false)?;
        tx.execute("DELETE FROM albums WHERE id = ?", [id])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Deletes artists that no longer have any tracks or albums, with their
/// cached info and images.
fn prune_orphan_artists(tx: &Transaction, cache_root: &Path) -> Result<(), String> {
    let mut stmt = tx
        .prepare(
            "SELECT id FROM artists a \
             WHERE NOT EXISTS (SELECT 1 FROM track_artists ta WHERE ta.artist_id = a.id) \
             AND NOT EXISTS (SELECT 1 FROM album_artists aa WHERE aa.artist_id = a.id)",
        )
        .map_err(|e| e.to_string())?;
    let ids: Vec<i64> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for id in ids {
        tx.execute("DELETE FROM artist_albums WHERE artist_id = ?", [id])
            .map_err(|e| e.to_string())?;
        cache::delete_artist_info(tx, cache_root, id)?;
        cache::delete_images(tx, cache_root, "artist", id, false)?;
        tx.execute("DELETE FROM artists WHERE id = ?", [id])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn list_enabled_folders(conn: &Connection) -> Result<Vec<Folder>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, path, enabled, scanned_at FROM folders WHERE enabled = 1 ORDER BY path",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Folder {
                id: row.get(0)?,
                path: row.get(1)?,
                enabled: row.get::<_, i32>(2)? != 0,
                scanned_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn collect_audio_files(folder: &str) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    let mut stack = vec![PathBuf::from(folder)];
    let mut visited_directories = HashSet::new();
    while let Some(dir) = stack.pop() {
        if !mark_directory_visited(&dir, &mut visited_directories)? {
            continue;
        }
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if is_audio_file(&path) {
                files.push(path.to_string_lossy().to_string());
            }
        }
    }
    Ok(files)
}

fn mark_directory_visited(
    directory: &Path,
    visited_directories: &mut HashSet<PathBuf>,
) -> Result<bool, String> {
    let canonical_directory =
        std::fs::canonicalize(directory).map_err(|e| format!("{}: {}", directory.display(), e))?;
    Ok(visited_directories.insert(canonical_directory))
}

fn path_is_within_folder(path: &str, folder: &str) -> bool {
    let path = comparable_path(path);
    let folder = comparable_path(folder);

    if path == folder {
        return true;
    }

    let separator = std::path::MAIN_SEPARATOR;
    if folder.ends_with(separator) {
        path.starts_with(&folder)
    } else {
        path.strip_prefix(&folder)
            .is_some_and(|remainder| remainder.starts_with(separator))
    }
}

fn comparable_path(path: &str) -> String {
    #[cfg(windows)]
    let comparable = path.replace('/', "\\").to_lowercase();
    #[cfg(not(windows))]
    let comparable = path.to_string();

    let separator = std::path::MAIN_SEPARATOR;
    let trimmed = comparable.trim_end_matches(separator);
    if trimmed.is_empty() {
        separator.to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let lower = e.to_lowercase();
            SUPPORTED_EXTENSIONS.iter().any(|ext| *ext == lower)
        })
        .unwrap_or(false)
}

fn process_file(
    tx: &Transaction,
    path: &str,
    regex: &Regex,
    exceptions: &[String],
    force: bool,
    artist_cache: &mut HashMap<String, i64>,
    album_cache: &mut HashMap<(String, Option<i64>), i64>,
) -> Result<FileResult, String> {
    let file_metadata = std::fs::metadata(path).ok();
    let file_mtime = file_metadata
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let file_size_bytes = file_metadata.map(|m| m.len() as i64);

    // Skip files that have not changed since they were last scanned. This is
    // what makes rescans fast — the tag is only parsed when the file changed.
    // A forced scan (e.g. after artist-splitting rules changed) re-parses
    // everything regardless.
    let existing: Option<(i64, i64, bool)> = tx
        .query_row(
            "SELECT id, file_mtime, \
                    audio_format IS NOT NULL AND sample_rate_hz IS NOT NULL \
                    AND channels IS NOT NULL AND file_size_bytes IS NOT NULL \
             FROM tracks WHERE file_path = ?",
            [path],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    if !force {
        if let Some((_, stored_mtime, technical_complete)) = existing {
            if stored_mtime == file_mtime && file_mtime != 0 && technical_complete {
                return Ok(FileResult {
                    added: false,
                    updated: false,
                });
            }
        }
    }

    let tagged_file = Probe::open(path)
        .and_then(|p| p.read())
        .map_err(|e| e.to_string())?;
    let props = tagged_file.properties();
    let duration_ms = props.duration().as_millis() as i64;
    let audio_format = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase());
    let audio_bitrate_kbps = props.audio_bitrate().map(i64::from);
    let sample_rate_hz = props.sample_rate().map(i64::from);
    let bit_depth = props.bit_depth().map(i64::from);
    let channels = props.channels().map(i64::from);
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
        .ok_or_else(|| "no tags found".to_string())?;

    let title = tag.title().map(|s| s.to_string());
    let album_title = tag.album().map(|s| s.to_string());
    let genre = tag.genre().map(|s| s.to_string());
    let track_number = tag.track().map(|n| n as i64);
    let disc_number = tag.disk().map(|n| n as i64);
    let year = parse_year(tag);
    let lyrics = tag
        .get_string(ItemKey::Lyrics)
        .or_else(|| tag.get_string(ItemKey::UnsyncLyrics))
        .map(|s| s.to_string());

    let track_artist_names = collect_artists(
        tag,
        ItemKey::TrackArtists,
        ItemKey::TrackArtist,
        regex,
        exceptions,
    );
    let album_artist_names = collect_artists(
        tag,
        ItemKey::AlbumArtists,
        ItemKey::AlbumArtist,
        regex,
        exceptions,
    );

    let existing_id = existing.map(|(id, _, _)| id);
    let added = existing_id.is_none();
    let updated = existing_id.is_some();
    let album_id = if let Some(ref album_title) = album_title {
        let (id, _inserted) = get_or_insert_album(tx, album_title, year, album_cache)?;
        Some(id)
    } else {
        None
    };

    let track_id = if let Some(id) = existing_id {
        tx.execute(
            "UPDATE tracks SET title = ?1, track_number = ?2, disc_number = ?3, duration_ms = ?4, \
             year = ?5, genre = ?6, album_id = ?7, embedded_lyrics = ?8, updated_at = ?9, file_mtime = ?10, \
             audio_format = ?11, audio_bitrate_kbps = ?12, sample_rate_hz = ?13, bit_depth = ?14, \
             channels = ?15, file_size_bytes = ?16 WHERE id = ?17",
            rusqlite::params![
                title,
                track_number,
                disc_number,
                duration_ms,
                year,
                genre,
                album_id,
                lyrics,
                now_seconds(),
                file_mtime,
                audio_format,
                audio_bitrate_kbps,
                sample_rate_hz,
                bit_depth,
                channels,
                file_size_bytes,
                id
            ],
        )
        .map_err(|e| e.to_string())?;
        id
    } else {
        tx.execute(
            "INSERT INTO tracks (file_path, title, track_number, disc_number, duration_ms, \
             year, genre, album_id, embedded_lyrics, created_at, updated_at, file_mtime, \
             audio_format, audio_bitrate_kbps, sample_rate_hz, bit_depth, channels, file_size_bytes) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            rusqlite::params![
                path,
                title,
                track_number,
                disc_number,
                duration_ms,
                year,
                genre,
                album_id,
                lyrics,
                now_seconds(),
                file_mtime,
                audio_format,
                audio_bitrate_kbps,
                sample_rate_hz,
                bit_depth,
                channels,
                file_size_bytes
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.last_insert_rowid()
    };

    tx.execute(
        "DELETE FROM track_artists WHERE track_id = ? AND role = 'main'",
        [track_id],
    )
    .map_err(|e| e.to_string())?;
    for name in &track_artist_names {
        let artist_id = get_or_insert_artist(tx, name, artist_cache)?;
        tx.execute(
            "INSERT OR IGNORE INTO track_artists (track_id, artist_id, role) VALUES (?1, ?2, 'main')",
            [track_id, artist_id],
        )
        .map_err(|e| e.to_string())?;
    }

    if let Some(album_id) = album_id {
        tx.execute("DELETE FROM album_artists WHERE album_id = ?", [album_id])
            .map_err(|e| e.to_string())?;
        let artists_to_link = if album_artist_names.is_empty() {
            track_artist_names.clone()
        } else {
            album_artist_names.clone()
        };
        for name in &artists_to_link {
            let artist_id = get_or_insert_artist(tx, name, artist_cache)?;
            tx.execute(
                "INSERT OR IGNORE INTO album_artists (album_id, artist_id) VALUES (?1, ?2)",
                [album_id, artist_id],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    Ok(FileResult { added, updated })
}

fn collect_artists(
    tag: &Tag,
    multi_key: ItemKey,
    single_key: ItemKey,
    regex: &Regex,
    exceptions: &[String],
) -> Vec<String> {
    let raw: Vec<String> = tag.get_strings(multi_key).map(|s| s.to_string()).collect();
    let names = if raw.is_empty() {
        tag.get_string(single_key)
            .map(|s| vec![s.to_string()])
            .unwrap_or_default()
    } else {
        raw
    };
    names
        .iter()
        .flat_map(|n| split_artists(n, regex, exceptions))
        .map(|n| normalize_artist_name(&n))
        .filter(|n| !n.is_empty())
        .collect()
}

fn get_or_insert_artist(
    tx: &Transaction,
    name: &str,
    cache: &mut HashMap<String, i64>,
) -> Result<i64, String> {
    if let Some(&id) = cache.get(name) {
        return Ok(id);
    }
    let existing: Option<i64> = tx
        .query_row("SELECT id FROM artists WHERE name = ?", [name], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())?;
    let id = if let Some(id) = existing {
        id
    } else {
        tx.execute("INSERT INTO artists (name) VALUES (?)", [name])
            .map_err(|e| e.to_string())?;
        tx.last_insert_rowid()
    };
    cache.insert(name.to_string(), id);
    Ok(id)
}

fn get_or_insert_album(
    tx: &Transaction,
    title: &str,
    year: Option<i64>,
    cache: &mut HashMap<(String, Option<i64>), i64>,
) -> Result<(i64, bool), String> {
    let key = (title.to_string(), year);
    if let Some(&id) = cache.get(&key) {
        return Ok((id, false));
    }
    let existing: Option<i64> = tx
        .query_row(
            "SELECT id FROM albums WHERE title = ? AND \
             ((? IS NULL AND year IS NULL) OR year = ?)",
            rusqlite::params![title, year, year],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (id, inserted) = if let Some(id) = existing {
        (id, false)
    } else {
        tx.execute(
            "INSERT INTO albums (title, year) VALUES (?1, ?2)",
            rusqlite::params![title, year],
        )
        .map_err(|e| e.to_string())?;
        (tx.last_insert_rowid(), true)
    };
    cache.insert(key, id);
    Ok((id, inserted))
}

fn parse_year(tag: &Tag) -> Option<i64> {
    tag.date().map(|date| date.year as i64).or_else(|| {
        tag.get_string(ItemKey::Year)
            .and_then(|s| s.parse::<i64>().ok())
    })
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn recompute_artist_stats(conn: &mut Connection) -> Result<(), String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    tx.execute("DELETE FROM artist_albums", [])
        .map_err(|e| e.to_string())?;

    tx.execute(
        "INSERT OR IGNORE INTO artist_albums (artist_id, album_id, role) \
         SELECT artist_id, album_id, 'album_artist' FROM album_artists",
        [],
    )
    .map_err(|e| e.to_string())?;

    tx.execute(
        "INSERT OR IGNORE INTO artist_albums (artist_id, album_id, role) \
         SELECT ta.artist_id, t.album_id, 'track_artist' \
         FROM track_artists ta \
         JOIN tracks t ON t.id = ta.track_id \
         WHERE t.album_id IS NOT NULL",
        [],
    )
    .map_err(|e| e.to_string())?;

    tx.execute(
        "WITH track_counts AS ( \
            SELECT artist_id, COUNT(DISTINCT track_id) AS c FROM track_artists WHERE role = 'main' GROUP BY artist_id \
         ) \
         UPDATE artists SET track_count = COALESCE((SELECT c FROM track_counts WHERE track_counts.artist_id = artists.id), 0)",
        [],
    )
    .map_err(|e| e.to_string())?;

    tx.execute(
        "WITH album_counts AS ( \
            SELECT artist_id, COUNT(DISTINCT album_id) AS c FROM artist_albums GROUP BY artist_id \
         ) \
         UPDATE artists SET album_count = COALESCE((SELECT c FROM album_counts WHERE album_counts.artist_id = artists.id), 0)",
        [],
    )
    .map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_directory(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "sparkle-scanner-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn folder_membership_requires_a_path_component_boundary() {
        let separator = std::path::MAIN_SEPARATOR;
        let folder = format!("library{separator}Music");
        let child = format!("{folder}{separator}Artist{separator}song.flac");
        let sibling = format!("library{separator}Music Backup{separator}song.flac");

        assert!(path_is_within_folder(&child, &folder));
        assert!(!path_is_within_folder(&sibling, &folder));
    }

    #[test]
    fn wav_is_not_advertised_as_a_supported_scan_format() {
        assert!(is_audio_file(Path::new("song.flac")));
        assert!(!is_audio_file(Path::new("song.wav")));
    }

    #[test]
    fn stale_pruning_does_not_delete_from_a_prefix_sibling() {
        let mut conn = Connection::open_in_memory().expect("open scanner test database");
        conn.execute_batch(
            "
            CREATE TABLE tracks (id INTEGER PRIMARY KEY, file_path TEXT NOT NULL);
            CREATE TABLE track_artists (track_id INTEGER NOT NULL);
            CREATE TABLE playlist_tracks (track_id INTEGER NOT NULL);
            CREATE TABLE play_queue (track_id INTEGER NOT NULL);
            CREATE TABLE lyrics (track_id INTEGER NOT NULL);
            ",
        )
        .expect("create scanner test tables");

        let separator = std::path::MAIN_SEPARATOR;
        let folder_path = format!("library{separator}Music");
        let stale_path = format!("{folder_path}{separator}stale.flac");
        let sibling_path = format!("library{separator}Music Backup{separator}outside.flac");
        conn.execute(
            "INSERT INTO tracks (id, file_path) VALUES (1, ?1), (2, ?2)",
            rusqlite::params![stale_path, sibling_path],
        )
        .expect("insert scanner test tracks");

        let tx = conn.transaction().expect("start scanner test transaction");
        let folders = [Folder {
            id: 1,
            path: folder_path,
            enabled: true,
            scanned_at: None,
        }];
        let cache_root = unique_test_directory("prune");
        std::fs::create_dir_all(&cache_root).expect("create scanner cache test directory");
        let removed = prune_stale_tracks(&tx, &folders, &HashSet::new(), &cache_root)
            .expect("prune stale scanner tracks");

        assert_eq!(removed, 1);
        assert_eq!(
            tx.query_row("SELECT file_path FROM tracks WHERE id = 2", [], |row| {
                row.get::<_, String>(0)
            })
            .expect("prefix sibling track remains"),
            sibling_path
        );
        drop(tx);
        std::fs::remove_dir(&cache_root).expect("remove scanner cache test directory");
    }

    #[cfg(windows)]
    #[test]
    fn folder_membership_handles_windows_case_and_separator_variants() {
        assert!(path_is_within_folder(
            "C:/MUSIC/Artist/song.flac",
            r"c:\music"
        ));
        assert!(!path_is_within_folder(
            r"C:\Music Backup\song.flac",
            r"C:\Music"
        ));
    }

    #[test]
    fn canonical_directory_identity_deduplicates_alias_paths() {
        let root = unique_test_directory("directory-identity");
        std::fs::create_dir_all(&root).expect("create temporary scanner directory");

        let mut visited = HashSet::new();

        assert!(mark_directory_visited(&root, &mut visited).expect("visit temporary directory"));
        assert!(!mark_directory_visited(&root.join("."), &mut visited)
            .expect("visit dot-directory alias"));

        std::fs::remove_dir(&root).expect("remove temporary scanner directory");
    }
}
