use crate::models::RepeatMode;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

const MONITORED_FOLDERS_KEY: &str = "monitored_folders";
const ARTIST_SPLIT_REGEX_KEY: &str = "artist_split_regex";
const ARTIST_SPLIT_EXCEPTIONS_KEY: &str = "artist_split_exceptions";
const LYRICS_SOURCES_KEY: &str = "lyrics_sources";
const ARTIST_INFO_SOURCES_KEY: &str = "artist_info_sources";
const ARTIST_IMAGE_SOURCES_KEY: &str = "artist_image_sources";
const ALBUM_ART_SOURCES_KEY: &str = "album_art_sources";
const SCAN_ON_STARTUP_KEY: &str = "scan_on_startup";
const UI_FONT_KEY: &str = "ui_font";
const LYRICS_FONT_KEY: &str = "lyrics_font";
const REDUCE_MOTION_KEY: &str = "reduce_motion";
const BRAVE_API_KEY_KEY: &str = "brave_api_key";
const ACCENT_COLOR_KEY: &str = "accent_color";
const DISCORD_ENABLED_KEY: &str = "discord_enabled";
const DISCORD_APP_ID_KEY: &str = "discord_app_id";
const DISCORD_CATBOX_USER_HASH_KEY: &str = "discord_catbox_user_hash";
const DISCORD_ARTWORK_STORE_KEY: &str = "discord_artwork_store";
const DISCORD_ARTWORK_S3_ENDPOINT_KEY: &str = "discord_artwork_s3_endpoint";
const DISCORD_ARTWORK_S3_BUCKET_KEY: &str = "discord_artwork_s3_bucket";
const DISCORD_ARTWORK_S3_PUBLIC_URL_KEY: &str = "discord_artwork_s3_public_url";
const DISCORD_ARTWORK_S3_ACCESS_KEY_KEY: &str = "discord_artwork_s3_access_key";
const DISCORD_ARTWORK_S3_SECRET_KEY_KEY: &str = "discord_artwork_s3_secret_key";
const DISCORD_ARTWORK_S3_SESSION_TOKEN_KEY: &str = "discord_artwork_s3_session_token";
const DISCORD_ARTWORK_S3_REGION_KEY: &str = "discord_artwork_s3_region";
const DISCORD_ARTWORK_S3_PREFIX_KEY: &str = "discord_artwork_s3_prefix";
const DEBUG_LOGGING_ENABLED_KEY: &str = "debug_logging_enabled";
const SESSION_SNAPSHOT_KEY: &str = "session.snapshot";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SessionSnapshot {
    pub queue: Vec<i64>,
    pub queue_index: Option<usize>,
    pub position_ms: i64,
    pub volume: f64,
    pub is_playing: bool,
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default)]
    pub repeat_mode: RepeatMode,
    #[serde(default)]
    pub play_order: Vec<usize>,
}

impl Default for SessionSnapshot {
    fn default() -> Self {
        Self {
            queue: Vec::new(),
            queue_index: None,
            position_ms: 0,
            volume: 0.8,
            is_playing: false,
            shuffle: false,
            repeat_mode: RepeatMode::Off,
            play_order: Vec::new(),
        }
    }
}

#[allow(dead_code)]
pub fn load_session(conn: &Connection) -> Result<SessionSnapshot, String> {
    load_json(conn, SESSION_SNAPSHOT_KEY, SessionSnapshot::default())
}

#[allow(dead_code)]
pub fn save_session(conn: &Connection, session: &SessionSnapshot) -> rusqlite::Result<()> {
    let json = serde_json::to_string(session)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        rusqlite::params![SESSION_SNAPSHOT_KEY, json],
    )?;
    Ok(())
}

const DEFAULT_SPLIT_REGEX: &str = r";";

fn default_monitored_folders() -> Vec<String> {
    Vec::new()
}

fn default_artist_split_regex() -> String {
    DEFAULT_SPLIT_REGEX.to_string()
}

