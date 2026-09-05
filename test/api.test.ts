// @ts-nocheck
import { afterEach, expect, test } from "bun:test";
import * as api from "../src/lib/api";
import { invoke, open, save } from "./support/platform";

afterEach(() => {
    invoke.mockReset();
    open.mockReset();
    save.mockReset();
});
const flush = async () => {
    for (let i = 0; i < 20; i++) await Promise.resolve();
};
function deferred() {
    let resolve, reject;
    const promise = new Promise((yes, no) => {
        resolve = yes;
        reject = no;
    });
    return { promise, resolve, reject };
}
const image = (id) => ({
    source: "embedded",
    file_path: `/cache/${id}.jpg`,
    mime_type: "image/jpeg",
});

test("read-only bridge commands preserve native results", async () => {
    const result = { native: true };
    invoke.mockResolvedValue(result);
    for (const [method, command] of [
        ["getStatus", "get_status"],
        ["getCacheStats", "get_cache_stats"],
        ["listFolders", "list_folders"],
        ["pickFolder", "pick_folder"],
        ["getArtists", "get_artists"],
        ["getGenres", "get_genres"],
        ["getQueue", "get_queue"],
        ["getPlaybackState", "get_playback_state"],
        ["getCacheDir", "get_cache_dir"],
        ["getDiscoveryTracks", "get_discovery_tracks"],
        ["getLibraryHealth", "get_library_health"],
        ["getOnlineSettings", "get_online_settings"],
        ["getLoudnessStatus", "get_loudness_status"],
        ["getPlaylists", "get_playlists"],
        ["testArtworkStorage", "test_artwork_storage"],
    ]) {
        expect(await api[method]()).toBe(result);
        expect(invoke).toHaveBeenLastCalledWith(command);
    }
    for (const [method, command] of [
        ["scanLoudness", "scan_loudness"],
        ["rescanLoudness", "rescan_loudness"],
        ["clearLyricsCache", "clear_lyrics_cache"],
        ["clearArtistInfoCache", "clear_artist_info_cache"],
        ["refreshLiveMixes", "refresh_live_mix_playlists"],
        ["enableMediaControlEvents", "enable_media_control_events"],
    ]) {
        await api[method]();
        expect(invoke).toHaveBeenLastCalledWith(command);
    }
});

