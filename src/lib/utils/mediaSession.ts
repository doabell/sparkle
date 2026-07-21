import { enableMediaControlEvents } from "$lib/api";
import { mediaControls } from "tauri-plugin-media-api";

let initialization: Promise<void> | null = null;

export function initializeMediaSessionOnce(): Promise<void> {
    if (initialization) return initialization;

    initialization = (async () => {
        await mediaControls.initialize("com.doabell.sparkle", "Sparkle");
        await enableMediaControlEvents();
    })().catch((error) => {
        initialization = null;
        throw error;
    });

    return initialization;
}
