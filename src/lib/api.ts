import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";

export interface AppStatus {
    db_path: string;
    log_path: string;
    schema_version: number;
}

export async function enableMediaControlEvents(): Promise<void> {
    return invoke("enable_media_control_events");
}

export interface Folder {
    id: number;
    path: string;
    enabled: boolean;
    scanned_at: number | null;
}

export interface Artist {
    id: number;
    name: string;
    sort_name?: string;
    track_count?: number;
    album_count?: number;
    bio?: string;
    info_provider?: string;
    image_provider?: string;
    info_term?: string;
    image_term?: string;
}

export interface Album {
    id: number;
    title: string;
    year?: number;
    artist_ids?: number[];
    artist_names?: string[];
    track_count?: number;
}

export interface Track {
    id: number;
    file_path: string;
    title: string | null;
    track_number: number | null;
    disc_number: number | null;
    duration_ms: number | null;
    year: number | null;
    genre: string | null;
    album_id: number | null;
    embedded_lyrics: string | null;
    artist_ids: number[];
    artist_names: string[];
    album_title: string | null;
    lrc_offset_ms: number;
    lyrics_source?: string | null;
}

export type RepeatMode = "off" | "all" | "one";

export interface PlaybackState {
    is_playing: boolean;
    current_track: Track | null;
    position_ms: number;
    duration_ms: number;
    volume: number;
    shuffle: boolean;
    repeat_mode: RepeatMode;
}

export interface ScanResult {
    scanned: number;
    added: number;
    updated: number;
    removed: number;
    errors: number;
}

export interface ScanProgress {
    phase: "scanning" | "cleaning";
    current_path?: string;
    scanned: number;
    total: number;
    added: number;
    updated: number;
    removed: number;
    errors: number;
}

export interface Lyrics {
    source: string;
    synced_text?: string;
    plain_text?: string;
}

export interface Genre {
    name: string;
    track_count: number;
}

export interface ArtistInfo {
    source: string;
    summary?: string;
}

export interface ImageData {
    source: string;
    data?: number[];
    mime_type: string;
}

/** A display image served from Sparkle's managed on-disk cache. */
export interface CachedImage {
    source: string;
    file_path?: string;
    mime_type: string;
}

export interface ImageCandidate {
    source: string;
    url: string;
}

export interface ImageSearchResults {
    candidates: ImageCandidate[];
    failed_sources: string[];
    timed_out_sources: string[];
}

export interface OnlineSettings {
    scan_on_startup: boolean;
    lyrics_sources: string[];
    artist_info_sources: string[];
    artist_image_sources: string[];
    album_art_sources: string[];
    artist_split_regex: string;
    artist_split_exceptions: string[];
    ui_font: string;
    lyrics_font: string;
    reduce_motion: boolean;
    brave_api_key: string;
    accent_color: string;
    discord_enabled: boolean;
    discord_app_id: string;
    discord_catbox_user_hash: string;
    debug_logging_enabled: boolean;
}

export interface CacheStat {
    name: string;
    items: number;
    bytes: number;
}

export async function getCacheStats(): Promise<CacheStat[]> {
    return invoke("get_cache_stats");
}

export async function setArtistProviders(
    artistId: number,
    options: {
        infoProvider?: string | null;
        imageProvider?: string | null;
        infoTerm?: string | null;
        imageTerm?: string | null;
    },
): Promise<void> {
    return invoke("set_artist_providers", {
        artistId,
        infoProvider: options.infoProvider ?? null,
        imageProvider: options.imageProvider ?? null,
        infoTerm: options.infoTerm ?? null,
        imageTerm: options.imageTerm ?? null,
    });
}

export async function setArtistBio(
    artistId: number,
    bio?: string,
): Promise<void> {
    return invoke("set_artist_bio", { artistId, bio: bio ?? null });
}

export async function setArtistImageFile(
    artistId: number,
    path: string,
): Promise<void> {
    return invoke("set_artist_image_file", { artistId, path });
}

export async function clearArtistCustomImage(artistId: number): Promise<void> {
    return invoke("clear_artist_custom_image", { artistId });
}

