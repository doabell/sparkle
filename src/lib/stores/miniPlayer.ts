import { tick } from "svelte";
import { writable } from "svelte/store";
import {
    currentMonitor,
    getCurrentWindow,
    LogicalSize,
    PhysicalPosition,
    type PhysicalSize,
    type Window as TauriWindow,
} from "@tauri-apps/api/window";
import { addToast } from "$lib/stores/toast";

export const MINI_PLAYER_SIZE = new LogicalSize(360, 154);

const PREFERENCES_KEY = "sparkle.ui.player";
type PreferenceStorage = Pick<Storage, "getItem" | "setItem">;

type PlayerWindow = Pick<
    TauriWindow,
    | "innerSize"
    | "outerPosition"
    | "isMaximized"
    | "isFullscreen"
    | "isResizable"
    | "isMaximizable"
    | "isAlwaysOnTop"
    | "unmaximize"
    | "maximize"
    | "setFullscreen"
    | "setResizable"
    | "setMaximizable"
    | "setAlwaysOnTop"
    | "setSize"
    | "setPosition"
    | "show"
>;

interface WindowSnapshot {
    size: PhysicalSize;
    position: PhysicalPosition;
    maximized: boolean;
    fullscreen: boolean;
    resizable: boolean;
    maximizable: boolean;
    alwaysOnTop: boolean;
}

