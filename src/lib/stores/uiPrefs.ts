import { writable, type Writable } from "svelte/store";

// UI state (sort field/direction, group-by, view mode) persists across
// navigation and restarts via localStorage, one key per page+field.
const PREFIX = "sparkle.ui.";

function read<T>(key: string, fallback: T): T {
    if (typeof window === "undefined") return fallback;
    try {
        const raw = window.localStorage.getItem(PREFIX + key);
        return raw === null ? fallback : (JSON.parse(raw) as T);
    } catch {
        return fallback;
    }
}

export function uiPref<T>(key: string, initial: T): Writable<T> {
    const store = writable<T>(read(key, initial));
    if (typeof window !== "undefined") {
        store.subscribe((value) => {
            try {
                window.localStorage.setItem(
                    PREFIX + key,
                    JSON.stringify(value),
                );
            } catch {
                // storage full / private mode — state just stays in memory
            }
        });
    }
    return store;
}

export type NowPlayingLayout = "album" | "artist" | "lyrics";

// This is a singleton so changing the setting updates an already-mounted
// player page immediately, without waiting for a reload or storage event.
export const nowPlayingLayout = uiPref<NowPlayingLayout>(
    "now-playing.layout",
    "album",
);
