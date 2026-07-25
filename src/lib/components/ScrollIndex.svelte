<script lang="ts">
    import { tick } from "svelte";

    export interface ScrollIndexEntry {
        key: string;
        label: string;
        index: number;
        title?: string;
        kind?: "bucket" | "group" | "year";
    }

    interface Props {
        entries: readonly ScrollIndexEntry[];
        anchorIdForEntry: (entry: ScrollIndexEntry) => string;
        ariaLabel?: string;
        onSelect?: (entry: ScrollIndexEntry) => void | Promise<void>;
    }

    let {
        entries,
        anchorIdForEntry,
        ariaLabel = "List index",
        onSelect,
    }: Props = $props();
    let nav = $state<HTMLElement | null>(null);
    let selectedKey = $state<string | null>(null);
    let activeKey: string | null = null;

    function updateSelectedEntry(
        root: HTMLElement,
        currentEntries: readonly ScrollIndexEntry[],
        getAnchorId: (entry: ScrollIndexEntry) => string,
    ) {
        const threshold = root.getBoundingClientRect().top + 24;
        let active = currentEntries[0];

        for (const entry of currentEntries) {
            const anchor = document.getElementById(getAnchorId(entry));
            if (anchor && anchor.getBoundingClientRect().top <= threshold) {
                active = entry;
            }
        }

        if (!active || active.key === activeKey) return;
        activeKey = active.key;
        selectedKey = active.key;

        const activeButton = Array.from(
            nav?.querySelectorAll<HTMLButtonElement>("button") ?? [],
        ).find((button) => button.dataset.indexKey === active.key);
        if (activeButton && nav) {
            nav.scrollTo({
                top:
                    activeButton.offsetTop -
                    nav.clientHeight / 2 +
                    activeButton.offsetHeight / 2,
                behavior: "smooth",
            });
        }
    }

    $effect(() => {
        const currentEntries = entries;
        const getAnchorId = anchorIdForEntry;
        const indexNav = nav;
        if (!indexNav || typeof window === "undefined") return;

        const root = indexNav.closest<HTMLElement>(".content");
        if (!root) return;

        let frame = 0;
        const update = () => {
            frame = 0;
            updateSelectedEntry(root, currentEntries, getAnchorId);
        };
        const scheduleUpdate = () => {
            if (frame) return;
            frame = window.requestAnimationFrame(update);
        };

        root.addEventListener("scroll", scheduleUpdate, { passive: true });
        window.addEventListener("resize", scheduleUpdate);
        const mutationObserver = new MutationObserver(scheduleUpdate);
        mutationObserver.observe(root, { childList: true, subtree: true });
        scheduleUpdate();

        return () => {
            root.removeEventListener("scroll", scheduleUpdate);
            window.removeEventListener("resize", scheduleUpdate);
            mutationObserver.disconnect();
            if (frame) window.cancelAnimationFrame(frame);
        };
    });

    async function findAnchor(
        entry: ScrollIndexEntry,
    ): Promise<HTMLElement | null> {
        for (let attempt = 0; attempt < 8; attempt += 1) {
            const anchor = document.getElementById(anchorIdForEntry(entry));
            if (anchor) return anchor;
            await tick();
            await new Promise<void>((resolve) =>
                requestAnimationFrame(() => resolve()),
            );
        }
        return null;
    }

    async function select(entry: ScrollIndexEntry) {
        activeKey = entry.key;
        selectedKey = entry.key;
        await onSelect?.(entry);

        const anchor = await findAnchor(entry);
        const root = anchor?.closest<HTMLElement>(".content");
        if (!anchor || !root) {
            anchor?.scrollIntoView({ behavior: "smooth", block: "start" });
            return;
        }

        const rootRect = root.getBoundingClientRect();
        const anchorOffset = anchor.getBoundingClientRect().top - rootRect.top;
        root.scrollTo({
            top: root.scrollTop + anchorOffset - 16,
            behavior: "smooth",
        });
    }
</script>

{#if entries.length > 1}
    <nav
        class="scroll-index"
        class:year-labels={entries.some((entry) => entry.kind === "year")}
        bind:this={nav}
        aria-label={ariaLabel}
    >
        {#each entries as entry (entry.key)}
            <button
                class:active={selectedKey === entry.key}
                data-index-key={entry.key}
                type="button"
                title={entry.title ?? `Jump to ${entry.label}`}
                aria-label={entry.title ?? `Jump to ${entry.label}`}
                aria-current={selectedKey === entry.key ? "true" : undefined}
                onclick={() => select(entry)}
            >
                {entry.label}
            </button>
        {/each}
    </nav>
{/if}

<style>
    .scroll-index {
        position: fixed;
        top: var(--spacing-xl);
        right: var(--spacing-md);
        z-index: 30;
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 1px;
        max-height: min(70vh, 36rem);
        max-width: min(10rem, 24vw);
        overflow-y: auto;
        padding: 0.25rem 0.15rem;
        border: none;
        background: transparent;
        box-shadow: none;
        backdrop-filter: none;
        scrollbar-width: none;
    }

    .scroll-index::-webkit-scrollbar {
        display: none;
    }

    .scroll-index button {
        min-width: 1.35rem;
        min-height: 1.15rem;
        max-width: min(10rem, 24vw);
        padding: 0 0.15rem;
        overflow: hidden;
        border-radius: var(--radius-full);
        color: var(--color-text-secondary);
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-semibold);
        line-height: 1.15;
        text-align: center;
        text-overflow: ellipsis;
        white-space: nowrap;
        width: max-content;
        transition:
            color var(--transition-fast),
            background-color var(--transition-fast),
            transform var(--transition-fast);
    }

    .scroll-index button:hover,
    .scroll-index button:focus-visible,
    .scroll-index button.active {
        color: var(--color-accent-content);
        background-color: transparent;
    }

    .scroll-index button:active {
        transform: scale(1.12);
    }

    .scroll-index.year-labels {
        /* Keep the centre of the year rail aligned with the one-character
           rail. The wider labels otherwise make the fixed rail appear to
           jump left when switching from alphabetical indexing. */
        width: 2.25rem;
        max-width: 2.25rem;
        right: calc(var(--spacing-md) - 0.45rem);
    }

    .scroll-index.year-labels button {
        width: 100%;
        max-width: 100%;
    }

    @media (max-width: 767px) {
        .scroll-index {
            right: var(--spacing-sm);
        }

        .scroll-index.year-labels {
            right: calc(var(--spacing-sm) - 0.45rem);
        }
    }
</style>