export async function pickImageFile(): Promise<string | null> {
    const result = await open({
        multiple: false,
        directory: false,
        filters: [
            {
                name: "Images",
                extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp"],
            },
        ],
    });
    return typeof result === "string" ? result : null;
}

export async function getStatus(): Promise<AppStatus> {
    return invoke("get_status");
}

export interface BackupSections {
    settings: boolean;
    playlists: boolean;
    custom_metadata: boolean;
    history: boolean;
}

export interface BackupManifest {
    created_at: number;
    app_version: string;
    file_version: number;
    file_size_bytes: number;
    settings: boolean;
    tracks: number;
    playlists: number;
    playlist_tracks: number;
    lyrics: number;
    artist_bios: number;
    artwork: number;
    history: number;
}

export interface BackupImportSummary {
    settings: boolean;
    playlists: number;
    playlist_tracks: number;
    lyrics: number;
    artist_bios: number;
    artwork: number;
    history: number;
    unmatched_tracks: number;
    unmatched_artwork: number;
}

export async function exportLibraryBackup(
    path: string,
    sections: BackupSections,
): Promise<BackupManifest> {
    return invoke("export_library_backup", { path, sections });
}

export async function inspectLibraryBackup(
    path: string,
): Promise<BackupManifest> {
    return invoke("inspect_library_backup", { path });
}

export async function importLibraryBackup(
    path: string,
    sections: BackupSections,
): Promise<BackupImportSummary> {
    return invoke("import_library_backup", { path, sections });
}

export async function pickBackupToSave(): Promise<string | null> {
    const result = await save({
        title: "Back up Sparkle library",
        defaultPath: "sparkle-library.sparklebackup",
        filters: [{ name: "Sparkle backup", extensions: ["sparklebackup"] }],
    });
    return typeof result === "string" ? result : null;
}

export async function pickBackupToImport(): Promise<string | null> {
    const result = await open({
        title: "Restore Sparkle library",
        multiple: false,
        directory: false,
        filters: [{ name: "Sparkle backup", extensions: ["sparklebackup"] }],
    });
    return typeof result === "string" ? result : null;
}

export async function listFolders(): Promise<Folder[]> {
    return invoke("list_folders");
}

export async function pickFolder(): Promise<string | null> {
    return invoke("pick_folder");
}

export async function addFolder(path: string): Promise<Folder> {
    return invoke("add_folder", { path });
}

export async function removeFolder(id: number): Promise<void> {
    return invoke("remove_folder", { id });
}

export async function setFolderEnabled(
    id: number,
    enabled: boolean,
): Promise<void> {
    return invoke("set_folder_enabled", { id, enabled });
}

export async function revealInExplorer(path: string): Promise<void> {
    return invoke("reveal_in_explorer", { path });
}

export async function searchArtistImages(
    artistId: number,
    query?: string,
): Promise<ImageSearchResults> {
    return invoke("search_artist_images", { artistId, query: query ?? null });
}

export async function downloadArtistImageCandidate(
    url: string,
    source: string,
): Promise<ImageData> {
    return invoke("download_artist_image_candidate", { url, source });
}

export async function setArtistImageData(
    artistId: number,
    data: number[],
): Promise<void> {
    return invoke("set_artist_image_data", { artistId, data });
}

export async function scanLibrary(force = false): Promise<ScanResult> {
    return invoke("scan_library", { force });
}

export async function getArtists(): Promise<Artist[]> {
    return invoke("get_artists");
}

export async function getAlbums(artistId?: number): Promise<Album[]> {
    return invoke("get_albums", { artistId });
}

export async function getAlbum(id: number): Promise<Album> {
    return invoke("get_album", { id });
}

export async function getTracks(albumId?: number): Promise<Track[]> {
    return invoke("get_tracks", { albumId });
}

export async function getTracksByArtist(artistId: number): Promise<Track[]> {
    return invoke("get_tracks_by_artist", { artistId });
}

export async function getArtist(id: number): Promise<Artist> {
    return invoke("get_artist", { id });
}

export async function getRelatedArtists(artistId: number): Promise<Artist[]> {
    return invoke("get_related_artists", { artistId });
}

