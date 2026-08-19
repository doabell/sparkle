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
const BACKUP_VERSION: u32 = 4;
const MIN_SUPPORTED_BACKUP_VERSION: u32 = 3;
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
    // Retained for v3 compatibility. Version 4 also carries millisecond-level
    // timing and the complete finalized listen fact below.
    started_at: i64,
    played_ms: i64,
    completed: bool,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    started_at_ms: Option<i64>,
    #[serde(default)]
    ended_at_ms: Option<i64>,
    #[serde(default)]
    last_activity_at_ms: Option<i64>,
    #[serde(default)]
    start_position_ms: i64,
    #[serde(default)]
    end_position_ms: i64,
    #[serde(default)]
    duration_ms: i64,
    #[serde(default)]
    meaningful: bool,
    #[serde(default)]
    start_source: String,
    #[serde(default)]
    start_reason: String,
    #[serde(default)]
    end_reason: Option<String>,
    #[serde(default)]
    context_type: String,
    #[serde(default)]
    context_id: Option<String>,
    #[serde(default)]
    queue_index: Option<i64>,
    #[serde(default)]
    play_order_index: Option<i64>,
    #[serde(default)]
    queue_length: i64,
    #[serde(default)]
    shuffle: bool,
    #[serde(default)]
    repeat_mode: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BackupPlaybackEvent {
    id: String,
    #[serde(default)]
    listen_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    occurred_at_ms: i64,
    event_type: String,
    source: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    track_key: Option<usize>,
    #[serde(default)]
    position_ms: Option<i64>,
    #[serde(default)]
    target_position_ms: Option<i64>,
    #[serde(default)]
    context_type: String,
    #[serde(default)]
    context_id: Option<String>,
    #[serde(default)]
    queue_index: Option<i64>,
    #[serde(default)]
    play_order_index: Option<i64>,
    #[serde(default)]
    queue_length: i64,
    #[serde(default)]
    shuffle: bool,
    #[serde(default)]
    repeat_mode: String,
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
    #[serde(default)]
    playback_events: Vec<BackupPlaybackEvent>,
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

struct ExportListenRow {
    id: String,
    session_id: String,
    track_id: i64,
    started_at_ms: i64,
    ended_at_ms: Option<i64>,
    last_activity_at_ms: i64,
    start_position_ms: i64,
    end_position_ms: i64,
    duration_ms: i64,
    listened_ms: i64,
    meaningful: bool,
    completed: bool,
    start_source: String,
    start_reason: String,
    end_reason: Option<String>,
    context_type: String,
    context_id: Option<String>,
    queue_index: Option<i64>,
    play_order_index: Option<i64>,
    queue_length: i64,
    shuffle: bool,
    repeat_mode: String,
}

struct ExportPlaybackEventRow {
    id: String,
    listen_id: Option<String>,
    session_id: Option<String>,
    occurred_at_ms: i64,
    event_type: String,
    source: String,
    reason: Option<String>,
    track_id: Option<i64>,
    position_ms: Option<i64>,
    target_position_ms: Option<i64>,
    context_type: String,
    context_id: Option<String>,
    queue_index: Option<i64>,
    play_order_index: Option<i64>,
    queue_length: i64,
    shuffle: bool,
    repeat_mode: String,
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
    let (listening_history, playback_events) = if sections.history {
        (
            export_history(conn, &mut catalogue)?,
            export_playback_events(conn, &mut catalogue)?,
        )
    } else {
        (Vec::new(), Vec::new())
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
        playback_events,
    };
    let bytes = encode(&data)?;
    std::fs::write(path, &bytes).map_err(|e| format!("failed to save backup: {e}"))?;
    let manifest = manifest(&data, bytes.len() as u64);
    log::info!(
        target: "sparkle::backup",
        "event=export_completed version={} tracks={} playlists={} listens={} playback_events={} bytes={}",
        manifest.file_version,
        manifest.tracks,
        manifest.playlists,
        manifest.history,
        data.playback_events.len(),
        manifest.file_size_bytes
    );
    Ok(manifest)
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
        used_track_keys.extend(
            data.playback_events
                .iter()
                .filter_map(|event| event.track_key),
        );
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

    let (history_count, event_count) = if sections.history {
        restore_analytics(
            &tx,
            &data.listening_history,
            &data.playback_events,
            &resolved_tracks,
            data.version,
            data.created_at,
        )?
    } else {
        (0, 0)
    };
    tx.commit().map_err(|e| e.to_string())?;

    if sections.history {
        log::debug!(
            target: "sparkle::backup",
            "event=analytics_restored listens={history_count} playback_events={event_count}"
        );
    }

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

    let summary = BackupImportSummary {
        settings: settings_restored,
        playlists,
        playlist_tracks,
        lyrics: lyric_count,
        artist_bios: bio_count,
        artwork: artwork_count,
        history: history_count,
        unmatched_tracks,
        unmatched_artwork,
    };
    log::info!(
        target: "sparkle::backup",
        "event=import_completed source_version={} settings={} playlists={} playlist_tracks={} listens={} playback_events={} unmatched_tracks={} unmatched_artwork={}",
        data.version,
        summary.settings,
        summary.playlists,
        summary.playlist_tracks,
        summary.history,
        event_count,
        summary.unmatched_tracks,
        summary.unmatched_artwork
    );
    Ok(summary)
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
                "SELECT id, session_id, track_id, started_at_ms, ended_at_ms, \
                        last_activity_at_ms, start_position_ms, end_position_ms, \
                        duration_ms, listened_ms, meaningful, completed, \
                        start_source, start_reason, end_reason, context_type, \
                        context_id, queue_index, play_order_index, queue_length, shuffle, repeat_mode \
                 FROM listens WHERE finalized = 1 ORDER BY started_at_ms, id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ExportListenRow {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    track_id: row.get(2)?,
                    started_at_ms: row.get(3)?,
                    ended_at_ms: row.get(4)?,
                    last_activity_at_ms: row.get(5)?,
                    start_position_ms: row.get(6)?,
                    end_position_ms: row.get(7)?,
                    duration_ms: row.get(8)?,
                    listened_ms: row.get(9)?,
                    meaningful: row.get::<_, i64>(10)? != 0,
                    completed: row.get::<_, i64>(11)? != 0,
                    start_source: row.get(12)?,
                    start_reason: row.get(13)?,
                    end_reason: row.get(14)?,
                    context_type: row.get(15)?,
                    context_id: row.get(16)?,
                    queue_index: row.get(17)?,
                    play_order_index: row.get(18)?,
                    queue_length: row.get(19)?,
                    shuffle: row.get::<_, i64>(20)? != 0,
                    repeat_mode: row.get(21)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    rows.into_iter()
        .map(|row| {
            Ok(BackupHistory {
                track_key: catalogue.key_for(conn, row.track_id)?,
                started_at: row.started_at_ms / 1_000,
                played_ms: row.listened_ms,
                completed: row.completed,
                id: Some(row.id),
                session_id: Some(row.session_id),
                started_at_ms: Some(row.started_at_ms),
                ended_at_ms: row.ended_at_ms,
                last_activity_at_ms: Some(row.last_activity_at_ms),
                start_position_ms: row.start_position_ms,
                end_position_ms: row.end_position_ms,
                duration_ms: row.duration_ms,
                meaningful: row.meaningful,
                start_source: row.start_source,
                start_reason: row.start_reason,
                end_reason: row.end_reason,
                context_type: row.context_type,
                context_id: row.context_id,
                queue_index: row.queue_index,
                play_order_index: row.play_order_index,
                queue_length: row.queue_length,
                shuffle: row.shuffle,
                repeat_mode: row.repeat_mode,
            })
        })
        .collect()
}

fn export_playback_events(
    conn: &Connection,
    catalogue: &mut TrackCatalogue,
) -> Result<Vec<BackupPlaybackEvent>, String> {
    let rows = {
        let mut stmt = conn
            .prepare(
                "SELECT e.id, e.listen_id, e.session_id, e.occurred_at_ms, \
                        e.event_type, e.source, e.reason, e.track_id, e.position_ms, \
                        e.target_position_ms, e.context_type, e.context_id, \
                        e.queue_index, e.play_order_index, e.queue_length, e.shuffle, e.repeat_mode \
                 FROM playback_events e \
                 LEFT JOIN listens l ON l.id = e.listen_id \
                 WHERE e.listen_id IS NULL OR l.finalized = 1 \
                 ORDER BY e.occurred_at_ms, e.id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ExportPlaybackEventRow {
                    id: row.get(0)?,
                    listen_id: row.get(1)?,
                    session_id: row.get(2)?,
                    occurred_at_ms: row.get(3)?,
                    event_type: row.get(4)?,
                    source: row.get(5)?,
                    reason: row.get(6)?,
                    track_id: row.get(7)?,
                    position_ms: row.get(8)?,
                    target_position_ms: row.get(9)?,
                    context_type: row.get(10)?,
                    context_id: row.get(11)?,
                    queue_index: row.get(12)?,
                    play_order_index: row.get(13)?,
                    queue_length: row.get(14)?,
                    shuffle: row.get::<_, i64>(15)? != 0,
                    repeat_mode: row.get(16)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };

    rows.into_iter()
        .map(|row| {
            Ok(BackupPlaybackEvent {
                id: row.id,
                listen_id: row.listen_id,
                session_id: row.session_id,
                occurred_at_ms: row.occurred_at_ms,
                event_type: row.event_type,
                source: row.source,
                reason: row.reason,
                track_key: row
                    .track_id
                    .map(|track_id| catalogue.key_for(conn, track_id))
                    .transpose()?,
                position_ms: row.position_ms,
                target_position_ms: row.target_position_ms,
                context_type: row.context_type,
                context_id: row.context_id,
                queue_index: row.queue_index,
                play_order_index: row.play_order_index,
                queue_length: row.queue_length,
                shuffle: row.shuffle,
                repeat_mode: row.repeat_mode,
            })
        })
        .collect()
}

fn restore_analytics(
    conn: &Connection,
    listens: &[BackupHistory],
    events: &[BackupPlaybackEvent],
    resolved_tracks: &[Option<i64>],
    backup_version: u32,
    backup_created_at: i64,
) -> Result<(usize, usize), String> {
    let mut listen_ids = HashMap::<String, String>::new();
    let mut session_ids = HashMap::<String, String>::new();
    let mut listen_count = 0;
    let mut legacy_session = 0_u64;
    let mut legacy_previous_end_ms: Option<i64> = None;

    for (index, listen) in listens.iter().enumerate() {
        let started_at_ms = listen
            .started_at_ms
            .unwrap_or_else(|| listen.started_at.saturating_mul(1_000));
        let listened_ms = listen.played_ms;
        if started_at_ms <= 0 || listened_ms < 0 {
            continue;
        }

        let computed_end_ms = started_at_ms.saturating_add(listened_ms);
        if backup_version == 3 {
            if legacy_previous_end_ms.is_none_or(|previous_end| {
                started_at_ms.saturating_sub(previous_end) > 20 * 60 * 1_000
            }) {
                legacy_session = legacy_session.saturating_add(1);
            }
            legacy_previous_end_ms = Some(computed_end_ms);
        }

        let Some(track_id) = resolved_tracks.get(listen.track_key).copied().flatten() else {
            continue;
        };
        let fallback_id = format!("backup-{backup_created_at}-listen-{index}");
        let id = sanitize_trace_id(listen.id.as_deref(), &fallback_id);
        let fallback_session = if backup_version == 3 {
            format!("backup-{backup_created_at}-session-{legacy_session}")
        } else {
            format!("backup-{backup_created_at}-session-{index}")
        };
        let session_id = sanitize_trace_id(listen.session_id.as_deref(), &fallback_session);
        if let Some(original) = &listen.id {
            listen_ids.insert(original.clone(), id.clone());
        }
        if let Some(original) = &listen.session_id {
            session_ids.insert(original.clone(), session_id.clone());
        }

        let ended_at_ms = listen
            .ended_at_ms
            .filter(|ended| *ended >= started_at_ms)
            .unwrap_or(computed_end_ms);
        let last_activity_at_ms = listen
            .last_activity_at_ms
            .unwrap_or(ended_at_ms)
            .max(started_at_ms);
        let (start_source, start_reason, end_reason, meaningful) = if backup_version == 3 {
            (
                "legacy".to_string(),
                "legacy_import".to_string(),
                Some("legacy_import".to_string()),
                true,
            )
        } else {
            (
                analytics_source(&listen.start_source),
                sanitize_token(&listen.start_reason, "unknown"),
                listen
                    .end_reason
                    .as_deref()
                    .map(|reason| sanitize_token(reason, "unknown")),
                listen.meaningful,
            )
        };
        let context_type = analytics_context(&listen.context_type);
        let context_id = analytics_context_id(&context_type, listen.context_id.as_deref());
        let repeat_mode = analytics_repeat_mode(&listen.repeat_mode);

        listen_count += conn
            .execute(
                "INSERT OR IGNORE INTO listens ( \
                    id, session_id, track_id, started_at_ms, ended_at_ms, \
                    last_activity_at_ms, start_position_ms, end_position_ms, duration_ms, \
                    listened_ms, meaningful, completed, finalized, start_source, \
                    start_reason, end_reason, context_type, context_id, queue_index, \
                    play_order_index, queue_length, shuffle, repeat_mode \
                 ) VALUES ( \
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, \
                    ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22 \
                 )",
                rusqlite::params![
                    id,
                    session_id,
                    track_id,
                    started_at_ms,
                    ended_at_ms,
                    last_activity_at_ms,
                    listen.start_position_ms.max(0),
                    listen.end_position_ms.max(0),
                    listen.duration_ms.max(0),
                    listened_ms,
                    meaningful as i64,
                    listen.completed as i64,
                    start_source,
                    start_reason,
                    end_reason,
                    context_type,
                    context_id,
                    listen.queue_index.filter(|position| *position >= 0),
                    listen.play_order_index.filter(|position| *position >= 0),
                    listen.queue_length.max(0),
                    listen.shuffle as i64,
                    repeat_mode,
                ],
            )
            .map_err(|e| e.to_string())?;
    }

    let mut event_count = 0;
    for (index, event) in events.iter().enumerate() {
        if event.occurred_at_ms <= 0 {
            continue;
        }
        let track_id = match event.track_key {
            Some(key) => {
                let Some(track_id) = resolved_tracks.get(key).copied().flatten() else {
                    continue;
                };
                Some(track_id)
            }
            None => None,
        };
        let fallback_id = format!("backup-{backup_created_at}-event-{index}");
        let id = sanitize_trace_id(Some(&event.id), &fallback_id);
        let listen_id = event
            .listen_id
            .as_ref()
            .and_then(|original| listen_ids.get(original))
            .cloned();
        let session_id = event
            .session_id
            .as_ref()
            .and_then(|original| session_ids.get(original).cloned())
            .or_else(|| {
                event
                    .session_id
                    .as_deref()
                    .map(|value| sanitize_trace_id(Some(value), &fallback_id))
            });
        let context_type = analytics_context(&event.context_type);
        let context_id = analytics_context_id(&context_type, event.context_id.as_deref());

        event_count += conn
            .execute(
                "INSERT OR IGNORE INTO playback_events ( \
                    id, listen_id, session_id, occurred_at_ms, event_type, source, \
                    reason, track_id, position_ms, target_position_ms, context_type, \
                    context_id, queue_index, play_order_index, queue_length, shuffle, repeat_mode \
                 ) VALUES ( \
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17 \
                 )",
                rusqlite::params![
                    id,
                    listen_id,
                    session_id,
                    event.occurred_at_ms,
                    analytics_event_type(&event.event_type),
                    analytics_source(&event.source),
                    event
                        .reason
                        .as_deref()
                        .map(|reason| sanitize_token(reason, "unknown")),
                    track_id,
                    event.position_ms.filter(|position| *position >= 0),
                    event.target_position_ms.filter(|position| *position >= 0),
                    context_type,
                    context_id,
                    event.queue_index.filter(|position| *position >= 0),
                    event.play_order_index.filter(|position| *position >= 0),
                    event.queue_length.max(0),
                    event.shuffle as i64,
                    analytics_repeat_mode(&event.repeat_mode),
                ],
            )
            .map_err(|e| e.to_string())?;
    }

    Ok((listen_count, event_count))
}

fn sanitize_trace_id(value: Option<&str>, fallback: &str) -> String {
    value
        .map(str::trim)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 200
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
        })
        .unwrap_or(fallback)
        .to_string()
}

