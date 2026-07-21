use crate::models::{
    Album, Artist, AudioFormatStat, DiscoveryTracks, Folder, Genre, LibraryHealth, ListeningStats,
    LyricMatch, PlayStatAlbum, PlayStatArtist, PlayStatBucket, PlayStatTrack, Playlist,
    PlaylistDetail, ScanResult, SearchResults, Track,
};
use crate::scanner;
use crate::settings;
use rusqlite::OptionalExtension;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

pub struct AppState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub audio: crate::audio_engine::AudioController,
    pub discord: crate::discord::DiscordPresence,
    /// Root of the on-disk metadata cache (`<app data>/cache`).
    pub cache_dir: std::path::PathBuf,
}

const LIVE_MIX_DEFINITIONS: &[(&str, &str, &str)] = &[
    (
        "Recently added",
        "Freshly indexed in your library",
        "mix:recently_added",
    ),
    (
        "Most played",
        "The songs you keep coming back to",
        "mix:most_played",
    ),
    (
        "Still waiting for you",
        "A gentle nudge toward something new",
        "mix:never_played",
    ),
];

fn live_mix_kind(smart_query: Option<&str>) -> Option<&str> {
    smart_query
        .and_then(|value| value.strip_prefix("mix:"))
        .filter(|kind| matches!(*kind, "recently_added" | "most_played" | "never_played"))
}

fn live_mix_query(kind: &str) -> Option<String> {
    let base =
        format!("SELECT {TRACK_COLUMNS} FROM tracks t LEFT JOIN albums al ON al.id = t.album_id ");
    match kind {
        "recently_added" => Some(format!(
            "{base}ORDER BY t.created_at DESC, t.id DESC LIMIT 50"
        )),
        "most_played" => Some(format!(
            "{base}JOIN (SELECT track_id, COUNT(*) AS plays FROM play_history GROUP BY track_id) p ON p.track_id = t.id ORDER BY p.plays DESC, t.title LIMIT 50"
        )),
        "never_played" => Some(format!(
            "{base}WHERE NOT EXISTS (SELECT 1 FROM play_history p WHERE p.track_id = t.id) ORDER BY t.created_at DESC, t.title LIMIT 50"
        )),
        _ => None,
    }
}

fn live_mix_tracks(conn: &rusqlite::Connection, kind: &str) -> Result<Vec<Track>, String> {
    let query = live_mix_query(kind).ok_or_else(|| "unknown live mix".to_string())?;
    load_tracks_with_query(conn, &query)
}

