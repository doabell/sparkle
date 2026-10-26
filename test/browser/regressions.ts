// @ts-nocheck — this fixture runs in the browser, outside the Bun test runner.
import { mount, unmount, tick, flushSync } from "svelte";
import { get } from "svelte/store";
import Lists from "./Lists.svelte";
import ArtworkRows from "./ArtworkRows.svelte";
import MiniPlayer from "../../src/lib/components/MiniPlayer.svelte";
import Playlist from "../../src/routes/playlists/[id]/+page.svelte";
import { navigate, navigations } from "./router";
import { getAlbumArt, getArtistImage } from "../../src/lib/api";
import { playback } from "../../src/lib/stores/playback";

const target = document.querySelector("#app");
const result = { passed: 0, failures: [] };
function check(name, actual, expected) {
    if (actual === expected) result.passed++;
    else
        result.failures.push(
            `${name}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
        );
}
const settle = async () => {
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 50));
    await tick();
};
const activeRow = () =>
    Number(
        document.activeElement?.closest("[data-virtual-index]")?.dataset
            .virtualIndex,
    );
async function key(value) {
    document.activeElement.dispatchEvent(
        new KeyboardEvent("keydown", { key: value, bubbles: true }),
    );
    await settle();
}
function deferred() {
    let resolve, reject;
    const promise = new Promise((yes, no) => {
        resolve = yes;
        reject = no;
    });
    return { promise, resolve, reject };
}
const playlist = (id, count = 3) => ({
    id,
    name: `Playlist ${id}`,
    description: null,
    folder_path: null,
    live_mix: null,
    tracks: Array.from({ length: count }, (_, i) => ({
        id: i + 1,
        title: `Track ${i + 1}`,
        album_id: null,
        album_title: null,
        artist_names: [],
        artist_ids: [],
        duration_ms: 1000,
    })),
});
const reads = [];
let removal;
let deletion;
const imageReads = [];
const pendingImages = new Map();
const imageUrl =
    "data:image/svg+xml," +
    encodeURIComponent(
        '<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48"><rect width="48" height="48" fill="#399"/></svg>',
    );
window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
        if (command === "get_playlist") {
            const read = deferred();
            reads.push({ args, ...read });
            return read.promise;
        }
        if (command === "remove_track_from_playlist") return removal.promise;
        if (command === "delete_playlist") return deletion.promise;
        if (command === "get_playlists") return [];
        if (command === "get_lyrics") return { source: "none" };
        if (command === "get_album_art" || command === "get_artist_image") {
            imageReads.push({ command, args });
            if (command === "get_album_art" && pendingImages.has(args.albumId))
                return pendingImages.get(args.albumId).promise;
            return {
                source: "embedded",
                file_path: imageUrl,
                mime_type: "image/svg+xml",
            };
        }
        if (command === "log_frontend") return;
        throw Error(`Unexpected IPC ${command}`);
    },
    convertFileSrc: (path) => path,
};
window.addEventListener("error", (event) =>
    result.failures.push(event.message),
);
window.addEventListener("unhandledrejection", (event) =>
    result.failures.push(String(event.reason)),
);

async function lists() {
    let app = mount(Lists, { target });
    try {
        await settle();
        target.querySelector('[data-virtual-index="6"] button').focus();
        await key("Home");
        check("Home skips the first group heading", activeRow(), 1);
        await key("ArrowUp");
        check("ArrowUp stays on the first track", activeRow(), 1);
        await key("End");
        check("End skips the trailing group heading", activeRow(), 11);
        await key("ArrowDown");
        check("ArrowDown stays on the last track", activeRow(), 11);
        target.querySelector('[data-virtual-index="5"] button').focus();
        await key("ArrowUp");
        check("ArrowUp skips a group heading", activeRow(), 3);
        await key("ArrowDown");
        check("ArrowDown skips a group heading", activeRow(), 5);
    } finally {
        await unmount(app);
    }
    app = mount(Lists, { target, props: { large: true } });
    try {
        await settle();
        target.querySelector('[data-virtual-index="0"] button').focus();
        await key("End");
        check("End reaches an unloaded queue page", activeRow(), 99999);
        check(
            "Large queues keep DOM rows bounded",
            target.querySelectorAll("[data-virtual-index]").length < 40,
            true,
        );
        await key("Home");
        check("Home returns to the first queue row", activeRow(), 0);
    } finally {
        await unmount(app);
    }
}

async function overlappingPlaylistRead(failRefresh) {
    reads.length = 0;
    removal = deferred();
    navigate(1);
    const app = mount(Playlist, { target });
    try {
        await settle();
        reads[0].resolve(playlist(1));
        await settle();
        target.querySelector("button.remove-track").click();
        await settle();
        navigate(2);
        await settle();
        reads[1].resolve(playlist(2));
        await settle();
        navigate(1);
        await settle();
        removal.resolve();
        await settle();
        check("Removal refresh starts after returning to A", reads.length, 4);
        if (failRefresh) reads[3].reject(Error("Refresh failed"));
        else reads[3].resolve(playlist(1, 2));
        await settle();
        // The older route read completes last and must not overwrite the refresh.
        reads[2].resolve(playlist(1));
        await settle();
        if (failRefresh) {
            check(
                "A failed refresh settles the pending route load",
                target
                    .querySelector(".error")
                    ?.textContent.includes("Refresh failed"),
                true,
            );
        } else {
            check(
                "A refresh settles the pending route load",
                target.querySelector(".playlist-title")?.textContent,
                "Playlist 1",
            );
            check(
                "Stale route results cannot restore removed tracks",
                target.querySelectorAll("button.remove-track").length,
                2,
            );
        }
    } finally {
        await unmount(app);
    }
}

async function playlistTeardown() {
    for (const deleting of [false, true]) {
        reads.length = 0;
        navigations.length = 0;
        removal = deferred();
        deletion = deferred();
        navigate(1);
        const app = mount(Playlist, { target });
        await settle();
        reads[0].resolve(playlist(1));
        await settle();
        target
            .querySelector(
                deleting ? ".delete-playlist-btn" : "button.remove-track",
            )
            .click();
        await settle();
        await unmount(app);
        (deleting ? deletion : removal).resolve();
        await settle();
        check("A destroyed playlist cannot start a refresh", reads.length, 1);
        check(
            "A destroyed playlist cannot redirect the new page",
            navigations.length,
            0,
        );
        // Settle any unexpected read so a failure cannot affect the next case.
        if (reads.length > 1) reads[1].resolve(playlist(1));
    }
}

async function artworkRecycling() {
    const originalObserver = window.IntersectionObserver;
    // Hold visibility notifications so a warm row must render from its cache.
    window.IntersectionObserver = class {
        observe() {}
        unobserve() {}
        disconnect() {}
    };
    try {
        await Promise.all([getAlbumArt(8000), getArtistImage(8000)]);
        const before = imageReads.length;
        for (let visit = 0; visit < 2; visit++) {
            const app = mount(ArtworkRows, { target });
            flushSync();
            check(
                `Cached album is present immediately on visit ${visit}`,
                !!target.querySelector('img[alt="Album 0"]'),
                true,
            );
            check(
                `Cached artist is present immediately on visit ${visit}`,
                !!target.querySelector('img[alt="Artist 0"]'),
                true,
            );
            await settle();
            check(
                "Cold offscreen artwork waits for visibility",
                imageReads.length,
                before,
            );
            await unmount(app);
        }
    } finally {
        window.IntersectionObserver = originalObserver;
    }
}

async function miniPlayerArtwork() {
    const original = get(playback);
    const originalDecode = HTMLImageElement.prototype.decode;
    const decoding = new Map();
    HTMLImageElement.prototype.decode = async function () {
        await originalDecode.call(this);
        const gate = decoding.get(this.getAttribute("src"));
        if (gate) await gate.promise;
    };
    const cover = (id) => ({
        source: "embedded",
        file_path: `${imageUrl.replaceAll("%2248%22", "%222048%22")}#album-${id}`,
        mime_type: "image/svg+xml",
    });
    const showTrack = (id, albumArt = null) => {
        playback.set({
            ...original,
            is_playing: false,
            error: null,
            current_track:
                id === null
                    ? null
                    : {
                          id,
                          title: `Track ${id}`,
                          album_id: id,
                          album_title: `Album ${id}`,
                          artist_names: ["Artist"],
                          duration_ms: 1000,
                      },
            album_art: albumArt,
        });
        flushSync();
    };
    const image = () =>
        target.querySelector('.artwork img:not([aria-hidden="true"])');
    const warm = await getAlbumArt(9100);
    showTrack(9100, warm);
    const app = mount(MiniPlayer, { target });
    try {
        await settle();
        const firstImage = image();
        check(
            "Mini player starts with the current cover",
            firstImage?.getAttribute("src"),
            warm.file_path,
        );
        for (const id of [9101, 9102, 9103, 9104, 9105, 9106, 9107])
            pendingImages.set(id, deferred());
        showTrack(9101);
        check(
            "Mini player keeps its image element while the next cover loads",
            image(),
            firstImage,
        );
        check(
            "Mini player keeps the current cover during a cold request",
            image()?.getAttribute("src"),
            warm.file_path,
        );
        await settle();
        const nextDecoded = deferred();
        decoding.set(cover(9101).file_path, nextDecoded);
        pendingImages.get(9101).resolve(cover(9101));
        await settle();
        check(
            "Mini player keeps the old cover until the new image is decoded",
            image()?.getAttribute("src"),
            warm.file_path,
        );
        check(
            "Decoding keeps only the displayed and incoming image mounted",
            target.querySelectorAll(".artwork img").length,
            2,
        );
        nextDecoded.resolve();
        await settle();
        check(
            "Mini player swaps directly to the loaded cover",
            image()?.getAttribute("src"),
            cover(9101).file_path,
        );
        check("Displayed image has decoded pixels", image().naturalWidth, 2048);
        check(
            "Large covers stay within the mini player",
            image().clientWidth,
            68,
        );
        check(
            "Large covers keep the mini player height",
            image().clientHeight,
            68,
        );
        check(
            "The previous image is released after the swap",
            target.querySelectorAll(".artwork img").length,
            1,
        );

        showTrack(9102);
        await settle();
        const skippedDecoded = deferred();
        decoding.set(cover(9102).file_path, skippedDecoded);
        pendingImages.get(9102).resolve(cover(9102));
        await settle();
        showTrack(9103);
        await settle();
        pendingImages.get(9103).resolve(cover(9103));
        await settle();
        skippedDecoded.resolve();
        await settle();
        check(
            "A slow skipped image decode cannot replace the current cover",
            image()?.getAttribute("src"),
            cover(9103).file_path,
        );

        const snapshotDecoded = deferred();
        decoding.set(cover(9104).file_path, snapshotDecoded);
        showTrack(9104, cover(9104));
        await settle();
        check(
            "Snapshot artwork also waits for decoding before replacing the cover",
            image()?.getAttribute("src"),
            cover(9103).file_path,
        );
        snapshotDecoded.resolve();
        await settle();
        check(
            "Mini player shows decoded snapshot artwork without waiting for IPC",
            image()?.getAttribute("src"),
            cover(9104).file_path,
        );
        await settle();
        pendingImages.get(9104).resolve(cover(9104));
        await settle();
        showTrack(9105);
        await settle();
        pendingImages.get(9105).resolve({ source: "none" });
        await settle();
        check(
            "A confirmed missing cover clears the previous album",
            image(),
            null,
        );
        showTrack(9104, cover(9104));
        await settle();
        showTrack(9106);
        pendingImages.get(9106).resolve({
            ...cover(9106),
            file_path: "data:image/png;base64,invalid",
        });
        await settle();
        check("An unreadable image shows the fallback", image(), null);
        check(
            "An unreadable image does not leave a broken image element",
            target.querySelectorAll(".artwork img").length,
            0,
        );

        const stoppedDecode = deferred();
        decoding.set(cover(9107).file_path, stoppedDecode);
        showTrack(9107, cover(9107));
        await settle();
        showTrack(null);
        check("Stopping playback clears the artwork", image(), null);
        stoppedDecode.resolve();
        await settle();
        check("A late decode cannot restore stopped artwork", image(), null);
    } finally {
        HTMLImageElement.prototype.decode = originalDecode;
        for (const gate of decoding.values()) gate.resolve();
        await unmount(app);
        for (const [id, request] of pendingImages) request.resolve(cover(id));
        pendingImages.clear();
        playback.set(original);
    }
}

try {
    await lists();
    await overlappingPlaylistRead(false);
    await overlappingPlaylistRead(true);
    await playlistTeardown();
    await artworkRecycling();
    await miniPlayerArtwork();
} catch (error) {
    result.failures.push(String(error.stack ?? error));
} finally {
    document.body.dataset.testResult = encodeURIComponent(
        JSON.stringify(result),
    );
}
