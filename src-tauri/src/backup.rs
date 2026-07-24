use crate::cache;
use crate::settings::{self, Settings};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const BACKUP_FORMAT: &str = "sparkle-library-backup";
const BACKUP_VERSION: u32 = 3;
const MAX_BACKUP_BYTES: u64 = 512 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct BackupSections {
    pub settings: bool,
    pub playlists: bool,
    pub custom_metadata: bool,
    pub history: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BackupTrackRef {
    file_path: String,
    title: Option<String>,
    album_title: Option<String>,
    artist_names: Vec<String>,
    duration_ms: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BackupPlaylist {
    name: String,
    description: Option<String>,
    smart_query: Option<String>,
    track_keys: Vec<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BackupLyrics {
    track_key: usize,
    selected_source: Option<String>,
    synced_text: Option<String>,
    plain_text: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BackupArtistBio {
    artist_name: String,
    bio: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum BackupArtworkOwner {
    Artist {
        name: String,
    },
    Album {
        title: String,
        year: Option<i64>,
        artist_names: Vec<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BackupArtwork {
    owner: BackupArtworkOwner,
    mime_type: String,
    data: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BackupHistory {
    track_key: usize,
    started_at: i64,
    played_ms: i64,
    completed: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BackupData {
    format: String,
    version: u32,
    created_at: i64,
    app_version: String,
    settings: Option<Settings>,
    tracks: Vec<BackupTrackRef>,
    playlists: Vec<BackupPlaylist>,
    lyrics: Vec<BackupLyrics>,
    artist_bios: Vec<BackupArtistBio>,
    artwork: Vec<BackupArtwork>,
    listening_history: Vec<BackupHistory>,
}

#[derive(Serialize, Clone, Debug)]
pub struct BackupManifest {
    pub created_at: i64,
    pub app_version: String,
    pub file_version: u32,
    pub file_size_bytes: u64,
    pub settings: bool,
    pub tracks: usize,
    pub playlists: usize,
    pub playlist_tracks: usize,
    pub lyrics: usize,
    pub artist_bios: usize,
    pub artwork: usize,
    pub history: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct BackupImportSummary {
    pub settings: bool,
    pub playlists: usize,
    pub playlist_tracks: usize,
    pub lyrics: usize,
    pub artist_bios: usize,
    pub artwork: usize,
    pub history: usize,
    pub unmatched_tracks: usize,
    pub unmatched_artwork: usize,
}

struct TrackCatalogue {
    tracks: Vec<BackupTrackRef>,
    keys_by_id: HashMap<i64, usize>,
}

impl TrackCatalogue {
    fn new() -> Self {
        Self {
            tracks: Vec::new(),
            keys_by_id: HashMap::new(),
        }
    }

    fn key_for(&mut self, conn: &Connection, track_id: i64) -> Result<usize, String> {
        if let Some(key) = self.keys_by_id.get(&track_id) {
            return Ok(*key);
        }
        let (file_path, title, album_title, duration_ms) = conn
            .query_row(
                "SELECT t.file_path, t.title, al.title, t.duration_ms \
                 FROM tracks t LEFT JOIN albums al ON al.id = t.album_id WHERE t.id = ?",
                [track_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|e| e.to_string())?;
        let artist_names = artist_names_for_track(conn, track_id)?;
        let key = self.tracks.len();
        self.tracks.push(BackupTrackRef {
            file_path,
            title,
            album_title,
            artist_names,
            duration_ms,
        });
        self.keys_by_id.insert(track_id, key);
        Ok(key)
    }
}

#[derive(Debug)]
struct LocalTrack {
    id: i64,
    file_path: String,
    title: Option<String>,
    album_title: Option<String>,
    artist_names: Vec<String>,
    duration_ms: Option<i64>,
}

pub fn export(
    conn: &Connection,
    cache_dir: &Path,
    path: &Path,
    sections: BackupSections,
) -> Result<BackupManifest, String> {
    if !sections.settings && !sections.playlists && !sections.custom_metadata && !sections.history {
        return Err("select at least one section".to_string());
    }

    let settings = if sections.settings {
        let mut value = settings::load_settings(conn)?;
        value.brave_api_key.clear();
        value.discord_catbox_user_hash.clear();
        value.discord_artwork_s3_access_key.clear();
        value.discord_artwork_s3_secret_key.clear();
        value.discord_artwork_s3_session_token.clear();
        value.monitored_folders.clear();
        Some(value)
    } else {
        None
    };

    let mut catalogue = TrackCatalogue::new();
    let playlists = if sections.playlists {
        export_playlists(conn, &mut catalogue)?
    } else {
        Vec::new()
    };
    let (lyrics, artist_bios, artwork) = if sections.custom_metadata {
        (
            export_lyrics(conn, &mut catalogue)?,
            export_artist_bios(conn)?,
            export_artwork(conn, cache_dir)?,
        )
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };
    let listening_history = if sections.history {
        export_history(conn, &mut catalogue)?
    } else {
        Vec::new()
    };

    let data = BackupData {
        format: BACKUP_FORMAT.to_string(),
        version: BACKUP_VERSION,
        created_at: unix_timestamp(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        settings,
        tracks: catalogue.tracks,
        playlists,
        lyrics,
        artist_bios,
        artwork,
        listening_history,
    };
    let bytes = encode(&data)?;
    std::fs::write(path, &bytes).map_err(|e| format!("failed to save backup: {e}"))?;
    Ok(manifest(&data, bytes.len() as u64))
}

pub fn inspect(path: &Path) -> Result<BackupManifest, String> {
    let (data, file_size) = read(path)?;
    Ok(manifest(&data, file_size))
}

pub fn import(
    conn: &Connection,
    cache_dir: &Path,
    path: &Path,
    sections: BackupSections,
) -> Result<BackupImportSummary, String> {
    if !sections.settings && !sections.playlists && !sections.custom_metadata && !sections.history {
        return Err("select at least one section".to_string());
    }
    let (data, _) = read(path)?;
    let local_tracks = load_local_tracks(conn)?;
    let resolved_tracks = data
        .tracks
        .iter()
        .map(|track| resolve_track(track, &local_tracks))
        .collect::<Vec<_>>();

    let mut used_track_keys = Vec::new();
    if sections.playlists {
        for playlist in &data.playlists {
            used_track_keys.extend(playlist.track_keys.iter().copied());
        }
    }
    if sections.custom_metadata {
        used_track_keys.extend(data.lyrics.iter().map(|lyrics| lyrics.track_key));
    }
    if sections.history {
        used_track_keys.extend(data.listening_history.iter().map(|event| event.track_key));
    }
    used_track_keys.sort_unstable();
    used_track_keys.dedup();
    let unmatched_tracks = used_track_keys
        .iter()
        .filter(|key| resolved_tracks.get(**key).copied().flatten().is_none())
        .count();

    // Apply database-backed sections together. Artwork files are copied only
    // after this transaction commits.
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let settings_restored = if sections.settings {
        if let Some(mut backup_settings) = data.settings.clone() {
            let current = settings::load_settings(&tx)?;
            backup_settings.brave_api_key = current.brave_api_key;
            backup_settings.discord_catbox_user_hash = current.discord_catbox_user_hash;
            backup_settings.discord_artwork_s3_access_key = current.discord_artwork_s3_access_key;
            backup_settings.discord_artwork_s3_secret_key = current.discord_artwork_s3_secret_key;
            backup_settings.discord_artwork_s3_session_token =
                current.discord_artwork_s3_session_token;
            backup_settings.monitored_folders = current.monitored_folders;
            if !backup_settings
                .lyrics_sources
                .iter()
                .any(|source| source == "custom")
            {
                backup_settings
                    .lyrics_sources
                    .insert(0, "custom".to_string());
            }
            settings::save_settings(&tx, &backup_settings)?;
            true
        } else {
            false
        }
    } else {
        false
    };

    let (playlists, playlist_tracks) = if sections.playlists {
        restore_playlists(&tx, &data.playlists, &resolved_tracks)?
    } else {
        (0, 0)
    };

    let mut lyric_count = 0;
    let mut bio_count = 0;
    if sections.custom_metadata {
        for lyric in &data.lyrics {
            let Some(track_id) = resolved_tracks.get(lyric.track_key).copied().flatten() else {
                continue;
            };
            if lyric.synced_text.is_some() || lyric.plain_text.is_some() {
                cache::set_lyrics(
                    &tx,
                    track_id,
                    "custom",
                    lyric.synced_text.as_deref(),
                    lyric.plain_text.as_deref(),
                )?;
            }
            tx.execute(
                "UPDATE tracks SET lyrics_source = ? WHERE id = ?",
                rusqlite::params![lyric.selected_source, track_id],
            )
            .map_err(|e| e.to_string())?;
            lyric_count += 1;
        }

        for artist in &data.artist_bios {
            let changed = tx
                .execute(
                    "UPDATE artists SET bio = ? WHERE LOWER(TRIM(name)) = LOWER(TRIM(?))",
                    rusqlite::params![artist.bio, artist.artist_name],
                )
                .map_err(|e| e.to_string())?;
            bio_count += changed;
        }
    }

    let mut history_count = 0;
    if sections.history {
        for event in &data.listening_history {
            let Some(track_id) = resolved_tracks.get(event.track_key).copied().flatten() else {
                continue;
            };
            if event.played_ms < 0 || event.started_at <= 0 {
                continue;
            }
            history_count += tx
                .execute(
                    "INSERT OR IGNORE INTO play_history (track_id, started_at, played_ms, completed) \
                     VALUES (?, ?, ?, ?)",
                    rusqlite::params![
                        track_id,
                        event.started_at,
                        event.played_ms,
                        event.completed as i64
                    ],
                )
                .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;

    let mut artwork_count = 0;
    let mut unmatched_artwork = 0;
    if sections.custom_metadata {
        for image in &data.artwork {
            let Some((entity_type, entity_id)) = resolve_artwork_owner(conn, &image.owner)? else {
                unmatched_artwork += 1;
                continue;
            };
            cache::set_image(
                conn,
                cache_dir,
                entity_type,
                entity_id,
                "custom",
                None,
                Some(&image.data),
            )?;
            artwork_count += 1;
        }
    }

    Ok(BackupImportSummary {
        settings: settings_restored,
        playlists,
        playlist_tracks,
        lyrics: lyric_count,
        artist_bios: bio_count,
        artwork: artwork_count,
        history: history_count,
        unmatched_tracks,
        unmatched_artwork,
    })
}

fn export_playlists(
    conn: &Connection,
    catalogue: &mut TrackCatalogue,
) -> Result<Vec<BackupPlaylist>, String> {
    let rows = {
        let mut stmt = conn
            .prepare("SELECT id, name, description, smart_query FROM playlists ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<(i64, String, Option<String>, Option<String>)>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    let mut result = Vec::with_capacity(rows.len());
    for (id, name, description, smart_query) in rows {
        let track_ids = {
            let mut stmt = conn
                .prepare(
                    "SELECT track_id FROM playlist_tracks WHERE playlist_id = ? ORDER BY position",
                )
                .map_err(|e| e.to_string())?;
            let track_ids = stmt
                .query_map([id], |row| row.get(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<i64>, _>>()
                .map_err(|e| e.to_string())?;
            track_ids
        };
        let track_keys = track_ids
            .into_iter()
            .map(|track_id| catalogue.key_for(conn, track_id))
            .collect::<Result<Vec<_>, _>>()?;
        result.push(BackupPlaylist {
            name,
            description,
            smart_query,
            track_keys,
        });
    }
    Ok(result)
}

fn export_lyrics(
    conn: &Connection,
    catalogue: &mut TrackCatalogue,
) -> Result<Vec<BackupLyrics>, String> {
    let rows = {
        let mut stmt = conn
            .prepare(
                "SELECT t.id, t.lyrics_source, l.synced_text, l.plain_text \
                 FROM tracks t LEFT JOIN lyrics l ON l.track_id = t.id AND l.source = 'custom' \
                 WHERE t.lyrics_source IS NOT NULL OR l.track_id IS NOT NULL",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<(i64, Option<String>, Option<String>, Option<String>)>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    rows.into_iter()
        .map(|(track_id, selected_source, synced_text, plain_text)| {
            Ok(BackupLyrics {
                track_key: catalogue.key_for(conn, track_id)?,
                selected_source,
                synced_text,
                plain_text,
            })
        })
        .collect()
}

fn export_artist_bios(conn: &Connection) -> Result<Vec<BackupArtistBio>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT name, bio FROM artists WHERE NULLIF(TRIM(bio), '') IS NOT NULL ORDER BY name",
        )
        .map_err(|e| e.to_string())?;
    let bios = stmt
        .query_map([], |row| {
            Ok(BackupArtistBio {
                artist_name: row.get(0)?,
                bio: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(bios)
}

fn export_artwork(conn: &Connection, cache_dir: &Path) -> Result<Vec<BackupArtwork>, String> {
    let rows = {
        let mut stmt = conn
            .prepare(
                "SELECT entity_type, entity_id, file_path, mime_type FROM images \
                 WHERE source = 'custom' AND file_path IS NOT NULL",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<(String, i64, String, Option<String>)>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };

    let mut result = Vec::new();
    for (entity_type, entity_id, file_name, mime_type) in rows {
        let owner = match entity_type.as_str() {
            "artist" => conn
                .query_row(
                    "SELECT name FROM artists WHERE id = ?",
                    [entity_id],
                    |row| Ok(BackupArtworkOwner::Artist { name: row.get(0)? }),
                )
                .optional()
                .map_err(|e| e.to_string())?,
            "album" => {
                let album = conn
                    .query_row(
                        "SELECT title, year FROM albums WHERE id = ?",
                        [entity_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?;
                album.map(|(title, year)| BackupArtworkOwner::Album {
                    title,
                    year,
                    artist_names: artist_names_for_album(conn, entity_id).unwrap_or_default(),
                })
            }
            _ => None,
        };
        let Some(owner) = owner else { continue };
        let file_path = cache::images_dir(cache_dir, &entity_type).join(file_name);
        let Ok(data) = std::fs::read(file_path) else {
            continue;
        };
        result.push(BackupArtwork {
            owner,
            mime_type: mime_type.unwrap_or_else(|| "image/jpeg".to_string()),
            data,
        });
    }
    Ok(result)
}

fn export_history(
    conn: &Connection,
    catalogue: &mut TrackCatalogue,
) -> Result<Vec<BackupHistory>, String> {
    let rows = {
        let mut stmt = conn
            .prepare(
                "SELECT track_id, started_at, played_ms, completed \
                 FROM play_history ORDER BY started_at",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get::<_, i64>(3)? != 0,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<(i64, i64, i64, bool)>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    rows.into_iter()
        .map(|(track_id, started_at, played_ms, completed)| {
            Ok(BackupHistory {
                track_key: catalogue.key_for(conn, track_id)?,
                started_at,
                played_ms,
                completed,
            })
        })
        .collect()
}

fn restore_playlists(
    conn: &Connection,
    playlists: &[BackupPlaylist],
    resolved_tracks: &[Option<i64>],
) -> Result<(usize, usize), String> {
    let mut playlist_count = 0;
    let mut track_count = 0;
    for playlist in playlists {
        let id = conn
            .query_row(
                "SELECT id FROM playlists WHERE name = ? AND COALESCE(smart_query, '') = COALESCE(?, '') LIMIT 1",
                rusqlite::params![playlist.name, playlist.smart_query],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let playlist_id = match id {
            Some(id) => {
                conn.execute(
                    "UPDATE playlists SET description = ? WHERE id = ?",
                    rusqlite::params![playlist.description, id],
                )
                .map_err(|e| e.to_string())?;
                id
            }
            None => {
                conn.execute(
                    "INSERT INTO playlists (name, description, smart_query) VALUES (?, ?, ?)",
                    rusqlite::params![playlist.name, playlist.description, playlist.smart_query],
                )
                .map_err(|e| e.to_string())?;
                conn.last_insert_rowid()
            }
        };
        conn.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?",
            [playlist_id],
        )
        .map_err(|e| e.to_string())?;
        let mut position = 0_i64;
        for track_key in &playlist.track_keys {
            let Some(track_id) = resolved_tracks.get(*track_key).copied().flatten() else {
                continue;
            };
            let inserted = conn
                .execute(
                    "INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position) VALUES (?, ?, ?)",
                    rusqlite::params![playlist_id, track_id, position],
                )
                .map_err(|e| e.to_string())?;
            if inserted > 0 {
                position += 1;
                track_count += 1;
            }
        }
        playlist_count += 1;
    }
    Ok((playlist_count, track_count))
}

fn load_local_tracks(conn: &Connection) -> Result<Vec<LocalTrack>, String> {
    let rows = {
        let mut stmt = conn
            .prepare(
                "SELECT t.id, t.file_path, t.title, al.title, t.duration_ms \
                 FROM tracks t LEFT JOIN albums al ON al.id = t.album_id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<(i64, String, Option<String>, Option<String>, Option<i64>)>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    rows.into_iter()
        .map(|(id, file_path, title, album_title, duration_ms)| {
            Ok(LocalTrack {
                id,
                file_path,
                title,
                album_title,
                artist_names: artist_names_for_track(conn, id)?,
                duration_ms,
            })
        })
        .collect()
}

fn resolve_track(track: &BackupTrackRef, local_tracks: &[LocalTrack]) -> Option<i64> {
    if let Some(found) = local_tracks
        .iter()
        .find(|candidate| candidate.file_path.eq_ignore_ascii_case(&track.file_path))
    {
        return Some(found.id);
    }
    let title = normalized(track.title.as_deref()?);
    if title.is_empty() {
        return None;
    }
    let album = track
        .album_title
        .as_deref()
        .map(normalized)
        .unwrap_or_default();
    let artist = track.artist_names.first().map(|name| normalized(name));
    let mut matches = local_tracks.iter().filter(|candidate| {
        candidate.title.as_deref().map(normalized).as_deref() == Some(title.as_str())
            && (album.is_empty()
                || candidate.album_title.as_deref().map(normalized).as_deref()
                    == Some(album.as_str()))
            && match (track.duration_ms, candidate.duration_ms) {
                (Some(expected), Some(actual)) => (expected - actual).abs() <= 2_000,
                _ => true,
            }
            && artist.as_ref().map_or(true, |expected| {
                candidate
                    .artist_names
                    .iter()
                    .any(|name| normalized(name) == *expected)
            })
    });
    let first = matches.next()?.id;
    matches.next().is_none().then_some(first)
}

fn resolve_artwork_owner(
    conn: &Connection,
    owner: &BackupArtworkOwner,
) -> Result<Option<(&'static str, i64)>, String> {
    match owner {
        BackupArtworkOwner::Artist { name } => {
            let id = conn
                .query_row(
                    "SELECT id FROM artists WHERE LOWER(TRIM(name)) = LOWER(TRIM(?)) LIMIT 1",
                    [name],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            Ok(id.map(|id| ("artist", id)))
        }
        BackupArtworkOwner::Album {
            title,
            year,
            artist_names,
        } => {
            let candidates = {
                let mut stmt = conn
                    .prepare(
                        "SELECT id FROM albums WHERE LOWER(TRIM(title)) = LOWER(TRIM(?)) \
                         AND (? IS NULL OR year = ?)",
                    )
                    .map_err(|e| e.to_string())?;
                let candidates = stmt
                    .query_map(rusqlite::params![title, year, year], |row| row.get(0))
                    .map_err(|e| e.to_string())?
                    .collect::<Result<Vec<i64>, _>>()
                    .map_err(|e| e.to_string())?;
                candidates
            };
            let expected_artist = artist_names.first().map(|name| normalized(name));
            let mut matches = Vec::new();
            for id in candidates {
                let names = artist_names_for_album(conn, id)?;
                if expected_artist.as_ref().map_or(true, |expected| {
                    names.iter().any(|name| normalized(name) == *expected)
                }) {
                    matches.push(id);
                }
            }
            Ok((matches.len() == 1).then(|| ("album", matches[0])))
        }
    }
}

fn artist_names_for_track(conn: &Connection, track_id: i64) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT ar.name FROM track_artists ta JOIN artists ar ON ar.id = ta.artist_id \
             WHERE ta.track_id = ? AND ta.role = 'main' ORDER BY ta.rowid",
        )
        .map_err(|e| e.to_string())?;
    let names = stmt
        .query_map([track_id], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(names)
}

fn artist_names_for_album(conn: &Connection, album_id: i64) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT ar.name FROM album_artists aa JOIN artists ar ON ar.id = aa.artist_id \
             WHERE aa.album_id = ? ORDER BY ar.name",
        )
        .map_err(|e| e.to_string())?;
    let names = stmt
        .query_map([album_id], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(names)
}

fn normalized(value: &str) -> String {
    value.trim().to_lowercase()
}

fn manifest(data: &BackupData, file_size_bytes: u64) -> BackupManifest {
    BackupManifest {
        created_at: data.created_at,
        app_version: data.app_version.clone(),
        file_version: data.version,
        file_size_bytes,
        settings: data.settings.is_some(),
        tracks: data.tracks.len(),
        playlists: data.playlists.len(),
        playlist_tracks: data
            .playlists
            .iter()
            .map(|playlist| playlist.track_keys.len())
            .sum(),
        lyrics: data.lyrics.len(),
        artist_bios: data.artist_bios.len(),
        artwork: data.artwork.len(),
        history: data.listening_history.len(),
    }
}

fn encode(data: &BackupData) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(data).map_err(|e| e.to_string())?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&json)
        .map_err(|e| format!("failed to compress backup: {e}"))?;
    encoder
        .finish()
        .map_err(|e| format!("failed to compress backup: {e}"))
}

fn read(path: &Path) -> Result<(BackupData, u64), String> {
    let file_size = std::fs::metadata(path)
        .map_err(|e| format!("failed to inspect backup: {e}"))?
        .len();
    if file_size > MAX_BACKUP_BYTES {
        return Err("backup is too large".to_string());
    }
    let file = File::open(path).map_err(|e| format!("failed to open backup: {e}"))?;
    let decoder = GzDecoder::new(file);
    let mut bytes = Vec::new();
    decoder
        .take(MAX_UNPACKED_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("invalid Sparkle backup: {e}"))?;
    if bytes.len() as u64 > MAX_UNPACKED_BYTES {
        return Err("backup expands beyond the size limit".to_string());
    }
    let data: BackupData =
        serde_json::from_slice(&bytes).map_err(|e| format!("invalid Sparkle backup: {e}"))?;
    if data.format != BACKUP_FORMAT || data.version != BACKUP_VERSION {
        return Err("unsupported Sparkle backup version".to_string());
    }
    Ok((data, file_size))
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_backup() -> BackupData {
        BackupData {
            format: BACKUP_FORMAT.to_string(),
            version: BACKUP_VERSION,
            created_at: 1_700_000_000,
            app_version: "0.1.0".to_string(),
            settings: None,
            tracks: vec![BackupTrackRef {
                file_path: "C:/Music/one.flac".to_string(),
                title: Some("One".to_string()),
                album_title: Some("Album".to_string()),
                artist_names: vec!["Artist".to_string()],
                duration_ms: Some(180_000),
            }],
            playlists: Vec::new(),
            lyrics: Vec::new(),
            artist_bios: Vec::new(),
            artwork: Vec::new(),
            listening_history: Vec::new(),
        }
    }

    #[test]
    fn backup_payload_is_gzipped() {
        let bytes = encode(&sample_backup()).unwrap();
        assert_eq!(&bytes[..2], &[0x1f, 0x8b]);
        let mut decoder = GzDecoder::new(bytes.as_slice());
        let mut json = Vec::new();
        decoder.read_to_end(&mut json).unwrap();
        let restored: BackupData = serde_json::from_slice(&json).unwrap();
        assert_eq!(restored.version, BACKUP_VERSION);
        assert_eq!(restored.tracks[0].title.as_deref(), Some("One"));
    }

    #[test]
    fn track_resolution_prefers_path_then_unique_metadata() {
        let local = vec![LocalTrack {
            id: 42,
            file_path: "D:/Moved/one.flac".to_string(),
            title: Some("One".to_string()),
            album_title: Some("Album".to_string()),
            artist_names: vec!["Artist".to_string()],
            duration_ms: Some(181_000),
        }];
        assert_eq!(resolve_track(&sample_backup().tracks[0], &local), Some(42));
    }

    fn create_playlist_test_schema(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE artists (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
             CREATE TABLE albums (id INTEGER PRIMARY KEY, title TEXT NOT NULL, year INTEGER);
             CREATE TABLE tracks (
                id INTEGER PRIMARY KEY,
                file_path TEXT NOT NULL UNIQUE,
                title TEXT,
                album_id INTEGER,
                duration_ms INTEGER
             );
             CREATE TABLE track_artists (
                track_id INTEGER NOT NULL,
                artist_id INTEGER NOT NULL,
                role TEXT NOT NULL
             );
             CREATE TABLE playlists (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                smart_query TEXT
             );
             CREATE TABLE playlist_tracks (
                playlist_id INTEGER NOT NULL,
                track_id INTEGER NOT NULL,
                position INTEGER NOT NULL,
                PRIMARY KEY (playlist_id, track_id)
             );",
        )
        .unwrap();
    }

    fn insert_test_track(conn: &Connection, id: i64, file_path: &str) {
        conn.execute("INSERT INTO artists (id, name) VALUES (?, 'Artist')", [id])
            .unwrap();
        conn.execute(
            "INSERT INTO albums (id, title, year) VALUES (?, 'Album', 2024)",
            [id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tracks (id, file_path, title, album_id, duration_ms) \
             VALUES (?, ?, 'One', ?, 180000)",
            rusqlite::params![id, file_path, id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO track_artists (track_id, artist_id, role) VALUES (?, ?, 'main')",
            [id, id],
        )
        .unwrap();
    }

    #[test]
    fn playlist_restore_matches_a_moved_reindexed_track() {
        let source = Connection::open_in_memory().unwrap();
        create_playlist_test_schema(&source);
        insert_test_track(&source, 1, "C:/Music/one.flac");
        source
            .execute(
                "INSERT INTO playlists (id, name, description) VALUES (1, 'Keepers', 'Test')",
                [],
            )
            .unwrap();
        source
            .execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (1, 1, 0)",
                [],
            )
            .unwrap();

        let path = std::env::temp_dir().join(format!(
            "sparkle-backup-test-{}-{}.sparklebackup",
            std::process::id(),
            unix_timestamp()
        ));
        let sections = BackupSections {
            settings: false,
            playlists: true,
            custom_metadata: false,
            history: false,
        };
        export(&source, Path::new("."), &path, sections).unwrap();
        assert_eq!(inspect(&path).unwrap().playlist_tracks, 1);

        let target = Connection::open_in_memory().unwrap();
        create_playlist_test_schema(&target);
        insert_test_track(&target, 99, "D:/Moved/one.flac");
        let summary = import(&target, Path::new("."), &path, sections).unwrap();
        let restored_track: i64 = target
            .query_row(
                "SELECT pt.track_id FROM playlist_tracks pt \
                 JOIN playlists p ON p.id = pt.playlist_id WHERE p.name = 'Keepers'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(summary.playlists, 1);
        assert_eq!(summary.unmatched_tracks, 0);
        assert_eq!(restored_track, 99);
        let _ = std::fs::remove_file(path);
    }
}