export async function getGenres(): Promise<Genre[]> {
    return invoke("get_genres");
}

export async function loadQueue(
    trackIds: number[],
    startIndex = 0,
    shuffle?: boolean,
): Promise<PlaybackState> {
    return invoke("load_queue", {
        trackIds,
        startIndex,
        shuffle: shuffle ?? null,
    });
}

export async function playTrack(trackId: number): Promise<PlaybackState> {
    return invoke("play_track", { trackId });
}

export async function play(): Promise<PlaybackState> {
    return invoke("play");
}

export async function pause(): Promise<PlaybackState> {
    return invoke("pause");
}

export async function stop(): Promise<PlaybackState> {
    return invoke("stop");
}

export async function seek(positionMs: number): Promise<PlaybackState> {
    return invoke("seek", { positionMs });
}

export async function nextTrack(): Promise<PlaybackState> {
    return invoke("next_track");
}

export async function previousTrack(): Promise<PlaybackState> {
    return invoke("previous_track");
}

export async function setVolume(volume: number): Promise<PlaybackState> {
    return invoke("set_volume", { volume });
}

export async function setVolumeLive(volume: number): Promise<void> {
    return invoke("set_volume", { volume });
}

export async function setShuffle(shuffle: boolean): Promise<PlaybackState> {
    return invoke("set_shuffle", { shuffle });
}

export async function cycleRepeatMode(): Promise<PlaybackState> {
    return invoke("cycle_repeat_mode");
}

export async function playNext(trackId: number): Promise<PlaybackState> {
    return invoke("play_next", { trackId });
}

export interface QueueView {
    tracks: Track[];
    current_pos: number | null;
}

export async function getQueue(): Promise<QueueView> {
    return invoke("get_queue");
}

export async function playQueueIndex(orderPos: number): Promise<PlaybackState> {
    return invoke("play_queue_index", { orderPos });
}

export async function getPlaybackState(): Promise<PlaybackState> {
    return invoke("get_playback_state");
}

export async function getLrcOffset(trackId: number): Promise<number> {
    return invoke("get_lrc_offset", { trackId });
}

export async function setLrcOffset(
    trackId: number,
    offsetMs: number,
): Promise<void> {
    return invoke("set_lrc_offset", { trackId, offsetMs });
}

const lyricRequests = new Map<number, Promise<Lyrics>>();

export const LYRICS_CHANGED_EVENT = "sparkle:lyrics-changed";

function invalidateLyricsRequest(trackId: number): void {
    lyricRequests.delete(trackId);
}

function notifyLyricsChanged(trackId: number): void {
    invalidateLyricsRequest(trackId);
    if (typeof window !== "undefined") {
        window.dispatchEvent(
            new CustomEvent<{ trackId: number }>(LYRICS_CHANGED_EVENT, {
                detail: { trackId },
            }),
        );
    }
}

export async function getLyrics(trackId: number): Promise<Lyrics> {
    const existing = lyricRequests.get(trackId);
    if (existing) return existing;

    const request = invoke<Lyrics>("get_lyrics", { trackId });
    lyricRequests.set(trackId, request);
    try {
        return await request;
    } finally {
        if (lyricRequests.get(trackId) === request) {
            lyricRequests.delete(trackId);
        }
    }
}

export async function getArtistInfo(artistId: number): Promise<ArtistInfo> {
    return invoke("get_artist_info", { artistId });
}

export type ImageRequestPriority = "foreground" | "background";

export async function getArtistImage(
    artistId: number,
    priority: ImageRequestPriority = "foreground",
): Promise<CachedImage> {
    return memoizedImage(
        `artist-image:${artistId}`,
        () => invoke("get_artist_image", { artistId }),
        priority,
    );
}

export async function getAlbumArt(
    albumId: number,
    priority: ImageRequestPriority = "foreground",
): Promise<CachedImage> {
    return memoizedImage(
        `album-art:${albumId}`,
        () => invoke("get_album_art", { albumId }),
        priority,
    );
}

