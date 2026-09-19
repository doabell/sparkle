// Portions of the Discord presence integration were adapted from DiscordBee:
// https://github.com/sll552/DiscordBee
// Those adapted portions are licensed under Apache-2.0 and have been modified
// substantially for Sparkle. Sparkle's artwork storage integration and related
// changes are original Sparkle work. See THIRD_PARTY_NOTICES.md.

use crate::artwork_store::S3ArtworkStore;
use crate::cache;
use crate::models::{CachedImage, PlaybackState, Track};
use crate::settings::{self, DiscordLayout};
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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
#[path = "discord/gdiplus.rs"]
mod gdiplus;

const CATBOX_API_URL: &str = "https://catbox.moe/user/api.php";
const CATBOX_URL_PREFIX: &str = "https://files.catbox.moe/";
const ARTWORK_MAX_DIMENSION: u32 = 256;
const JPEG_QUALITY: u8 = 85;
const RETRY_INTERVAL: Duration = Duration::from_secs(10);
// Presence is a coarse status surface, not a karaoke display. Coalesce lyric
// changes to the current line instead of queueing every intervening sentence.
const LYRIC_UPDATE_INTERVAL: Duration = Duration::from_secs(15);
const TEST_ARTWORK_CACHE_KEY: &str = "sparkle-artwork-test";

/// A small, asynchronous bridge from the audio thread to Discord's local IPC.
/// Networking and image encoding happen on the worker so playback transitions
/// never wait for Discord or artwork storage.
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
        let _ = self.tx.send(DiscordCommand::Playback(PlaybackSnapshot {
            playback: Box::new(playback.clone()),
            received_at: Instant::now(),
        }));
    }

    /// Re-evaluate the most recent playback state after Settings changes.
    pub fn refresh(&self) {
        let _ = self.tx.send(DiscordCommand::Refresh);
    }
}

enum DiscordCommand {
    Playback(PlaybackSnapshot),
    Refresh,
}

struct ConnectedDiscordClient {
    app_id: String,
    activity_name: String,
    client: DiscordIpcClient,
    fields: Option<PresenceFields>,
    last_update: Instant,
}

struct PlaybackSnapshot {
    playback: Box<PlaybackState>,
    received_at: Instant,
}

impl PlaybackSnapshot {
    fn current(&self, now: Instant) -> PlaybackState {
        let mut playback = (*self.playback).clone();
        if playback.is_playing {
            let elapsed = now
                .saturating_duration_since(self.received_at)
                .as_millis()
                .min(i64::MAX as u128) as i64;
            playback.position_ms = playback.position_ms.max(0).saturating_add(elapsed);
            let duration = playback.duration_ms.max(
                playback
                    .current_track
                    .as_ref()
                    .and_then(|track| track.duration_ms)
                    .unwrap_or(0),
            );
            if duration > 0 {
                playback.position_ms = playback.position_ms.min(duration);
            }
        }
        playback
    }
}

struct ArtworkPayload {
    jpeg: Vec<u8>,
    content_keys: Vec<String>,
    persistent_key: String,
}

