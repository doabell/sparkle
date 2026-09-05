<script lang="ts">
    import { getQueue, type QueueView, type Track } from "$lib/api";
    import { playQueueIndex } from "$lib/stores/playback";
    import { formatTime } from "$lib/utils/formatTime";
    import Artwork from "$lib/components/Artwork.svelte";
    import { listen } from "@tauri-apps/api/event";
    import { onMount, tick } from "svelte";
    import { addToast } from "$lib/stores/toast";

    interface Props {
        onClose: () => void;
    }

    let { onClose }: Props = $props();

    let queue = $state<QueueView | null>(null);
    let panelRef = $state<HTMLDivElement | null>(null);
    let scrollRef = $state<HTMLDivElement | null>(null);
    let currentRowRef = $state<HTMLDivElement | null>(null);

    let currentTrack = $derived.by<Track | null>(() => {
        if (!queue || queue.current_pos === null) return null;
        return queue.tracks[queue.current_pos] ?? null;
    });

    let history = $derived.by<{ track: Track; pos: number }[]>(() => {
        if (!queue || queue.current_pos === null) return [];
        return queue.tracks
            .slice(0, queue.current_pos)
            .map((track, i) => ({ track, pos: i }));
    });

    let upNext = $derived.by<{ track: Track; pos: number }[]>(() => {
        if (!queue || queue.current_pos === null) return [];
        const start = queue.current_pos + 1;
        return queue.tracks
            .slice(start)
            .map((track, i) => ({ track, pos: start + i }));
    });

    async function refresh(scrollToCurrent = false) {
        try {
            queue = await getQueue();
            if (scrollToCurrent) {
                await tick();
                currentRowRef?.scrollIntoView({ block: "center" });
            }
        } catch (e) {
            console.error("Failed to load queue:", e);
        }
    }

    onMount(() => {
        refresh(true);
        const unlisteners: Array<() => void> = [];
        (async () => {
            unlisteners.push(await listen("queue-changed", () => refresh()));
            unlisteners.push(
                await listen("playback-state-changed", () => refresh()),
            );
        })();
        return () => {
            for (const unlisten of unlisteners) unlisten();
        };
    });

    function handleWindowClick(e: MouseEvent) {
        if (panelRef && !panelRef.contains(e.target as Node)) {
            onClose();
        }
    }

    function handleKeydown(e: KeyboardEvent) {
        if (e.key === "Escape") {
            e.stopPropagation();
            onClose();
        }
    }

    async function jumpTo(pos: number) {
        try {
            await playQueueIndex(pos);
        } catch (e) {
            addToast(String(e), "error");
        }
    }
</script>

<svelte:window onclick={handleWindowClick} onkeydown={handleKeydown} />

<div
    class="queue-panel"
    bind:this={panelRef}
    role="dialog"
    aria-label="Play queue"
