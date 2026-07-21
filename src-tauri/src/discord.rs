use crate::cache;
use crate::models::{PlaybackState, Track};
use crate::settings;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use discord_rich_presence::activity::{
    Activity, ActivityType, Assets, StatusDisplayType, Timestamps,
};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
#[cfg(not(windows))]
use image::codecs::jpeg::JpegEncoder;
#[cfg(not(windows))]
use image::imageops::FilterType;
use md5::{Digest, Md5};
use reqwest::blocking::multipart::{Form, Part};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
#[path = "discord/gdiplus.rs"]
mod gdiplus;

const CATBOX_API_URL: &str = "https://catbox.moe/user/api.php";
const CATBOX_URL_PREFIX: &str = "https://files.catbox.moe/";
const ARTWORK_MAX_DIMENSION: u32 = 256;
const JPEG_QUALITY: u8 = 85;
const RETRY_INTERVAL: Duration = Duration::from_secs(10);

/// A small, asynchronous bridge from the audio thread to Discord's local IPC.
/// Networking and image encoding happen on the worker so playback transitions
/// never wait for Discord or Catbox.
#[derive(Clone)]
pub struct DiscordPresence {
    tx: Sender<DiscordCommand>,
}

impl DiscordPresence {
    pub fn new(db_path: PathBuf, image_cache_dir: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || worker(rx, db_path, image_cache_dir));
        Self { tx }
    }

    pub fn update(&self, playback: &PlaybackState) {
        let _ = self.tx.send(DiscordCommand::Playback(playback.clone()));
    }

    /// Re-evaluate the most recent playback state after Settings changes.
    pub fn refresh(&self) {
        let _ = self.tx.send(DiscordCommand::Refresh);
    }
}

enum DiscordCommand {
    Playback(PlaybackState),
    Refresh,
}

struct ConnectedDiscordClient {
    app_id: String,
    client: DiscordIpcClient,
}

struct ArtworkPayload {
    jpeg: Vec<u8>,
    cache_keys: Vec<String>,
    persistent_key: String,
}

struct PresenceFields {
    title: String,
    artist: String,
    album: String,
    artwork_url: Option<String>,
    timestamp_start: Option<i64>,
    timestamp_end: Option<i64>,
}

struct CatboxCache {
    entries: HashMap<String, String>,
}

impl CatboxCache {
    fn load(conn: &Connection) -> Result<Self, String> {
        let mut statement = conn
            .prepare("SELECT cache_key, url FROM discord_artwork_cache")
            .map_err(|err| err.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|err| err.to_string())?;
        let mut entries = HashMap::new();
        for row in rows {
            let (key, url) = row.map_err(|err| err.to_string())?;
            if !key.is_empty() && is_catbox_url(&url) {
                entries.insert(key, url);
            }
        }
        Ok(Self { entries })
    }

    fn lookup(&self, keys: &[String]) -> Option<String> {
        keys.iter().find_map(|key| {
            self.entries
                .get(key)
                .filter(|url| is_catbox_url(url))
                .cloned()
        })
    }