// Native media controls need raw base64 artwork, unlike the webview. First
// resolving the shared cached reference collapses this with the UI request;
// the second command then reads only that one compact cache file.
export async function getAlbumArtData(albumId: number): Promise<ImageData> {
    const image = await getAlbumArt(albumId);
    if (!image.file_path) {
        return {
            source: image.source,
            mime_type: image.mime_type,
        };
    }
    return invoke("get_album_art_data", { albumId, source: image.source });
}

// Pages render many cards and often share the same album/artist. Keep
// completed file references in a bounded LRU cache while separately
// collapsing concurrent fetches for the same image. References are tiny;
// image bytes stay on disk and are decoded by the webview only when needed.
const MAX_IMAGE_CACHE_ENTRIES = 128;
const MAX_CONCURRENT_IMAGE_REQUESTS = 3;
// Keep a lane free for the player and detail views. A grid can have many
// background requests, but it must never make foreground artwork wait for all
// active network/disk slots to drain.
const MAX_BACKGROUND_IMAGE_REQUESTS = 2;
const imageMemo = new Map<string, CachedImage>();
const inFlightImageMemo = new Map<string, Promise<CachedImage>>();

interface QueuedImageRequest {
    key: string;
    priority: ImageRequestPriority;
    run: () => void;
}

const imageRequestQueue: QueuedImageRequest[] = [];
let activeImageRequests = 0;

function startNextImageRequest(): void {
    while (activeImageRequests < MAX_CONCURRENT_IMAGE_REQUESTS) {
        const nextIndex = imageRequestQueue.findIndex(
            (request) =>
                request.priority === "foreground" ||
                activeImageRequests < MAX_BACKGROUND_IMAGE_REQUESTS,
        );
        if (nextIndex < 0) return;
        const [request] = imageRequestQueue.splice(nextIndex, 1);
        request.run();
    }
}

function queueImageRequest(
    key: string,
    fetcher: () => Promise<CachedImage>,
    priority: ImageRequestPriority,
): Promise<CachedImage> {
    return new Promise((resolve, reject) => {
        const request: QueuedImageRequest = {
            key,
            priority,
            run: () => {
                activeImageRequests += 1;
                void Promise.resolve()
                    .then(fetcher)
                    .then(resolve, reject)
                    .finally(() => {
                        activeImageRequests -= 1;
                        startNextImageRequest();
                    });
            },
        };
        if (priority === "foreground") {
            imageRequestQueue.unshift(request);
        } else {
            imageRequestQueue.push(request);
        }
        startNextImageRequest();
    });
}

function promoteQueuedImageRequest(key: string): void {
    const index = imageRequestQueue.findIndex((request) => request.key === key);
    if (index < 0) return;
    const [request] = imageRequestQueue.splice(index, 1);
    request.priority = "foreground";
    imageRequestQueue.unshift(request);
    startNextImageRequest();
}

function memoizedImage(
    key: string,
    fetcher: () => Promise<CachedImage>,
    priority: ImageRequestPriority,
): Promise<CachedImage> {
    const cached = imageMemo.get(key);
    if (cached) {
        // Refresh recency on read so frequently visible artwork stays warm.
        imageMemo.delete(key);
        imageMemo.set(key, cached);
        return Promise.resolve(cached);
    }

    const inFlight = inFlightImageMemo.get(key);
    if (inFlight) {
        if (priority === "foreground") promoteQueuedImageRequest(key);
        return inFlight;
    }

    let request: Promise<CachedImage>;
    request = queueImageRequest(key, fetcher, priority).then(
        (image) => {
            // An invalidated request may still finish. Its caller can use the
            // value, but it must not repopulate the cache with stale artwork.
            if (inFlightImageMemo.get(key) !== request) return image;
            inFlightImageMemo.delete(key);
            imageMemo.set(key, image);
            trimImageMemo();
            return image;
        },
        (error) => {
            if (inFlightImageMemo.get(key) === request) {
                inFlightImageMemo.delete(key);
            }
            throw error;
        },
    );
    inFlightImageMemo.set(key, request);
    return request;
}

