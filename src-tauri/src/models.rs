use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Folder {
    pub id: i64,
    pub path: String,
    pub enabled: bool,
    pub scanned_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Artist {
    pub id: i64,
    pub name: String,
    pub sort_name: Option<String>,
    pub track_count: Option<i64>,
    pub album_count: Option<i64>,
    #[serde(default)]
    pub bio: Option<String>,
    /// Per-field metadata providers: None = follow the global list,
    /// "custom" = user content, "wikipedia:{lang}", "brave".
    #[serde(default)]
    pub info_provider: Option<String>,
    #[serde(default)]
    pub image_provider: Option<String>,
    #[serde(default)]
    pub info_term: Option<String>,
    #[serde(default)]
    pub image_term: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Album {
    pub id: i64,
    pub title: String,
    pub year: Option<i64>,
    #[serde(default)]
    pub artist_ids: Vec<i64>,
    pub artist_names: Vec<String>,
    pub track_count: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Track {
    pub id: i64,
    pub file_path: String,
    pub title: Option<String>,
    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
    pub duration_ms: Option<i64>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub album_id: Option<i64>,
    pub embedded_lyrics: Option<String>,
    pub artist_ids: Vec<i64>,
    pub artist_names: Vec<String>,
    pub album_title: Option<String>,
    pub lrc_offset_ms: i64,
    #[serde(default)]
    pub lyrics_source: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlaybackState {
    pub is_playing: bool,
    pub current_track: Option<Track>,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub volume: f64,
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,
}

#[derive(Serialize, Clone, Debug)]
pub struct QueueView {
    /// Tracks in effective playback order (shuffle applied).
    pub tracks: Vec<Track>,
    /// Position within `tracks` of the currently loaded track.
    pub current_pos: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Genre {
    pub name: String,
    pub track_count: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScanResult {
    pub scanned: usize,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub errors: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct ScanProgress {
    pub phase: String,
    pub current_path: Option<String>,
    pub scanned: usize,
    pub total: usize,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub errors: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Lyrics {
    pub source: String,
    pub synced_text: Option<String>,
    pub plain_text: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ArtistInfo {
    pub source: String,
    pub summary: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ImageData {
    pub source: String,
    pub data: Option<Vec<u8>>,
    pub mime_type: String,
}

/// A cached image that can be loaded directly by the webview's asset
/// protocol. Keeping image bytes out of command responses avoids duplicating
/// them across Rust, IPC, JavaScript arrays, and base64 data URLs.
#[derive(Serialize, Clone, Debug)]
pub struct CachedImage {
    pub source: String,
    pub file_path: Option<String>,
    pub mime_type: String,
}

impl CachedImage {
    pub fn none() -> Self {
        Self {
            source: "none".to_string(),
            file_path: None,
            mime_type: "image/jpeg".to_string(),
        }
    }
}

pub fn detect_image_mime_type(data: &[u8]) -> String {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png".to_string()
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg".to_string()
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        "image/gif".to_string()
    } else if data.starts_with(b"RIFF") && data.len() >= 12 && &data[8..12] == b"WEBP" {
        "image/webp".to_string()
    } else if data.starts_with(b"BM") {
        "image/bmp".to_string()
    } else {
        "image/jpeg".to_string()
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AccentForegroundPreference {
    Light,
    Dark,
    #[default]
    #[serde(other)]
    Auto,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OnlineSettings {
    pub scan_on_startup: bool,
    pub lyrics_sources: Vec<String>,
    pub artist_info_sources: Vec<String>,
    pub artist_image_sources: Vec<String>,
    pub album_art_sources: Vec<String>,
    pub artist_split_regex: String,
    pub artist_split_exceptions: Vec<String>,
    pub ui_font: String,
    pub lyrics_font: String,
    pub reduce_motion: bool,
    pub brave_api_key: String,
    #[serde(default)]
    pub accent_color: String,
    #[serde(default)]
    pub accent_foreground_preference: AccentForegroundPreference,
    #[serde(default)]
    pub discord_enabled: bool,
    #[serde(default)]
    pub discord_app_id: String,
    #[serde(default)]
    pub discord_catbox_user_hash: String,
    #[serde(default)]
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
    #[serde(default)]
    pub debug_logging_enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub folder_path: Option<String>,
    pub live_mix: Option<String>,
    pub track_count: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlaylistDetail {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub folder_path: Option<String>,
    pub live_mix: Option<String>,
    pub tracks: Vec<Track>,
}

#[derive(Serialize, Clone, Debug)]
pub struct LyricMatch {
    pub track: Track,
    pub snippet: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct SearchResults {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub tracks: Vec<Track>,
    pub lyric_tracks: Vec<LyricMatch>,
}

#[derive(Serialize, Clone, Debug)]
pub struct LyricCandidate {
    pub source: String,
    pub synced_text: Option<String>,
    pub plain_text: Option<String>,
    pub preview: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct LyricSearchResults {
    pub candidates: Vec<LyricCandidate>,
    pub enabled_sources: Vec<String>,
    pub failed_sources: Vec<String>,
    pub timed_out_sources: Vec<String>,
}

/// An online image search candidate: just a URL and its source. Bytes are
/// only downloaded when the user picks one — searching must stay fast.
#[derive(Serialize, Clone, Debug)]
pub struct ImageCandidate {
    pub source: String,
    pub url: String,
}

/// The outcome of a manual artist-image search. Candidate URLs are returned
/// even when one enabled provider failed or exceeded the search budget.
#[derive(Serialize, Clone, Debug)]
pub struct ImageSearchResults {
    pub candidates: Vec<ImageCandidate>,
    pub failed_sources: Vec<String>,
    pub timed_out_sources: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct PlayStatTrack {
    pub track_id: i64,
    pub title: Option<String>,
    pub artist_names: Vec<String>,
    pub album_id: Option<i64>,
    pub plays: i64,
    pub ms: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct PlayStatArtist {
    pub artist_id: i64,
    pub name: String,
    pub plays: i64,
    pub ms: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct PlayStatAlbum {
    pub album_id: i64,
    pub title: String,
    pub artist_names: Vec<String>,
    pub plays: i64,
    pub ms: i64,
}

/// One activity bucket: a day ("2026-07-18") or, for long ranges, a month
/// ("2026-07").
#[derive(Serialize, Clone, Debug)]
pub struct PlayStatBucket {
    pub label: String,
    pub plays: i64,
    pub ms: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct ListeningStats {
    pub total_plays: i64,
    pub total_ms: i64,
    pub active_days: i64,
    pub unique_tracks: i64,
    pub unique_artists: i64,
    pub completed_plays: i64,
    pub discovery_tracks: i64,
    pub longest_streak_days: i64,
    pub session_count: i64,
    pub peak_hour: Option<i64>,
    pub peak_hour_ms: i64,
    pub morning_ms: i64,
    pub afternoon_ms: i64,
    pub evening_ms: i64,
    pub late_night_ms: i64,
    pub weekend_ms: i64,
    pub top_genre: Option<String>,
    pub top_genre_ms: i64,
    pub average_year: Option<f64>,
    pub top_tracks: Vec<PlayStatTrack>,
    pub top_artists: Vec<PlayStatArtist>,
    pub top_albums: Vec<PlayStatAlbum>,
    /// Sparse activity buckets, oldest first. Bucketing is by day for ranges
    /// up to 120 days, by month beyond that ("all time" included).
    pub activity: Vec<PlayStatBucket>,
    /// True when `activity` is bucketed by month rather than by day.
    pub activity_by_month: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct DiscoveryTracks {
    pub recently_added: Vec<Track>,
    pub most_played: Vec<Track>,
    pub never_played: Vec<Track>,
}

#[derive(Serialize, Clone, Debug)]
pub struct LibraryHealth {
    pub track_count: i64,
    pub album_count: i64,
    pub artist_count: i64,
    pub missing_titles: i64,
    pub missing_artists: i64,
    pub missing_albums: i64,
    pub missing_genres: i64,
    pub missing_lyrics: i64,
    pub missing_years: i64,
    pub missing_track_numbers: i64,
    pub duplicate_titles: i64,
    pub never_played: i64,
    pub lossless_tracks: i64,
    pub lossy_tracks: i64,
    pub unclassified_tracks: i64,
    pub high_resolution_tracks: i64,
    pub low_bitrate_tracks: i64,
    pub missing_audio_properties: i64,
    pub missing_durations: i64,
    pub very_short_tracks: i64,
    pub very_long_tracks: i64,
    pub mono_tracks: i64,
    pub total_size_bytes: i64,
    pub formats: Vec<AudioFormatStat>,
}

#[derive(Serialize, Clone, Debug)]
pub struct AudioFormatStat {
    pub format: String,
    pub tracks: i64,
}