fn analytics_context_id(context_type: &str, value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    match context_type {
        "album" | "artist" | "playlist"
            if !value.is_empty()
                && value.len() <= 20
                && value.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            Some(value.to_string())
        }
        "health"
            if !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_') =>
        {
            Some(value.to_string())
        }
        _ => None,
    }
}

fn sanitize_token(value: &str, fallback: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    if !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        value
    } else {
        fallback.to_string()
    }
}

fn analytics_source(value: &str) -> String {
    let value = sanitize_token(value, "unknown");
    if matches!(
        value.as_str(),
        "ui" | "keyboard"
            | "system_media"
            | "automatic"
            | "restore"
            | "internal"
            | "legacy"
            | "unknown"
    ) {
        value
    } else {
        "unknown".to_string()
    }
}

fn analytics_context(value: &str) -> String {
    let value = sanitize_token(value, "unknown");
    if matches!(
        value.as_str(),
        "album"
            | "artist"
            | "genre"
            | "health"
            | "home"
            | "playlist"
            | "queue"
            | "search"
            | "single"
            | "songs"
            | "unknown"
    ) {
        value
    } else {
        "unknown".to_string()
    }
}

fn analytics_repeat_mode(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "all" => "all".to_string(),
        "one" => "one".to_string(),
        _ => "off".to_string(),
    }
}

