import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import {
    getLibraryScanStatus,
    scanLibrary,
    type LibraryScanStatus,
} from "$lib/api";
import { addToast } from "./toast";

interface ScanState extends LibraryScanStatus {
    ready: boolean;
    starting: boolean;
}

export function createLibraryScanStore() {
    let current: ScanState = {
        revision: 0,
        running: false,
        progress: null,
        result: null,
        error: null,
        ready: false,
        starting: false,
    };
    const { subscribe, set } = writable(current);
    let pending: Promise<void> | null = null;

    function update(next: ScanState) {
        current = next;
        set(current);
    }

    function apply(status: LibraryScanStatus) {
        // An initial IPC snapshot may arrive after a newer progress event.
        if (current.ready && status.revision <= current.revision) return;
        const notify = current.ready && !status.running;
        update({ ...current, ...status, ready: true });
        if (!notify) return;
        if (status.error) {
            addToast(status.error, "error");
        } else if (status.result) {
            const errors = status.result.errors;
            addToast(
                errors
                    ? `Scan complete with ${errors} error${errors === 1 ? "" : "s"}`
                    : "Scan complete",
                errors ? "error" : "success",
            );
        }
    }

    // The root layout owns this connection; page subscriptions never start,
    // stop, or reset a scan. The snapshot also recovers a running startup scan.
    function connect() {
        let disposed = false;
        let unlisten: (() => void) | undefined;
        void (async () => {
            try {
                const cleanup = await listen<LibraryScanStatus>(
                    "library-scan-status",
                    ({ payload }) => {
                        if (!disposed) apply(payload);
                    },
                );
                if (disposed) {
                    cleanup();
                    return;
                }
                unlisten = cleanup;
                const status = await getLibraryScanStatus();
                if (!disposed) apply(status);
            } catch (error) {
                if (!disposed) {
                    update({ ...current, error: String(error) });
                }
            }
        })();
        return () => {
            disposed = true;
            unlisten?.();
        };
    }

    function start(force = false): Promise<void> {
        if (pending) return pending;
        if (!current.ready || current.running) return Promise.resolve();
        update({ ...current, starting: true, result: null, error: null });
        pending = (async () => {
            try {
                await scanLibrary(force);
                apply(await getLibraryScanStatus());
            } catch (error) {
                const message = String(error);
                let reported = current.error === message;
                // The backend may have rejected this request because a
                // startup scan already owns the worker. Recover its status.
                try {
                    apply(await getLibraryScanStatus());
                    reported ||= current.error === message;
                } catch {
                    update({ ...current, error: message });
                }
                if (!reported) addToast(message, "error");
            } finally {
                pending = null;
                update({ ...current, starting: false });
            }
        })();
        return pending;
    }

    return { subscribe, connect, start };
}

export const libraryScan = createLibraryScanStore();
