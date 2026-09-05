// @ts-nocheck
import { expect, test, spyOn } from "bun:test";
import { get } from "svelte/store";
import { uiPref, nowPlayingLayout } from "../src/lib/stores/uiPrefs";
import { songIndexLanguage } from "../src/lib/stores/songIndex";
import { windowPageTitle } from "../src/lib/stores/windowPageTitle";
import { toasts } from "../src/lib/stores/toast";
import {
    playback,
    interpolatedPositionMs,
    createPlaybackStore,
} from "../src/lib/stores/playback";
import { invoke, initializeMedia, listen } from "./support/platform";
import { initializeMediaSessionOnce } from "../src/lib/utils/mediaSession";

test("UI preferences default without a window and persist falsy values and updates", () => {
    expect(get(uiPref("server", "fallback"))).toBe("fallback");
    expect(get(nowPlayingLayout)).toBe("album");
    expect(get(songIndexLanguage)).toBe("auto");
    const storage = new Map([["sparkle.ui.view", "false"]]);
    globalThis.window = {
        localStorage: {
            getItem: (key) => storage.get(key) ?? null,
            setItem: (key, value) => storage.set(key, value),
        },
    };
    try {
        const pref = uiPref("view", true);
        expect(get(pref)).toBe(false);
        pref.set(true);
        expect(storage.get("sparkle.ui.view")).toBe("true");
        expect(get(uiPref("view", false))).toBe(true);
        storage.set("sparkle.ui.corrupt", "{");
        expect(get(uiPref("corrupt", "fallback"))).toBe("fallback");
        expect(get(uiPref("missing", 0))).toBe(0);
        window.localStorage.getItem = () => {
            throw Error("blocked");
        };
        window.localStorage.setItem = () => {
            throw Error("full");
        };
        const memory = uiPref("blocked", 1);
        memory.update((value) => value + 1);
        expect(get(memory)).toBe(2);
    } finally {
        delete globalThis.window;
    }
});

test("page title can be set and cleared for route fallback", () => {
    windowPageTitle.set("Artist");
    expect(get(windowPageTitle)).toBe("Artist");
    windowPageTitle.set(null);
    expect(get(windowPageTitle)).toBe(null);
});

test("toasts have distinct IDs, expire independently and tolerate manual dismissal", () => {
    const callbacks = [];
    const timer = spyOn(globalThis, "setTimeout").mockImplementation(
        (fn, ms) => {
            expect(ms).toBe(5000);
            callbacks.push(fn);
            return callbacks.length;
        },
    );
    try {
        toasts.clearToasts();
        toasts.addToast("First");
        toasts.addToast("Second", "error");
        const [first, second] = get(toasts);
        expect(first.type).toBe("info");
        expect(second.type).toBe("error");
        expect(first.id).not.toBe(second.id);
        toasts.removeToast(first.id);
        callbacks[0]();
        expect(get(toasts)).toEqual([second]);
        callbacks[1]();
        expect(get(toasts)).toEqual([]);
        toasts.addToast("Third", "success");
        toasts.clearToasts();
        callbacks[2]();
        expect(get(toasts)).toEqual([]);
    } finally {
        timer.mockRestore();
        toasts.clearToasts();
    }
});

const state = {
    is_playing: true,
    current_track: { id: 7, title: "Song" },
    first_lyric_line: null,
    album_art: null,
    position_ms: 100,
    duration_ms: 1000,
    volume: 0.5,
    shuffle: false,
    repeat_mode: "off",
};

test("playback initializes from native state and merges events without losing volume", async () => {
    const handlers = new Map();
    globalThis.window = {};
    invoke.mockResolvedValue(state);
    listen.mockImplementation(async (name, handler) => {
        handlers.set(name, handler);
        return () => {};
    });
    try {
        const store = createPlaybackStore();
        for (let i = 0; i < 10; i++) await Promise.resolve();
        expect(get(store)).toEqual({ ...state, error: null });
        handlers.get("playback-state-changed")({
            payload: { ...state, is_playing: false, volume: 0.1 },
        });
        expect(get(store).is_playing).toBe(false);
        expect(get(store).volume).toBe(0.5);
        handlers.get("playback-progress")({
            payload: { track_id: 7, position_ms: 400, duration_ms: 1200 },
        });
        expect(get(store).position_ms).toBe(400);
        expect(get(store).duration_ms).toBe(1200);
    } finally {
        delete globalThis.window;
        invoke.mockReset();
        listen.mockReset();
    }
});

test("playback initialization failures stay observable without unhandled rejections", async () => {
    globalThis.window = {};
    const log = spyOn(console, "error").mockImplementation(() => {});
    invoke.mockRejectedValue(Error("unavailable"));
    listen.mockRejectedValue(Error("events unavailable"));
    try {
        const store = createPlaybackStore();
        for (let i = 0; i < 10; i++) await Promise.resolve();
        expect(get(store).error).toBe("Error: unavailable");
        expect(log).toHaveBeenCalledTimes(3);
    } finally {
        delete globalThis.window;
        invoke.mockReset();
        listen.mockReset();
        log.mockRestore();
    }
});