    fn contains_key(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    fn store(&mut self, conn: &Connection, keys: &[String], url: String) -> Result<(), String> {
        let tx = conn
            .unchecked_transaction()
            .map_err(|err| err.to_string())?;
        let mut statement = tx
            .prepare(
                "INSERT INTO discord_artwork_cache (cache_key, url) VALUES (?1, ?2) \
                 ON CONFLICT(cache_key) DO UPDATE SET url = excluded.url, updated_at = unixepoch()",
            )
            .map_err(|err| err.to_string())?;
        for key in keys {
            statement
                .execute([key, &url])
                .map_err(|err| err.to_string())?;
        }
        drop(statement);
        tx.commit().map_err(|err| err.to_string())?;
        for key in keys {
            self.entries.insert(key.clone(), url.clone());
        }
        Ok(())
    }
}

fn worker(rx: Receiver<DiscordCommand>, db_path: PathBuf, image_cache_dir: PathBuf) {
    let conn = match crate::db::open_connection(&db_path) {
        Ok(conn) => conn,
        Err(err) => {
            log::error!(target: "sparkle::discord::presence", "event=database_open_failed error={err}");
            return;
        }
    };
    let mut client = None;
    let mut catbox_cache = match CatboxCache::load(&conn) {
        Ok(cache) => {
            log::info!(
                target: "sparkle::discord::catbox",
                "event=cache_loaded entries={}",
                cache.entries.len()
            );
            cache
        }
        Err(err) => {
            log::error!(target: "sparkle::discord::catbox", "event=cache_load_failed error={err}");
            return;
        }
    };
    log::info!(target: "sparkle::discord::presence", "event=worker_started");
    let mut latest_playback = None;

    loop {
        match rx.recv_timeout(RETRY_INTERVAL) {
            Ok(DiscordCommand::Playback(playback)) => {
                latest_playback = Some(playback);
                if let Some(playback) = latest_playback.as_ref() {
                    apply_playback(
                        &conn,
                        &image_cache_dir,
                        &mut catbox_cache,
                        &mut client,
                        playback,
                    );
                }
            }
            Ok(DiscordCommand::Refresh) => {
                if let Some(playback) = latest_playback.as_ref() {
                    apply_playback(
                        &conn,
                        &image_cache_dir,
                        &mut catbox_cache,
                        &mut client,
                        playback,
                    );
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Discord may start after Sparkle. Retry only while an active
                // presence has not connected. A paused track has no presence
                // to restore and must not produce periodic clear attempts.
                if client.is_none() {
                    if let Some(playback) = latest_playback
                        .as_ref()
                        .filter(|playback| playback.is_playing && playback.current_track.is_some())
                    {
                        apply_playback(
                            &conn,
                            &image_cache_dir,
                            &mut catbox_cache,
                            &mut client,
                            playback,
                        );
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    close_client(&mut client);
}

fn apply_playback(
    conn: &Connection,
    image_cache_dir: &Path,
    catbox_cache: &mut CatboxCache,
    client: &mut Option<ConnectedDiscordClient>,
    playback: &PlaybackState,
) {
    let settings = match settings::load_settings(conn) {
        Ok(settings) => settings,
        Err(err) => {
            log::warn!(target: "sparkle::discord::presence", "event=settings_load_failed error={err}");
            return;
        }
    };

    if !settings.discord_enabled {
        close_client(client);
        return;
    }

    let app_id = settings.discord_app_id.trim();
    if app_id.is_empty() || !app_id.chars().all(|character| character.is_ascii_digit()) {
        log::warn!(target: "sparkle::discord::presence", "event=invalid_application_id");
        clear_presence(client);
        return;
    }

    let Some(track) = playback.current_track.as_ref() else {
        clear_presence(client);
        return;
    };
    if !playback.is_playing {
        // Do not show a stale song while it is paused or stopped.
        clear_presence(client);
        return;
    }

    // Upload artwork only after Discord is connected, so an offline session
    // cannot turn every cache miss into a Catbox upload.
    let Some(discord) = ensure_client(client, app_id) else {
        return;
    };

    let artwork_url = if let Some(url) = persistent_catbox_url(catbox_cache, track.album_id) {
        // This is durable user-owned metadata: use it without touching the
        // disposable album-art cache or doing any image work.
        log::debug!(target: "sparkle::discord::catbox", "event=persistent_hit");
        Some(url)
    } else {
        match artwork_for_track(conn, image_cache_dir, track) {
            Ok(Some(artwork)) => match catbox_cache.lookup(&artwork.cache_keys) {
                Some(url) => {
                    // Older entries only have content hashes. Once one is reused,
                    // add the durable album key so clearing/re-fetching image
                    // cache data cannot cause another Catbox upload.
                    if !catbox_cache.contains_key(&artwork.persistent_key) {
                        if let Err(err) = catbox_cache.store(conn, &artwork.cache_keys, url.clone())
                        {
                            log::warn!(target: "sparkle::discord::catbox", "event=persistent_key_store_failed error={err}");
                        }
                    }
                    log::debug!(target: "sparkle::discord::catbox", "event=cache_hit");
                    Some(url)
                }
                None => {
                    log::info!(
                        target: "sparkle::discord::catbox",
                        "event=upload_started bytes={} cache_keys={}",
                        artwork.jpeg.len(),
                        artwork.cache_keys.len()
                    );
                    match upload_to_catbox(
                        artwork.jpeg,
                        &artwork.cache_keys[0],
                        settings.discord_catbox_user_hash.trim(),
                    ) {
                        Ok(url) => {
                            log::info!(target: "sparkle::discord::catbox", "event=upload_succeeded");
                            if let Err(err) =
                                catbox_cache.store(conn, &artwork.cache_keys, url.clone())
                            {
                                log::warn!(target: "sparkle::discord::catbox", "event=cache_store_failed error={err}");
                            } else {
                                log::debug!(
                                    target: "sparkle::discord::catbox",
                                    "event=cache_stored keys={}",
                                    artwork.cache_keys.len()
                                );
                            }
                            Some(url)
                        }
                        Err(err) => {
                            log::warn!(target: "sparkle::discord::catbox", "event=upload_failed error={err}");
                            None
                        }
                    }
                }
            },
            Ok(None) => None,
            Err(err) => {
                log::warn!(target: "sparkle::discord::catbox", "event=artwork_prepare_failed error={err}");
                None
            }
        }
    };

    let fields = presence_fields(playback, track, artwork_url);
    if let Err(err) = discord.set_activity(build_activity(fields)) {
        log::debug!(target: "sparkle::discord::presence", "event=activity_update_failed error={err}");
        close_client(client);
    } else {
        log::debug!(target: "sparkle::discord::presence", "event=activity_updated");
    }
}

fn ensure_client<'a>(
    client: &'a mut Option<ConnectedDiscordClient>,
    app_id: &str,
) -> Option<&'a mut DiscordIpcClient> {
    let needs_connection = client
        .as_ref()
        .map(|existing| existing.app_id != app_id)
        .unwrap_or(true);
    if needs_connection {
        close_client(client);
        let mut new_client = DiscordIpcClient::new(app_id);
        if let Err(err) = new_client.connect() {
            log::debug!(target: "sparkle::discord::presence", "event=ipc_unavailable error={err}");
            return None;
        }
        *client = Some(ConnectedDiscordClient {
            app_id: app_id.to_string(),
            client: new_client,
        });
        log::info!(target: "sparkle::discord::presence", "event=ipc_connected");
    }
    client.as_mut().map(|connected| &mut connected.client)
}

fn clear_presence(client: &mut Option<ConnectedDiscordClient>) {
    let failed = client
        .as_mut()
        .map(|connected| connected.client.clear_activity().is_err())
        .unwrap_or(false);
    if failed {
        log::debug!(target: "sparkle::discord::presence", "event=presence_clear_failed");
        close_client(client);
    }
}

fn close_client(client: &mut Option<ConnectedDiscordClient>) {
    if let Some(connected) = client.as_mut() {
        let _ = connected.client.clear_activity();
        let _ = connected.client.close();
        log::info!(target: "sparkle::discord::presence", "event=ipc_disconnected");
    }
    *client = None;
}

fn artwork_for_track(
    conn: &Connection,
    image_cache_dir: &Path,
    track: &Track,
) -> Result<Option<ArtworkPayload>, String> {
    let Some(album_id) = track.album_id else {
        return Ok(None);
    };
    let Some(image) = cache::get_image(conn, image_cache_dir, "album", album_id)? else {
        return Ok(None);
    };
    let image = cache::read_cached_image(&image)?;
    let Some(original) = image.data else {
        return Ok(None);
    };

    let jpeg = resize_to_cache_jpeg(&original, image_cache_dir)?;
    let persistent_key = album_artwork_key(album_id);
    let cache_keys = artwork_cache_keys(album_id, &jpeg, &original);

    Ok(Some(ArtworkPayload {
        jpeg,
        cache_keys,
        persistent_key,
    }))
}

fn resize_to_cache_jpeg(original: &[u8], work_dir: &Path) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        return gdiplus::resize_to_cache_jpeg(
            original,
            work_dir,
            &md5_hex(original),
            ARTWORK_MAX_DIMENSION,
            JPEG_QUALITY,
        );
    }

    #[cfg(not(windows))]
    {
        let image = image::load_from_memory(original).map_err(|e| e.to_string())?;
        // Non-Windows clients use the closest built-in resize while Windows
        // uses its GDI+ pipeline above.
        let resized = image.resize(
            ARTWORK_MAX_DIMENSION,
            ARTWORK_MAX_DIMENSION,
            FilterType::CatmullRom,
        );
        let mut jpeg = Vec::new();
        let encoder = JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY);
        resized
            .write_with_encoder(encoder)
            .map_err(|e| e.to_string())?;
        Ok(jpeg)
    }
}

fn unique_cache_keys(keys: [String; 3]) -> Vec<String> {
    let mut unique = Vec::with_capacity(keys.len());
    for key in keys {
        if !unique.contains(&key) {
            unique.push(key);
        }
    }
    unique
}

fn album_artwork_key(album_id: i64) -> String {
    format!("album:{album_id}")
}

fn persistent_catbox_url(cache: &CatboxCache, album_id: Option<i64>) -> Option<String> {
    album_id.and_then(|id| cache.lookup(&[album_artwork_key(id)]))
}

fn artwork_cache_keys(album_id: i64, normalized_jpeg: &[u8], original: &[u8]) -> Vec<String> {
    let normalized_base64 = STANDARD.encode(normalized_jpeg);
    let original_base64 = STANDARD.encode(original);
    let mut keys = unique_cache_keys([
        md5_hex(normalized_base64.as_bytes()),
        // Keep the original-artwork hash as a fallback so old cache entries
        // remain reusable without another upload.
        md5_hex(original_base64.as_bytes()),
        md5_hex(original),
    ]);
    keys.push(album_artwork_key(album_id));
    keys
}

fn md5_hex(input: &[u8]) -> String {
    // This is not used for security: the Catbox cache uses MD5(base64 artwork)
    // as its stable lookup key.
    Md5::digest(input)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn upload_to_catbox(jpeg: Vec<u8>, cache_key: &str, user_hash: &str) -> Result<String, String> {
    let image = Part::bytes(jpeg)
        .file_name(format!("{cache_key}.jpg"))
        .mime_str("image/jpeg")
        .map_err(|e| e.to_string())?;
    let mut form = Form::new()
        .text("reqtype", "fileupload")
        .part("fileToUpload", image);
    if !user_hash.is_empty() {
        form = form.text("userhash", user_hash.to_string());
    }
    let response = reqwest::blocking::Client::builder()
        .user_agent("Sparkle/0.1")
        .build()
        .map_err(|e| e.to_string())?
        .post(CATBOX_API_URL)
        .multipart(form)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let url = response
        .text()
        .map_err(|e| e.to_string())?
        .trim()
        .to_string();
    if !is_catbox_url(&url) {
        return Err("Catbox returned an unexpected upload URL".to_string());
    }
    Ok(url)
}

fn presence_fields(
    playback: &PlaybackState,
    track: &Track,
    artwork_url: Option<String>,
) -> PresenceFields {
    let title = track
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            Path::new(&track.file_path)
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "Unknown track".to_string());
    let artist = if track.artist_names.is_empty() {
        "Unknown artist".to_string()
    } else {
        track.artist_names.join(", ")
    };
    let album = track
        .album_title
        .as_deref()
        .filter(|album| !album.trim().is_empty())
        .unwrap_or("Unknown album")
        .to_string();
    let duration_ms = playback.duration_ms.max(track.duration_ms.unwrap_or(0));
    let (timestamp_start, timestamp_end) = if duration_ms > 0 {
        let now = unix_time_millis();
        let start = now.saturating_sub(playback.position_ms.max(0));
        (Some(start), Some(start.saturating_add(duration_ms)))
    } else {
        (None, None)
    };

    PresenceFields {
        title: discord_text(&title, 128),
        artist: discord_text(&artist, 128),
        album: discord_text(&album, 128),
        artwork_url,
        timestamp_start,
        timestamp_end,
    }
}

fn build_activity(fields: PresenceFields) -> Activity<'static> {
    let artwork = fields.artwork_url.unwrap_or_else(|| "logo".to_string());
    let assets = Assets::new().large_image(artwork).large_text(fields.album);
    let mut activity = Activity::new()
        .activity_type(ActivityType::Listening)
        .status_display_type(StatusDisplayType::State)
        .state(fields.artist)
        .details(fields.title)
        .assets(assets);
    if let (Some(start), Some(end)) = (fields.timestamp_start, fields.timestamp_end) {
        activity = activity.timestamps(Timestamps::new().start(start).end(end));
    }
    activity
}

fn discord_text(value: &str, max_bytes: usize) -> String {
    let trimmed = value.trim();
    let mut result = truncate_utf8(trimmed, max_bytes);
    if result.is_empty() {
        result = "Unknown".to_string();
    }
    // Discord rejects one-character fields, so pad without changing visible
    // content.
    if result.chars().count() < 2 {
        result.push('\u{180e}');
    }
    result
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn unix_time_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn is_catbox_url(url: &str) -> bool {
    url.starts_with(CATBOX_URL_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::fs;

    #[test]
    fn preserves_md5_base64_cache_key() {
        let keys = unique_cache_keys([md5_hex(b""), md5_hex(b""), md5_hex(b"")]);
        assert_eq!(keys, vec!["d41d8cd98f00b204e9800998ecf8427e"]);
    }

    #[test]
    fn catbox_artwork_is_pinned_to_the_album() {
        let first = artwork_cache_keys(42, b"normalized-one", b"original-one");
        let second = artwork_cache_keys(42, b"normalized-two", b"original-two");
        let persistent_key = album_artwork_key(42);
        let url = "https://files.catbox.moe/existing.jpg".to_string();
        let cache = CatboxCache {
            entries: HashMap::from([(persistent_key.clone(), url.clone())]),
        };

        assert!(first.contains(&persistent_key));
        assert!(second.contains(&persistent_key));
        assert_eq!(cache.lookup(&second), Some(url));
    }

    #[test]
    fn catbox_artwork_can_be_reused_without_a_local_image() {
        let url = "https://files.catbox.moe/existing.jpg".to_string();
        let cache = CatboxCache {
            entries: HashMap::from([(album_artwork_key(42), url.clone())]),
        };

        assert_eq!(persistent_catbox_url(&cache, Some(42)), Some(url));
        assert_eq!(persistent_catbox_url(&cache, None), None);
    }

    #[test]
    fn cache_cleanup_keeps_persisted_catbox_urls() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE lyrics (track_id INTEGER PRIMARY KEY, source TEXT NOT NULL);
            CREATE TABLE artist_info (artist_id INTEGER PRIMARY KEY, source TEXT NOT NULL);
            CREATE TABLE images (
                entity_type TEXT NOT NULL,
                entity_id INTEGER NOT NULL,
                source TEXT NOT NULL,
                file_path TEXT,
                PRIMARY KEY (entity_type, entity_id, source)
            );
            CREATE TABLE discord_artwork_cache (
                cache_key TEXT NOT NULL PRIMARY KEY,
                url TEXT NOT NULL,
                updated_at INTEGER NOT NULL DEFAULT 0
            );
            ",
        )
        .unwrap();
        let persistent_key = album_artwork_key(42);
        let url = "https://files.catbox.moe/existing.jpg";
        conn.execute(
            "INSERT INTO discord_artwork_cache (cache_key, url) VALUES (?1, ?2)",
            [&persistent_key, url],
        )
        .unwrap();
        let root = std::env::temp_dir().join(format!(
            "sparkle-catbox-persistence-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();

        cache::clear_lyrics(&conn).unwrap();
        cache::clear_artist_info(&conn, &root).unwrap();
        cache::clear_images(&conn, &root).unwrap();

        let store = CatboxCache::load(&conn).unwrap();
        assert_eq!(store.lookup(&[persistent_key]), Some(url.to_string()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn truncates_without_splitting_utf8_characters() {
        assert_eq!(truncate_utf8("hello\u{1f30d}", 6), "hello");
    }

    #[cfg(windows)]
    #[test]
    fn preserves_gdiplus_cache_keys() {
        const SOURCE: &str = "iVBORw0KGgoAAAANSUhEUgAAAAMAAAACCAYAAACddGYaAAAAAXNSR0IArs4c6QAAAARnQU1BAACxjwv8YQUAAAAJcEhZcwAADsMAAA7DAcdvqGQAAAAYSURBVBhXY/jPAEQgyPAfSIKZQMDw/z8Aqm8O8p3BH9oAAAAASUVORK5CYII=";
        const CACHE_KEY: &str = "acfb71ea714c8fea3149fa3ddecad7d9";

        let work_dir = std::env::temp_dir().join(format!(
            "sparkle-discord-gdiplus-test-{}",
            std::process::id()
        ));
        let source = STANDARD.decode(SOURCE).unwrap();
        let normalized = resize_to_cache_jpeg(&source, &work_dir);
        let _ = fs::remove_dir_all(&work_dir);
        let normalized = normalized.unwrap();
        let normalized_base64 = STANDARD.encode(normalized);

        assert_eq!(md5_hex(normalized_base64.as_bytes()), CACHE_KEY);
        let cache = CatboxCache {
            entries: HashMap::from([(
                CACHE_KEY.to_string(),
                "https://files.catbox.moe/existing.jpg".to_string(),
            )]),
        };
        assert_eq!(
            cache.lookup(&[md5_hex(normalized_base64.as_bytes())]),
            Some("https://files.catbox.moe/existing.jpg".to_string())
        );
    }
}