pub fn ensure_live_mix_playlists(conn: &rusqlite::Connection) -> Result<(), String> {
    for (name, description, smart_query) in LIVE_MIX_DEFINITIONS {
        conn.execute(
            "INSERT INTO playlists (name, description, smart_query) SELECT ?1, ?2, ?3 WHERE NOT EXISTS (SELECT 1 FROM playlists WHERE smart_query = ?3)",
            rusqlite::params![name, description, smart_query],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn refresh_live_mix_playlists(state: State<'_, AppState>) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    refresh_live_mix_playlists_with_connection(&mut conn)
}

pub fn refresh_live_mix_playlists_with_connection(
    conn: &mut rusqlite::Connection,
) -> Result<(), String> {
    ensure_live_mix_playlists(conn)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for (_, _, smart_query) in LIVE_MIX_DEFINITIONS {
        let kind =
            live_mix_kind(Some(smart_query)).ok_or_else(|| "unknown live mix".to_string())?;
        let playlist_id: i64 = tx
            .query_row(
                "SELECT id FROM playlists WHERE smart_query = ?",
                [smart_query],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        let tracks = live_mix_tracks(&tx, kind)?;
        tx.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?",
            [playlist_id],
        )
        .map_err(|e| e.to_string())?;
        for (position, track) in tracks.iter().enumerate() {
            tx.execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
                rusqlite::params![playlist_id, track.id, position as i64],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn export_library_backup(
    state: State<'_, AppState>,
    path: String,
    sections: crate::backup::BackupSections,
) -> Result<crate::backup::BackupManifest, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    crate::backup::export(
        &conn,
        &state.cache_dir,
        std::path::Path::new(&path),
        sections,
    )
}

#[tauri::command]
pub fn inspect_library_backup(path: String) -> Result<crate::backup::BackupManifest, String> {
    crate::backup::inspect(std::path::Path::new(&path))
}

#[tauri::command]
pub fn import_library_backup(
    state: State<'_, AppState>,
    path: String,
    sections: crate::backup::BackupSections,
) -> Result<crate::backup::BackupImportSummary, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    crate::backup::import(
        &conn,
        &state.cache_dir,
        std::path::Path::new(&path),
        sections,
    )
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    let file_path = app
        .dialog()
        .file()
        .set_title("Select a music folder")
        .blocking_pick_folder();
    match file_path {
        Some(FilePath::Path(path)) => Ok(Some(path.to_string_lossy().to_string())),
        Some(FilePath::Url(url)) => url
            .to_file_path()
            .map(|p| Some(p.to_string_lossy().to_string()))
            .map_err(|_| "selected folder path is invalid".to_string()),
        None => Ok(None),
    }
}

#[tauri::command]
pub fn add_folder(state: State<'_, AppState>, path: String) -> Result<Folder, String> {
    let path_buf = PathBuf::from(&path);
    if !path_buf.exists() {
        return Err(format!("path does not exist: {}", path));
    }
    if !path_buf.is_dir() {
        return Err(format!("path is not a directory: {}", path));
    }
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let existing: Option<i64> = conn
        .query_row("SELECT id FROM folders WHERE path = ?", [&path], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())?;
    if existing.is_some() {
        return Err(format!("folder is already monitored: {}", path));
    }
    conn.execute(
        "INSERT INTO folders (path, enabled) VALUES (?1, 1)",
        [&path],
    )
    .map_err(|e| e.to_string())?;
    let id = conn.last_insert_rowid();
    Ok(Folder {
        id,
        path,
        enabled: true,
        scanned_at: None,
    })
}

#[tauri::command]
pub fn remove_folder(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM folders WHERE id = ?", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn set_folder_enabled(
    state: State<'_, AppState>,
    id: i64,
    enabled: bool,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE folders SET enabled = ? WHERE id = ?",
        rusqlite::params![enabled as i64, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Opens the OS file manager with the given path selected (Explorer on
/// Windows). Used by the debug info panel.
#[tauri::command]
pub fn reveal_in_explorer(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let parent = std::path::Path::new(&path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from(&path));
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(opener)
            .arg(parent)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn list_folders(state: State<'_, AppState>) -> Result<Vec<Folder>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, path, enabled, scanned_at FROM folders ORDER BY path")
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

#[tauri::command]
pub fn scan_library(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    force: Option<bool>,
) -> Result<ScanResult, String> {
    // Scan on a dedicated connection (WAL mode) so library reads from the UI
    // are not blocked for the duration of the scan.
    let settings = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        settings::load_settings(&conn)?
    };
    let path = crate::db::db_path(&app);
    let mut scan_conn = crate::db::open_connection(&path).map_err(|e| e.to_string())?;
    let progress_app = app.clone();
    scanner::scan_library_with_progress(
        &mut scan_conn,
        &settings,
        force.unwrap_or(false),
        &state.cache_dir,
        move |progress| {
            use tauri::Emitter;
            let _ = progress_app.emit("scan-progress", progress);
        },
    )
}

/// Sets (or clears) the per-track lyrics provider override. Cached provider
/// lyrics are retained independently so switching sources can reuse them.
#[tauri::command]
#[allow(non_snake_case)]
pub fn set_track_lyrics_source(
    state: State<'_, AppState>,
    trackId: i64,
    source: Option<String>,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let cleaned = source
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    set_track_lyrics_source_record(&conn, trackId, cleaned.as_deref())
}

fn set_track_lyrics_source_record(
    conn: &rusqlite::Connection,
    track_id: i64,
    source: Option<&str>,
) -> Result<(), String> {
    if source == Some("custom") {
        let has_custom: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM lyrics WHERE track_id = ? AND source = 'custom')",
                [track_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !has_custom {
            return Err("no custom lyrics saved for this track".to_string());
        }
    }
    conn.execute(
        "UPDATE tracks SET lyrics_source = ? WHERE id = ?",
        rusqlite::params![source, track_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Stores a user-picked .lrc/.txt file as the track's custom lyrics. Custom
/// lyrics are retained as a permanent provider until deleted or replaced.
#[tauri::command]
#[allow(non_snake_case)]
pub fn set_track_custom_lyrics(
    state: State<'_, AppState>,
    trackId: i64,
    path: String,
) -> Result<(), String> {
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("failed to read lyrics file: {e}"))?;
    if content.trim().is_empty() {
        return Err("lyrics file is empty".to_string());
    }
    let source_path = std::path::Path::new(&path);
    let is_lrc = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("lrc"))
        .unwrap_or(false);
    let (synced, plain) = if is_lrc {
        let plain = crate::providers::lyrics::strip_lrc_timestamps(&content);
        (Some(content), Some(plain))
    } else {
        (None, Some(content))
    };
    crate::cache::copy_custom_lyrics_file(
        &state.cache_dir,
        trackId,
        source_path,
        if is_lrc { "lrc" } else { "txt" },
    )?;
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    // Far-future expiry: custom lyrics are permanent until replaced.
    crate::cache::set_lyrics(
        &conn,
        trackId,
        "custom",
        synced.as_deref(),
        plain.as_deref(),
    )?;
    conn.execute(
        "UPDATE tracks SET lyrics_source = 'custom' WHERE id = ?",
        [trackId],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Removes the track's custom lyrics, falling back to providers.
fn clear_custom_lyrics_record(conn: &rusqlite::Connection, track_id: i64) -> Result<(), String> {
    crate::cache::delete_lyrics_from_source(conn, track_id, "custom")?;
    conn.execute(
        "UPDATE tracks SET lyrics_source = NULL WHERE id = ? AND lyrics_source = 'custom'",
        [track_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn clear_track_custom_lyrics(state: State<'_, AppState>, trackId: i64) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    clear_custom_lyrics_record(&conn, trackId)?;
    crate::cache::delete_custom_lyrics_file(&state.cache_dir, trackId);
    Ok(())
}

/// Stores a user-picked image file as the album's custom artwork. Custom art
/// overrides any provider and does not expire.
#[tauri::command]
#[allow(non_snake_case)]
pub fn set_album_art_file(
    state: State<'_, AppState>,
    albumId: i64,
    path: String,
) -> Result<(), String> {
    let data = crate::cache::read_image_file(std::path::Path::new(&path))?;
    let data = crate::cache::validate_image_for_cache(data)?;
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    crate::cache::set_image(
        &conn,
        &state.cache_dir,
        "album",
        albumId,
        "custom",
        None,
        Some(&data),
    )?;
    Ok(())
}

/// Removes the album's custom artwork, falling back to providers.
#[tauri::command]
#[allow(non_snake_case)]
pub fn clear_album_custom_art(state: State<'_, AppState>, albumId: i64) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let path: Option<String> = conn
        .query_row(
            "SELECT file_path FROM images WHERE entity_type = 'album' AND entity_id = ? AND source = 'custom'",
            [albumId],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    conn.execute(
        "DELETE FROM images WHERE entity_type = 'album' AND entity_id = ? AND source = 'custom'",
        [albumId],
    )
    .map_err(|e| e.to_string())?;
    if let Some(p) = path {
        let _ = std::fs::remove_file(crate::cache::images_dir(&state.cache_dir, "album").join(p));
    }
    Ok(())
}

#[tauri::command]
pub fn get_genres(state: State<'_, AppState>) -> Result<Vec<Genre>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT genre, COUNT(*) as track_count \
             FROM tracks \
             WHERE genre IS NOT NULL AND genre != '' \
             GROUP BY genre \
             ORDER BY genre",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Genre {
                name: row.get(0)?,
                track_count: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_artists(state: State<'_, AppState>) -> Result<Vec<Artist>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT a.id, a.name, a.sort_name, a.track_count, a.album_count, a.bio, a.info_provider, a.image_provider, a.info_term, a.image_term \
             FROM artists a \
             ORDER BY a.name"
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Artist {
                id: row.get(0)?,
                name: row.get(1)?,
                sort_name: row.get(2)?,
                track_count: row.get(3)?,
                album_count: row.get(4)?,
                bio: row.get(5)?,
                info_provider: row.get(6)?,
                image_provider: row.get(7)?,
                info_term: row.get(8)?,
                image_term: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn get_albums(state: State<'_, AppState>, artistId: Option<i64>) -> Result<Vec<Album>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT al.id, al.title, al.year, COUNT(DISTINCT t.id) AS track_count \
             FROM albums al \
             LEFT JOIN tracks t ON t.album_id = al.id \
             WHERE (? IS NULL OR al.id IN ( \
                 SELECT DISTINCT ab.album_id FROM artist_albums ab WHERE ab.artist_id = ?)) \
             GROUP BY al.id \
             ORDER BY al.year, al.title",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![artistId, artistId], |row| {
            Ok(Album {
                id: row.get(0)?,
                title: row.get(1)?,
                year: row.get(2)?,
                artist_ids: Vec::new(),
                artist_names: Vec::new(),
                track_count: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut albums = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for album in &mut albums {
        let pairs = album_artist_pairs(&conn, album.id)?;
        album.artist_ids = pairs.iter().map(|(id, _)| *id).collect();
        album.artist_names = pairs.into_iter().map(|(_, name)| name).collect();
    }
    Ok(albums)
}

#[tauri::command]
pub fn get_album(state: State<'_, AppState>, id: i64) -> Result<Album, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT al.id, al.title, al.year, COUNT(DISTINCT t.id) AS track_count \
             FROM albums al \
             LEFT JOIN tracks t ON t.album_id = al.id \
             WHERE al.id = ? \
             GROUP BY al.id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([id], |row| {
            Ok(Album {
                id: row.get(0)?,
                title: row.get(1)?,
                year: row.get(2)?,
                artist_ids: Vec::new(),
                artist_names: Vec::new(),
                track_count: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut albums = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut album = albums.pop().ok_or_else(|| "album not found".to_string())?;
    let pairs = album_artist_pairs(&conn, album.id)?;
    album.artist_ids = pairs.iter().map(|(id, _)| *id).collect();
    album.artist_names = pairs.into_iter().map(|(_, name)| name).collect();
    Ok(album)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn get_tracks(state: State<'_, AppState>, albumId: Option<i64>) -> Result<Vec<Track>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT t.id, t.file_path, t.title, t.track_number, t.disc_number, t.duration_ms, \
             t.year, t.genre, t.album_id, t.embedded_lyrics, t.lrc_offset_ms, al.title AS album_title, t.lyrics_source \
             FROM tracks t \
             LEFT JOIN albums al ON al.id = t.album_id \
             WHERE (? IS NULL OR t.album_id = ?) \
             ORDER BY t.disc_number, t.track_number, t.title",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![albumId, albumId], |row| {
            Ok(Track {
                id: row.get(0)?,
                file_path: row.get(1)?,
                title: row.get(2)?,
                track_number: row.get(3)?,
                disc_number: row.get(4)?,
                duration_ms: row.get(5)?,
                year: row.get(6)?,
                genre: row.get(7)?,
                album_id: row.get(8)?,
                embedded_lyrics: row.get(9)?,
                lrc_offset_ms: row.get(10)?,
                lyrics_source: row.get(12)?,
                artist_ids: Vec::new(),
                artist_names: Vec::new(),
                album_title: row.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut tracks = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for track in &mut tracks {
        let artists = track_artists(&conn, track.id)?;
        track.artist_ids = artists.iter().map(|(id, _)| *id).collect();
        track.artist_names = artists.iter().map(|(_, name)| name.clone()).collect();
    }
    Ok(tracks)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn get_tracks_by_artist(
    state: State<'_, AppState>,
    artistId: i64,
) -> Result<Vec<Track>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT t.id, t.file_path, t.title, t.track_number, t.disc_number, t.duration_ms, \
             t.year, t.genre, t.album_id, t.embedded_lyrics, t.lrc_offset_ms, al.title AS album_title, t.lyrics_source \
             FROM tracks t \
             JOIN track_artists ta ON ta.track_id = t.id AND ta.role = 'main' \
             LEFT JOIN albums al ON al.id = t.album_id \
             WHERE ta.artist_id = ? \
             ORDER BY t.disc_number, t.track_number, t.title",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([artistId], |row| {
            Ok(Track {
                id: row.get(0)?,
                file_path: row.get(1)?,
                title: row.get(2)?,
                track_number: row.get(3)?,
                disc_number: row.get(4)?,
                duration_ms: row.get(5)?,
                year: row.get(6)?,
                genre: row.get(7)?,
                album_id: row.get(8)?,
                embedded_lyrics: row.get(9)?,
                lrc_offset_ms: row.get(10)?,
                lyrics_source: row.get(12)?,
                artist_ids: Vec::new(),
                artist_names: Vec::new(),
                album_title: row.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut tracks = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for track in &mut tracks {
        let artists = track_artists(&conn, track.id)?;
        track.artist_ids = artists.iter().map(|(id, _)| *id).collect();
        track.artist_names = artists.iter().map(|(_, name)| name.clone()).collect();
    }
    Ok(tracks)
}

#[tauri::command]
pub fn get_artist(state: State<'_, AppState>, id: i64) -> Result<Artist, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, name, sort_name, track_count, album_count, bio, info_provider, image_provider, info_term, image_term FROM artists WHERE id = ?")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([id], |row| {
            Ok(Artist {
                id: row.get(0)?,
                name: row.get(1)?,
                sort_name: row.get(2)?,
                track_count: row.get(3)?,
                album_count: row.get(4)?,
                bio: row.get(5)?,
                info_provider: row.get(6)?,
                image_provider: row.get(7)?,
                info_term: row.get(8)?,
                image_term: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut artists = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    artists.pop().ok_or_else(|| "artist not found".to_string())
}

fn album_artist_pairs(
    conn: &rusqlite::Connection,
    album_id: i64,
) -> Result<Vec<(i64, String)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT a.id, a.name FROM artists a \
             JOIN album_artists aa ON aa.artist_id = a.id \
             WHERE aa.album_id = ? \
             ORDER BY a.name",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([album_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn track_artists(conn: &rusqlite::Connection, track_id: i64) -> Result<Vec<(i64, String)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT a.id, a.name FROM artists a \
             JOIN track_artists ta ON ta.artist_id = a.id \
             WHERE ta.track_id = ? AND ta.role = 'main' \
             ORDER BY a.name",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([track_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn get_tracks_by_genre(
    state: State<'_, AppState>,
    genre: String,
) -> Result<Vec<Track>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT t.id, t.file_path, t.title, t.track_number, t.disc_number, t.duration_ms, \
             t.year, t.genre, t.album_id, t.embedded_lyrics, t.lrc_offset_ms, al.title AS album_title, t.lyrics_source \
             FROM tracks t \
             LEFT JOIN albums al ON al.id = t.album_id \
             WHERE t.genre = ? \
             ORDER BY t.album_id, t.disc_number, t.track_number, t.title",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([genre], track_from_row)
        .map_err(|e| e.to_string())?;
    let mut tracks = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for track in &mut tracks {
        let artists = track_artists(&conn, track.id)?;
        track.artist_ids = artists.iter().map(|(id, _)| *id).collect();
        track.artist_names = artists.iter().map(|(_, name)| name.clone()).collect();
    }
    Ok(tracks)
}

/// The small, fixed artwork sample used by a genre card. Keeping this query
/// separate avoids loading every track (and every track's artists) merely to
/// draw a four-image collage.
#[tauri::command]
#[allow(non_snake_case)]
pub fn get_genre_collage_album_ids(
    state: State<'_, AppState>,
    genre: String,
) -> Result<Vec<i64>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT album_id \
             FROM tracks \
             WHERE genre = ? AND album_id IS NOT NULL \
             ORDER BY album_id \
             LIMIT 4",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([genre], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

// Playlists

fn track_from_row(row: &rusqlite::Row) -> Result<Track, rusqlite::Error> {
    Ok(Track {
        id: row.get(0)?,
        file_path: row.get(1)?,
        title: row.get(2)?,
        track_number: row.get(3)?,
        disc_number: row.get(4)?,
        duration_ms: row.get(5)?,
        year: row.get(6)?,
        genre: row.get(7)?,
        album_id: row.get(8)?,
        embedded_lyrics: row.get(9)?,
        lrc_offset_ms: row.get(10)?,
        lyrics_source: row.get(12)?,
        artist_ids: Vec::new(),
        artist_names: Vec::new(),
        album_title: row.get(11)?,
    })
}

#[tauri::command]
pub fn create_playlist(
    state: State<'_, AppState>,
    name: String,
    description: Option<String>,
    folder_path: Option<String>,
) -> Result<Playlist, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    if name.trim().is_empty() {
        return Err("playlist name is required".to_string());
    }
    let trimmed = name.trim().to_string();
    conn.execute(
        "INSERT INTO playlists (name, description, smart_query) VALUES (?1, ?2, ?3)",
        rusqlite::params![trimmed, description, folder_path],
    )
    .map_err(|e| e.to_string())?;
    let id = conn.last_insert_rowid();
    let track_count = playlist_track_count(&conn, id, folder_path.as_deref())?;
    Ok(Playlist {
        id,
        name: trimmed,
        description,
        folder_path,
        live_mix: None,
        track_count,
    })
}

#[tauri::command]
pub fn get_playlists(state: State<'_, AppState>) -> Result<Vec<Playlist>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    ensure_live_mix_playlists(&conn)?;
    let mut stmt = conn
        .prepare("SELECT id, name, description, smart_query FROM playlists ORDER BY name")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            let description: Option<String> = row.get(2)?;
            let smart_query: Option<String> = row.get(3)?;
            let track_count = playlist_track_count(
                // Using the same connection inside query_map is not allowed, so we
                // count dynamically on the client / detail page. Returning 0 here.
                //
                // NOTE: We will compute this in a second pass instead.
                &conn,
                id,
                smart_query.as_deref(),
            );
            Ok((id, name, description, smart_query, track_count))
        })
        .map_err(|e| e.to_string())?;

    // Collect rows before reusing the connection.
    let mut tuples: Vec<(
        i64,
        String,
        Option<String>,
        Option<String>,
        Result<i64, String>,
    )> = Vec::new();
    for row in rows {
        tuples.push(row.map_err(|e| e.to_string())?);
    }

    let mut playlists = Vec::new();
    for (id, name, description, folder_path, count_result) in tuples {
        let track_count = count_result?;
        let live_mix = live_mix_kind(folder_path.as_deref()).map(str::to_string);
        let folder_path = folder_path.filter(|value| live_mix_kind(Some(value)).is_none());
        playlists.push(Playlist {
            id,
            name,
            description,
            folder_path: folder_path.clone(),
            live_mix,
            track_count,
        });
    }
    Ok(playlists)
}

#[tauri::command]
pub fn get_playlist(state: State<'_, AppState>, id: i64) -> Result<PlaylistDetail, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, name, description, smart_query FROM playlists WHERE id = ?")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut rows: Vec<(i64, String, Option<String>, Option<String>)> = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let (id, name, description, smart_query) =
        rows.pop().ok_or_else(|| "playlist not found".to_string())?;

    let live_mix = live_mix_kind(smart_query.as_deref()).map(str::to_string);
    let folder_path = smart_query.filter(|value| live_mix_kind(Some(value)).is_none());
    let tracks = if let Some(ref folder) = folder_path {
        tracks_in_folder(&conn, folder)?
    } else {
        tracks_in_playlist(&conn, id)?
    };

    Ok(PlaylistDetail {
        id,
        name,
        description,
        folder_path,
        live_mix,
        tracks,
    })
}

/// Returns only the distinct albums needed for a playlist-card collage. The
/// full playlist remains available from `get_playlist` when a user opens it.
#[tauri::command]
pub fn get_playlist_collage_album_ids(
    state: State<'_, AppState>,
    id: i64,
) -> Result<Vec<i64>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let smart_query = conn
        .query_row(
            "SELECT smart_query FROM playlists WHERE id = ?",
            [id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "playlist not found".to_string())?;

    let ids = if let Some(folder) = smart_query.filter(|value| live_mix_kind(Some(value)).is_none())
    {
        let pattern = format!("{}%", escape_like(&folder));
        let mut stmt = conn
            .prepare(
                "SELECT album_id \
                 FROM tracks \
                 WHERE file_path LIKE ?1 ESCAPE '\\' AND album_id IS NOT NULL \
                 GROUP BY album_id \
                 ORDER BY MIN(file_path), album_id \
                 LIMIT 4",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([pattern], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT t.album_id \
                 FROM playlist_tracks pt \
                 JOIN tracks t ON t.id = pt.track_id \
                 WHERE pt.playlist_id = ? AND t.album_id IS NOT NULL \
                 GROUP BY t.album_id \
                 ORDER BY MIN(pt.position), t.album_id \
                 LIMIT 4",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([id], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };

    Ok(ids)
}

#[tauri::command]
pub fn update_playlist(
    state: State<'_, AppState>,
    id: i64,
    name: String,
    description: Option<String>,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("playlist name is required".to_string());
    }
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let smart_query: Option<String> = conn
        .query_row(
            "SELECT smart_query FROM playlists WHERE id = ?",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    if live_mix_kind(smart_query.as_deref()).is_some() {
        return Err("live mixes are managed automatically".to_string());
    }
    conn.execute(
        "UPDATE playlists SET name = ?1, description = ?2 WHERE id = ?3",
        rusqlite::params![name.trim(), description, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_playlist(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let smart_query: Option<String> = conn
        .query_row(
            "SELECT smart_query FROM playlists WHERE id = ?",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    if live_mix_kind(smart_query.as_deref()).is_some() {
        return Err("live mixes cannot be deleted".to_string());
    }
    conn.execute("DELETE FROM playlists WHERE id = ?", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn add_tracks_to_playlist(
    state: State<'_, AppState>,
    playlistId: i64,
    trackIds: Vec<i64>,
) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    let folder: Option<String> = tx
        .query_row(
            "SELECT smart_query FROM playlists WHERE id = ?",
            [playlistId],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map(|opt| opt.flatten())
        .map_err(|e| e.to_string())?;
    if folder.is_some() {
        return Err("cannot add tracks to a managed playlist".to_string());
    }

    let mut position_stmt = tx
        .prepare("SELECT COALESCE(MAX(position), 0) FROM playlist_tracks WHERE playlist_id = ?")
        .map_err(|e| e.to_string())?;
    let next_position: i64 = position_stmt
        .query_row([playlistId], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    drop(position_stmt);

    let mut insert = tx
        .prepare(
            "INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)"
        )
        .map_err(|e| e.to_string())?;
    for (offset, track_id) in trackIds.iter().enumerate() {
        insert
            .execute(rusqlite::params![
                playlistId,
                track_id,
                next_position + offset as i64 + 1
            ])
            .map_err(|e| e.to_string())?;
    }
    drop(insert);
    tx.commit().map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn remove_track_from_playlist(
    state: State<'_, AppState>,
    playlistId: i64,
    trackId: i64,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let smart_query: Option<String> = conn
        .query_row(
            "SELECT smart_query FROM playlists WHERE id = ?",
            [playlistId],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    if live_mix_kind(smart_query.as_deref()).is_some() {
        return Err("cannot remove tracks from a live mix".to_string());
    }
    conn.execute(
        "DELETE FROM playlist_tracks WHERE playlist_id = ? AND track_id = ?",
        [playlistId, trackId],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn playlist_track_count(
    conn: &rusqlite::Connection,
    playlist_id: i64,
    smart_query: Option<&str>,
) -> Result<i64, String> {
    if live_mix_kind(smart_query).is_some() {
        conn.query_row(
            "SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = ?",
            [playlist_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())
    } else if let Some(folder) = smart_query {
        let mut stmt = conn
            .prepare("SELECT COUNT(*) FROM tracks WHERE file_path LIKE ?1 ESCAPE '\\'")
            .map_err(|e| e.to_string())?;
        let pattern = format!("{}%", escape_like(folder));
        let count: i64 = stmt
            .query_row([pattern], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        Ok(count)
    } else {
        let mut stmt = conn
            .prepare("SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = ?")
            .map_err(|e| e.to_string())?;
        let count: i64 = stmt
            .query_row([playlist_id], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        Ok(count)
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

const TRACK_COLUMNS: &str = "t.id, t.file_path, t.title, t.track_number, t.disc_number, t.duration_ms, \
     t.year, t.genre, t.album_id, t.embedded_lyrics, t.lrc_offset_ms, al.title AS album_title, t.lyrics_source";

fn load_tracks_with_query(conn: &rusqlite::Connection, query: &str) -> Result<Vec<Track>, String> {
    let mut stmt = conn.prepare(query).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], track_from_row)
        .map_err(|e| e.to_string())?;
    let mut tracks = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for track in &mut tracks {
        let artists = track_artists(conn, track.id)?;
        track.artist_ids = artists.iter().map(|(id, _)| *id).collect();
        track.artist_names = artists.iter().map(|(_, name)| name.clone()).collect();
    }
    Ok(tracks)
}

#[tauri::command]
pub fn get_discovery_tracks(state: State<'_, AppState>) -> Result<DiscoveryTracks, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let base =
        format!("SELECT {TRACK_COLUMNS} FROM tracks t LEFT JOIN albums al ON al.id = t.album_id ");
    let recently_added = load_tracks_with_query(
        &conn,
        &format!("{base}ORDER BY t.updated_at DESC, t.id DESC LIMIT 12"),
    )?;
    let most_played = load_tracks_with_query(
        &conn,
        &format!(
            "{base}JOIN (SELECT track_id, COUNT(*) AS plays FROM play_history GROUP BY track_id) p ON p.track_id = t.id ORDER BY p.plays DESC, t.title LIMIT 12"
        ),
    )?;
    let never_played = load_tracks_with_query(
        &conn,
        &format!(
            "{base}WHERE NOT EXISTS (SELECT 1 FROM play_history p WHERE p.track_id = t.id) ORDER BY t.updated_at DESC, t.title LIMIT 12"
        ),
    )?;
    Ok(DiscoveryTracks {
        recently_added,
        most_played,
        never_played,
    })
}

#[tauri::command]
pub fn get_library_health(state: State<'_, AppState>) -> Result<LibraryHealth, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let count = |query: &str| -> Result<i64, String> {
        conn.query_row(query, [], |row| row.get(0))
            .map_err(|e| e.to_string())
    };
    let lossless = "LOWER(COALESCE(audio_format, '')) IN ('flac', 'alac', 'wav')";
    let lossy = "LOWER(COALESCE(audio_format, '')) IN ('mp3', 'aac', 'ogg', 'opus')";
    let mut format_stmt = conn
        .prepare(
            "SELECT UPPER(COALESCE(NULLIF(audio_format, ''), 'unknown')), COUNT(*) AS tracks \
             FROM tracks GROUP BY LOWER(COALESCE(audio_format, 'unknown')) \
             ORDER BY tracks DESC, audio_format LIMIT 12",
        )
        .map_err(|e| e.to_string())?;
    let formats = format_stmt
        .query_map([], |row| {
            Ok(AudioFormatStat {
                format: row.get(0)?,
                tracks: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let lossless_tracks = count(&format!("SELECT COUNT(*) FROM tracks WHERE {lossless}"))?;
    let lossy_tracks = count(&format!("SELECT COUNT(*) FROM tracks WHERE {lossy}"))?;
    let track_count = count("SELECT COUNT(*) FROM tracks")?;
    Ok(LibraryHealth {
        track_count,
        album_count: count("SELECT COUNT(*) FROM albums")?,
        artist_count: count("SELECT COUNT(*) FROM artists")?,
        missing_titles: count(
            "SELECT COUNT(*) FROM tracks WHERE title IS NULL OR TRIM(title) = ''",
        )?,
        missing_artists: count(
            "SELECT COUNT(*) FROM tracks t WHERE NOT EXISTS (SELECT 1 FROM track_artists ta WHERE ta.track_id = t.id AND ta.role = 'main')",
        )?,
        missing_albums: count("SELECT COUNT(*) FROM tracks WHERE album_id IS NULL")?,
        missing_genres: count(
            "SELECT COUNT(*) FROM tracks WHERE genre IS NULL OR TRIM(genre) = ''",
        )?,
        missing_lyrics: count(
            "SELECT COUNT(*) FROM tracks t WHERE (t.embedded_lyrics IS NULL OR TRIM(t.embedded_lyrics) = '') AND NOT EXISTS (SELECT 1 FROM lyrics l WHERE l.track_id = t.id)",
        )?,
        missing_years: count("SELECT COUNT(*) FROM tracks WHERE year IS NULL")?,
        missing_track_numbers: count("SELECT COUNT(*) FROM tracks WHERE track_number IS NULL")?,
        duplicate_titles: count(
            "SELECT COUNT(*) FROM tracks t WHERE t.title IS NOT NULL AND TRIM(t.title) != '' AND EXISTS (SELECT 1 FROM tracks duplicate WHERE duplicate.id != t.id AND LOWER(TRIM(duplicate.title)) = LOWER(TRIM(t.title)))",
        )?,
        never_played: count(
            "SELECT COUNT(*) FROM tracks t WHERE NOT EXISTS (SELECT 1 FROM play_history p WHERE p.track_id = t.id)",
        )?,
        lossless_tracks,
        lossy_tracks,
        unclassified_tracks: (track_count - lossless_tracks - lossy_tracks).max(0),
        high_resolution_tracks: count(&format!(
            "SELECT COUNT(*) FROM tracks WHERE {lossless} AND (sample_rate_hz >= 96000 OR bit_depth >= 24)"
        ))?,
        low_bitrate_tracks: count(&format!(
            "SELECT COUNT(*) FROM tracks WHERE {lossy} AND audio_bitrate_kbps > 0 AND audio_bitrate_kbps < 192"
        ))?,
        missing_audio_properties: count(
            "SELECT COUNT(*) FROM tracks WHERE audio_format IS NULL OR sample_rate_hz IS NULL OR channels IS NULL OR file_size_bytes IS NULL",
        )?,
        missing_durations: count(
            "SELECT COUNT(*) FROM tracks WHERE duration_ms IS NULL OR duration_ms <= 0",
        )?,
        very_short_tracks: count(
            "SELECT COUNT(*) FROM tracks WHERE duration_ms > 0 AND duration_ms < 30000",
        )?,
        very_long_tracks: count("SELECT COUNT(*) FROM tracks WHERE duration_ms > 1200000")?,
        mono_tracks: count("SELECT COUNT(*) FROM tracks WHERE channels = 1")?,
        total_size_bytes: count("SELECT COALESCE(SUM(file_size_bytes), 0) FROM tracks")?,
        formats,
    })
}

/// Returns a focused slice of tracks behind one health metric so the health
/// center can lead to a useful next action instead of stopping at a count.
#[tauri::command]
pub fn get_health_tracks(state: State<'_, AppState>, kind: String) -> Result<Vec<Track>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let where_clause = match kind.as_str() {
        "titles" => "t.title IS NULL OR TRIM(t.title) = ''",
        "artists" => "NOT EXISTS (SELECT 1 FROM track_artists ta WHERE ta.track_id = t.id AND ta.role = 'main')",
        "albums" => "t.album_id IS NULL",
        "genres" => "t.genre IS NULL OR TRIM(t.genre) = ''",
        "lyrics" => "(t.embedded_lyrics IS NULL OR TRIM(t.embedded_lyrics) = '') AND NOT EXISTS (SELECT 1 FROM lyrics l WHERE l.track_id = t.id)",
        "years" => "t.year IS NULL",
        "track_numbers" => "t.track_number IS NULL",
        "duplicate_titles" => "t.title IS NOT NULL AND TRIM(t.title) != '' AND EXISTS (SELECT 1 FROM tracks duplicate WHERE duplicate.id != t.id AND LOWER(TRIM(duplicate.title)) = LOWER(TRIM(t.title)))",
        "never_played" => "NOT EXISTS (SELECT 1 FROM play_history p WHERE p.track_id = t.id)",
        "lossless" => "LOWER(COALESCE(t.audio_format, '')) IN ('flac', 'alac', 'wav')",
        "lossy" => "LOWER(COALESCE(t.audio_format, '')) IN ('mp3', 'aac', 'ogg', 'opus')",
        "high_resolution" => "LOWER(COALESCE(t.audio_format, '')) IN ('flac', 'alac', 'wav') AND (t.sample_rate_hz >= 96000 OR t.bit_depth >= 24)",
        "low_bitrate" => "LOWER(COALESCE(t.audio_format, '')) IN ('mp3', 'aac', 'ogg', 'opus') AND t.audio_bitrate_kbps > 0 AND t.audio_bitrate_kbps < 192",
        "audio_properties" => "t.audio_format IS NULL OR t.sample_rate_hz IS NULL OR t.channels IS NULL OR t.file_size_bytes IS NULL",
        "durations" => "t.duration_ms IS NULL OR t.duration_ms <= 0",
        "very_short" => "t.duration_ms > 0 AND t.duration_ms < 30000",
        "very_long" => "t.duration_ms > 1200000",
        "mono" => "t.channels = 1",
        _ => return Err("unknown health category".to_string()),
    };
    load_tracks_with_query(
        &conn,
        &format!("SELECT {TRACK_COLUMNS} FROM tracks t LEFT JOIN albums al ON al.id = t.album_id WHERE {where_clause} ORDER BY t.updated_at DESC, t.title LIMIT 100"),
    )
}

/// Searches tracks (title, artist, album, genre, and lyrics text), artists,
/// and albums with one LIKE pattern. Lyrics live in the database again
/// (schema v2) so they are searchable here.
#[tauri::command]
pub fn search(state: State<'_, AppState>, query: String) -> Result<SearchResults, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(SearchResults {
            artists: Vec::new(),
            albums: Vec::new(),
            tracks: Vec::new(),
            lyric_tracks: Vec::new(),
        });
    }
    let like = format!("%{}%", escape_like(trimmed));

    let mut artist_stmt = conn
        .prepare(
            "SELECT a.id, a.name, a.sort_name, a.track_count, a.album_count, a.bio, a.info_provider, a.image_provider, a.info_term, a.image_term \
             FROM artists a WHERE a.name LIKE ?1 ESCAPE '\\' ORDER BY a.name LIMIT 20",
        )
        .map_err(|e| e.to_string())?;
    let artists = artist_stmt
        .query_map([&like], |row| {
            Ok(Artist {
                id: row.get(0)?,
                name: row.get(1)?,
                sort_name: row.get(2)?,
                track_count: row.get(3)?,
                album_count: row.get(4)?,
                bio: row.get(5)?,
                info_provider: row.get(6)?,
                image_provider: row.get(7)?,
                info_term: row.get(8)?,
                image_term: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut album_stmt = conn
        .prepare(
            "SELECT al.id, al.title, al.year, COUNT(DISTINCT t.id) AS track_count \
             FROM albums al LEFT JOIN tracks t ON t.album_id = al.id \
             WHERE al.title LIKE ?1 ESCAPE '\\' \
             GROUP BY al.id ORDER BY al.title LIMIT 20",
        )
        .map_err(|e| e.to_string())?;
    let mut albums = album_stmt
        .query_map([&like], |row| {
            Ok(Album {
                id: row.get(0)?,
                title: row.get(1)?,
                year: row.get(2)?,
                artist_ids: Vec::new(),
                artist_names: Vec::new(),
                track_count: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for album in &mut albums {
        let pairs = album_artist_pairs(&conn, album.id)?;
        album.artist_ids = pairs.iter().map(|(id, _)| *id).collect();
        album.artist_names = pairs.into_iter().map(|(_, name)| name).collect();
    }

    let track_sql = format!(
        "SELECT {TRACK_COLUMNS} \
         FROM tracks t \
         LEFT JOIN albums al ON al.id = t.album_id \
         WHERE t.title LIKE ?1 ESCAPE '\\' \
            OR t.genre LIKE ?1 \
            OR al.title LIKE ?1 \
            OR EXISTS (SELECT 1 FROM track_artists ta JOIN artists ar ON ar.id = ta.artist_id \
                       WHERE ta.track_id = t.id AND ar.name LIKE ?1) \
         ORDER BY t.title LIMIT 100"
    );
    let mut track_stmt = conn.prepare(&track_sql).map_err(|e| e.to_string())?;
    let mut tracks = track_stmt
        .query_map([&like], track_from_row)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for track in &mut tracks {
        let artists = track_artists(&conn, track.id)?;
        track.artist_ids = artists.iter().map(|(id, _)| *id).collect();
        track.artist_names = artists.iter().map(|(_, name)| name.clone()).collect();
    }

    // Lyrics matches get their own panel: tracks whose lyrics contain the
    // query but whose metadata does not, each with the matching line.
    let lyric_sql = format!(
        "SELECT {TRACK_COLUMNS}, l.plain_text, l.synced_text \
         FROM tracks t \
         JOIN lyrics l ON l.track_id = t.id \
           AND l.rowid = (SELECT l2.rowid FROM lyrics l2 \
                          WHERE l2.track_id = t.id \
                            AND (l2.plain_text LIKE ?1 ESCAPE '\\' OR l2.synced_text LIKE ?1) \
                          ORDER BY CASE WHEN l2.source = 'custom' THEN 0 ELSE 1 END, l2.fetched_at DESC \
                          LIMIT 1) \
         LEFT JOIN albums al ON al.id = t.album_id \
         WHERE (l.plain_text LIKE ?1 ESCAPE '\\' OR l.synced_text LIKE ?1) \
           AND NOT (t.title LIKE ?1 \
            OR t.genre LIKE ?1 \
            OR al.title LIKE ?1 \
            OR EXISTS (SELECT 1 FROM track_artists ta JOIN artists ar ON ar.id = ta.artist_id \
                       WHERE ta.track_id = t.id AND ar.name LIKE ?1)) \
         ORDER BY t.title LIMIT 50"
    );
    let mut lyric_stmt = conn.prepare(&lyric_sql).map_err(|e| e.to_string())?;
    let lyric_rows = lyric_stmt
        .query_map([&like], |row| {
            let track = track_from_row(row)?;
            let plain: Option<String> = row.get(13)?;
            let synced: Option<String> = row.get(14)?;
            Ok((track, plain, synced))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut lyric_tracks = Vec::with_capacity(lyric_rows.len());
    for (mut track, plain, synced) in lyric_rows {
        let artists = track_artists(&conn, track.id)?;
        track.artist_ids = artists.iter().map(|(id, _)| *id).collect();
        track.artist_names = artists.iter().map(|(_, name)| name.clone()).collect();
        lyric_tracks.push(LyricMatch {
            track,
            snippet: lyric_snippet(plain.as_deref(), synced.as_deref(), trimmed),
        });
    }

    Ok(SearchResults {
        artists,
        albums,
        tracks,
        lyric_tracks,
    })
}

/// The first lyric line containing the query, for the search results panel.
fn lyric_snippet(plain: Option<&str>, synced: Option<&str>, query: &str) -> String {
    let needle = query.to_lowercase();
    let candidate = plain
        .map(|p| p.to_string())
        .or_else(|| synced.map(|s| crate::providers::lyrics::strip_lrc_timestamps(s)))
        .unwrap_or_default();
    for line in candidate.lines() {
        let line = line.trim();
        if !line.is_empty() && line.to_lowercase().contains(&needle) {
            return line.chars().take(120).collect();
        }
    }
    candidate
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(120)
        .collect()
}

/// Artists related to this one, ranked by a relationship score built from:
///   · co-artists on the same track (strongest)
///   · co-artists on the same album
///   · song-artist ↔ album-artist links: they are the album artist of an
///     album this artist has tracks on, or vice versa (features/appearances)
/// Artists with no credit relationship at all are excluded — never a padded
/// list. Genre similarity was tried and dropped: "same genre" is far too
/// broad to mean "related".
#[tauri::command]
#[allow(non_snake_case)]
pub fn get_related_artists(
    state: State<'_, AppState>,
    artistId: i64,
) -> Result<Vec<Artist>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT a2.id, a2.name, a2.sort_name, a2.track_count, a2.album_count, a2.bio, \
                    a2.info_provider, a2.image_provider, a2.info_term, a2.image_term, \
                    ( \
                        3 * (SELECT COUNT(DISTINCT ta2.track_id) FROM track_artists ta2 \
                             JOIN track_artists ta1 ON ta1.track_id = ta2.track_id \
                             WHERE ta1.artist_id = ?1 AND ta2.artist_id = a2.id) \
                        + 3 * (SELECT COUNT(DISTINCT aa2.album_id) FROM album_artists aa2 \
                             JOIN album_artists aa1 ON aa1.album_id = aa2.album_id \
                             WHERE aa1.artist_id = ?1 AND aa2.artist_id = a2.id) \
                        + 2 * (SELECT COUNT(DISTINCT t.album_id) FROM track_artists ta_me \
                             JOIN tracks t ON t.id = ta_me.track_id AND t.album_id IS NOT NULL \
                             JOIN album_artists aa ON aa.album_id = t.album_id AND aa.artist_id = a2.id \
                             WHERE ta_me.artist_id = ?1) \
                        + 2 * (SELECT COUNT(DISTINCT t2.album_id) FROM track_artists ta2 \
                             JOIN tracks t2 ON t2.id = ta2.track_id AND t2.album_id IS NOT NULL \
                             JOIN album_artists aa_me ON aa_me.album_id = t2.album_id AND aa_me.artist_id = ?1 \
                             WHERE ta2.artist_id = a2.id) \
                    ) AS shared \
             FROM artists a2 \
             WHERE a2.id != ?1 \
             GROUP BY a2.id \
             HAVING shared > 0 \
             ORDER BY shared DESC, a2.name \
             LIMIT 12",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([artistId], |row| {
            Ok(Artist {
                id: row.get(0)?,
                name: row.get(1)?,
                sort_name: row.get(2)?,
                track_count: row.get(3)?,
                album_count: row.get(4)?,
                bio: row.get(5)?,
                info_provider: row.get(6)?,
                image_provider: row.get(7)?,
                info_term: row.get(8)?,
                image_term: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// Listening statistics for the settings page ("sparkle unwrapped").
#[tauri::command]
#[allow(non_snake_case)]
pub fn get_listening_stats(
    state: State<'_, AppState>,
    days: Option<i64>,
) -> Result<ListeningStats, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let by_month = days.map(|d| d > 120).unwrap_or(true);
    let since: i64 = days
        .map(|d| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|t| t.as_secs() as i64)
                .unwrap_or(0);
            now - d.max(1) * 86_400
        })
        .unwrap_or(0);

    let (total_plays, total_ms): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(played_ms), 0) FROM play_history WHERE started_at >= ?1",
            [since],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())?;

    let active_days: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT date(started_at, 'unixepoch', 'localtime')) \
             FROM play_history WHERE started_at >= ?1",
            [since],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let (unique_tracks, completed_plays): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(DISTINCT track_id), COALESCE(SUM(completed), 0) \
             FROM play_history WHERE started_at >= ?1",
            [since],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let unique_artists: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT ta.artist_id) FROM play_history ph \
             JOIN track_artists ta ON ta.track_id = ph.track_id AND ta.role = 'main' \
             WHERE ph.started_at >= ?1",
            [since],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let discovery_tracks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM (SELECT track_id, MIN(started_at) AS first_play \
             FROM play_history GROUP BY track_id HAVING first_play >= ?1)",
            [since],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let longest_streak_days: i64 = conn
        .query_row(
            "WITH active AS ( \
                 SELECT DISTINCT CAST(julianday(date(started_at, 'unixepoch', 'localtime')) AS INTEGER) AS day \
                 FROM play_history WHERE started_at >= ?1 \
             ), numbered AS ( \
                 SELECT day, day - ROW_NUMBER() OVER (ORDER BY day) AS island FROM active \
             ) SELECT COALESCE(MAX(streak), 0) FROM (SELECT COUNT(*) AS streak FROM numbered GROUP BY island)",
            [since],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let session_count: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(is_new), 0) FROM ( \
                 SELECT CASE WHEN previous_end IS NULL OR started_at - previous_end > 1200 THEN 1 ELSE 0 END AS is_new \
                 FROM (SELECT started_at, LAG(started_at + played_ms / 1000) OVER (ORDER BY started_at) AS previous_end \
                       FROM play_history WHERE started_at >= ?1) \
             )",
            [since],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let peak_hour_row: Option<(i64, i64)> = conn
        .query_row(
            "SELECT CAST(strftime('%H', started_at, 'unixepoch', 'localtime') AS INTEGER) AS hour, \
                    SUM(played_ms) AS ms FROM play_history WHERE started_at >= ?1 \
             GROUP BY hour ORDER BY ms DESC, hour LIMIT 1",
            [since],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (peak_hour, peak_hour_ms) = peak_hour_row
        .map(|(hour, ms)| (Some(hour), ms))
        .unwrap_or((None, 0));
    let (morning_ms, afternoon_ms, evening_ms, late_night_ms, weekend_ms): (
        i64,
        i64,
        i64,
        i64,
        i64,
    ) = conn
        .query_row(
            "SELECT \
                COALESCE(SUM(CASE WHEN hour >= 5 AND hour < 12 THEN played_ms ELSE 0 END), 0), \
                COALESCE(SUM(CASE WHEN hour >= 12 AND hour < 17 THEN played_ms ELSE 0 END), 0), \
                COALESCE(SUM(CASE WHEN hour >= 17 AND hour < 23 THEN played_ms ELSE 0 END), 0), \
                COALESCE(SUM(CASE WHEN hour >= 23 OR hour < 5 THEN played_ms ELSE 0 END), 0), \
                COALESCE(SUM(CASE WHEN weekday IN ('0', '6') THEN played_ms ELSE 0 END), 0) \
             FROM (SELECT played_ms, strftime('%w', started_at, 'unixepoch', 'localtime') AS weekday, \
                          CAST(strftime('%H', started_at, 'unixepoch', 'localtime') AS INTEGER) AS hour \
                   FROM play_history WHERE started_at >= ?1)",
            [since],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;

    let top_genre_row: Option<(String, i64)> = conn
        .query_row(
            "SELECT TRIM(t.genre), SUM(ph.played_ms) AS ms FROM play_history ph \
             JOIN tracks t ON t.id = ph.track_id \
             WHERE ph.started_at >= ?1 AND t.genre IS NOT NULL AND TRIM(t.genre) != '' \
             GROUP BY LOWER(TRIM(t.genre)) ORDER BY ms DESC LIMIT 1",
            [since],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (top_genre, top_genre_ms) = top_genre_row
        .map(|(genre, ms)| (Some(genre), ms))
        .unwrap_or((None, 0));
    let average_year: Option<f64> = conn
        .query_row(
            "SELECT CAST(SUM(t.year * ph.played_ms) AS REAL) / NULLIF(SUM(ph.played_ms), 0) \
             FROM play_history ph JOIN tracks t ON t.id = ph.track_id \
             WHERE ph.started_at >= ?1 AND t.year BETWEEN 1000 AND 3000",
            [since],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    // Activity buckets, oldest first — the client fills the gaps.
    let bucket_expr = if by_month {
        "strftime('%Y-%m', started_at, 'unixepoch', 'localtime')"
    } else {
        "date(started_at, 'unixepoch', 'localtime')"
    };
    let mut activity_stmt = conn
        .prepare(&format!(
            "SELECT {bucket_expr} AS label, COUNT(*) AS plays, COALESCE(SUM(played_ms), 0) AS ms \
                 FROM play_history WHERE started_at >= ?1 \
                 GROUP BY label ORDER BY label"
        ))
        .map_err(|e| e.to_string())?;
    let activity = activity_stmt
        .query_map([since], |row| {
            Ok(PlayStatBucket {
                label: row.get(0)?,
                plays: row.get(1)?,
                ms: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut track_stmt = conn
        .prepare(
            "SELECT t.id, t.title, t.album_id, COUNT(*) AS plays, COALESCE(SUM(ph.played_ms), 0) AS ms \
             FROM play_history ph JOIN tracks t ON t.id = ph.track_id \
             WHERE ph.started_at >= ?1 \
             GROUP BY ph.track_id ORDER BY ms DESC, plays DESC LIMIT 10",
        )
        .map_err(|e| e.to_string())?;
    let top_track_rows = track_stmt
        .query_map([since], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut top_tracks = Vec::with_capacity(top_track_rows.len());
    for (track_id, title, album_id, plays, ms) in top_track_rows {
        let artists = track_artists(&conn, track_id)?;
        top_tracks.push(PlayStatTrack {
            track_id,
            title,
            artist_names: artists.into_iter().map(|(_, name)| name).collect(),
            album_id,
            plays,
            ms,
        });
    }

    let mut artist_stmt = conn
        .prepare(
            "SELECT ar.id, ar.name, COUNT(*) AS plays, COALESCE(SUM(ph.played_ms), 0) AS ms \
             FROM play_history ph \
             JOIN track_artists ta ON ta.track_id = ph.track_id AND ta.role = 'main' \
             JOIN artists ar ON ar.id = ta.artist_id \
             WHERE ph.started_at >= ?1 \
             GROUP BY ar.id ORDER BY ms DESC, plays DESC LIMIT 10",
        )
        .map_err(|e| e.to_string())?;
    let top_artists = artist_stmt
        .query_map([since], |row| {
            Ok(PlayStatArtist {
                artist_id: row.get(0)?,
                name: row.get(1)?,
                plays: row.get(2)?,
                ms: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut album_stmt = conn
        .prepare(
            "SELECT al.id, al.title, COUNT(*) AS plays, COALESCE(SUM(ph.played_ms), 0) AS ms \
             FROM play_history ph \
             JOIN tracks t ON t.id = ph.track_id AND t.album_id IS NOT NULL \
             JOIN albums al ON al.id = t.album_id \
             WHERE ph.started_at >= ?1 \
             GROUP BY al.id ORDER BY ms DESC, plays DESC LIMIT 10",
        )
        .map_err(|e| e.to_string())?;
    let top_album_rows = album_stmt
        .query_map([since], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut top_albums = Vec::with_capacity(top_album_rows.len());
    {
        let mut names_stmt = conn
            .prepare(
                "SELECT ar.name FROM album_artists aa \
                 JOIN artists ar ON ar.id = aa.artist_id \
                 WHERE aa.album_id = ?1 ORDER BY aa.rowid",
            )
            .map_err(|e| e.to_string())?;
        for (album_id, title, plays, ms) in top_album_rows {
            let names: Vec<String> = names_stmt
                .query_map([album_id], |row| row.get(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            top_albums.push(PlayStatAlbum {
                album_id,
                title,
                artist_names: names,
                plays,
                ms,
            });
        }
    }

    Ok(ListeningStats {
        total_plays,
        total_ms,
        active_days,
        unique_tracks,
        unique_artists,
        completed_plays,
        discovery_tracks,
        longest_streak_days,
        session_count,
        peak_hour,
        peak_hour_ms,
        morning_ms,
        afternoon_ms,
        evening_ms,
        late_night_ms,
        weekend_ms,
        top_genre,
        top_genre_ms,
        average_year,
        top_tracks,
        top_artists,
        top_albums,
        activity,
        activity_by_month: by_month,
    })
}

fn tracks_in_folder(conn: &rusqlite::Connection, folder: &str) -> Result<Vec<Track>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT t.id, t.file_path, t.title, t.track_number, t.disc_number, t.duration_ms, \
             t.year, t.genre, t.album_id, t.embedded_lyrics, t.lrc_offset_ms, al.title AS album_title, t.lyrics_source \
             FROM tracks t \
             LEFT JOIN albums al ON al.id = t.album_id \
             WHERE t.file_path LIKE ?1 ESCAPE '\\' \
             ORDER BY t.file_path",
        )
        .map_err(|e| e.to_string())?;
    let pattern = format!("{}%", escape_like(folder));
    let rows = stmt
        .query_map([pattern], track_from_row)
        .map_err(|e| e.to_string())?;
    let mut tracks = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for track in &mut tracks {
        let artists = track_artists(conn, track.id)?;
        track.artist_ids = artists.iter().map(|(id, _)| *id).collect();
        track.artist_names = artists.iter().map(|(_, name)| name.clone()).collect();
    }
    Ok(tracks)
}

fn tracks_in_playlist(conn: &rusqlite::Connection, playlist_id: i64) -> Result<Vec<Track>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT t.id, t.file_path, t.title, t.track_number, t.disc_number, t.duration_ms, \
             t.year, t.genre, t.album_id, t.embedded_lyrics, t.lrc_offset_ms, al.title AS album_title, t.lyrics_source \
             FROM playlist_tracks pt \
             JOIN tracks t ON t.id = pt.track_id \
             LEFT JOIN albums al ON al.id = t.album_id \
             WHERE pt.playlist_id = ? \
             ORDER BY pt.position, t.id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([playlist_id], track_from_row)
        .map_err(|e| e.to_string())?;
    let mut tracks = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for track in &mut tracks {
        let artists = track_artists(conn, track.id)?;
        track.artist_ids = artists.iter().map(|(id, _)| *id).collect();
        track.artist_names = artists.iter().map(|(_, name)| name.clone()).collect();
    }
    Ok(tracks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_custom_lyrics_releases_the_custom_provider_override() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tracks (id INTEGER PRIMARY KEY, lyrics_source TEXT); \
             CREATE TABLE lyrics (track_id INTEGER NOT NULL, source TEXT NOT NULL, PRIMARY KEY (track_id, source));",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tracks (id, lyrics_source) VALUES (1, 'custom')",
            [],
        )
        .unwrap();

        clear_custom_lyrics_record(&conn, 1).unwrap();

        let source: Option<String> = conn
            .query_row("SELECT lyrics_source FROM tracks WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(source, None);
    }

    #[test]
    fn changing_lyrics_source_keeps_cached_provider_rows() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tracks (id INTEGER PRIMARY KEY, lyrics_source TEXT); \
             CREATE TABLE lyrics (track_id INTEGER NOT NULL, source TEXT NOT NULL, PRIMARY KEY (track_id, source)); \
             INSERT INTO tracks (id) VALUES (1); \
             INSERT INTO lyrics (track_id, source) VALUES (1, 'lrclib'), (1, 'netease');",
        )
        .unwrap();

        set_track_lyrics_source_record(&conn, 1, Some("netease")).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM lyrics WHERE track_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        let source: String = conn
            .query_row("SELECT lyrics_source FROM tracks WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(source, "netease");
    }
}