fn default_artist_split_exceptions() -> Vec<String> {
    vec!["AC/DC".to_string(), "Tyler, The Creator".to_string()]
}

fn default_scan_on_startup() -> bool {
    false
}

fn default_ui_font() -> String {
    "System".to_string()
}

fn default_lyrics_font() -> String {
    "Monospace".to_string()
}

fn default_reduce_motion() -> bool {
    false
}

fn default_brave_api_key() -> String {
    String::new()
}

fn default_accent_color() -> String {
    "#fa243c".to_string()
}

fn default_discord_enabled() -> bool {
    true
}

fn default_discord_app_id() -> String {
    String::new()
}

fn default_discord_catbox_user_hash() -> String {
    String::new()
}

fn default_discord_artwork_store() -> String {
    "catbox".to_string()
}

fn default_debug_logging_enabled() -> bool {
    false
}

fn default_lyrics_sources() -> Vec<String> {
    vec![
        "custom".to_string(),
        "embedded".to_string(),
        "lrc".to_string(),
        "lrclib".to_string(),
        "netease".to_string(),
        "kashinavi".to_string(),
        "qq".to_string(),
    ]
}

fn default_artist_info_sources() -> Vec<String> {
    vec!["custom".to_string(), "wikipedia:en".to_string()]
}

fn default_artist_image_sources() -> Vec<String> {
    vec![
        "custom".to_string(),
        "wikipedia:en".to_string(),
        "shazam".to_string(),
        "duckduckgo".to_string(),
    ]
}

fn default_album_art_sources() -> Vec<String> {
    vec![
        "custom".to_string(),
        "embedded".to_string(),
        "cover_art_archive".to_string(),
    ]
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    #[serde(default = "default_monitored_folders")]
    pub monitored_folders: Vec<String>,
    #[serde(default = "default_artist_split_regex")]
    pub artist_split_regex: String,
    #[serde(default = "default_artist_split_exceptions")]
    pub artist_split_exceptions: Vec<String>,
    #[serde(default = "default_scan_on_startup")]
    pub scan_on_startup: bool,
    #[serde(default = "default_ui_font")]
    pub ui_font: String,
    #[serde(default = "default_lyrics_font")]
    pub lyrics_font: String,
    #[serde(default = "default_reduce_motion")]
    pub reduce_motion: bool,
    #[serde(default = "default_brave_api_key")]
    pub brave_api_key: String,
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
    #[serde(default = "default_discord_enabled")]
    pub discord_enabled: bool,
    #[serde(default = "default_discord_app_id")]
    pub discord_app_id: String,
    #[serde(default = "default_discord_catbox_user_hash")]
    pub discord_catbox_user_hash: String,
    #[serde(default = "default_discord_artwork_store")]
    pub discord_artwork_store: String,
    #[serde(default)]
    pub discord_artwork_s3_endpoint: String,
    #[serde(default)]
    pub discord_artwork_s3_bucket: String,
    #[serde(default)]
    pub discord_artwork_s3_public_url: String,
    #[serde(default)]
    pub discord_artwork_s3_access_key: String,
    #[serde(default)]
    pub discord_artwork_s3_secret_key: String,
    #[serde(default)]
    pub discord_artwork_s3_session_token: String,
    #[serde(default)]
    pub discord_artwork_s3_region: String,
    #[serde(default)]
    pub discord_artwork_s3_prefix: String,
    #[serde(default = "default_debug_logging_enabled")]
    pub debug_logging_enabled: bool,
    #[serde(default = "default_lyrics_sources")]
    pub lyrics_sources: Vec<String>,
    #[serde(default = "default_artist_info_sources")]
    pub artist_info_sources: Vec<String>,
    #[serde(default = "default_artist_image_sources")]
    pub artist_image_sources: Vec<String>,
    #[serde(default = "default_album_art_sources")]
    pub album_art_sources: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            monitored_folders: default_monitored_folders(),
            artist_split_regex: default_artist_split_regex(),
            artist_split_exceptions: default_artist_split_exceptions(),
            scan_on_startup: default_scan_on_startup(),
            ui_font: default_ui_font(),
            lyrics_font: default_lyrics_font(),
            reduce_motion: default_reduce_motion(),
            brave_api_key: default_brave_api_key(),
            accent_color: default_accent_color(),
            discord_enabled: default_discord_enabled(),
            discord_app_id: default_discord_app_id(),
            discord_catbox_user_hash: default_discord_catbox_user_hash(),
            discord_artwork_store: default_discord_artwork_store(),
            discord_artwork_s3_endpoint: String::new(),
            discord_artwork_s3_bucket: String::new(),
            discord_artwork_s3_public_url: String::new(),
            discord_artwork_s3_access_key: String::new(),
            discord_artwork_s3_secret_key: String::new(),
            discord_artwork_s3_session_token: String::new(),
            discord_artwork_s3_region: String::new(),
            discord_artwork_s3_prefix: String::new(),
            debug_logging_enabled: default_debug_logging_enabled(),
            lyrics_sources: default_lyrics_sources(),
            artist_info_sources: default_artist_info_sources(),
            artist_image_sources: default_artist_image_sources(),
            album_art_sources: default_album_art_sources(),
        }
    }
}