test("playback commands preserve intent, return canonical state, and recover from errors", async () => {
    const errorLog = spyOn(console, "error").mockImplementation(() => {});
    invoke.mockImplementation(async () => state);
    try {
        for (const [method, args, command, payload] of [
            ["play", [], "play", { source: "ui" }],
            ["pause", ["keyboard"], "pause", { source: "keyboard" }],
            ["stop", [], "stop", { source: "ui" }],
            ["seek", [250], "seek", { positionMs: 250, source: "ui" }],
            ["nextTrack", [], "next_track", { source: "ui" }],
            ["previousTrack", [], "previous_track", { source: "ui" }],
            ["setVolume", [0.3], "set_volume", { volume: 0.3, source: "ui" }],
            [
                "setShuffle",
                [false],
                "set_shuffle",
                { shuffle: false, source: "ui" },
            ],
            ["cycleRepeatMode", [], "cycle_repeat_mode", { source: "ui" }],
            ["playNext", [7], "play_next", { trackId: 7, source: "ui" }],
            [
                "playQueueIndex",
                [2],
                "play_queue_index",
                { orderPos: 2, source: "ui" },
            ],
            [
                "loadQueue",
                [[7, 8]],
                "load_queue",
                {
                    trackIds: [7, 8],
                    startIndex: 0,
                    shuffle: null,
                    context: null,
                    source: "ui",
                },
            ],
            [
                "loadQueue",
                [[7, 8], 1, false, { kind: "album", id: "9" }, "system_media"],
                "load_queue",
                {
                    trackIds: [7, 8],
                    startIndex: 1,
                    shuffle: false,
                    context: { kind: "album", id: "9" },
                    source: "system_media",
                },
            ],
            [
                "playTrack",
                [7],
                "play_track",
                { trackId: 7, context: null, source: "ui" },
            ],
        ]) {
            expect(await playback[method](...args)).toBe(state);
            expect(invoke).toHaveBeenLastCalledWith(command, payload);
            expect(get(playback)).toEqual({ ...state, error: null });
        }
        playback.updateCurrentTrackLrcOffset(99, -50);
        expect(get(playback).current_track).toEqual(state.current_track);
        playback.updateCurrentTrackLrcOffset(7, -50);
        playback.updateCurrentTrackLyricsSource(7, "lrc");
        expect(get(playback).current_track).toEqual({
            ...state.current_track,
            lrc_offset_ms: -50,
            lyrics_source: "lrc",
        });
        const error = Error("offline");
        invoke.mockImplementation(async () => {
            throw error;
        });
        await expect(playback.play()).rejects.toBe(error);
        expect(get(playback).is_playing).toBe(false);
        expect(get(playback).error).toBe("Error: offline");
        await playback.setVolumeLive(0.2);
        expect(errorLog).toHaveBeenLastCalledWith(
            "Live volume update failed:",
            error,
        );
        invoke.mockImplementation(async () => state);
        await playback.play();
        expect(get(playback).error).toBe(null);
        await playback.setVolumeLive(0.4, "keyboard");
        expect(invoke).toHaveBeenLastCalledWith("set_volume", {
            volume: 0.4,
            source: "keyboard",
        });
    } finally {
        invoke.mockReset();
        errorLog.mockRestore();
    }
});

test("failed queue loads preserve the last metadata and a retry accepts the new canonical state", async () => {
    const store = createPlaybackStore();
    const log = spyOn(console, "error").mockImplementation(() => {});
    store.set({ ...state, error: null });
    const failure = Error("track not found");
    try {
        invoke.mockRejectedValueOnce(failure);
        await expect(store.loadQueue([999], 0, true)).rejects.toBe(failure);
        expect(get(store)).toEqual({
            ...state,
            is_playing: false,
            error: String(failure),
        });
        const recovered = {
            ...state,
            current_track: { id: 8, title: "Recovered" },
            first_lyric_line: "New lyric",
            album_art: { file_path: "new-cover.jpg", mime_type: "image/jpeg" },
            position_ms: 0,
            duration_ms: 2000,
            shuffle: true,
        };
        invoke.mockResolvedValueOnce(recovered);
        expect(await store.loadQueue([7, 8], 1, true)).toBe(recovered);
        expect(get(store)).toEqual({ ...recovered, error: null });
    } finally {
        invoke.mockReset();
        log.mockRestore();
    }
});

