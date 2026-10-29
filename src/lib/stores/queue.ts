import { readable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import {
    getQueue,
    type QueueView,
    type QueuedTrack,
    type PlaybackState,
} from "#lib/api.ts";

export const QUEUE_PAGE_SIZE = 100;
const MAX_PAGES = 6;
interface QueueState {
    queue: QueueView | null;
    pages: ReadonlyMap<number, (QueuedTrack | null)[]>;
    error: string | null;
}

// One in-flight read per mounted panel. Cache size and IPC payloads are bounded
// independently of queue length; invalidated reads can never repopulate it.
export function createQueueStore() {
    let requestRange = (_start: number, _end: number) => {};
    let retry = () => {};
    const store = readable<QueueState>(
        { queue: null, pages: new Map(), error: null },
        (set) => {
            let disposed = false;
            let connected = false;
            let running = false;
            let dirty = true;
            let generation = 0;
            let range: [number, number] | null = null;
            let lastTrackId: number | null | undefined;
            let queue: QueueView | null = null;
            let error: string | null = null;
            const pages = new Map<number, (QueuedTrack | null)[]>();
            const failed = new Set<number>();
            const unlisteners: Array<() => void> = [];
            const publish = () => set({ queue, pages: new Map(pages), error });
            publish();

            function neededPage(): number | undefined {
                if (!range || !queue) return;
                for (
                    let offset =
                        Math.floor(range[0] / QUEUE_PAGE_SIZE) *
                        QUEUE_PAGE_SIZE;
                    offset < Math.min(queue.total, range[1]);
                    offset += QUEUE_PAGE_SIZE
                ) {
                    if (!pages.has(offset) && !failed.has(offset))
                        return offset;
                }
            }

            async function pump() {
                if (!connected || running || disposed) return;
                running = true;
                try {
                    while (!disposed) {
                        const refresh = dirty;
                        const offset = refresh
                            ? range
                                ? Math.floor(range[0] / QUEUE_PAGE_SIZE) *
                                  QUEUE_PAGE_SIZE
                                : undefined
                            : neededPage();
                        if (!refresh && offset === undefined) break;
                        dirty = false;
                        const current = generation;
                        try {
                            const next = await getQueue(
                                offset,
                                QUEUE_PAGE_SIZE,
                            );
                            if (disposed || current !== generation) continue;
                            if (queue?.revision !== next.revision) {
                                pages.clear();
                                failed.clear();
                            }
                            queue = next;
                            // Other visible pages can still be failed. Keep the
                            // retry affordance until a refresh clears them.
                            if (failed.size === 0) error = null;
                            pages.delete(next.offset);
                            pages.set(next.offset, next.tracks);
                            while (pages.size > MAX_PAGES)
                                pages.delete(pages.keys().next().value!);
                            range ??= [
                                next.offset,
                                next.offset + QUEUE_PAGE_SIZE,
                            ];
                            publish();
                        } catch (cause) {
                            if (disposed || current !== generation) continue;
                            if (offset !== undefined) {
                                failed.add(offset);
                                while (failed.size > MAX_PAGES)
                                    failed.delete(
                                        failed.values().next().value!,
                                    );
                            }
                            error = String(cause);
                            publish();
                        }
                    }
                } finally {
                    running = false;
                }
            }

            function refresh() {
                if (disposed) return;
                generation++;
                dirty = true;
                failed.clear();
                void pump();
            }
            retry = refresh;
            requestRange = (start, end) => {
                range = [
                    Math.max(0, start),
                    Math.max(0, Math.min(end, start + QUEUE_PAGE_SIZE * 3)),
                ];
                // Protect visible pages from eviction while scrolling across boundaries.
                for (const [offset, tracks] of [...pages]) {
                    if (
                        offset < range[1] &&
                        offset + QUEUE_PAGE_SIZE > range[0]
                    ) {
                        pages.delete(offset);
                        pages.set(offset, tracks);
                    }
                }
                void pump();
            };

            async function connect() {
                const registrations = [
                    listen("queue-changed", refresh),
                    listen<PlaybackState>(
                        "playback-state-changed",
                        ({ payload }) => {
                            const id = payload.current_track?.id ?? null;
                            if (id === lastTrackId) return;
                            lastTrackId = id;
                            refresh();
                        },
                    ),
                ];
                await Promise.all(
                    registrations.map(async (registration) => {
                        try {
                            const stop = await registration;
                            if (disposed) stop();
                            else unlisteners.push(stop);
                        } catch (cause) {
                            if (!disposed) {
                                error = String(cause);
                                publish();
                            }
                        }
                    }),
                );
                connected = true;
                await pump();
            }
            void connect();
            return () => {
                disposed = true;
                requestRange = () => {};
                retry = () => {};
                for (const stop of unlisteners) stop();
            };
        },
    );
    return {
        subscribe: store.subscribe,
        setRange: (start: number, end: number) => requestRange(start, end),
        retry: () => retry(),
    };
}