impl ArtworkPayload {
    fn durable_cache_keys(&self) -> Vec<String> {
        let mut keys = self.content_keys.clone();
        keys.push(self.persistent_key.clone());
        keys
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PresenceFields {
    name: String,
    status_display: String,
    show_artwork: bool,
    title: String,
    artist: String,
    album: String,
    artwork_url: Option<String>,
    timestamp_start: Option<i64>,
    timestamp_end: Option<i64>,
}

#[derive(Clone, Debug, Default)]
struct ArtworkUrls {
    catbox_url: Option<String>,
    s3_url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArtworkStoreKind {
    Disabled,
    Catbox,
    S3,
}

impl ArtworkStoreKind {
    fn from_setting(value: &str) -> Self {
        match value.trim() {
            "disabled" => Self::Disabled,
            "s3" => Self::S3,
            _ => Self::Catbox,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Catbox => "catbox",
            Self::S3 => "s3",
        }
    }
}

struct ArtworkStoreState {
    kind: ArtworkStoreKind,
    s3_store: Option<S3ArtworkStore>,
}

impl ArtworkStoreState {
    fn accepts_url(&self, url: &str) -> bool {
        match self.kind {
            ArtworkStoreKind::Disabled => false,
            ArtworkStoreKind::Catbox => is_catbox_url(url),
            ArtworkStoreKind::S3 => self
                .s3_store
                .as_ref()
                .map(|store| store.owns_public_url(url))
                .unwrap_or(false),
        }
    }
}

impl ArtworkUrls {
    fn url_for(&self, kind: ArtworkStoreKind) -> Option<&str> {
        match kind {
            ArtworkStoreKind::Disabled => None,
            ArtworkStoreKind::Catbox => self.catbox_url.as_deref(),
            ArtworkStoreKind::S3 => self.s3_url.as_deref(),
        }
    }

    fn any_url(&self) -> Option<&str> {
        self.catbox_url.as_deref().or(self.s3_url.as_deref())
    }

    fn set(&mut self, kind: ArtworkStoreKind, url: String) {
        match kind {
            ArtworkStoreKind::Disabled => {}
            ArtworkStoreKind::Catbox => self.catbox_url = Some(url),
            ArtworkStoreKind::S3 => self.s3_url = Some(url),
        }
    }
}

struct ArtworkCache {
    entries: HashMap<String, ArtworkUrls>,
}

impl ArtworkCache {
    fn load(conn: &Connection) -> Result<Self, String> {
        let mut statement = conn
            .prepare("SELECT cache_key, catbox_url, s3_url FROM discord_artwork_cache")
            .map_err(|err| err.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|err| err.to_string())?;
        let mut entries = HashMap::new();
        for row in rows {
            let (key, catbox_url, s3_url) = row.map_err(|err| err.to_string())?;
            let urls = ArtworkUrls {
                catbox_url: catbox_url.filter(|url| is_artwork_url(url)),
                s3_url: s3_url.filter(|url| is_artwork_url(url)),
            };
            if !key.is_empty() && urls.any_url().is_some() {
                entries.insert(key, urls);
            }
        }
        Ok(Self { entries })
    }

    #[cfg(test)]
    fn lookup(&self, keys: &[String]) -> Option<String> {
        keys.iter().find_map(|key| {
            self.entries
                .get(key)
                .and_then(|urls| urls.any_url())
                .filter(|url| is_artwork_url(url))
                .map(str::to_string)
        })
    }

    fn lookup_for_store(
        &self,
        keys: &[String],
        artwork_store: &ArtworkStoreState,
    ) -> Option<String> {
        keys.iter().find_map(|key| {
            self.entries
                .get(key)
                .and_then(|urls| urls.url_for(artwork_store.kind))
                .filter(|url| is_artwork_url(url))
                .filter(|url| artwork_store.accepts_url(url))
                .map(str::to_string)
        })
    }

    fn keys_match_url_for_store(
        &self,
        keys: &[String],
        kind: ArtworkStoreKind,
        expected_url: &str,
    ) -> bool {
        keys.iter().all(|key| {
            self.entries
                .get(key)
                .and_then(|urls| urls.url_for(kind))
                .filter(|url| is_artwork_url(url))
                == Some(expected_url)
        })
    }

    fn store(
        &mut self,
        conn: &Connection,
        keys: &[String],
        kind: ArtworkStoreKind,
        url: String,
    ) -> Result<(), String> {
        let (catbox_url, s3_url) = match kind {
            ArtworkStoreKind::Disabled => {
                return Err("artwork storage is disabled".to_string());
            }
            ArtworkStoreKind::Catbox => (Some(url.as_str()), None),
            ArtworkStoreKind::S3 => (None, Some(url.as_str())),
        };
        let tx = conn
            .unchecked_transaction()
            .map_err(|err| err.to_string())?;
        let mut statement = tx
            .prepare(
                "INSERT INTO discord_artwork_cache (cache_key, catbox_url, s3_url) \
                 VALUES (?1, ?2, ?3) \
                 ON CONFLICT(cache_key) DO UPDATE SET \
                   catbox_url = COALESCE(excluded.catbox_url, discord_artwork_cache.catbox_url), \
                   s3_url = COALESCE(excluded.s3_url, discord_artwork_cache.s3_url), \
                   updated_at = unixepoch()",
            )
            .map_err(|err| err.to_string())?;
        for key in keys {
            statement
                .execute(rusqlite::params![key, catbox_url, s3_url])
                .map_err(|err| err.to_string())?;
        }
        drop(statement);
        tx.commit().map_err(|err| err.to_string())?;
        for key in keys {
            self.entries
                .entry(key.clone())
                .or_default()
                .set(kind, url.clone());
        }
        Ok(())
    }
}

fn load_artwork_store(conn: &Connection) -> ArtworkStoreState {
    let settings = match settings::load_settings(conn) {
        Ok(settings) => settings,
        Err(err) => {
            log::error!(
                target: "sparkle::discord::artwork",
                "event=settings_load_failed store=catbox error={err}"
            );
            return ArtworkStoreState {
                kind: ArtworkStoreKind::Catbox,
                s3_store: None,
            };
        }
    };
    let raw_kind = settings.discord_artwork_store.trim();
    let kind = ArtworkStoreKind::from_setting(raw_kind);
    if !matches!(raw_kind, "disabled" | "catbox" | "s3") {
        log::warn!(
            target: "sparkle::discord::artwork",
            "event=invalid_store_setting store=catbox requested={raw_kind}"
        );
    }
    log::debug!(
        target: "sparkle::discord::artwork",
        "event=store_selected store={}",
        kind.name()
    );
    match kind {
        ArtworkStoreKind::S3 => match S3ArtworkStore::from_settings(&settings) {
            Ok(Some(store)) => {
                log::debug!(
                    target: "sparkle::discord::s3",
                    "event=store_configured store=s3"
                );
                ArtworkStoreState {
                    kind,
                    s3_store: Some(store),
                }
            }
            Ok(None) => {
                log::warn!(
                    target: "sparkle::discord::s3",
                    "event=configuration_invalid store=s3 error=endpoint and bucket are required"
                );
                ArtworkStoreState {
                    kind,
                    s3_store: None,
                }
            }
            Err(err) => {
                log::warn!(
                    target: "sparkle::discord::s3",
                    "event=configuration_invalid store=s3 error={err}"
                );
                ArtworkStoreState {
                    kind,
                    s3_store: None,
                }
            }
        },
        ArtworkStoreKind::Disabled | ArtworkStoreKind::Catbox => ArtworkStoreState {
            kind,
            s3_store: None,
        },
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
    let mut artwork_cache = match ArtworkCache::load(&conn) {
        Ok(cache) => {
            log::debug!(
                target: "sparkle::discord::artwork",
                "event=cache_loaded entries={}",
                cache.entries.len()
            );
            cache
        }
        Err(err) => {
            log::error!(target: "sparkle::discord::artwork", "event=cache_load_failed error={err}");
            return;
        }
    };
    let mut artwork_store = load_artwork_store(&conn);
    log::debug!(
        target: "sparkle::discord::presence",
        "event=worker_started store={}",
        artwork_store.kind.name()
    );
    let mut latest_playback: Option<PlaybackSnapshot> = None;
    let mut last_attempt = Instant::now();

    loop {
        let mut apply = false;
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(DiscordCommand::Playback(playback)) => {
                latest_playback = Some(playback);
                apply = true;
            }
            Ok(DiscordCommand::Refresh) => {
                artwork_store = load_artwork_store(&conn);
                match ArtworkCache::load(&conn) {
                    Ok(cache) => artwork_cache = cache,
                    Err(err) => log::warn!(
                        target: "sparkle::discord::artwork",
                        "event=cache_reload_failed error={err}"
                    ),
                }
                apply = true;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        let Some(snapshot) = latest_playback.as_ref() else {
            continue;
        };
        let playback = snapshot.current(Instant::now());
        let active = playback.is_playing && playback.current_track.is_some();
        // Retry if Discord starts later. Paused sessions must stay cleared.
        if apply || (active && client.is_none() && last_attempt.elapsed() >= RETRY_INTERVAL) {
            last_attempt = Instant::now();
            apply_playback(
                &conn,
                &image_cache_dir,
                &mut artwork_cache,
                &mut artwork_store,
                &mut client,
                snapshot,
            );
        } else if active {
            update_live_lyrics(&conn, &mut client, &playback);
        }
    }

    close_client(&mut client);
}

fn apply_playback(
    conn: &Connection,
    image_cache_dir: &Path,
    artwork_cache: &mut ArtworkCache,
    artwork_store: &mut ArtworkStoreState,
    client: &mut Option<ConnectedDiscordClient>,
    snapshot: &PlaybackSnapshot,
) {
    let playback = snapshot.current(Instant::now());
    let settings = match settings::load_settings(conn) {
        Ok(settings) => settings,
        Err(err) => {
            log::warn!(
                target: "sparkle::discord::presence",
                "event=settings_load_failed store={} error={err}",
                artwork_store.kind.name()
            );
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
    // cannot turn every cache miss into a remote upload.
    let Some(discord) = ensure_client(client, app_id, &settings.discord_layout.name) else {
        return;
    };

    let store_name = artwork_store.kind.name();
    let artwork_url = if !settings.discord_layout.show_artwork
        || artwork_store.kind == ArtworkStoreKind::Disabled
    {
        log::debug!(
            target: "sparkle::discord::artwork",
            "event=artwork_disabled store=disabled"
        );
        None
    } else {
        match artwork_for_track(conn, image_cache_dir, track) {
            Ok(Some(artwork)) => {
                let durable_keys = artwork.durable_cache_keys();
                match artwork_cache.lookup_for_store(&artwork.content_keys, artwork_store) {
                    Some(url) => {
                        // Content hashes prove that the durable album pointer still
                        // describes the current image. Repair missing legacy aliases
                        // and replace a stale album pointer without re-uploading.
                        if !artwork_cache.keys_match_url_for_store(
                            &durable_keys,
                            artwork_store.kind,
                            &url,
                        ) {
                            if let Err(err) = artwork_cache.store(
                                conn,
                                &durable_keys,
                                artwork_store.kind,
                                url.clone(),
                            ) {
                                log::warn!(
                                    target: "sparkle::discord::artwork",
                                    "event=persistent_key_store_failed store={store_name} error={err}"
                                );
                            }
                        }
                        log::debug!(
                            target: "sparkle::discord::artwork",
                            "event=cache_hit store={store_name}"
                        );
                        Some(url)
                    }
                    None => {
                        log::debug!(
                            target: "sparkle::discord::artwork",
                            "event=upload_started store={store_name} bytes={} content_hashes={}",
                            artwork.jpeg.len(),
                            artwork.content_keys.len()
                        );
                        let upload_result = match artwork_store.kind {
                            ArtworkStoreKind::S3 => match artwork_store.s3_store.as_mut() {
                                Some(s3_store) => {
                                    log::debug!(
                                        target: "sparkle::discord::s3",
                                        "event=probing_content_keys store=s3 candidates={}",
                                        artwork.content_keys.len()
                                    );
                                    s3_store.find_or_upload(artwork.jpeg, &artwork.content_keys)
                                }
                                None => Err("S3 artwork store is unavailable".to_string()),
                            },
                            ArtworkStoreKind::Catbox => upload_to_catbox(
                                artwork.jpeg,
                                &artwork.content_keys[0],
                                settings.discord_catbox_user_hash.trim(),
                            ),
                            ArtworkStoreKind::Disabled => {
                                Err("artwork storage is disabled".to_string())
                            }
                        };
                        match upload_result {
                            Ok(url) => {
                                log::debug!(
                                    target: "sparkle::discord::artwork",
                                    "event=upload_succeeded store={}",
                                    store_name
                                );
                                if let Err(err) = artwork_cache.store(
                                    conn,
                                    &durable_keys,
                                    artwork_store.kind,
                                    url.clone(),
                                ) {
                                    log::warn!(
                                        target: "sparkle::discord::artwork",
                                        "event=cache_store_failed store={store_name} error={err}"
                                    );
                                } else {
                                    log::debug!(
                                        target: "sparkle::discord::artwork",
                                        "event=cache_stored store={store_name} keys={}",
                                        durable_keys.len()
                                    );
                                }
                                Some(url)
                            }
                            Err(err) => {
                                log::warn!(
                                    target: "sparkle::discord::artwork",
                                    "event=upload_failed store={} error={err}",
                                    store_name
                                );
                                None
                            }
                        }
                    }
                }
            }
            Ok(None) => {
                let fallback =
                    persistent_artwork_url_for_store(artwork_cache, track.album_id, artwork_store);
                log::debug!(
                    target: "sparkle::discord::artwork",
                    "event=artwork_unavailable store={store_name} persistent_fallback={}",
                    fallback.is_some()
                );
                fallback
            }
            Err(err) => {
                let fallback =
                    persistent_artwork_url_for_store(artwork_cache, track.album_id, artwork_store);
                log::warn!(
                    target: "sparkle::discord::artwork",
                    "event=artwork_prepare_failed store={store_name} persistent_fallback={} error={err}",
                    fallback.is_some()
                );
                fallback
            }
        }
    };

    // Artwork storage can take seconds; use the clock again after it finishes.
    let playback = snapshot.current(Instant::now());
    let lyric = current_lyric(conn, &playback, &settings.discord_layout);
    let fields = presence_fields(
        &playback,
        track,
        artwork_url,
        &settings.discord_layout,
        lyric.as_deref(),
        &load_presence_metadata(conn, track),
    );
    if !publish_fields(discord, fields) {
        close_client(client);
    }
}

fn publish_fields(discord: &mut ConnectedDiscordClient, fields: PresenceFields) -> bool {
    if let Err(err) = discord.client.set_activity(build_activity(fields.clone())) {
        log::debug!(target: "sparkle::discord::presence", "event=activity_update_failed error={err}");
        false
    } else {
        discord.fields = Some(fields);
        discord.last_update = Instant::now();
        log::trace!(target: "sparkle::discord::presence", "event=activity_updated");
        true
    }
}

fn update_live_lyrics(
    conn: &Connection,
    client: &mut Option<ConnectedDiscordClient>,
    playback: &PlaybackState,
) {
    let Some(discord) = client.as_mut() else {
        return;
    };
    if discord.last_update.elapsed() < LYRIC_UPDATE_INTERVAL {
        return;
    }
    discord.last_update = Instant::now();
    let Ok(settings) = settings::load_settings(conn) else {
        return;
    };
    if !settings.discord_enabled || !settings.discord_layout.uses_lyrics() {
        return;
    }
    let Some(track) = playback.current_track.as_ref() else {
        return;
    };
    let Some(previous) = discord.fields.as_ref() else {
        return;
    };
    let lyric = current_lyric(conn, playback, &settings.discord_layout);
    // Reuse the artwork and timestamps: no encoding, upload, or moving time bar
    // on lyric updates. Always render the newest line after a seek or delay.
    let mut fields = presence_fields(
        playback,
        track,
        previous.artwork_url.clone(),
        &settings.discord_layout,
        lyric.as_deref(),
        &load_presence_metadata(conn, track),
    );
    fields.timestamp_start = previous.timestamp_start;
    fields.timestamp_end = previous.timestamp_end;
    let changed = fields != *previous;
    if changed && !publish_fields(discord, fields) {
        close_client(client);
    }
}

fn current_lyric(
    conn: &Connection,
    playback: &PlaybackState,
    layout: &DiscordLayout,
) -> Option<String> {
    if !layout.uses_lyrics() {
        return None;
    }
    cached_current_lyric(conn, playback)
}

fn cached_current_lyric(conn: &Connection, playback: &PlaybackState) -> Option<String> {
    let track = playback.current_track.as_ref()?;
    let text = crate::providers::lyrics::known_lyrics(conn, track)
        .ok()??
        .synced_text?;
    // Read the saved offset so edits take effect even without a playback event.
    let offset: i64 = conn
        .query_row(
            "SELECT lrc_offset_ms FROM tracks WHERE id = ?",
            [track.id],
            |row| row.get(0),
        )
        .unwrap_or(track.lrc_offset_ms);
    lyric_at_position(&text, playback.position_ms.saturating_sub(offset))
}

fn lyric_at_position(text: &str, position_ms: i64) -> Option<String> {
    crate::providers::lyrics::parse_lrc(text)
        .into_iter()
        .take_while(|line| line.time_ms <= position_ms)
        .last()
        .map(|line| line.text)
}

fn ensure_client<'a>(
    client: &'a mut Option<ConnectedDiscordClient>,
    app_id: &str,
    activity_name: &str,
) -> Option<&'a mut ConnectedDiscordClient> {
    let activity_name = normalized_activity_name(activity_name);
    let needs_connection = client
        .as_ref()
        .map(|existing| existing.app_id != app_id || existing.activity_name != activity_name)
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
            activity_name,
            client: new_client,
            fields: None,
            last_update: Instant::now(),
        });
        log::info!(target: "sparkle::discord::presence", "event=ipc_connected application_id={app_id}");
    }
    client.as_mut()
}

fn clear_presence(client: &mut Option<ConnectedDiscordClient>) {
    let failed = client
        .as_mut()
        .map(|connected| {
            connected.fields = None;
            connected.client.clear_activity().is_err()
        })
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
    let content_keys = artwork_content_keys(&jpeg, &original);

    Ok(Some(ArtworkPayload {
        jpeg,
        content_keys,
        persistent_key,
    }))
}

fn resize_to_cache_jpeg(original: &[u8], work_dir: &Path) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        gdiplus::resize_to_cache_jpeg(
            original,
            work_dir,
            &md5_hex(original),
            ARTWORK_MAX_DIMENSION,
            JPEG_QUALITY,
        )
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

/// Removes only the album-to-current-art pointer. Content-hash entries remain
/// reusable, so replacing artwork cannot force an upload when those bytes were
/// seen before.
pub(crate) fn invalidate_album_artwork(conn: &Connection, album_id: i64) -> Result<(), String> {
    conn.execute(
        "DELETE FROM discord_artwork_cache WHERE cache_key = ?",
        [album_artwork_key(album_id)],
    )
    .map(|_| ())
    .map_err(|err| err.to_string())
}

#[cfg(test)]
fn persistent_artwork_url(cache: &ArtworkCache, album_id: Option<i64>) -> Option<String> {
    album_id.and_then(|id| cache.lookup(&[album_artwork_key(id)]))
}

fn persistent_artwork_url_for_store(
    cache: &ArtworkCache,
    album_id: Option<i64>,
    artwork_store: &ArtworkStoreState,
) -> Option<String> {
    album_id.and_then(|id| cache.lookup_for_store(&[album_artwork_key(id)], artwork_store))
}

fn artwork_content_keys(normalized_jpeg: &[u8], original: &[u8]) -> Vec<String> {
    let normalized_base64 = STANDARD.encode(normalized_jpeg);
    let original_base64 = STANDARD.encode(original);
    unique_cache_keys([
        md5_hex(normalized_base64.as_bytes()),
        // Keep the original-artwork hash as a fallback so old cache entries
        // remain reusable without another upload.
        md5_hex(original_base64.as_bytes()),
        md5_hex(original),
    ])
}

fn md5_hex(input: &[u8]) -> String {
    // This is not used for security: the artwork cache uses MD5(base64 artwork)
    // as its stable lookup key.
    Md5::digest(input)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn test_artwork_storage(settings: &settings::Settings) -> Result<String, String> {
    let kind = ArtworkStoreKind::from_setting(&settings.discord_artwork_store);
    log::info!(
        target: "sparkle::discord::artwork",
        "event=test_started store={}",
        kind.name()
    );
    if kind == ArtworkStoreKind::Disabled {
        let err = "artwork storage is disabled".to_string();
        log::info!(
            target: "sparkle::discord::artwork",
            "event=test_skipped store=disabled"
        );
        return Err(err);
    }

    let jpeg = test_artwork_jpeg()?;
    let result = match kind {
        ArtworkStoreKind::S3 => {
            let mut store = S3ArtworkStore::from_settings(settings)?
                .ok_or_else(|| "S3 endpoint and bucket are required".to_string())?;
            store.test_access_and_upload(jpeg)
        }
        ArtworkStoreKind::Catbox => upload_to_catbox(
            jpeg,
            TEST_ARTWORK_CACHE_KEY,
            settings.discord_catbox_user_hash.trim(),
        ),
        ArtworkStoreKind::Disabled => unreachable!(),
    };
    match result {
        Ok(url) => {
            log::info!(
                target: "sparkle::discord::artwork",
                "event=test_succeeded store={}",
                kind.name()
            );
            Ok(url)
        }
        Err(err) => {
            log::warn!(
                target: "sparkle::discord::artwork",
                "event=test_failed store={} error={err}",
                kind.name()
            );
            Err(err)
        }
    }
}

fn test_artwork_jpeg() -> Result<Vec<u8>, String> {
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        1,
        1,
        image::Rgb([250, 36, 60]),
    ));
    let mut jpeg = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY);
    image
        .write_with_encoder(encoder)
        .map_err(|err| err.to_string())?;
    Ok(jpeg)
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

/// Read existing scan data on the Discord worker, never on the audio thread.
#[derive(Default)]
struct PresenceMetadata {
    album_artist: String,
    format: Option<String>,
    bitrate: Option<i64>,
    sample_rate: Option<i64>,
    bit_depth: Option<i64>,
    channels: Option<i64>,
}

fn load_presence_metadata(conn: &Connection, track: &Track) -> PresenceMetadata {
    let mut metadata = conn
        .query_row(
            "SELECT audio_format, audio_bitrate_kbps, sample_rate_hz, bit_depth, channels \
             FROM tracks WHERE id = ?1",
            [track.id],
            |row| {
                Ok(PresenceMetadata {
                    format: row.get(0)?,
                    bitrate: row.get(1)?,
                    sample_rate: row.get(2)?,
                    bit_depth: row.get(3)?,
                    channels: row.get(4)?,
                    ..Default::default()
                })
            },
        )
        .unwrap_or_default();
    // Prefer this track's explicit credits over the album's combined credits.
    let artists = (|| -> rusqlite::Result<Vec<String>> {
        let mut statement = conn.prepare(
            "SELECT name FROM (\
                 SELECT a.name, ta.position, a.id FROM track_album_artists ta \
                 JOIN artists a ON a.id = ta.artist_id WHERE ta.track_id = ?1 \
                 UNION ALL \
                 SELECT a.name, aa.position, a.id FROM album_artists aa \
                 JOIN artists a ON a.id = aa.artist_id \
                 WHERE aa.album_id = ?2 AND NOT EXISTS \
                     (SELECT 1 FROM track_album_artists WHERE track_id = ?1)\
             ) ORDER BY position, id",
        )?;
        let rows = statement.query_map(rusqlite::params![track.id, track.album_id], |row| {
            row.get(0)
        })?;
        rows.collect()
    })();
    metadata.album_artist = artists.unwrap_or_default().join(", ");
    metadata
}

fn positive_number(value: Option<i64>) -> String {
    value
        .filter(|value| *value > 0)
        .map(|value| value.to_string())
        .unwrap_or_default()
}

fn duration_text(duration_ms: i64) -> String {
    if duration_ms <= 0 {
        return String::new();
    }
    let seconds = duration_ms / 1000;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

fn presence_template_values(
    playback: &PlaybackState,
    track: &Track,
    lyric: Option<&str>,
    metadata: &PresenceMetadata,
) -> HashMap<&'static str, String> {
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
    let with_unit = |value: Option<i64>, unit: &str| {
        value
            .filter(|value| *value > 0)
            .map(|value| format!("{value}{unit}"))
            .unwrap_or_default()
    };
    HashMap::from([
        ("{title}", title),
        ("{artist}", artist),
        (
            "{lyrics}",
            lyric
                .filter(|line| !line.trim().is_empty())
                .unwrap_or(&album)
                .to_string(),
        ),
        ("{album}", album),
        ("{album_artist}", metadata.album_artist.clone()),
        ("{year}", positive_number(track.year)),
        ("{genre}", track.genre.clone().unwrap_or_default()),
        ("{track}", positive_number(track.track_number)),
        ("{disc}", positive_number(track.disc_number)),
        ("{duration}", duration_text(duration_ms)),
        (
            "{format}",
            metadata
                .format
                .as_deref()
                .unwrap_or_default()
                .to_uppercase(),
        ),
        ("{bitrate}", with_unit(metadata.bitrate, " kbps")),
        (
            "{sample_rate}",
            metadata
                .sample_rate
                .filter(|value| *value > 0)
                .map(|value| format!("{} kHz", value as f64 / 1000.0))
                .unwrap_or_default(),
        ),
        ("{bit_depth}", with_unit(metadata.bit_depth, "-bit")),
        (
            "{channels}",
            match metadata.channels {
                Some(1) => "Mono".to_string(),
                Some(2) => "Stereo".to_string(),
                _ => with_unit(metadata.channels, " channels"),
            },
        ),
    ])
}

fn presence_fields(
    playback: &PlaybackState,
    track: &Track,
    artwork_url: Option<String>,
    layout: &DiscordLayout,
    lyric: Option<&str>,
    metadata: &PresenceMetadata,
) -> PresenceFields {
    let values = presence_template_values(playback, track, lyric, metadata);
    let duration_ms = playback.duration_ms.max(track.duration_ms.unwrap_or(0));
    let (timestamp_start, timestamp_end) = if layout.show_progress && duration_ms > 0 {
        let now = unix_time_millis();
        let start = now.saturating_sub(playback.position_ms.max(0));
        (Some(start), Some(start.saturating_add(duration_ms)))
    } else {
        (None, None)
    };
    PresenceFields {
        name: normalized_activity_name(&layout.name),
        status_display: layout.status_display.clone(),
        show_artwork: layout.show_artwork,
        title: render_template(&layout.details, &values),
        artist: render_template(&layout.state, &values),
        album: render_template(&layout.image_text, &values),
        artwork_url,
        timestamp_start,
        timestamp_end,
    }
}

#[derive(serde::Serialize)]
pub struct DiscordPreview {
    track_id: i64,
    values: HashMap<&'static str, String>,
    artwork: Option<CachedImage>,
}

fn preview_for_playback(
    conn: &Connection,
    cache_dir: &Path,
    playback: &PlaybackState,
    track_id: i64,
) -> Option<DiscordPreview> {
    let track = playback
        .current_track
        .as_ref()
        .filter(|track| track.id == track_id)?;
    let lyric = cached_current_lyric(conn, playback);
    let values = presence_template_values(
        playback,
        track,
        lyric.as_deref(),
        &load_presence_metadata(conn, track),
    )
    .into_iter()
    .map(|(key, value)| (key.trim_matches(['{', '}']), value))
    .collect();
    let artwork = track.album_id.and_then(|id| {
        cache::get_image(conn, cache_dir, "album", id)
            .ok()
            .flatten()
    });
    Some(DiscordPreview {
        track_id,
        values,
        artwork,
    })
}

/// Preview existing playback/cache data without publishing or fetching artwork/lyrics.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn get_discord_preview(
    state: tauri::State<'_, crate::commands::AppState>,
    trackId: i64,
) -> Result<Option<DiscordPreview>, String> {
    let db = state.db.clone();
    let audio = state.audio.clone();
    let cache_dir = state.cache_dir.clone();
    tokio::task::spawn_blocking(move || {
        let playback = audio.get_playback_state()?;
        let conn = db.lock().map_err(|error| error.to_string())?;
        Ok(preview_for_playback(&conn, &cache_dir, &playback, trackId))
    })
    .await
    .map_err(|error| error.to_string())?
}