test("bridge contracts preserve IDs, paths, explicit false and optional values", async () => {
    invoke.mockResolvedValue({ ok: true });
    const sections = {
        settings: true,
        playlists: false,
        custom_metadata: true,
        history: false,
    };
    const settings = { theme_mode: "system", reduce_motion: false };
    for (const [method, args, command, payload] of [
        ["getAlbums", [], "get_albums", { artistId: undefined }],
        ["getAlbums", [7], "get_albums", { artistId: 7 }],
        ["getTracks", [], "get_tracks", { albumId: undefined }],
        ["getTracks", [7], "get_tracks", { albumId: 7 }],
        ["getAlbum", [7], "get_album", { id: 7 }],
        ["getArtist", [7], "get_artist", { id: 7 }],
        ["getTracksByArtist", [7], "get_tracks_by_artist", { artistId: 7 }],
        ["getRelatedArtists", [7], "get_related_artists", { artistId: 7 }],
        ["getArtistInfo", [7], "get_artist_info", { artistId: 7 }],
        ["getLrcOffset", [7], "get_lrc_offset", { trackId: 7 }],
        [
            "setLrcOffset",
            [7, -50],
            "set_lrc_offset",
            { trackId: 7, offsetMs: -50 },
        ],
        ["scanLibrary", [], "scan_library", { force: false }],
        ["scanLibrary", [true], "scan_library", { force: true }],
        ["addFolder", ["C:/Music"], "add_folder", { path: "C:/Music" }],
        ["removeFolder", [7], "remove_folder", { id: 7 }],
        [
            "setFolderEnabled",
            [7, false],
            "set_folder_enabled",
            { id: 7, enabled: false },
        ],
        [
            "revealInExplorer",
            ["C:/Music"],
            "reveal_in_explorer",
            { path: "C:/Music" },
        ],
        ["setArtistBio", [7], "set_artist_bio", { artistId: 7, bio: null }],
        ["setArtistBio", [7, ""], "set_artist_bio", { artistId: 7, bio: "" }],
        [
            "setArtistProviders",
            [7, {}],
            "set_artist_providers",
            {
                artistId: 7,
                infoProvider: null,
                imageProvider: null,
                infoTerm: null,
                imageTerm: null,
            },
        ],
        [
            "setArtistProviders",
            [
                7,
                {
                    infoProvider: "wiki",
                    imageProvider: "custom",
                    infoTerm: "Alice",
                    imageTerm: "Bob",
                },
            ],
            "set_artist_providers",
            {
                artistId: 7,
                infoProvider: "wiki",
                imageProvider: "custom",
                infoTerm: "Alice",
                imageTerm: "Bob",
            },
        ],
        [
            "setArtistImageFile",
            [7, "art.png"],
            "set_artist_image_file",
            { artistId: 7, path: "art.png" },
        ],
        [
            "clearArtistCustomImage",
            [7],
            "clear_artist_custom_image",
            { artistId: 7 },
        ],
        [
            "setArtistImageData",
            [7, [1, 2]],
            "set_artist_image_data",
            { artistId: 7, data: [1, 2] },
        ],
        [
            "searchArtistImages",
            [7],
            "search_artist_images",
            { artistId: 7, query: null },
        ],
        [
            "searchArtistImages",
            [7, "Alice"],
            "search_artist_images",
            { artistId: 7, query: "Alice" },
        ],
        [
            "downloadArtistImageCandidate",
            ["https://example.test/a", "custom"],
            "download_artist_image_candidate",
            { url: "https://example.test/a", source: "custom" },
        ],
        [
            "setAlbumArtFile",
            [7, "art.png"],
            "set_album_art_file",
            { albumId: 7, path: "art.png" },
        ],
        ["clearAlbumCustomArt", [7], "clear_album_custom_art", { albumId: 7 }],
        [
            "exportLibraryBackup",
            ["backup", sections],
            "export_library_backup",
            { path: "backup", sections },
        ],
        [
            "inspectLibraryBackup",
            ["backup"],
            "inspect_library_backup",
            { path: "backup" },
        ],
        [
            "importLibraryBackup",
            ["backup", sections],
            "import_library_backup",
            { path: "backup", sections },
        ],
        ["search", ["song"], "search", { query: "song" }],
        [
            "searchLyricsOnline",
            [7],
            "search_lyrics_online",
            { trackId: 7, query: null },
        ],
        [
            "searchLyricsOnline",
            [7, "song"],
            "search_lyrics_online",
            { trackId: 7, query: "song" },
        ],
        [
            "getHealthTracks",
            ["titles"],
            "get_health_tracks",
            { kind: "titles" },
        ],
        ["getListeningStats", [], "get_listening_stats", { days: null }],
        ["getListeningStats", [30], "get_listening_stats", { days: 30 }],
        ["setOnlineSettings", [settings], "set_online_settings", { settings }],
        ["getTracksByGenre", ["Pop"], "get_tracks_by_genre", { genre: "Pop" }],
        [
            "getGenreCollageAlbumIds",
            ["Pop"],
            "get_genre_collage_album_ids",
            { genre: "Pop" },
        ],
        ["getPlaylist", [7], "get_playlist", { id: 7 }],
        [
            "getPlaylistCollageAlbumIds",
            [7],
            "get_playlist_collage_album_ids",
            { id: 7 },
        ],
        [
            "createPlaylist",
            ["Mix"],
            "create_playlist",
            { name: "Mix", description: undefined, folderPath: undefined },
        ],
        [
            "createPlaylist",
            ["Mix", "Desc", "C:/Music"],
            "create_playlist",
            { name: "Mix", description: "Desc", folderPath: "C:/Music" },
        ],
        [
            "updatePlaylist",
            [7, "Mix", "Desc"],
            "update_playlist",
            { id: 7, name: "Mix", description: "Desc" },
        ],
        ["deletePlaylist", [7], "delete_playlist", { id: 7 }],
        [
            "addTracksToPlaylist",
            [7, [1, 2]],
            "add_tracks_to_playlist",
            { playlistId: 7, trackIds: [1, 2] },
        ],
        [
            "removeTrackFromPlaylist",
            [7, 1],
            "remove_track_from_playlist",
            { playlistId: 7, trackId: 1 },
        ],
    ]) {
        await api[method](...args);
        expect(invoke).toHaveBeenLastCalledWith(command, payload);
    }
    invoke.mockRejectedValue(Error("native failure"));
    await expect(api.getTracks()).rejects.toThrow("native failure");
});