function trimImageMemo(): void {
    while (imageMemo.size > MAX_IMAGE_CACHE_ENTRIES) {
        const oldestKey = imageMemo.keys().next().value;
        if (oldestKey === undefined) return;
        imageMemo.delete(oldestKey);
    }
}

function invalidateImage(key: string): void {
    imageMemo.delete(key);
    inFlightImageMemo.delete(key);
}

function invalidateAllImages(): void {
    imageMemo.clear();
    inFlightImageMemo.clear();
}

export function invalidateArtistImage(artistId: number): void {
    invalidateImage(`artist-image:${artistId}`);
}

export function invalidateAlbumArt(albumId: number): void {
    invalidateImage(`album-art:${albumId}`);
}

export async function setTrackLyricsSource(
    trackId: number,
    source?: string,
): Promise<void> {
    await invoke("set_track_lyrics_source", {
        trackId,
        source: source ?? null,
    });
    notifyLyricsChanged(trackId);
}

export async function setTrackCustomLyrics(
    trackId: number,
    path: string,
): Promise<void> {
    await invoke("set_track_custom_lyrics", { trackId, path });
    notifyLyricsChanged(trackId);
}

export async function clearTrackCustomLyrics(trackId: number): Promise<void> {
    await invoke("clear_track_custom_lyrics", { trackId });
    notifyLyricsChanged(trackId);
}

export interface LyricCandidate {
    source: string;
    synced_text?: string;
    plain_text?: string;
    preview: string;
}

export interface LyricSearchResults {
    candidates: LyricCandidate[];
    enabled_sources: string[];
    failed_sources: string[];
    timed_out_sources: string[];
}

export async function searchLyricsOnline(
    trackId: number,
    query?: string,
): Promise<LyricSearchResults> {
    return invoke("search_lyrics_online", { trackId, query: query ?? null });
}

export async function setTrackLyricsChoice(
    trackId: number,
    choice: { source: string; syncedText?: string; plainText?: string },
): Promise<void> {
    await invoke("set_track_lyrics_choice", {
        trackId,
        source: choice.source,
        syncedText: choice.syncedText ?? null,
        plainText: choice.plainText ?? null,
    });
    notifyLyricsChanged(trackId);
}

export async function pickLyricsFile(): Promise<string | null> {
    const result = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "Lyrics", extensions: ["lrc", "txt"] }],
    });
    return typeof result === "string" ? result : null;
}

export async function setAlbumArtFile(
    albumId: number,
    path: string,
): Promise<void> {
    return invoke("set_album_art_file", { albumId, path });
}

export async function clearAlbumCustomArt(albumId: number): Promise<void> {
    return invoke("clear_album_custom_art", { albumId });
}

export async function getCacheDir(): Promise<string> {
    return invoke("get_cache_dir");
}

export interface SearchResults {
    artists: Artist[];
    albums: Album[];
    tracks: Track[];
    lyric_tracks: LyricMatch[];
}

export interface LyricMatch {
    track: Track;
    snippet: string;
}

export async function search(query: string): Promise<SearchResults> {
    return invoke("search", { query });
}

export interface PlayStatTrack {
    track_id: number;
    title?: string;
    artist_names: string[];
    album_id?: number;
    plays: number;
    ms: number;
}

export interface PlayStatArtist {
    artist_id: number;
    name: string;
    plays: number;
    ms: number;
}

export interface PlayStatAlbum {
    album_id: number;
    title: string;
    artist_names: string[];
    plays: number;
    ms: number;
}

export interface PlayStatBucket {
    label: string;
    plays: number;
    ms: number;
}

export interface ListeningStats {
    total_plays: number;
    total_ms: number;
    active_days: number;
    unique_tracks: number;
    unique_artists: number;
    completed_plays: number;
    discovery_tracks: number;
    longest_streak_days: number;
    session_count: number;
    peak_hour: number | null;
    peak_hour_ms: number;
    morning_ms: number;
    afternoon_ms: number;
    evening_ms: number;
    late_night_ms: number;
    weekend_ms: number;
    top_genre: string | null;
    top_genre_ms: number;
    average_year: number | null;
    top_tracks: PlayStatTrack[];
    top_artists: PlayStatArtist[];
    top_albums: PlayStatAlbum[];
    activity: PlayStatBucket[];
    activity_by_month: boolean;
}

