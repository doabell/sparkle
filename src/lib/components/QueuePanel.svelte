<script lang="ts">
    import { createQueueStore, QUEUE_PAGE_SIZE } from "#lib/stores/queue.ts";
    import { playQueueIndex } from "#lib/stores/playback.ts";
    import { formatTime } from "#lib/utils/formatTime.ts";
    import Artwork from "#lib/components/Artwork.svelte";
    import VirtualList from "#lib/components/VirtualList.svelte";
    import { addToast } from "#lib/stores/toast.ts";

    let { onClose }: { onClose: () => void } = $props();
    const queueStore = createQueueStore();
    let queue = $derived($queueStore.queue);
    let panelRef = $state<HTMLDivElement | null>(null);

    function handleWindowClick(e: MouseEvent) {
        if (panelRef && !panelRef.contains(e.target as Node)) onClose();
    }
    function handleKeydown(e: KeyboardEvent) {
        if (e.key === "Escape") {
            e.stopPropagation();
            onClose();
        }
    }
    async function jumpTo(pos: number, revision: number) {
        try {
            await playQueueIndex(pos, "ui", revision);
        } catch (e) {
            addToast(String(e), "error");
            queueStore.retry();
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

    {#if $queueStore.error}
        <p class="queue-status" role="alert">
            {$queueStore.error}
            <button onclick={queueStore.retry}>Retry</button>
        </p>
    {/if}
    {#if queue === null}
        {#if !$queueStore.error}<p class="queue-status">Loading...</p>{/if}
    {:else if queue.total === 0}
        <p class="queue-status">
            The queue is empty. Play a song, album, or playlist to build it.
        </p>
    {:else}
        <div class="queue-section-label">
            {queue.total} tracks · {queue.current_pos === null
                ? queue.total
                : queue.total - queue.current_pos - 1} up next
        </div>
        <div class="queue-scroll" data-virtual-scroll>
            <VirtualList
                count={queue.total}
                initialIndex={queue.current_pos ?? 0}
                rowHeight={3}
                ariaLabel="Play queue tracks"
                onRange={queueStore.setRange}
            >
                {#snippet children(pos)}
                    {@const offset =
                        Math.floor(pos / QUEUE_PAGE_SIZE) * QUEUE_PAGE_SIZE}
                    {@const track =
                        $queueStore.pages.get(offset)?.[pos - offset]}
                    <button
                        class="queue-row"
                        class:played={queue.current_pos !== null &&
                            pos < queue.current_pos}
                        class:current={pos === queue.current_pos}
                        aria-disabled={!track}
                        aria-current={pos === queue.current_pos
                            ? "true"
                            : undefined}
                        onclick={() => track && jumpTo(pos, queue.revision)}
                    >
                        <span class="row-cover"
                            >{#if track}<Artwork
                                    albumId={track.album_id}
                                    alt=""
                                    class="cover-img"
                                />{/if}</span
                        >
                        <span class="row-text">
                            <span class="row-title ellipsis"
                                >{track
                                    ? (track.title ?? "Unknown")
                                    : track === null
                                      ? "Unavailable track"
                                      : "Loading..."}</span
                            >
                            <span class="row-artist ellipsis"
                                >{pos === queue.current_pos
                                    ? "Now playing · "
                                    : ""}{track?.artist_names?.join(", ") ??
                                    ""}</span
                            >
                        </span>
                        {#if track}<span class="row-duration"
                                >{formatTime(track.duration_ms ?? 0)}</span
                            >{/if}
                    </button>
                {/snippet}
            </VirtualList>
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
            color var(--transition-feedback),
            background-color var(--transition-feedback);
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
        min-height: 0;
        height: 20rem;
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

    .queue-row {
        height: 100%;
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        width: 100%;
        padding: var(--spacing-xs) var(--spacing-md);
        border-radius: var(--radius);
        text-align: left;
        transition:
            background-color var(--transition-feedback),
            opacity var(--transition-fast);
    }

    button.queue-row {
        cursor: pointer;
    }

    button.queue-row[aria-disabled="true"] {
        cursor: default;
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