test("failed seeks preserve position and recover through native state events or a retry", async () => {
    const handlers = new Map();
    const log = spyOn(console, "error").mockImplementation(() => {});
    globalThis.window = {};
    invoke.mockResolvedValue(state);
    listen.mockImplementation(async (name, handler) => {
        handlers.set(name, handler);
        return () => {};
    });
    try {
        const store = createPlaybackStore();
        for (let i = 0; i < 10; i++) await Promise.resolve();
        invoke.mockRejectedValueOnce(Error("seek unavailable"));
        await expect(store.seek(800)).rejects.toThrow("seek unavailable");
        expect(get(store).position_ms).toBe(100);
        expect(get(store).current_track.id).toBe(7);
        expect(get(store).error).toBe("Error: seek unavailable");
        // The worker is authoritative even if the original command failed.
        const resumed = { ...state, position_ms: 300 };
        handlers.get("playback-state-changed")({ payload: resumed });
        expect(get(store)).toEqual({ ...resumed, error: null });
        const clamped = {
            ...state,
            is_playing: false,
            position_ms: state.duration_ms,
        };
        invoke.mockResolvedValueOnce(clamped);
        await store.seek(9999, "keyboard");
        expect(invoke).toHaveBeenLastCalledWith("seek", {
            positionMs: 9999,
            source: "keyboard",
        });
        expect(get(store)).toEqual({ ...clamped, error: null });
    } finally {
        delete globalThis.window;
        invoke.mockReset();
        listen.mockReset();
        log.mockRestore();
    }
});

test("late progress from an old track cannot corrupt a recovered track or a stopped player", async () => {
    const handlers = new Map();
    globalThis.window = {};
    invoke.mockResolvedValue(state);
    listen.mockImplementation(async (name, handler) => {
        handlers.set(name, handler);
        return () => {};
    });
    try {
        const store = createPlaybackStore();
        for (let i = 0; i < 10; i++) await Promise.resolve();
        const recovered = {
            ...state,
            current_track: { id: 8, title: "Next" },
            position_ms: 0,
            duration_ms: 2000,
        };
        handlers.get("playback-state-changed")({ payload: recovered });
        const progress = handlers.get("playback-progress");
        progress({
            payload: { track_id: 7, position_ms: 900, duration_ms: 1000 },
        });
        expect(get(store)).toEqual({ ...recovered, error: null });
        progress({
            payload: { track_id: 8, position_ms: 250, duration_ms: 2000 },
        });
        expect(get(store).position_ms).toBe(250);
        const stopped = {
            ...state,
            current_track: null,
            is_playing: false,
            position_ms: 0,
            duration_ms: 0,
        };
        handlers.get("playback-state-changed")({ payload: stopped });
        progress({
            payload: { track_id: 8, position_ms: 500, duration_ms: 2000 },
        });
        expect(get(store)).toEqual({ ...stopped, error: null });
    } finally {
        delete globalThis.window;
        invoke.mockReset();
        listen.mockReset();
    }
});

test("lyric clock interpolates, clamps, resamples seeks and stops on unsubscribe", () => {
    expect(get(interpolatedPositionMs)).toBe(0);
    globalThis.window = {};
    const clock = spyOn(performance, "now").mockReturnValue(1000);
    const oldRaf = globalThis.requestAnimationFrame,
        oldCancel = globalThis.cancelAnimationFrame;
    let frame, cancelled, position;
    globalThis.requestAnimationFrame = (fn) => {
        frame = fn;
        return 1;
    };
    globalThis.cancelAnimationFrame = (id) => {
        cancelled = id;
    };
    let unsubscribe;
    try {
        playback.set({ ...state, error: null });
        unsubscribe = interpolatedPositionMs.subscribe(
            (value) => (position = value),
        );
        expect(position).toBe(100);
        frame(1250);
        expect(position).toBe(350);
        frame(5000);
        expect(position).toBe(1000);
        playback.set({
            ...state,
            position_ms: 200,
            is_playing: false,
            error: null,
        });
        frame(5000);
        expect(position).toBe(200);
        playback.set({ ...state, position_ms: 0, duration_ms: 0, error: null });
        frame(1500);
        expect(position).toBe(500);
        frame(900);
        expect(position).toBe(0);
        unsubscribe();
        expect(cancelled).toBe(1);
    } finally {
        unsubscribe?.();
        clock.mockRestore();
        delete globalThis.window;
        if (oldRaf) globalThis.requestAnimationFrame = oldRaf;
        else delete globalThis.requestAnimationFrame;
        if (oldCancel) globalThis.cancelAnimationFrame = oldCancel;
        else delete globalThis.cancelAnimationFrame;
    }
});

test("media initialization deduplicates concurrent requests and retries failed stages", async () => {
    initializeMedia.mockRejectedValueOnce(Error("native unavailable"));
    await expect(initializeMediaSessionOnce()).rejects.toThrow(
        "native unavailable",
    );
    initializeMedia.mockResolvedValue(undefined);
    invoke.mockRejectedValueOnce(Error("event setup failed"));
    await expect(initializeMediaSessionOnce()).rejects.toThrow(
        "event setup failed",
    );
    invoke.mockResolvedValue(undefined);
    const first = initializeMediaSessionOnce();
    expect(initializeMediaSessionOnce()).toBe(first);
    await first;
    expect(initializeMediaSessionOnce()).toBe(first);
    expect(initializeMedia).toHaveBeenLastCalledWith(
        "com.doabell.sparkle",
        "Sparkle",
    );
    expect(invoke).toHaveBeenLastCalledWith("enable_media_control_events");
    invoke.mockReset();
});