test("file pickers restrict extensions and treat cancel/unexpected multi-select as null", async () => {
    for (const [method, boundary, extensions] of [
        ["pickImageFile", open, ["png", "jpg", "jpeg", "webp", "gif", "bmp"]],
        ["pickLyricsFile", open, ["lrc", "txt"]],
        ["pickBackupToImport", open, ["sparklebackup"]],
        ["pickBackupToSave", save, ["sparklebackup"]],
    ]) {
        for (const result of [null, ["a", "b"], "C:/chosen.file"]) {
            boundary.mockResolvedValue(result);
            expect(await api[method]()).toBe(
                typeof result === "string" ? result : null,
            );
            expect(boundary.mock.lastCall[0].filters[0].extensions).toEqual(
                extensions,
            );
        }
    }
});

test("lyrics share in-flight reads, retry failures, and invalidate before announcing edits", async () => {
    const first = deferred(),
        next = deferred();
    invoke.mockImplementation((command) =>
        command === "get_lyrics" ? first.promise : Promise.resolve(),
    );
    const a = api.getLyrics(101),
        b = api.getLyrics(101);
    expect(invoke).toHaveBeenCalledTimes(1);
    const events = [];
    globalThis.window = { dispatchEvent: (event) => events.push(event) };
    try {
        for (const [method, args, command, payload] of [
            [
                "setTrackLyricsSource",
                [101],
                "set_track_lyrics_source",
                { trackId: 101, source: null },
            ],
            [
                "setTrackLyricsSource",
                [101, "lrc"],
                "set_track_lyrics_source",
                { trackId: 101, source: "lrc" },
            ],
            [
                "setTrackCustomLyrics",
                [101, "song.lrc"],
                "set_track_custom_lyrics",
                { trackId: 101, path: "song.lrc" },
            ],
            [
                "clearTrackCustomLyrics",
                [101],
                "clear_track_custom_lyrics",
                { trackId: 101 },
            ],
            [
                "setTrackLyricsChoice",
                [101, { source: "custom" }],
                "set_track_lyrics_choice",
                {
                    trackId: 101,
                    source: "custom",
                    syncedText: null,
                    plainText: null,
                },
            ],
            [
                "setTrackLyricsChoice",
                [
                    101,
                    {
                        source: "custom",
                        syncedText: "[00:01]Hi",
                        plainText: "Hi",
                    },
                ],
                "set_track_lyrics_choice",
                {
                    trackId: 101,
                    source: "custom",
                    syncedText: "[00:01]Hi",
                    plainText: "Hi",
                },
            ],
        ]) {
            await api[method](...args);
            expect(invoke).toHaveBeenLastCalledWith(command, payload);
            expect(events.at(-1).type).toBe(api.LYRICS_CHANGED_EVENT);
            expect(events.at(-1).detail).toEqual({ trackId: 101 });
        }
        invoke.mockImplementation(() => next.promise);
        const c = api.getLyrics(101);
        first.resolve({ source: "old" });
        await Promise.all([a, b]);
        const count = invoke.mock.calls.length;
        const d = api.getLyrics(101);
        expect(invoke.mock.calls.length).toBe(count);
        next.resolve({ source: "new" });
        expect(await c).toEqual({ source: "new" });
        expect(await d).toEqual({ source: "new" });
        invoke.mockRejectedValue(Error("offline"));
        await expect(api.getLyrics(101)).rejects.toThrow("offline");
        const eventCount = events.length;
        await expect(api.clearTrackCustomLyrics(101)).rejects.toThrow(
            "offline",
        );
        expect(events.length).toBe(eventCount);
        invoke.mockResolvedValue({ source: "retry" });
        expect(await api.getLyrics(101)).toEqual({ source: "retry" });
    } finally {
        delete globalThis.window;
    }
});