export interface DiscoveryTracks {
    recently_added: Track[];
    most_played: Track[];
    never_played: Track[];
}

export interface LibraryHealth {
    track_count: number;
    album_count: number;
    artist_count: number;
    missing_titles: number;
    missing_artists: number;
    missing_albums: number;
    missing_genres: number;
    missing_lyrics: number;
    missing_years: number;
    missing_track_numbers: number;
    duplicate_titles: number;
    never_played: number;
    lossless_tracks: number;
    lossy_tracks: number;
    unclassified_tracks: number;
    high_resolution_tracks: number;
    low_bitrate_tracks: number;
    missing_audio_properties: number;
    missing_durations: number;
    very_short_tracks: number;
    very_long_tracks: number;
    mono_tracks: number;
    total_size_bytes: number;
    formats: { format: string; tracks: number }[];
}

export async function getDiscoveryTracks(): Promise<DiscoveryTracks> {
    return invoke("get_discovery_tracks");
}

export async function getLibraryHealth(): Promise<LibraryHealth> {
    return invoke("get_library_health");
}

export async function getHealthTracks(kind: string): Promise<Track[]> {
    return invoke("get_health_tracks", { kind });
}

export async function getListeningStats(
    days?: number,
): Promise<ListeningStats> {
    return invoke("get_listening_stats", { days: days ?? null });
}

export async function getOnlineSettings(): Promise<OnlineSettings> {
    return invoke("get_online_settings");
}

export async function setOnlineSettings(
    settings: OnlineSettings,
): Promise<void> {
    return invoke("set_online_settings", { settings });
}

export async function clearLyricsCache(): Promise<void> {
    return invoke("clear_lyrics_cache");
}

export async function clearArtistInfoCache(): Promise<void> {
    return invoke("clear_artist_info_cache");
}

export async function clearImagesCache(): Promise<void> {
    await invoke("clear_images_cache");
    invalidateAllImages();
}

export async function clearAllCaches(): Promise<void> {
    await invoke("clear_all_caches");
    invalidateAllImages();
}

export async function getTracksByGenre(genre: string): Promise<Track[]> {
    return invoke("get_tracks_by_genre", { genre });
}

export async function getGenreCollageAlbumIds(
    genre: string,
): Promise<number[]> {
    return invoke("get_genre_collage_album_ids", { genre });
}

export interface Playlist {
    id: number;
    name: string;
    description?: string;
    folder_path?: string;
    live_mix?: string;
    track_count: number;
}

export interface PlaylistDetail {
    id: number;
    name: string;
    description?: string;
    folder_path?: string;
    live_mix?: string;
    tracks: Track[];
}

export async function getPlaylists(): Promise<Playlist[]> {
    return invoke("get_playlists");
}

export async function refreshLiveMixes(): Promise<void> {
    return invoke("refresh_live_mix_playlists");
}

export async function getPlaylist(id: number): Promise<PlaylistDetail> {
    return invoke("get_playlist", { id });
}

export async function getPlaylistCollageAlbumIds(
    id: number,
): Promise<number[]> {
    return invoke("get_playlist_collage_album_ids", { id });
}

export async function createPlaylist(
    name: string,
    description?: string,
    folderPath?: string,
): Promise<Playlist> {
    return invoke("create_playlist", { name, description, folderPath });
}

export async function updatePlaylist(
    id: number,
    name: string,
    description?: string,
): Promise<void> {
    return invoke("update_playlist", { id, name, description });
}

export async function deletePlaylist(id: number): Promise<void> {
    return invoke("delete_playlist", { id });
}

export async function addTracksToPlaylist(
    playlistId: number,
    trackIds: number[],
): Promise<void> {
    return invoke("add_tracks_to_playlist", { playlistId, trackIds });
}

export async function removeTrackFromPlaylist(
    playlistId: number,
    trackId: number,
): Promise<void> {
    return invoke("remove_track_from_playlist", { playlistId, trackId });
}
