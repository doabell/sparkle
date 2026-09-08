use crate::cache;
use crate::models::{Folder, ScanProgress, ScanResult};
use crate::normalizer::split_artists;
use crate::settings::Settings;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, Tag};
use rusqlite::{Connection, OptionalExtension, Transaction};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SUPPORTED_EXTENSIONS: [&str; 7] = ["mp3", "flac", "ogg", "m4a", "aac", "alac", "opus"];
const SCAN_VERSION: i64 = 1;

struct FileResult {
    added: bool,
    updated: bool,
}

/// Scans all enabled folders. With `force`, every file is re-parsed even if
/// its mtime is unchanged, rebuilding the derived metadata. Missing files are
/// hidden from browsing, while their IDs, lyrics, playlists and history remain
/// available to reconnect when the audio is found again.
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
    _settings: &Settings,
    force: bool,
    cache_root: &Path,
    mut on_progress: F,
) -> Result<ScanResult, String>
where
    F: FnMut(ScanProgress),
{
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
    files.sort_by_cached_key(|path| comparable_path(path));
    files.dedup_by(|a, b| comparable_path(a) == comparable_path(b));
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

    let seen_paths: HashSet<String> = files.iter().map(|p| comparable_path(p)).collect();
    let (file_ids, fingerprints) = match_file_identities(conn, &files, &seen_paths)?;

    // One transaction for the entire scan instead of one per file — this is
    // the single biggest scan-speed win (500 files = 1 fsync instead of 500).
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    for path in files {
        tx.execute_batch("SAVEPOINT scan_file")
            .map_err(|e| e.to_string())?;
        match process_file(
            &tx,
            &path,
            force,
            file_ids.get(&comparable_path(&path)).copied(),
            fingerprints.get(&path),
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
            Err(e) => {
                tx.execute_batch("ROLLBACK TO scan_file")
                    .map_err(|e| e.to_string())?;
                artist_cache.clear();
                album_cache.clear();
                log::debug!(target: "sparkle::scanner", "event=file_failed path={path} error={e}");
                result.errors += 1;
            }
        }
        tx.execute_batch("RELEASE scan_file")
            .map_err(|e| e.to_string())?;
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
    rebuild_album_credits(&tx)?;
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

/// Archives missing files without destroying user-owned data. A file may be
/// temporarily unavailable, renamed before its first fingerprint was indexed,
/// or one of several identical copies whose identities cannot be disambiguated.
fn prune_stale_tracks(
    tx: &Transaction,
    folders: &[Folder],
    seen_paths: &HashSet<String>,
    _cache_root: &Path,
) -> Result<usize, String> {
    let mut stale: HashSet<i64> = HashSet::new();
    for folder in folders {
        let pattern = format!("{}%", escape_like(&folder.path));
        let mut stmt = tx
            .prepare("SELECT id, file_path FROM tracks WHERE missing_since IS NULL AND file_path LIKE ?1 ESCAPE '\\'")
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
            if path_is_within_folder(&path, &folder.path)
                && !seen_paths.contains(&comparable_path(&path))
            {
                stale.insert(id);
            }
        }
    }
    let removed = stale.len();
    let mut stale: Vec<i64> = stale.into_iter().collect();
    stale.sort_unstable();
    for id in stale {
        tx.execute("UPDATE tracks SET missing_since = unixepoch(), lyrics_revision = lyrics_revision + 1 WHERE id = ?", [id])
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

type IdentityMatches = (HashMap<String, i64>, HashMap<String, Option<String>>);

fn file_fingerprint(path: &str) -> Option<String> {
    match crate::audio_identity::fingerprint(Path::new(path)) {
        Ok(fingerprint) => Some(fingerprint),
        Err(error) => {
            // An unsupported/corrupt audio stream can still have readable tags.
            // Leave its identity unknown instead of guessing a match.
            log::debug!(target: "sparkle::scanner", "event=identity_unavailable path={path} error={error}");
            None
        }
    }
}

fn match_file_identities(
    conn: &Connection,
    files: &[String],
    seen_paths: &HashSet<String>,
) -> Result<IdentityMatches, String> {
    let mut stmt = conn
        .prepare("SELECT id, file_path, audio_fingerprint FROM tracks")
        .map_err(|e| e.to_string())?;
    let known = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut ids: HashMap<String, i64> = known
        .iter()
        .map(|(id, path, _)| (comparable_path(path), *id))
        .collect();
    let mut missing: HashMap<String, Vec<i64>> = HashMap::new();
    for (id, path, fingerprint) in known {
        if !seen_paths.contains(&comparable_path(&path))
            && matches!(Path::new(&path).try_exists(), Ok(false))
        {
            if let Some(fingerprint) = fingerprint {
                missing.entry(fingerprint).or_default().push(id);
            }
        }
    }
    let mut fingerprints = HashMap::new();
    let mut incoming: HashMap<String, Vec<String>> = HashMap::new();
    for path in files {
        if ids.contains_key(&comparable_path(path)) {
            continue;
        }
        let fingerprint = file_fingerprint(path);
        if let Some(ref fingerprint) = fingerprint {
            incoming
                .entry(fingerprint.clone())
                .or_default()
                .push(comparable_path(path));
        }
        fingerprints.insert(path.clone(), fingerprint);
    }
    for (fingerprint, paths) in incoming {
        if let Some(candidates) = missing.get(&fingerprint) {
            // Never steal an existing copy's identity, or choose between
            // identical missing/incoming copies based on directory scan order.
            if paths.len() == 1 && candidates.len() == 1 {
                ids.insert(paths[0].clone(), candidates[0]);
            }
        }
    }
    Ok((ids, fingerprints))
}

struct ExistingFile {
    id: i64,
    path: String,
    mtime_ns: Option<i64>,
    size: Option<i64>,
    complete: bool,
    scan_version: i64,
    missing: bool,
}

fn process_file(
    tx: &Transaction,
    path: &str,
    force: bool,
    matched_id: Option<i64>,
    fingerprint: Option<&Option<String>>,
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
    let file_size_bytes = file_metadata.as_ref().map(|m| m.len() as i64);
    let file_mtime_ns = file_metadata
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_nanos()).ok());

    // Skip files that have not changed since they were last scanned. This is
    // what makes rescans fast — the tag is only parsed when the file changed.
    // A forced scan re-parses everything regardless.
    let existing: Option<ExistingFile> = tx
        .query_row(
            "SELECT id, file_path, file_mtime_ns, file_size_bytes, \
                    audio_format IS NOT NULL AND sample_rate_hz IS NOT NULL \
                    AND channels IS NOT NULL AND file_size_bytes IS NOT NULL, \
                    scan_version, missing_since IS NOT NULL \
             FROM tracks WHERE id = ?1 OR file_path = ?2",
            rusqlite::params![matched_id, path],
            |row| {
                Ok(ExistingFile {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    mtime_ns: row.get(2)?,
                    size: row.get(3)?,
                    complete: row.get(4)?,
                    scan_version: row.get(5)?,
                    missing: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;

    if !force {
        if let Some(ref existing) = existing {
            if existing.mtime_ns == file_mtime_ns
                && file_mtime_ns.is_some()
                && existing.size == file_size_bytes
                && existing.complete
                && existing.scan_version == SCAN_VERSION
            {
                let updated = existing.path != path || existing.missing;
                if updated {
                    tx.execute("UPDATE tracks SET file_path = ?, missing_since = NULL, lyrics_revision = lyrics_revision + 1 WHERE id = ?",
                        rusqlite::params![path, existing.id]).map_err(|e| e.to_string())?;
                }
                return Ok(FileResult {
                    added: false,
                    updated,
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

    let track_artist_names = collect_artists(tag, ItemKey::TrackArtist, ItemKey::TrackArtists);
    let album_artist_names = collect_artists(tag, ItemKey::AlbumArtist, ItemKey::AlbumArtists);

    let fingerprint = fingerprint
        .cloned()
        .unwrap_or_else(|| file_fingerprint(path));
    let existing_id = existing.map(|file| file.id);
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
        "UPDATE tracks SET file_path = ?1, file_mtime_ns = ?2, audio_fingerprint = ?3, \
         scan_version = ?4, missing_since = NULL, lyrics_revision = lyrics_revision + 1 WHERE id = ?5",
        rusqlite::params![path, file_mtime_ns, fingerprint, SCAN_VERSION, track_id],
    ).map_err(|e| e.to_string())?;
    // Local sources are cheap to read again. Keep custom and remote results.
    cache::delete_lyrics_from_source(tx, track_id, "embedded")?;
    cache::delete_lyrics_from_source(tx, track_id, "lrc")?;

    tx.execute(
        "DELETE FROM track_artists WHERE track_id = ? AND role = 'main'",
        [track_id],
    )
    .map_err(|e| e.to_string())?;
    for (position, name) in track_artist_names.iter().enumerate() {
        let artist_id = get_or_insert_artist(tx, name, artist_cache)?;
        tx.execute(
            "INSERT OR IGNORE INTO track_artists (track_id, artist_id, role, position) VALUES (?1, ?2, 'main', ?3)",
            rusqlite::params![track_id, artist_id, position as i64],
        )
        .map_err(|e| e.to_string())?;
    }

    tx.execute(
        "DELETE FROM track_album_artists WHERE track_id = ?",
        [track_id],
    )
    .map_err(|e| e.to_string())?;
    for (position, name) in album_artist_names.iter().enumerate() {
        let artist_id = get_or_insert_artist(tx, name, artist_cache)?;
        tx.execute(
                "INSERT OR IGNORE INTO track_album_artists (track_id, artist_id, position) VALUES (?1, ?2, ?3)",
                rusqlite::params![track_id, artist_id, position as i64],
            )
            .map_err(|e| e.to_string())?;
    }

    Ok(FileResult { added, updated })
}

fn collect_artists(tag: &Tag, primary_key: ItemKey, fallback_key: ItemKey) -> Vec<String> {
    // Lofty exposes every NUL-separated TPE1/TPE2 value as a separate string,
    // as it does repeated Vorbis/MP4 artist values. Never split punctuation in
    // a name. Prefer the standard frames over an older ARTISTS custom field.
    let primary: Vec<&str> = tag.get_strings(primary_key).collect();
    let names = if primary.is_empty() {
        tag.get_strings(fallback_key).collect()
    } else {
        primary
    };
    let mut seen = HashSet::new();
    names
        .iter()
        .flat_map(|name| split_artists(name))
        .filter(|name| seen.insert(name.clone()))
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

fn rebuild_album_credits(conn: &Connection) -> Result<(), String> {
    // Album identity remains title + year. Collect explicit album credits in
    // disc/track/tag order instead of letting the last scanned file overwrite
    // every other track's credits. Only fall back to track artists when the
    // album has no explicit album-artist tags at all.
    conn.execute_batch(
        "DELETE FROM album_artists;
         WITH credits AS (
             SELECT t.album_id, c.artist_id, c.position, t.disc_number, t.track_number, t.title, t.id
             FROM tracks t JOIN track_album_artists c ON c.track_id = t.id
             WHERE t.album_id IS NOT NULL
             UNION ALL
             SELECT t.album_id, c.artist_id, c.position, t.disc_number, t.track_number, t.title, t.id
             FROM tracks t JOIN track_artists c ON c.track_id = t.id AND c.role = 'main'
             WHERE t.album_id IS NOT NULL AND NOT EXISTS (
                 SELECT 1 FROM tracks other JOIN track_album_artists explicit ON explicit.track_id = other.id
                 WHERE other.album_id = t.album_id)
         ), ordered AS (
             SELECT album_id, artist_id, ROW_NUMBER() OVER (
                 PARTITION BY album_id ORDER BY COALESCE(disc_number, 1), COALESCE(track_number, 2147483647),
                 title, id, position, artist_id) AS credit_order FROM credits
         ), first_credit AS (
             SELECT album_id, artist_id, MIN(credit_order) AS credit_order FROM ordered GROUP BY album_id, artist_id
         )
         INSERT INTO album_artists (album_id, artist_id, position)
             SELECT album_id, artist_id, ROW_NUMBER() OVER (PARTITION BY album_id ORDER BY credit_order) - 1
             FROM first_credit;",
    ).map_err(|e| e.to_string())
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
         SELECT artist_id, album_id, 'album_artist' FROM album_artists aa \
         WHERE EXISTS (SELECT 1 FROM available_tracks t WHERE t.album_id = aa.album_id)",
        [],
    )
    .map_err(|e| e.to_string())?;

    tx.execute(
        "INSERT OR IGNORE INTO artist_albums (artist_id, album_id, role) \
         SELECT ta.artist_id, t.album_id, 'track_artist' \
         FROM track_artists ta \
         JOIN available_tracks t ON t.id = ta.track_id \
         WHERE t.album_id IS NOT NULL",
        [],
    )
    .map_err(|e| e.to_string())?;

    tx.execute(
        "WITH track_counts AS ( \
            SELECT ta.artist_id, COUNT(DISTINCT ta.track_id) AS c FROM track_artists ta \
            JOIN available_tracks t ON t.id = ta.track_id WHERE ta.role = 'main' GROUP BY ta.artist_id \
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
#[path = "tests/scanner.rs"]
mod tests;