test("artwork scheduler reserves a foreground lane and promotes a queued duplicate", async () => {
    const pending = new Map();
    invoke.mockImplementation((command, { albumId }) => {
        const request = deferred();
        pending.set(albumId, request);
        return request.promise;
    });
    const a = api.getAlbumArt(201, "background"),
        b = api.getAlbumArt(202, "background"),
        c = api.getAlbumArt(203, "background");
    await flush();
    expect([...pending.keys()]).toEqual([201, 202]);
    const promoted = api.getAlbumArt(203);
    await flush();
    expect([...pending.keys()]).toEqual([201, 202, 203]);
    const d = api.getAlbumArt(204);
    await flush();
    expect(pending.has(204)).toBe(false);
    pending.get(201).resolve(image(201));
    await a;
    await flush();
    expect(pending.has(204)).toBe(true);
    for (const id of [202, 203, 204]) pending.get(id).resolve(image(id));
    expect(await c).toEqual(await promoted);
    await Promise.all([b, d]);
    await flush();
    const calls = invoke.mock.calls.length;
    expect(await api.getAlbumArt(203)).toEqual(image(203));
    expect(invoke.mock.calls.length).toBe(calls);
});

test("invalidated artwork cannot overwrite fresh data; failures release slots for retries", async () => {
    const stale = deferred();
    invoke.mockImplementationOnce(() => stale.promise);
    const a = api.getArtistImage(301);
    await flush();
    api.invalidateArtistImage(301);
    invoke.mockResolvedValue(image("new"));
    expect(await api.getArtistImage(301)).toEqual(image("new"));
    stale.resolve(image("old"));
    expect(await a).toEqual(image("old"));
    expect(await api.getArtistImage(301)).toEqual(image("new"));
    api.invalidateAlbumArt(302);
    invoke.mockRejectedValueOnce(Error("network"));
    await expect(api.getAlbumArt(302)).rejects.toThrow("network");
    expect(await api.getAlbumArt(302)).toEqual(image("new"));
    await flush();
});

test("artwork cache is LRU-bounded and cache clears invalidate completed references", async () => {
    invoke.mockResolvedValue(undefined);
    await api.clearAllCaches();
    invoke.mockImplementation(async (command, args) => image(args.albumId));
    for (let id = 400; id < 528; id++) await api.getAlbumArt(id);
    await api.getAlbumArt(400); // Refresh the oldest entry.
    await api.getAlbumArt(528);
    const calls = invoke.mock.calls.length;
    await api.getAlbumArt(400);
    expect(invoke.mock.calls.length).toBe(calls);
    await api.getAlbumArt(401);
    expect(invoke.mock.calls.length).toBe(calls + 1);
    for (const clear of [api.clearImagesCache, api.clearAllCaches]) {
        invoke.mockResolvedValue(undefined);
        await clear();
        invoke.mockResolvedValue(image("fresh"));
        expect(await api.getAlbumArt(400)).toEqual(image("fresh"));
    }
    await flush();
});

test("media artwork reads bytes only when a cached file exists", async () => {
    invoke.mockResolvedValue({ source: "none", mime_type: "image/jpeg" });
    expect(await api.getAlbumArtData(601)).toEqual({
        source: "none",
        mime_type: "image/jpeg",
    });
    invoke.mockResolvedValueOnce(image(602)).mockResolvedValueOnce({
        data: [1, 2],
        source: "embedded",
        mime_type: "image/jpeg",
    });
    expect((await api.getAlbumArtData(602)).data).toEqual([1, 2]);
    expect(invoke).toHaveBeenLastCalledWith("get_album_art_data", {
        albumId: 602,
        source: "embedded",
    });
    await flush();
});