fn build_activity(fields: PresenceFields) -> Activity<'static> {
    let status_display = match fields.status_display.as_str() {
        "state" if !fields.artist.is_empty() => StatusDisplayType::State,
        "details" if !fields.title.is_empty() => StatusDisplayType::Details,
        _ => StatusDisplayType::Name,
    };
    let mut activity = Activity::new()
        .name(fields.name)
        .activity_type(ActivityType::Listening)
        .status_display_type(status_display);
    if !fields.artist.is_empty() {
        activity = activity.state(fields.artist);
    }
    if !fields.title.is_empty() {
        activity = activity.details(fields.title);
    }
    if fields.show_artwork {
        let artwork = fields.artwork_url.unwrap_or_else(|| "logo".to_string());
        let mut assets = Assets::new().large_image(artwork);
        if !fields.album.is_empty() {
            assets = assets.large_text(fields.album);
        }
        activity = activity.assets(assets);
    }
    if let (Some(start), Some(end)) = (fields.timestamp_start, fields.timestamp_end) {
        activity = activity.timestamps(Timestamps::new().start(start).end(end));
    }
    activity
}

fn normalized_activity_name(value: &str) -> String {
    discord_text(
        if value.trim().is_empty() {
            "Sparkle"
        } else {
            value
        },
        128,
    )
}

fn render_template(template: &str, values: &HashMap<&str, String>) -> String {
    let mut rendered = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        rendered.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find('}') else { break };
        let token = &rest[..=end];
        rendered.push_str(values.get(token).map(String::as_str).unwrap_or(token));
        rest = &rest[end + 1..];
    }
    rendered.push_str(rest);
    let rendered = rendered.split_whitespace().collect::<Vec<_>>().join(" ");
    if rendered.is_empty() {
        String::new()
    } else {
        discord_text(&rendered, 128)
    }
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

fn is_artwork_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

#[cfg(test)]
#[path = "tests/discord.rs"]
mod tests;