// Reuse the main webview: there is still only one media session and one queue.
// Keep a snapshot until restoration succeeds, including after a partial failure.
export function createMiniPlayer(
    getWindow: () => PlayerWindow = getCurrentWindow,
    getMonitor = currentMonitor,
    reportError = (message: string) => addToast(message, "error"),
    getStorage: () => PreferenceStorage | null = () =>
        typeof window === "undefined" ? null : window.localStorage,
    beforeShow: () => Promise<void> = tick,
) {
    const { subscribe, set } = writable({
        active: false,
        busy: false,
        pinned: true,
    });
    let active = false;
    let busy = false;
    let pinned = true;
    let startupRestored = false;
    let saved: WindowSnapshot | null = null;
    let miniPosition: PhysicalPosition | null = null;

    function rememberMode() {
        try {
            getStorage()?.setItem(
                PREFERENCES_KEY,
                JSON.stringify({
                    mode: active ? "mini" : "large",
                    pinned,
                }),
            );
        } catch {
            // Storage can be unavailable or full; native controls still work.
        }
    }

    async function restoreMode() {
        if (startupRestored || busy || active) return;
        startupRestored = true;
        let mode = "large";
        try {
            const raw = getStorage()?.getItem(PREFERENCES_KEY);
            const preference: unknown = raw ? JSON.parse(raw) : null;
            if (preference && typeof preference === "object") {
                if ("mode" in preference && preference.mode === "mini")
                    mode = "mini";
                if (
                    "pinned" in preference &&
                    typeof preference.pinned === "boolean"
                ) {
                    pinned = preference.pinned;
                }
            }
        } catch {
            // Missing, invalid, or inaccessible preferences default to large.
        }
        try {
            set({ active, busy, pinned });
            if (mode === "mini") await toggle();
            // The native window starts hidden. Flush the selected layout before
            // showing it so saved mini mode never exposes the large window.
            await beforeShow();
        } catch (error) {
            console.error("Failed to prepare the startup player:", error);
        } finally {
            try {
                await getWindow().show();
            } catch (error) {
                console.error("Failed to show the player window:", error);
            }
        }
    }

    async function restore(window: PlayerWindow, snapshot: WindowSnapshot) {
        const errors: unknown[] = [];
        // A failed operation must not prevent the others (especially unpinning).
        for (const action of [
            () => window.setFullscreen(false),
            () => window.unmaximize(),
            () => window.setResizable(true),
            () => window.setPosition(snapshot.position),
            () => window.setSize(snapshot.size),
            () => window.setAlwaysOnTop(snapshot.alwaysOnTop),
            () => window.setMaximizable(snapshot.maximizable),
            () => window.setResizable(snapshot.resizable),
            ...(snapshot.maximized ? [() => window.maximize()] : []),
            ...(snapshot.fullscreen ? [() => window.setFullscreen(true)] : []),
        ]) {
            try {
                await action();
            } catch (error) {
                errors.push(error);
            }
        }
        if (errors.length)
            throw new AggregateError(errors, "Window restore failed");
    }

    async function toggle() {
        if (busy) return;
        startupRestored = true;
        busy = true;
        set({ active, busy, pinned });
        try {
            const window = getWindow();
            if (active && saved) {
                // Remembering placement is optional; a failed read must not
                // prevent restoring and unpinning the full window.
                miniPosition = await window
                    .outerPosition()
                    .catch(() => miniPosition);
                await restore(window, saved);
                saved = null;
                active = false;
            } else {
                const [
                    size,
                    position,
                    maximized,
                    fullscreen,
                    resizable,
                    maximizable,
                    alwaysOnTop,
                    monitor,
                ] = await Promise.all([
                    window.innerSize(),
                    window.outerPosition(),
                    window.isMaximized(),
                    window.isFullscreen(),
                    window.isResizable(),
                    window.isMaximizable(),
                    window.isAlwaysOnTop(),
                    getMonitor(),
                ]);
                saved = {
                    size,
                    position,
                    maximized,
                    fullscreen,
                    resizable,
                    maximizable,
                    alwaysOnTop,
                };
                try {
                    if (fullscreen) await window.setFullscreen(false);
                    if (maximized) await window.unmaximize();
                    // Capture normal bounds as well as the maximized state so
                    // Restore Down still works after leaving the mini player.
                    if (fullscreen || maximized) {
                        saved.size = await window.innerSize();
                        saved.position = await window.outerPosition();
                    }
                    await window.setMaximizable(false);
                    await window.setSize(MINI_PLAYER_SIZE);
                    await window.setResizable(false);
                    if (monitor) {
                        const { position: origin, size: area } =
                            monitor.workArea;
                        const scale = monitor.scaleFactor;
                        const margin = Math.round(16 * scale);
                        const right =
                            origin.x +
                            area.width -
                            Math.round(MINI_PLAYER_SIZE.width * scale) -
                            margin;
                        const bottom =
                            origin.y +
                            area.height -
                            Math.round(MINI_PLAYER_SIZE.height * scale) -
                            margin;
                        await window.setPosition(
                            new PhysicalPosition(
                                Math.max(
                                    origin.x,
                                    Math.min(miniPosition?.x ?? right, right),
                                ),
                                Math.max(
                                    origin.y,
                                    Math.min(miniPosition?.y ?? bottom, bottom),
                                ),
                            ),
                        );
                    }
                    await window.setAlwaysOnTop(pinned);
                    active = true;
                } catch (error) {
                    try {
                        await restore(window, saved);
                        saved = null;
                    } catch {
                        // Keep the restore action available for a retry.
                        active = true;
                    }
                    throw error;
                }
            }
            // Commit the preference only after every native step succeeds.
            rememberMode();
        } catch (error) {
            console.error("Failed to change mini player mode:", error);
            reportError(
                active
                    ? "Couldn't restore the window. Try leaving Mini Player again."
                    : "Couldn't open Mini Player. Please try again in the desktop app.",
            );
        } finally {
            busy = false;
            set({ active, busy, pinned });
        }
    }

    async function togglePinned() {
        if (!active || busy) return;
        busy = true;
        set({ active, busy, pinned });
        try {
            await getWindow().setAlwaysOnTop(!pinned);
            pinned = !pinned;
            rememberMode();
        } catch (error) {
            console.error("Failed to change mini player pin:", error);
            reportError("Couldn't change the pin setting. Please try again.");
        } finally {
            busy = false;
            set({ active, busy, pinned });
        }
    }

    return { subscribe, toggle, togglePinned, restoreMode };
}

export const miniPlayer = createMiniPlayer();
