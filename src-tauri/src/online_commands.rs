use crate::cache;
use crate::commands::AppState;
use crate::models::{
    detect_image_mime_type, ArtistInfo, CachedImage, ImageCandidate, ImageData, ImageSearchResults,
    LyricCandidate, LyricSearchResults, Lyrics, OnlineSettings,
};
use crate::providers::lyrics::{self, TrackMetadata};
use crate::settings;
use crate::settings::Settings;
use rusqlite::OptionalExtension;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::State;

async fn load_settings_async(db: &Arc<Mutex<rusqlite::Connection>>) -> Result<Settings, String> {
    let db = db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().map_err(|e| e.to_string())?;
        settings::load_settings(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e)
}

fn can_cache_lyrics_result(
    expected_override: &Option<String>,
    current_override: &Option<String>,
) -> bool {
    expected_override == current_override
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn get_lyrics(state: State<'_, AppState>, trackId: i64) -> Result<Lyrics, String> {
    let db = state.db.clone();
    let settings = load_settings_async(&db).await?;

    // 1. Cache + per-track provider override with a short DB lock. Custom
    //    lyrics are retained independently, like custom artwork.
    let (custom, cached, override_source) = tokio::task::spawn_blocking({
        let db = db.clone();
        move || -> Result<(Option<Lyrics>, Vec<Lyrics>, Option<String>), String> {
            let conn = db.lock().map_err(|e| e.to_string())?;
            let custom = cache::get_lyrics_from_source(&conn, trackId, "custom")?;
            let cached = cache::get_non_custom_lyrics(&conn, trackId)?;
            let override_source: Option<String> = conn
                .query_row(
                    "SELECT NULLIF(lyrics_source, '') FROM tracks WHERE id = ?",
                    [trackId],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?
                .flatten();
            Ok((custom, cached, override_source))
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    if let Some(source) = override_source.as_deref() {
        if source == "custom" {
            if let Some(lyrics) = custom.clone() {
                return Ok(lyrics);
            }
        } else if let Some(lyrics) = cached.iter().find(|lyrics| lyrics.source == source) {
            return Ok(lyrics.clone());
        }
    } else {
        let cached = settings.lyrics_sources.iter().find_map(|source| {
            if source == "custom" {
                custom.as_ref()
            } else {
                cached.iter().find(|lyrics| lyrics.source == *source)
            }
        });
        if let Some(lyrics) = cached {
            return Ok(lyrics.clone());
        }
    }

    // 2. Read the track metadata needed by providers. Still a short DB lock.
    let metadata = tokio::task::spawn_blocking({
        let db = db.clone();
        move || -> Result<TrackMetadata, String> {
            let conn = db.lock().map_err(|e| e.to_string())?;
            lyrics::fetch_track_metadata(&conn, trackId)
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    // 3. Fetch lyrics from the override source (if any) or the configured
    //    list, without holding the DB lock. Network I/O happens here.
    let expected_override = override_source.clone();
    let sources = match override_source {
        Some(source) => vec![source],
        None => settings.lyrics_sources.clone(),
    };
    let custom_for_fetch = custom.clone();
    let lyrics = tokio::task::spawn_blocking(move || -> Result<Option<Lyrics>, String> {
        lyrics::fetch_lyrics_from_sources_with_custom(
            &sources,
            &metadata,
            custom_for_fetch.as_ref(),
        )
    })
    .await
    .map_err(|e| e.to_string())??;

    // 4. Write cache with a short DB lock.
    if let Some(ref lyrics) = lyrics {
        let lyrics = lyrics.clone();
        tokio::task::spawn_blocking({
            let db = db.clone();
            let expected_override = expected_override.clone();
            move || -> Result<(), String> {
                let conn = db.lock().map_err(|e| e.to_string())?;
                let current_override: Option<String> = conn
                    .query_row(
                        "SELECT NULLIF(lyrics_source, '') FROM tracks WHERE id = ?",
                        [trackId],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?
                    .flatten();
                if !can_cache_lyrics_result(&expected_override, &current_override) {
                    // A source selection or custom lyric changed while this
                    // network request was running. Let its newer request own
                    // the cache instead of restoring stale lyrics.
                    return Ok(());
                }
                cache::set_lyrics(
                    &conn,
                    trackId,
                    &lyrics.source,
                    lyrics.synced_text.as_deref(),
                    lyrics.plain_text.as_deref(),
                )
            }
        })
        .await
        .map_err(|e| e.to_string())??;
    }

    lyrics.ok_or_else(|| "lyrics not found".to_string())
}

fn lyric_preview(plain: Option<&str>) -> String {
    plain
        .unwrap_or("")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" / ")
        .chars()
        .take(140)
        .collect()
}

/// Manual lyrics search across every enabled provider that can yield a
/// candidate, for the per-song lyrics picker. Runs the providers in parallel.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn search_lyrics_online(
    state: State<'_, AppState>,
    trackId: i64,
    query: Option<String>,
) -> Result<LyricSearchResults, String> {
    let db = state.db.clone();
    let settings = load_settings_async(&db).await?;
    let metadata = tokio::task::spawn_blocking({
        let db = db.clone();
        move || -> Result<TrackMetadata, String> {
            let conn = db.lock().map_err(|e| e.to_string())?;
            lyrics::fetch_track_metadata(&conn, trackId)
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    let title = metadata.title.clone().unwrap_or_default();
    let artist = metadata.artist.clone().unwrap_or_default();
    let default_query = format!("{} {}", title, artist).trim().to_string();
    let has_explicit_query = query.as_deref().is_some_and(|q| !q.trim().is_empty());
    let query = query
        .map(|q| q.trim().to_string())
        .filter(|q| !q.is_empty())
        .unwrap_or(default_query);

    tokio::task::spawn_blocking(move || -> Result<LyricSearchResults, String> {
        let sources = manual_lyrics_search_sources(&settings);
        let enabled_sources = sources.clone();
        let album = metadata.album.clone();
        let duration_sec = metadata.duration_ms.map(|d| (d / 1000) as u64);
        let outcome = collect_manual_image_search(
            sources,
            MANUAL_IMAGE_SEARCH_TIMEOUT,
            move |source| -> Result<Vec<Lyrics>, String> {
                match source.as_str() {
                    "embedded" if !has_explicit_query => {
                        Ok(lyrics::embedded::fetch(&metadata)?.into_iter().collect())
                    }
                    "lrc" if !has_explicit_query => {
                        Ok(lyrics::lrc::fetch(&metadata)?.into_iter().collect())
                    }
                    "embedded" | "lrc" => Ok(Vec::new()),
                    "lrclib" => lyrics::lrclib::fetch_candidates(&query, 3),
                    "netease" if has_explicit_query => {
                        lyrics::netease::fetch_candidates_for_query(&query, 3)
                    }
                    "netease" => lyrics::netease::fetch_candidates(
                        &title,
                        &artist,
                        album.as_deref(),
                        duration_sec,
                        3,
                    ),
                    "qq" if has_explicit_query => lyrics::qq::fetch_candidates_for_query(&query, 3),
                    "qq" => lyrics::qq::fetch_candidates(&title, &artist, 3),
                    "kashinavi" if has_explicit_query => {
                        lyrics::kashinavi::fetch_candidates_for_query(&query, 3)
                    }
                    "kashinavi" => Ok(lyrics::kashinavi::fetch_kashinavi_lyrics_blocking(
                        &title, &artist,
                    )?
                    .into_iter()
                    .collect()),
                    _ => Ok(Vec::new()),
                }
            },
        );
        let candidates = outcome
            .candidates
            .into_iter()
            .map(|l| LyricCandidate {
                preview: lyric_preview(l.plain_text.as_deref()),
                source: l.source,
                synced_text: l.synced_text,
                plain_text: l.plain_text,
            })
            .collect();
        Ok(LyricSearchResults {
            candidates,
            enabled_sources,
            failed_sources: outcome.failed_sources.into_iter().map(|(s, _)| s).collect(),
            timed_out_sources: outcome.timed_out_sources,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn store_manual_lyrics_choice(
    conn: &rusqlite::Connection,
    track_id: i64,
    _selected_source: &str,
    synced_text: Option<&str>,
    plain_text: Option<&str>,
) -> Result<(), String> {
    // A result chosen by the user is user-owned content, not a disposable
    // provider cache entry. This mirrors manually selected artist artwork.
    cache::set_lyrics(conn, track_id, "custom", synced_text, plain_text)?;
    conn.execute(
        "UPDATE tracks SET lyrics_source = 'custom' WHERE id = ?",
        [track_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Stores a manually picked lyrics result as durable custom content.
#[tauri::command]
#[allow(non_snake_case)]
pub fn set_track_lyrics_choice(
    state: State<'_, AppState>,
    trackId: i64,
    source: String,
    syncedText: Option<String>,
    plainText: Option<String>,
) -> Result<(), String> {
    if syncedText
        .as_deref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
        && plainText
            .as_deref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
    {
        return Err("lyrics are empty".to_string());
    }
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    store_manual_lyrics_choice(
        &conn,
        trackId,
        &source,
        syncedText.as_deref(),
        plainText.as_deref(),
    )
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn get_artist_info(
    state: State<'_, AppState>,
    artistId: i64,
) -> Result<ArtistInfo, String> {
    let db = state.db.clone();
    let cache_dir = state.cache_dir.clone();
    let settings = load_settings_async(&db).await?;

    // 1. Bio + cache + query term + explicit provider, short DB lock.
    let (bio, cached, title, info_provider) = tokio::task::spawn_blocking({
        let db = db.clone();
        let cache_dir = cache_dir.clone();
        move || -> Result<(Option<String>, Option<ArtistInfo>, Option<String>, Option<String>), String> {
            let conn = db.lock().map_err(|e| e.to_string())?;
            let (bio, info_provider): (Option<String>, Option<String>) = conn
                .query_row(
                    "SELECT NULLIF(bio, ''), NULLIF(info_provider, '') FROM artists WHERE id = ?",
                    [artistId],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|e| e.to_string())?
                .unwrap_or((None, None));
            let cached = crate::providers::artist::get_cached_artist_info(&conn, &cache_dir, artistId)?;
            let title = crate::providers::artist::artist_query_title(&conn, artistId)?;
            Ok((bio, cached, title, info_provider))
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    // An explicit per-artist provider (mix & match) overrides the global
    // list entirely: "custom" serves the user's bio, anything else fetches
    // from that single provider.
    if let Some(provider) = info_provider {
        if provider == "custom" {
            return Ok(ArtistInfo {
                source: "custom".to_string(),
                summary: bio,
            });
        }
        if let Some(info) = cached {
            return Ok(info);
        }
        let info = match title {
            Some(title) => {
                let provider = provider.clone();
                tokio::task::spawn_blocking(move || {
                    crate::providers::artist::fetch_artist_info_from_provider(&provider, &title)
                })
                .await
                .map_err(|e| e.to_string())??
            }
            None => None,
        };
        cache_artist_info_result(&db, &cache_dir, artistId, &info).await?;
        return Ok(info.unwrap_or(ArtistInfo {
            source: "none".to_string(),
            summary: None,
        }));
    }

    // A user-provided bio wins when the "custom" provider is enabled, and
    // never hits the network.
    let custom_enabled = settings.artist_info_sources.iter().any(|s| s == "custom");
    if custom_enabled {
        if let Some(bio) = bio {
            return Ok(ArtistInfo {
                source: "custom".to_string(),
                summary: Some(bio),
            });
        }
    }

    if let Some(info) = cached {
        return Ok(info);
    }

    // 2. Network fetch without holding the DB lock.
    let info = match title {
        Some(title) => {
            let settings_for_fetch = settings.clone();
            tokio::task::spawn_blocking(move || {
                crate::providers::artist::fetch_artist_info_online(&title, &settings_for_fetch)
            })
            .await
            .map_err(|e| e.to_string())??
        }
        None => None,
    };

    cache_artist_info_result(&db, &cache_dir, artistId, &info).await?;

    Ok(info.unwrap_or(ArtistInfo {
        source: "none".to_string(),
        summary: None,
    }))
}

/// Caches an artist-info result (including negatives, so artists without a
/// page are not re-fetched on every visit) with a short DB lock.
async fn cache_artist_info_result(
    db: &Arc<Mutex<rusqlite::Connection>>,
    cache_dir: &std::path::Path,
    artist_id: i64,
    info: &Option<ArtistInfo>,
) -> Result<(), String> {
    let to_cache = info.clone().unwrap_or(ArtistInfo {
        source: "none".to_string(),
        summary: None,
    });
    let cache_dir = cache_dir.to_path_buf();
    tokio::task::spawn_blocking({
        let db = db.clone();
        move || -> Result<(), String> {
            let conn = db.lock().map_err(|e| e.to_string())?;
            crate::providers::artist::cache_artist_info(&conn, &cache_dir, artist_id, &to_cache)
        }
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(())
}

fn empty_image() -> ImageData {
    ImageData {
        source: "none".to_string(),
        data: None,
        mime_type: "image/jpeg".to_string(),
    }
}

/// Enforces the cache byte budget before taking the database mutex. Image
/// decoding stays in the webview so automatic artwork cannot monopolize a
/// blocking worker while a manual search is starting.
fn cacheable_image(image: Option<ImageData>) -> ImageData {
    let Some(mut image) = image else {
        return empty_image();
    };
    let Some(data) = image.data.take() else {
        return empty_image();
    };
    match cache::validate_image_for_cache(data) {
        Ok(data) => {
            image.mime_type = detect_image_mime_type(&data);
            image.data = Some(data);
            image
        }
        Err(error) => {
            // Cache a negative result rather than retrying an unsupported or
            // oversized provider response for every visible card.
            log::debug!(target: "sparkle::image_cache", "image_validation_failed error={error}");
            empty_image()
        }
    }
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn get_artist_image(
    state: State<'_, AppState>,
    artistId: i64,
) -> Result<CachedImage, String> {
    let db = state.db.clone();
    let cache_dir = state.cache_dir.clone();
    let settings = load_settings_async(&db).await?;

    let (custom_image, cached, title, image_provider, info_provider) = tokio::task::spawn_blocking({
        let db = db.clone();
        let cache_dir = cache_dir.clone();
        move || -> Result<(Option<CachedImage>, Option<CachedImage>, Option<String>, Option<String>, Option<String>), String> {
            let conn = db.lock().map_err(|e| e.to_string())?;
            let custom = cache::get_custom_image(&conn, &cache_dir, "artist", artistId)?;
            let cached = crate::providers::artist::get_cached_artist_image(&conn, &cache_dir, artistId)?;
            let title = crate::providers::artist::artist_image_query_title(&conn, artistId)?;
            let providers: (Option<String>, Option<String>) = conn
                .query_row(
                    "SELECT NULLIF(image_provider, ''), NULLIF(info_provider, '') FROM artists WHERE id = ?",
                    [artistId],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|e| e.to_string())?
                .unwrap_or((None, None));
            Ok((custom, cached, title, providers.0, providers.1))
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    // Explicit per-artist provider (mix & match).
    if let Some(provider) = image_provider {
        if provider == "custom" {
            return Ok(custom_image.unwrap_or_else(CachedImage::none));
        }
        if let Some(image) = cached {
            return Ok(image);
        }
        let image = match title {
            Some(title) => {
                let settings_for_fetch = settings.clone();
                let provider = provider.clone();
                // Brave relevance: use the artist's Wikipedia locale (from
                // either field) so results match the artist's home market.
                let hint_source = match provider.as_str() {
                    "brave" => info_provider.as_deref(),
                    other => Some(other),
                };
                let lang_hint =
                    crate::providers::artist::brave_lang_hint(hint_source, &settings).to_string();
                tokio::task::spawn_blocking(move || {
                    let fetched = crate::providers::artist::fetch_artist_image_from_provider(
                        &provider,
                        &title,
                        &lang_hint,
                        &settings_for_fetch,
                    );
                    cacheable_image(fetched.unwrap_or_else(|error| {
                        log::debug!(target: "sparkle::artist_image", "provider={provider} fetch_failed error={error}");
                        None
                    }))
                })
                .await
                .map_err(|e| e.to_string())?
            }
            None => empty_image(),
        };
        return cache_artist_image_result(&db, &cache_dir, artistId, image).await;
    }

    // Global flow: a custom image wins when "custom" is enabled in the list.
    let custom_enabled = settings.artist_image_sources.iter().any(|s| s == "custom");
    if custom_enabled {
        if let Some(image) = custom_image {
            return Ok(image);
        }
    }

    if let Some(image) = cached {
        return Ok(image);
    }

    let image = match title {
        Some(title) => {
            let settings_for_fetch = settings.clone();
            tokio::task::spawn_blocking(move || {
                let fetched = crate::providers::artist::fetch_artist_image_online(
                    &title,
                    &settings_for_fetch,
                );
                cacheable_image(fetched.unwrap_or_else(|error| {
                    log::debug!(target: "sparkle::artist_image", "fetch_failed error={error}");
                    None
                }))
            })
            .await
            .map_err(|e| e.to_string())?
        }
        None => empty_image(),
    };

    cache_artist_image_result(&db, &cache_dir, artistId, image).await
}

/// Caches artist-image results, including misses, so a provider miss cannot
/// trigger a new network request every time the artist appears in a grid.
async fn cache_artist_image_result(
    db: &Arc<Mutex<rusqlite::Connection>>,
    cache_dir: &std::path::Path,
    artist_id: i64,
    image: ImageData,
) -> Result<CachedImage, String> {
    let cache_dir = cache_dir.to_path_buf();
    tokio::task::spawn_blocking({
        let db = db.clone();
        move || -> Result<CachedImage, String> {
            let conn = db.lock().map_err(|e| e.to_string())?;
            crate::providers::artist::cache_artist_image(&conn, &cache_dir, artist_id, &image)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Sets the artist's per-field metadata providers (mix & match): each of
/// bio and image can come from the global list, a specific provider
/// ("wikipedia:{lang}", "brave"), or the user's own content ("custom").
/// Search terms override the artist name per field. Clears cached online
/// data so the next view refetches.
#[tauri::command]
#[allow(non_snake_case)]
pub fn set_artist_providers(
    state: State<'_, AppState>,
    artistId: i64,
    infoProvider: Option<String>,
    imageProvider: Option<String>,
    infoTerm: Option<String>,
    imageTerm: Option<String>,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let clean = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    conn.execute(
        "UPDATE artists SET info_provider = ?1, image_provider = ?2, info_term = ?3, image_term = ?4 WHERE id = ?5",
        rusqlite::params![
            clean(infoProvider),
            clean(imageProvider),
            clean(infoTerm),
            clean(imageTerm),
            artistId
        ],
    )
    .map_err(|e| e.to_string())?;
    cache::delete_artist_info(&conn, &state.cache_dir, artistId)?;
    cache::delete_images(&conn, &state.cache_dir, "artist", artistId, true)?;
    Ok(())
}

/// Sets (or clears) a user-provided biography for an artist. A custom bio
/// overrides any online source.
#[tauri::command]
#[allow(non_snake_case)]
pub fn set_artist_bio(
    state: State<'_, AppState>,
    artistId: i64,
    bio: Option<String>,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let cleaned = bio.map(|b| b.trim().to_string()).filter(|b| !b.is_empty());
    conn.execute(
        "UPDATE artists SET bio = ? WHERE id = ?",
        rusqlite::params![cleaned, artistId],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

const MAX_CUSTOM_IMAGE_BYTES: usize = 20 * 1024 * 1024;

fn manual_image_search_sources(settings: &Settings) -> Vec<String> {
    settings
        .artist_image_sources
        .iter()
        .filter(|source| {
            matches!(source.as_str(), "brave" | "duckduckgo" | "shazam")
                || source.starts_with("wikipedia:")
        })
        .cloned()
        .collect()
}

fn manual_lyrics_search_sources(settings: &Settings) -> Vec<String> {
    settings
        .lyrics_sources
        .iter()
        .filter(|source| {
            matches!(
                source.as_str(),
                "embedded" | "lrc" | "lrclib" | "netease" | "kashinavi" | "qq"
            )
        })
        .cloned()
        .collect()
}

fn manual_image_search_title(
    query: Option<String>,
    fallback: impl FnOnce() -> Result<Option<String>, String>,
) -> Result<Option<String>, String> {
    let explicit_query = query
        .map(|query| query.trim().to_string())
        .filter(|query| !query.is_empty());
    match explicit_query {
        Some(query) => Ok(Some(query)),
        None => fallback(),
    }
}

struct ManualImageSearchOutcome<T> {
    candidates: Vec<T>,
    failed_sources: Vec<(String, String)>,
    timed_out_sources: Vec<String>,
}

fn unique_image_candidates(
    candidates: impl IntoIterator<Item = ImageCandidate>,
    limit: usize,
) -> Vec<ImageCandidate> {
    let mut seen_urls = HashSet::new();
    candidates
        .into_iter()
        .filter(|candidate| seen_urls.insert(candidate.url.clone()))
        .take(limit)
        .collect()
}

// The chooser must become actionable even when a third-party search endpoint
// stalls. Individual requests still have their own timeout and finish in the
// background, but their late results are intentionally ignored.
const MANUAL_IMAGE_SEARCH_TIMEOUT: Duration = Duration::from_secs(8);

fn collect_manual_image_search<T, F>(
    sources: Vec<String>,
    timeout: Duration,
    search: F,
) -> ManualImageSearchOutcome<T>
where
    T: Send + 'static,
    F: Fn(String) -> Result<Vec<T>, String> + Send + Sync + 'static,
{
    let source_count = sources.len();
    let (sender, receiver) = mpsc::channel();
    let search = Arc::new(search);
    for (index, source) in sources.iter().cloned().enumerate() {
        let sender = sender.clone();
        let search = Arc::clone(&search);
        std::thread::spawn(move || {
            let result = search(source);
            let _ = sender.send((index, result));
        });
    }
    drop(sender);

    let deadline = Instant::now() + timeout;
    let mut results = (0..source_count).map(|_| None).collect::<Vec<_>>();
    let mut received = 0;
    while received < source_count {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining == Duration::ZERO {
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok((index, result)) => {
                results[index] = Some(result);
                received += 1;
            }
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let mut candidates = Vec::new();
    let mut failed_sources = Vec::new();
    let mut timed_out_sources = Vec::new();
    for (source, result) in sources.into_iter().zip(results) {
        match result {
            Some(Ok(found)) => candidates.extend(found),
            Some(Err(error)) => failed_sources.push((source, error)),
            None => timed_out_sources.push(source),
        }
    }
    ManualImageSearchOutcome {
        candidates,
        failed_sources,
        timed_out_sources,
    }
}

/// Fetches candidate artist images for the chooser as URLs only — no
/// downloads. The webview renders the candidates directly (parallel,
/// progressive), which keeps search fast; bytes are fetched only when the
/// user picks one (see download_artist_image_candidate).
#[tauri::command]
#[allow(non_snake_case)]
pub async fn search_artist_images(
    state: State<'_, AppState>,
    artistId: i64,
    query: Option<String>,
) -> Result<ImageSearchResults, String> {
    let db = state.db.clone();
    let settings = load_settings_async(&db).await?;
    let title = tokio::task::spawn_blocking({
        let db = db.clone();
        move || {
            manual_image_search_title(query, || {
                let conn = db.lock().map_err(|e| e.to_string())?;
                crate::providers::artist::artist_image_query_title(&conn, artistId)
            })
        }
    })
    .await
    .map_err(|e| e.to_string())??;
    let Some(title) = title else {
        return Ok(ImageSearchResults {
            candidates: Vec::new(),
            failed_sources: Vec::new(),
            timed_out_sources: Vec::new(),
        });
    };

    tokio::task::spawn_blocking(move || {
        // Start every enabled source in parallel, but do not let a stalled
        // provider trap the chooser in its loading state.
        let sources = manual_image_search_sources(&settings);
        log::debug!(
            target: "sparkle::manual_image_search",
            "event=search_started provider_count={} providers={}",
            sources.len(),
            sources.join(",")
        );
        if sources.is_empty() {
            log::debug!(
                target: "sparkle::manual_image_search",
                "event=no_enabled_providers"
            );
        }
        let api_key = settings.brave_api_key.clone();
        let lang_hint = crate::providers::artist::brave_lang_hint(None, &settings).to_string();
        let outcome = collect_manual_image_search(
            sources,
            MANUAL_IMAGE_SEARCH_TIMEOUT,
            move |source| -> Result<Vec<ImageCandidate>, String> {
                let urls = match source.as_str() {
                    "brave" => crate::providers::artist::brave::search_image_urls(
                        &title, &api_key, 10, &lang_hint,
                    ),
                    "duckduckgo" => {
                        crate::providers::artist::duckduckgo::search_image_urls(&title, 10)
                    }
                    "shazam" => {
                        crate::providers::artist::shazam::search_image_urls(&title, 10)
                    }
                    source if source.starts_with("wikipedia:") => {
                        let lang = source.trim_start_matches("wikipedia:");
                        crate::providers::artist::wikipedia::image_urls_by_title(&title, lang, 4)
                    }
                    _ => Ok(Vec::new()),
                };
                if let Err(error) = &urls {
                    log::debug!(target: "sparkle::manual_image_search", "event=provider_failed provider={source} error={error}");
                }
                let urls = urls?;
                Ok(urls
                    .into_iter()
                    .map(|url| ImageCandidate {
                        source: source.clone(),
                        url,
                    })
                    .collect())
            },
        );
        if !outcome.timed_out_sources.is_empty() {
            log::debug!(
                target: "sparkle::manual_image_search",
                "event=search_timeout timeout_seconds={} providers={}",
                MANUAL_IMAGE_SEARCH_TIMEOUT.as_secs(),
                outcome.timed_out_sources.join(",")
            );
        }
        let candidates = unique_image_candidates(outcome.candidates, 24);
        let failed_sources = outcome
            .failed_sources
            .into_iter()
            .map(|(source, _)| source)
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            log::debug!(
                target: "sparkle::manual_image_search",
                "event=no_results failed_provider_count={} timed_out_provider_count={}",
                failed_sources.len(),
                outcome.timed_out_sources.len()
            );
        }
        log::debug!(
            target: "sparkle::manual_image_search",
            "event=search_completed candidate_count={} failed_provider_count={} timed_out_provider_count={}",
            candidates.len(),
            failed_sources.len(),
            outcome.timed_out_sources.len()
        );
        ImageSearchResults {
            candidates,
            failed_sources,
            timed_out_sources: outcome.timed_out_sources,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Downloads a single chooser candidate when selected or when its direct
/// webview preview fails. Normal previews stay as remote URLs.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn download_artist_image_candidate(
    url: String,
    source: String,
) -> Result<ImageData, String> {
    let log_url = url.clone();
    let log_source = source.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<ImageData, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent("SparkleMusicPlayer/0.1.0 (local desktop music player)")
            .build()
            .map_err(|e| e.to_string())?;
        let mut request = client.get(&url);
        if source == "duckduckgo" {
            request = request.header("Referer", "https://duckduckgo.com/");
        } else if let Some(lang) = source.strip_prefix("wikipedia:") {
            request = request.header("Referer", format!("https://{lang}.wikipedia.org/"));
        }
        let response = request.send().map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("download failed (HTTP {})", response.status()));
        }
        let data = crate::providers::read_image_response(response)?;
        if data.len() < 512 {
            return Err("download is too small to be an image".to_string());
        }
        if data.len() > MAX_CUSTOM_IMAGE_BYTES {
            return Err("image is too large (max 20 MB)".to_string());
        }
        let mime_type = detect_image_mime_type(&data);
        Ok(ImageData {
            source,
            data: Some(data),
            mime_type,
        })
    })
    .await
    .map_err(|e| e.to_string())?;
    if let Err(error) = &result {
        log::debug!(
            target: "sparkle::manual_image_search",
            "event=candidate_preview_failed source={} url={} error={}",
            log_source,
            log_url,
            error
        );
    }
    result
}

/// Stores externally chosen image bytes as the artist's custom image
/// (e.g. picked from the online candidate chooser).
#[tauri::command]
#[allow(non_snake_case)]
pub fn set_artist_image_data(
    state: State<'_, AppState>,
    artistId: i64,
    data: Vec<u8>,
) -> Result<(), String> {
    if data.len() > MAX_CUSTOM_IMAGE_BYTES {
        return Err("image is too large (max 20 MB)".to_string());
    }
    let data = cache::validate_image_for_cache(data)?;
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    cache::set_image(
        &conn,
        &state.cache_dir,
        "artist",
        artistId,
        "custom",
        None,
        Some(&data),
    )?;
    Ok(())
}

/// Stores a user-picked image file as the artist's custom image. Custom
/// images override any online source and do not expire.
#[tauri::command]
#[allow(non_snake_case)]
pub fn set_artist_image_file(
    state: State<'_, AppState>,
    artistId: i64,
    path: String,
) -> Result<(), String> {
    let data = cache::read_image_file(std::path::Path::new(&path))?;
    let data = cache::validate_image_for_cache(data)?;
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    // Far-future expiry: custom images are permanent until replaced or cleared.
    cache::set_image(
        &conn,
        &state.cache_dir,
        "artist",
        artistId,
        "custom",
        None,
        Some(&data),
    )?;
    Ok(())
}

/// Removes the artist's custom image, falling back to online sources.
#[tauri::command]
#[allow(non_snake_case)]
pub fn clear_artist_custom_image(state: State<'_, AppState>, artistId: i64) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let path: Option<String> = conn
        .query_row(
            "SELECT file_path FROM images WHERE entity_type = 'artist' AND entity_id = ? AND source = 'custom'",
            [artistId],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    conn.execute(
        "DELETE FROM images WHERE entity_type = 'artist' AND entity_id = ? AND source = 'custom'",
        [artistId],
    )
    .map_err(|e| e.to_string())?;
    if let Some(p) = path {
        let _ = std::fs::remove_file(cache::images_dir(&state.cache_dir, "artist").join(p));
    }
    Ok(())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn get_album_art(
    state: State<'_, AppState>,
    albumId: i64,
) -> Result<CachedImage, String> {
    let db = state.db.clone();
    let cache_dir = state.cache_dir.clone();
    let settings = load_settings_async(&db).await?;

    // 1. Custom art + cache + lookup data with a short DB lock.
    let custom_enabled = settings.album_art_sources.iter().any(|s| s == "custom");
    let (cached, lookup) = tokio::task::spawn_blocking({
        let db = db.clone();
        let cache_dir = cache_dir.clone();
        move || -> Result<(Option<CachedImage>, crate::providers::album_art::AlbumArtLookup), String> {
            let conn = db.lock().map_err(|e| e.to_string())?;
            // A user-provided custom cover wins when the "custom" provider is
            // enabled, and never expires.
            if custom_enabled {
                if let Some(custom) = cache::get_custom_image(&conn, &cache_dir, "album", albumId)? {
                    return Ok((Some(custom), crate::providers::album_art::AlbumArtLookup {
                        file_path: None,
                        mbid: None,
                    }));
                }
            }
            let cached = crate::providers::album_art::get_cached_album_art(&conn, &cache_dir, albumId)?;
            let lookup = crate::providers::album_art::album_art_lookup(&conn, albumId)?;
            Ok((cached, lookup))
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    if let Some(image) = cached {
        return Ok(image);
    }

    // 2. Fetch without holding the DB lock. Embedded art reads the audio file
    // from disk here, and cache-image decoding happens here too, which is why
    // neither operation may run under the mutex.
    let image = {
        let settings_for_fetch = settings.clone();
        tokio::task::spawn_blocking(move || {
            let fetched =
                crate::providers::album_art::fetch_album_art_online(&lookup, &settings_for_fetch);
            cacheable_image(fetched.unwrap_or_else(|error| {
                log::debug!(target: "sparkle::album_art", "fetch_failed error={error}");
                None
            }))
        })
        .await
        .map_err(|e| e.to_string())?
    };

    // 3. Cache the result (including negatives, so artless albums are not
    // re-probed on every view) with a short DB lock.
    tokio::task::spawn_blocking({
        let db = db.clone();
        let cache_dir = cache_dir.clone();
        move || -> Result<CachedImage, String> {
            let conn = db.lock().map_err(|e| e.to_string())?;
            crate::providers::album_art::cache_album_art(&conn, &cache_dir, albumId, &image)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Raw bytes are only needed for the native media session. Webview callers
/// use `get_album_art`, which returns a file reference and avoids IPC/base64
/// copies. The caller supplies the source returned by that command, so this
/// cannot be used to read an arbitrary cache file.
#[tauri::command]
#[allow(non_snake_case)]
pub fn get_album_art_data(
    state: State<'_, AppState>,
    albumId: i64,
    source: String,
) -> Result<ImageData, String> {
    let image = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        cache::get_image_from_source(&conn, &state.cache_dir, "album", albumId, &source)?
    };
    match image {
        Some(image) => cache::read_cached_image(&image),
        None => Ok(empty_image()),
    }
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn get_online_settings(state: State<'_, AppState>) -> Result<OnlineSettings, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let settings = settings::load_settings(&conn)?;
    Ok(OnlineSettings {
        scan_on_startup: settings.scan_on_startup,
        lyrics_sources: settings.lyrics_sources,
        artist_info_sources: settings.artist_info_sources,
        artist_image_sources: settings.artist_image_sources,
        album_art_sources: settings.album_art_sources,
        artist_split_regex: settings.artist_split_regex,
        artist_split_exceptions: settings.artist_split_exceptions,
        ui_font: settings.ui_font,
        lyrics_font: settings.lyrics_font,
        reduce_motion: settings.reduce_motion,
        brave_api_key: settings.brave_api_key,
        accent_color: settings.accent_color,
        discord_enabled: settings.discord_enabled,
        discord_app_id: settings.discord_app_id,
        discord_catbox_user_hash: settings.discord_catbox_user_hash,
        discord_artwork_store: settings.discord_artwork_store,
        discord_artwork_s3_endpoint: settings.discord_artwork_s3_endpoint,
        discord_artwork_s3_bucket: settings.discord_artwork_s3_bucket,
        discord_artwork_s3_public_url: settings.discord_artwork_s3_public_url,
        discord_artwork_s3_access_key: settings.discord_artwork_s3_access_key,
        discord_artwork_s3_secret_key: settings.discord_artwork_s3_secret_key,
        discord_artwork_s3_session_token: settings.discord_artwork_s3_session_token,
        discord_artwork_s3_region: settings.discord_artwork_s3_region,
        discord_artwork_s3_prefix: settings.discord_artwork_s3_prefix,
        debug_logging_enabled: settings.debug_logging_enabled,
    })
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn set_online_settings(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    settings: OnlineSettings,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut full = settings::load_settings(&conn)?;
    full.scan_on_startup = settings.scan_on_startup;
    full.lyrics_sources = settings.lyrics_sources;
    full.artist_info_sources = settings.artist_info_sources;
    full.artist_image_sources = settings.artist_image_sources;
    full.album_art_sources = settings.album_art_sources;
    full.artist_split_regex = settings.artist_split_regex;
    full.artist_split_exceptions = settings.artist_split_exceptions;
    full.ui_font = settings.ui_font;
    full.lyrics_font = settings.lyrics_font;
    full.reduce_motion = settings.reduce_motion;
    full.brave_api_key = settings.brave_api_key;
    full.accent_color = settings.accent_color;
    full.discord_enabled = settings.discord_enabled;
    full.discord_app_id = settings.discord_app_id;
    full.discord_catbox_user_hash = settings.discord_catbox_user_hash;
    full.discord_artwork_store = settings.discord_artwork_store;
    full.discord_artwork_s3_endpoint = settings.discord_artwork_s3_endpoint;
    full.discord_artwork_s3_bucket = settings.discord_artwork_s3_bucket;
    full.discord_artwork_s3_public_url = settings.discord_artwork_s3_public_url;
    full.discord_artwork_s3_access_key = settings.discord_artwork_s3_access_key;
    full.discord_artwork_s3_secret_key = settings.discord_artwork_s3_secret_key;
    full.discord_artwork_s3_session_token = settings.discord_artwork_s3_session_token;
    full.discord_artwork_s3_region = settings.discord_artwork_s3_region;
    full.discord_artwork_s3_prefix = settings.discord_artwork_s3_prefix;
    full.debug_logging_enabled = settings.debug_logging_enabled;
    settings::save_settings(&conn, &full)?;
    drop(conn);
    crate::set_debug_logging_enabled(settings.debug_logging_enabled);
    state.discord.refresh();
    // Lyrics views re-read provider biases and source lists live.
    use tauri::Emitter;
    let _ = app.emit("online-settings-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn test_artwork_storage(state: State<'_, AppState>) -> Result<String, String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let settings = settings::load_settings(&conn)?;
        crate::discord::test_artwork_storage(&settings)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn clear_lyrics_cache(state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    cache::clear_lyrics(&conn)
}

#[tauri::command]
pub fn clear_artist_info_cache(state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    cache::clear_artist_info(&conn, &state.cache_dir)
}

#[tauri::command]
pub fn clear_images_cache(state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    cache::clear_images(&conn, &state.cache_dir)
}

#[tauri::command]
pub fn clear_all_caches(state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    cache::clear_lyrics(&conn)?;
    cache::clear_artist_info(&conn, &state.cache_dir)?;
    // Uploaded Discord artwork is user-owned metadata, not disposable cache.
    // It intentionally survives this cleanup to avoid re-uploading.
    cache::clear_images(&conn, &state.cache_dir)
}

#[derive(serde::Serialize)]
pub struct CacheStat {
    pub name: String,
    pub items: i64,
    pub bytes: i64,
}

#[tauri::command]
pub fn get_cache_stats(state: State<'_, AppState>) -> Result<Vec<CacheStat>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    Ok(cache::cache_stats(&conn, &state.cache_dir)
        .into_iter()
        .map(|(name, items, bytes)| CacheStat {
            name: name.to_string(),
            items,
            bytes,
        })
        .collect())
}

/// The directory the on-disk metadata cache lives in, shown in settings.
#[tauri::command]
pub fn get_cache_dir(state: State<'_, AppState>) -> Result<String, String> {
    Ok(PathBuf::from(&state.cache_dir)
        .to_string_lossy()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn manual_artwork_search_uses_all_enabled_online_providers() {
        let mut settings = Settings::default();
        settings.artist_image_sources = vec![
            "custom".to_string(),
            "wikipedia:ja".to_string(),
            "shazam".to_string(),
            "brave".to_string(),
            "duckduckgo".to_string(),
        ];

        assert_eq!(
            manual_image_search_sources(&settings),
            vec![
                "wikipedia:ja".to_string(),
                "shazam".to_string(),
                "brave".to_string(),
                "duckduckgo".to_string(),
            ]
        );
    }

    #[test]
    fn manual_lyrics_search_uses_all_enabled_online_providers() {
        let mut settings = Settings::default();
        settings.lyrics_sources = vec![
            "embedded".to_string(),
            "lrc".to_string(),
            "lrclib".to_string(),
            "netease".to_string(),
            "kashinavi".to_string(),
            "qq".to_string(),
        ];

        assert_eq!(
            manual_lyrics_search_sources(&settings),
            vec![
                "embedded".to_string(),
                "lrc".to_string(),
                "lrclib".to_string(),
                "netease".to_string(),
                "kashinavi".to_string(),
                "qq".to_string(),
            ]
        );
    }

    #[test]
    fn manual_lyrics_search_keeps_enabled_provider_failures_visible() {
        let outcome = collect_manual_image_search(
            vec![
                "embedded".to_string(),
                "lrclib".to_string(),
                "qq".to_string(),
            ],
            Duration::from_millis(200),
            |source| {
                if source == "lrclib" {
                    Err("provider unavailable".to_string())
                } else {
                    Ok(vec![source])
                }
            },
        );
        assert_eq!(
            outcome.candidates,
            vec!["embedded".to_string(), "qq".to_string()]
        );
        assert_eq!(outcome.failed_sources[0].0, "lrclib");
        assert!(outcome.timed_out_sources.is_empty());
    }

    #[test]
    fn manual_artwork_query_skips_the_database_fallback() {
        let fallback_called = AtomicBool::new(false);
        let title = manual_image_search_title(Some("Björk".to_string()), || {
            fallback_called.store(true, Ordering::SeqCst);
            Ok(Some("fallback".to_string()))
        })
        .unwrap();

        assert_eq!(title.as_deref(), Some("Björk"));
        assert!(!fallback_called.load(Ordering::SeqCst));
    }

    #[test]
    fn manual_image_search_returns_partial_results_within_its_budget() {
        let started = std::time::Instant::now();
        let outcome = collect_manual_image_search(
            vec!["fast".to_string(), "slow".to_string()],
            Duration::from_millis(100),
            |source| {
                if source == "slow" {
                    std::thread::sleep(Duration::from_millis(500));
                    Ok(Vec::new())
                } else {
                    Ok(vec![source])
                }
            },
        );

        assert_eq!(outcome.candidates, vec!["fast"]);
        assert_eq!(outcome.timed_out_sources, vec!["slow"]);
        assert!(
            started.elapsed() < Duration::from_millis(300),
            "manual search waited for the slow provider"
        );
    }

    #[test]
    fn manual_image_search_keeps_configured_provider_order() {
        let outcome = collect_manual_image_search(
            vec!["first".to_string(), "second".to_string()],
            Duration::from_millis(200),
            |source| {
                if source == "first" {
                    // Complete second first to prove collection order is not
                    // accidentally determined by network timing.
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(vec![source])
            },
        );

        assert_eq!(outcome.candidates, vec!["first", "second"]);
    }

    #[test]
    fn manual_image_search_deduplicates_urls_preserving_first_provider() {
        let candidates = unique_image_candidates(
            vec![
                ImageCandidate {
                    source: "shazam".to_string(),
                    url: "https://images.example/shared.jpg".to_string(),
                },
                ImageCandidate {
                    source: "wikipedia:ja".to_string(),
                    url: "https://images.example/shared.jpg".to_string(),
                },
                ImageCandidate {
                    source: "duckduckgo".to_string(),
                    url: "https://images.example/other.jpg".to_string(),
                },
            ],
            24,
        );

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].source, "shazam");
        assert_eq!(candidates[1].source, "duckduckgo");
    }

    #[test]
    fn manual_lyrics_choice_is_persisted_as_custom_content() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tracks (id INTEGER PRIMARY KEY, lyrics_source TEXT); \
             CREATE TABLE lyrics (track_id INTEGER NOT NULL, source TEXT NOT NULL, synced_text TEXT, plain_text TEXT, fetched_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, PRIMARY KEY (track_id, source));",
        )
        .unwrap();
        conn.execute("INSERT INTO tracks (id) VALUES (1)", [])
            .unwrap();

        store_manual_lyrics_choice(&conn, 1, "netease", Some("[00:00.00]Manual lyric"), None)
            .unwrap();

        let lyrics = cache::get_lyrics_from_source(&conn, 1, "custom")
            .unwrap()
            .unwrap();
        let source: Option<String> = conn
            .query_row("SELECT lyrics_source FROM tracks WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(lyrics.source, "custom");
        assert_eq!(source.as_deref(), Some("custom"));
    }

    #[test]
    fn stale_automatic_result_cannot_restore_replaced_lyrics() {
        let automatic = None;
        let netease = Some("netease".to_string());

        assert!(!can_cache_lyrics_result(&automatic, &netease));
        assert!(!can_cache_lyrics_result(
            &automatic,
            &Some("custom".to_string()),
        ));
        assert!(can_cache_lyrics_result(&netease, &netease));
    }
}