>
    <div class="queue-header">
        <h2 class="queue-title">Queue</h2>
        <button class="close-btn" aria-label="Close queue" onclick={onClose}>
            <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
            >
                <path d="M18 6 6 18" />
                <path d="m6 6 12 12" />
            </svg>
        </button>
    </div>

    {#if queue === null}
        <p class="queue-status">Loading...</p>
    {:else if !currentTrack}
        <p class="queue-status">
            The queue is empty. Play a song, album, or playlist to build it.
        </p>
    {:else}
        <div class="queue-scroll" bind:this={scrollRef}>
            {#if history.length > 0}
                <div class="queue-section-label">History</div>
                <ul class="queue-list">
                    {#each history as item (item.pos)}
                        <li>
                            <button
                                class="queue-row played"
                                onclick={() => jumpTo(item.pos)}
                            >
                                <span class="row-cover"
                                    ><Artwork
                                        albumId={item.track.album_id}
                                        alt=""
                                        class="cover-img"
                                    /></span
                                >
                                <span class="row-text">
                                    <span class="row-title ellipsis"
                                        >{item.track.title ?? "Unknown"}</span
                                    >
                                    <span class="row-artist ellipsis"
                                        >{item.track.artist_names?.join(", ") ??
                                            ""}</span
                                    >
                                </span>
                                <span class="row-duration"
                                    >{formatTime(
                                        item.track.duration_ms ?? 0,
                                    )}</span
                                >
                            </button>
                        </li>
                    {/each}
                </ul>
            {/if}

            <div class="queue-section-label">Now Playing</div>
            <div class="queue-row current" bind:this={currentRowRef}>
                <span class="row-cover"
                    ><Artwork
                        albumId={currentTrack.album_id}
                        alt=""
                        class="cover-img"
                    /></span
                >
                <span class="row-text">
                    <span class="row-title ellipsis"
                        >{currentTrack.title ?? "Unknown"}</span
                    >
                    <span class="row-artist ellipsis"
                        >{currentTrack.artist_names?.join(", ") ?? ""}</span
                    >
                </span>
                <span class="row-eq" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="currentColor">
                        <path
                            d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"
                        />
                    </svg>
                </span>
            </div>

            {#if upNext.length > 0}
                <div class="queue-section-label">Up Next · {upNext.length}</div>
                <ul class="queue-list">
                    {#each upNext as item (item.pos)}
                        <li>
                            <button
                                class="queue-row"
                                onclick={() => jumpTo(item.pos)}
                            >
                                <span class="row-cover"
                                    ><Artwork
                                        albumId={item.track.album_id}
                                        alt=""
                                        class="cover-img"
                                    /></span
                                >
                                <span class="row-text">
                                    <span class="row-title ellipsis"
                                        >{item.track.title ?? "Unknown"}</span
                                    >
                                    <span class="row-artist ellipsis"
                                        >{item.track.artist_names?.join(", ") ??
                                            ""}</span
                                    >
                                </span>
                                <span class="row-duration"
                                    >{formatTime(
                                        item.track.duration_ms ?? 0,
                                    )}</span
                                >
                            </button>
                        </li>
                    {/each}
                </ul>
            {:else}
                <p class="queue-status">End of queue.</p>
            {/if}
        </div>
    {/if}
</div>

<style>
    .queue-panel {
        position: absolute;
        bottom: calc(100% + var(--spacing-sm));
        right: var(--spacing-lg);
        z-index: 50;
        width: 22rem;
        max-width: calc(100vw - 2 * var(--spacing-lg));
        max-height: 24rem;
        display: flex;
        flex-direction: column;
        background: rgba(var(--color-surface-rgb), 0.92);
        backdrop-filter: blur(24px) saturate(1.8);
        -webkit-backdrop-filter: blur(24px) saturate(1.8);
        border: 1px solid rgba(255, 255, 255, 0.1);
        border-radius: var(--radius-xl);
        box-shadow: var(--shadow-lg);
        overflow: hidden;
    }

    .queue-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: var(--spacing-md) var(--spacing-lg) var(--spacing-sm);
    }

    .queue-title {
        font-size: var(--font-size-base);
        font-weight: var(--font-weight-bold);
        letter-spacing: -0.01em;
    }

    .close-btn {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 1.5rem;
        height: 1.5rem;
        border-radius: var(--radius-full);
        color: var(--color-text-muted);
        transition:
            color var(--transition-fast),
            background-color var(--transition-fast);
    }

    .close-btn:hover {
        color: var(--color-text);
        background-color: var(--interactive-hover);
    }

    .close-btn svg {
        width: 0.875rem;
        height: 0.875rem;
    }

    .queue-scroll {
        overflow-y: auto;
        padding: 0 var(--spacing-sm) var(--spacing-sm);
    }

    .queue-section-label {
        padding: var(--spacing-sm) var(--spacing-md) var(--spacing-xs);
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-semibold);
        letter-spacing: normal;
        color: var(--color-text-muted);
    }

    .queue-list {
        display: flex;
        flex-direction: column;
    }

    .queue-row {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        width: 100%;
        padding: var(--spacing-xs) var(--spacing-md);
        border-radius: var(--radius);
        text-align: left;
        transition:
            background-color var(--transition-fast),
            opacity var(--transition-fast);
    }

    button.queue-row {
        cursor: pointer;
    }

    button.queue-row:hover {
        background-color: var(--interactive-hover);
    }

    .queue-row.played {
        opacity: 0.45;
    }

    .queue-row.played:hover {
        opacity: 0.8;
    }

    .queue-row.current {
        background-color: color-mix(
            in srgb,
            var(--color-accent-subtle) 42%,
            transparent
        );
    }

    .row-eq {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 1rem;
        height: 1rem;
        flex-shrink: 0;
        color: var(--color-accent-graphic);
    }

    .row-eq svg {
        width: 100%;
        height: 100%;
    }

    .row-cover {
        width: 2rem;
        height: 2rem;
        flex-shrink: 0;
        border-radius: var(--radius-sm);
        overflow: hidden;
    }

    .row-cover :global(.cover-img) {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }

    .row-text {
        display: flex;
        flex-direction: column;
        gap: 1px;
        min-width: 0;
        flex: 1;
    }

    .row-title {
        font-size: var(--font-size-sm);
        font-weight: var(--font-weight-medium);
        color: var(--color-text);
    }

    .queue-row.current .row-title {
        color: var(--color-accent-content);
    }

    .row-artist {
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
    }

    .row-duration {
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
        font-variant-numeric: tabular-nums;
        flex-shrink: 0;
    }

    .queue-status {
        padding: var(--spacing-md) var(--spacing-lg) var(--spacing-lg);
        font-size: var(--font-size-sm);
        color: var(--color-text-muted);
    }
</style>
