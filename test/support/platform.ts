// @ts-nocheck
// Mock only native/browser boundaries; production API and stores stay real.
import { mock } from "bun:test";
import { writable } from "svelte/store";
import * as tauriCore from "@tauri-apps/api/core";
import * as tauriEvent from "@tauri-apps/api/event";

export const invoke = mock(async () => undefined);
export const logFrontend = mock(async () => undefined);
export const open = mock(async () => null);
export const save = mock(async () => null);
export const listen = mock(async () => () => {});
export const initializeMedia = mock(async () => undefined);
export const goto = mock(async () => undefined);
export const page = writable({ url: new URL("https://sparkle.test/") });
mock.module("@tauri-apps/api/core", () => ({
    ...tauriCore,
    invoke: (...args) =>
        args[0] === "log_frontend" ? logFrontend(args[1]) : invoke(...args),
    convertFileSrc: (path) => `asset://localhost/${encodeURIComponent(path)}`,
}));
mock.module("@tauri-apps/api/event", () => ({ ...tauriEvent, listen }));
mock.module("@tauri-apps/plugin-dialog", () => ({ open, save }));
mock.module("tauri-plugin-media-api", () => ({
    mediaControls: { initialize: initializeMedia },
}));
mock.module("$app/navigation", () => ({ goto }));
mock.module("$app/stores", () => ({ page }));