fn analytics_event_type(value: &str) -> String {
    let value = sanitize_token(value, "unknown");
    if matches!(
        value.as_str(),
        "queue_loaded"
            | "track_started"
            | "playback_resumed"
            | "playback_paused"
            | "seeked"
            | "listen_ended"
            | "playback_stopped"
            | "shuffle_changed"
            | "repeat_changed"
            | "queued_next"
            | "output_unavailable"
            | "output_restored"
    ) {
        value
    } else {
        "unknown".to_string()
    }
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
            && artist.as_ref().is_none_or(|expected| {
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
                if expected_artist
                    .as_ref()
                    .is_none_or(|expected| names.iter().any(|name| normalized(name) == *expected))
                {
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
    if data.format != BACKUP_FORMAT
        || !(MIN_SUPPORTED_BACKUP_VERSION..=BACKUP_VERSION).contains(&data.version)
    {
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
            playback_events: Vec::new(),
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
    fn version_three_history_deserializes_without_trace_fields() {
        let json = serde_json::json!({
            "format": BACKUP_FORMAT,
            "version": 3,
            "created_at": 1_700_000_000,
            "app_version": "0.1.0",
            "settings": null,
            "tracks": [{
                "file_path": "C:/Music/one.flac",
                "title": "One",
                "album_title": "Album",
                "artist_names": ["Artist"],
                "duration_ms": 180000
            }],
            "playlists": [],
            "lyrics": [],
            "artist_bios": [],
            "artwork": [],
            "listening_history": [{
                "track_key": 0,
                "started_at": 1700000000,
                "played_ms": 60000,
                "completed": false
            }]
        });
        let restored: BackupData = serde_json::from_value(json).unwrap();
        assert_eq!(restored.version, 3);
        assert_eq!(restored.listening_history.len(), 1);
        assert!(restored.playback_events.is_empty());
        assert!(restored.listening_history[0].id.is_none());
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

    fn create_analytics_test_schema(conn: &Connection) {
        create_playlist_test_schema(conn);
        conn.execute_batch(
            "CREATE TABLE listens (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
                started_at_ms INTEGER NOT NULL,
                ended_at_ms INTEGER,
                last_activity_at_ms INTEGER NOT NULL,
                start_position_ms INTEGER NOT NULL,
                end_position_ms INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                listened_ms INTEGER NOT NULL,
                meaningful INTEGER NOT NULL,
                completed INTEGER NOT NULL,
                finalized INTEGER NOT NULL,
                start_source TEXT NOT NULL,
                start_reason TEXT NOT NULL,
                end_reason TEXT,
                context_type TEXT NOT NULL,
                context_id TEXT,
                queue_index INTEGER,
                play_order_index INTEGER,
                queue_length INTEGER NOT NULL,
                shuffle INTEGER NOT NULL,
                repeat_mode TEXT NOT NULL
             );
             CREATE TABLE playback_events (
                id TEXT PRIMARY KEY,
                listen_id TEXT REFERENCES listens(id) ON DELETE CASCADE,
                session_id TEXT,
                occurred_at_ms INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                source TEXT NOT NULL,
                reason TEXT,
                track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE,
                position_ms INTEGER,
                target_position_ms INTEGER,
                context_type TEXT NOT NULL,
                context_id TEXT,
                queue_index INTEGER,
                play_order_index INTEGER,
                queue_length INTEGER NOT NULL,
                shuffle INTEGER NOT NULL,
                repeat_mode TEXT NOT NULL
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

    #[test]
    fn analytics_backup_roundtrip_preserves_trace_and_is_idempotent() {
        let source = Connection::open_in_memory().unwrap();
        source.execute("PRAGMA foreign_keys = ON", []).unwrap();
        create_analytics_test_schema(&source);
        insert_test_track(&source, 1, "C:/Music/one.flac");
        source
            .execute(
                "INSERT INTO listens VALUES (
                    'listen-1', 'session-1', 1, 1700000000123, 1700000060123,
                    1700000060123, 0, 60000, 180000, 60000, 1, 0, 1,
                    'keyboard', 'queue_started', 'manual_next', 'album', '1',
                    0, 0, 3, 0, 'off'
                 )",
                [],
            )
            .unwrap();
        source
            .execute(
                "INSERT INTO playback_events VALUES (
                    'event-1', 'listen-1', 'session-1', 1700000060123,
                    'listen_ended', 'keyboard', 'manual_next', 1, 60000, NULL,
                    'album', '1', 0, 0, 3, 0, 'off'
                 )",
                [],
            )
            .unwrap();

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "sparkle-analytics-backup-{}-{unique}.sparklebackup",
            std::process::id()
        ));
        let sections = BackupSections {
            settings: false,
            playlists: false,
            custom_metadata: false,
            history: true,
        };
        let manifest = export(&source, Path::new("."), &path, sections).unwrap();
        assert_eq!(manifest.history, 1);
        assert_eq!(manifest.file_version, 4);

        let target = Connection::open_in_memory().unwrap();
        target.execute("PRAGMA foreign_keys = ON", []).unwrap();
        create_analytics_test_schema(&target);
        insert_test_track(&target, 99, "D:/Moved/one.flac");

        let first = import(&target, Path::new("."), &path, sections).unwrap();
        let second = import(&target, Path::new("."), &path, sections).unwrap();
        assert_eq!(first.history, 1);
        assert_eq!(second.history, 0);
        assert_eq!(
            target
                .query_row("SELECT COUNT(*) FROM playback_events", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let restored: (i64, String, String, i64, i64, i64) = target
            .query_row(
                "SELECT track_id, start_source, context_type, listened_ms, \
                        queue_index, play_order_index FROM listens",
                [],
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
            .unwrap();
        assert_eq!(
            restored,
            (99, "keyboard".into(), "album".into(), 60_000, 0, 0)
        );
        let _ = std::fs::remove_file(path);
    }
}