fn load_json<T: for<'de> Deserialize<'de>>(
    conn: &Connection,
    key: &str,
    default: T,
) -> Result<T, String> {
    let value: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?", [key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())?;
    match value {
        Some(s) => serde_json::from_str(&s).map_err(|e| e.to_string()),
        None => Ok(default),
    }
}

#[allow(dead_code)]
fn save_json<T: Serialize>(conn: &Connection, key: &str, value: &T) -> Result<(), String> {
    let json = serde_json::to_string(value).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        rusqlite::params![key, json],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_settings(conn: &Connection) -> Result<Settings, String> {
    let mut lyrics_sources: Vec<String> =
        load_json(conn, LYRICS_SOURCES_KEY, default_lyrics_sources())?;
    for source in &mut lyrics_sources {
        if source == "petitlyrics" {
            *source = "kashinavi".to_string();
        }
    }
    lyrics_sources.dedup();
    Ok(Settings {
        monitored_folders: load_json(conn, MONITORED_FOLDERS_KEY, default_monitored_folders())?,
        artist_split_regex: load_json(conn, ARTIST_SPLIT_REGEX_KEY, default_artist_split_regex())?,
        artist_split_exceptions: load_json(
            conn,
            ARTIST_SPLIT_EXCEPTIONS_KEY,
            default_artist_split_exceptions(),
        )?,
        scan_on_startup: load_json(conn, SCAN_ON_STARTUP_KEY, default_scan_on_startup())?,
        ui_font: load_json(conn, UI_FONT_KEY, default_ui_font())?,
        lyrics_font: load_json(conn, LYRICS_FONT_KEY, default_lyrics_font())?,
        reduce_motion: load_json(conn, REDUCE_MOTION_KEY, default_reduce_motion())?,
        brave_api_key: load_json(conn, BRAVE_API_KEY_KEY, default_brave_api_key())?,
        accent_color: load_json(conn, ACCENT_COLOR_KEY, default_accent_color())?,
        discord_enabled: load_json(conn, DISCORD_ENABLED_KEY, default_discord_enabled())?,
        discord_app_id: load_json(conn, DISCORD_APP_ID_KEY, default_discord_app_id())?,
        discord_catbox_user_hash: load_json(
            conn,
            DISCORD_CATBOX_USER_HASH_KEY,
            default_discord_catbox_user_hash(),
        )?,
        discord_artwork_store: load_json(
            conn,
            DISCORD_ARTWORK_STORE_KEY,
            default_discord_artwork_store(),
        )?,
        discord_artwork_s3_endpoint: load_json(
            conn,
            DISCORD_ARTWORK_S3_ENDPOINT_KEY,
            String::new(),
        )?,
        discord_artwork_s3_bucket: load_json(conn, DISCORD_ARTWORK_S3_BUCKET_KEY, String::new())?,
        discord_artwork_s3_public_url: load_json(
            conn,
            DISCORD_ARTWORK_S3_PUBLIC_URL_KEY,
            String::new(),
        )?,
        discord_artwork_s3_access_key: load_json(
            conn,
            DISCORD_ARTWORK_S3_ACCESS_KEY_KEY,
            String::new(),
        )?,
        discord_artwork_s3_secret_key: load_json(
            conn,
            DISCORD_ARTWORK_S3_SECRET_KEY_KEY,
            String::new(),
        )?,
        discord_artwork_s3_session_token: load_json(
            conn,
            DISCORD_ARTWORK_S3_SESSION_TOKEN_KEY,
            String::new(),
        )?,
        discord_artwork_s3_region: load_json(conn, DISCORD_ARTWORK_S3_REGION_KEY, String::new())?,
        discord_artwork_s3_prefix: load_json(conn, DISCORD_ARTWORK_S3_PREFIX_KEY, String::new())?,
        debug_logging_enabled: load_json(
            conn,
            DEBUG_LOGGING_ENABLED_KEY,
            default_debug_logging_enabled(),
        )?,
        lyrics_sources,
        artist_info_sources: load_json(
            conn,
            ARTIST_INFO_SOURCES_KEY,
            default_artist_info_sources(),
        )?,
        artist_image_sources: load_json(
            conn,
            ARTIST_IMAGE_SOURCES_KEY,
            default_artist_image_sources(),
        )?,
        album_art_sources: load_json(conn, ALBUM_ART_SOURCES_KEY, default_album_art_sources())?,
    })
}

#[allow(dead_code)]
pub fn save_settings(conn: &Connection, settings: &Settings) -> Result<(), String> {
    save_json(conn, MONITORED_FOLDERS_KEY, &settings.monitored_folders)?;
    save_json(conn, ARTIST_SPLIT_REGEX_KEY, &settings.artist_split_regex)?;
    save_json(
        conn,
        ARTIST_SPLIT_EXCEPTIONS_KEY,
        &settings.artist_split_exceptions,
    )?;
    save_json(conn, SCAN_ON_STARTUP_KEY, &settings.scan_on_startup)?;
    save_json(conn, UI_FONT_KEY, &settings.ui_font)?;
    save_json(conn, LYRICS_FONT_KEY, &settings.lyrics_font)?;
    save_json(conn, REDUCE_MOTION_KEY, &settings.reduce_motion)?;
    save_json(conn, BRAVE_API_KEY_KEY, &settings.brave_api_key)?;
    save_json(conn, ACCENT_COLOR_KEY, &settings.accent_color)?;
    save_json(conn, DISCORD_ENABLED_KEY, &settings.discord_enabled)?;
    save_json(conn, DISCORD_APP_ID_KEY, &settings.discord_app_id)?;
    save_json(
        conn,
        DISCORD_CATBOX_USER_HASH_KEY,
        &settings.discord_catbox_user_hash,
    )?;
    save_json(
        conn,
        DISCORD_ARTWORK_STORE_KEY,
        &settings.discord_artwork_store,
    )?;
    save_json(
        conn,
        DISCORD_ARTWORK_S3_ENDPOINT_KEY,
        &settings.discord_artwork_s3_endpoint,
    )?;
    save_json(
        conn,
        DISCORD_ARTWORK_S3_BUCKET_KEY,
        &settings.discord_artwork_s3_bucket,
    )?;
    save_json(
        conn,
        DISCORD_ARTWORK_S3_PUBLIC_URL_KEY,
        &settings.discord_artwork_s3_public_url,
    )?;
    save_json(
        conn,
        DISCORD_ARTWORK_S3_ACCESS_KEY_KEY,
        &settings.discord_artwork_s3_access_key,
    )?;
    save_json(
        conn,
        DISCORD_ARTWORK_S3_SECRET_KEY_KEY,
        &settings.discord_artwork_s3_secret_key,
    )?;
    save_json(
        conn,
        DISCORD_ARTWORK_S3_SESSION_TOKEN_KEY,
        &settings.discord_artwork_s3_session_token,
    )?;
    save_json(
        conn,
        DISCORD_ARTWORK_S3_REGION_KEY,
        &settings.discord_artwork_s3_region,
    )?;
    save_json(
        conn,
        DISCORD_ARTWORK_S3_PREFIX_KEY,
        &settings.discord_artwork_s3_prefix,
    )?;
    save_json(
        conn,
        DEBUG_LOGGING_ENABLED_KEY,
        &settings.debug_logging_enabled,
    )?;
    save_json(conn, LYRICS_SOURCES_KEY, &settings.lyrics_sources)?;
    save_json(conn, ARTIST_INFO_SOURCES_KEY, &settings.artist_info_sources)?;
    save_json(
        conn,
        ARTIST_IMAGE_SOURCES_KEY,
        &settings.artist_image_sources,
    )?;
    save_json(conn, ALBUM_ART_SOURCES_KEY, &settings.album_art_sources)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )
        .unwrap();
        let mut settings = Settings::default();
        settings.monitored_folders.push("C:\\Music".to_string());
        settings.artist_split_regex = "foo".to_string();
        settings.debug_logging_enabled = true;
        settings.discord_artwork_s3_endpoint = "https://s3.example.test".to_string();
        settings.discord_artwork_s3_bucket = "artwork".to_string();
        settings.discord_artwork_s3_access_key = "access".to_string();
        settings.discord_artwork_s3_secret_key = "secret".to_string();
        settings.discord_artwork_store = "s3".to_string();
        save_settings(&conn, &settings).unwrap();
        let loaded = load_settings(&conn).unwrap();
        assert_eq!(loaded.monitored_folders, settings.monitored_folders);
        assert_eq!(loaded.artist_split_regex, settings.artist_split_regex);
        assert_eq!(
            loaded.artist_split_exceptions,
            settings.artist_split_exceptions
        );
        assert_eq!(loaded.scan_on_startup, settings.scan_on_startup);
        assert_eq!(loaded.ui_font, settings.ui_font);
        assert_eq!(loaded.lyrics_font, settings.lyrics_font);
        assert_eq!(loaded.reduce_motion, settings.reduce_motion);
        assert_eq!(loaded.brave_api_key, settings.brave_api_key);
        assert_eq!(loaded.accent_color, settings.accent_color);
        assert_eq!(loaded.discord_enabled, settings.discord_enabled);
        assert_eq!(loaded.discord_app_id, settings.discord_app_id);
        assert_eq!(
            loaded.discord_catbox_user_hash,
            settings.discord_catbox_user_hash
        );
        assert_eq!(loaded.discord_artwork_store, settings.discord_artwork_store);
        assert_eq!(
            loaded.discord_artwork_s3_endpoint,
            settings.discord_artwork_s3_endpoint
        );
        assert_eq!(
            loaded.discord_artwork_s3_bucket,
            settings.discord_artwork_s3_bucket
        );
        assert_eq!(
            loaded.discord_artwork_s3_access_key,
            settings.discord_artwork_s3_access_key
        );
        assert_eq!(
            loaded.discord_artwork_s3_secret_key,
            settings.discord_artwork_s3_secret_key
        );
        assert_eq!(loaded.debug_logging_enabled, settings.debug_logging_enabled);
        assert_eq!(loaded.lyrics_sources, settings.lyrics_sources);
        assert_eq!(loaded.artist_info_sources, settings.artist_info_sources);
        assert_eq!(loaded.artist_image_sources, settings.artist_image_sources);
        assert_eq!(loaded.album_art_sources, settings.album_art_sources);
    }
}
