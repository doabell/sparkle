<script lang="ts">
    import { tick, untrack, type Snippet } from "svelte";

    interface Props {
        count: number;
        children: Snippet<[number]>;
        rowHeight?: number;
        initialIndex?: number;
        ariaLabel?: string;
        onRange?: (start: number, end: number) => void;
    }
    let {
        count,
        children,
        rowHeight = 3.625,
        initialIndex = 0,
        ariaLabel = "Tracks",
        onRange,
    }: Props = $props();
    let host = $state<HTMLUListElement>();
    let probe = $state<HTMLLIElement>();
    let root = $state<HTMLElement | null>(null);
    let size = $state(58);
    let start = $state(0);
    let end = $state(20);
    let focused = $state<number | null>(null);
    let schedule = () => {};
    let navigation = 0;
    const overscan = 6;

    let indices = $derived.by(() => {
        const result = Array.from(
            { length: Math.max(0, Math.min(count, end) - start) },
            (_, i) => start + i,
        );
        // Keep the focused row alive when the user scrolls with the mouse.
        if (focused !== null && focused < count && !result.includes(focused))
            result.push(focused);
        return result.sort((a, b) => a - b);
    });

    export function topForIndex(index: number): number {
        return (host?.getBoundingClientRect().top ?? 0) + index * size;
    }

    export function scrollToIndex(index: number, center = false) {
        if (!root || !host) return;
        const offset = center ? (root.clientHeight - size) / 2 : 16;
        root.scrollTop +=
            topForIndex(Math.max(0, Math.min(count - 1, index))) -
            root.getBoundingClientRect().top -
            offset;
        schedule();
    }

    $effect(() => {
        const node = host;
        const measure = probe;
        if (!node || !measure) return;
        const scroller =
            node.closest<HTMLElement>("[data-virtual-scroll], .content") ??
            node.parentElement;
        if (!scroller) return;
        root = scroller;
        let frame = 0;
        let disposed = false;
        const update = () => {
            frame = 0;
            if (disposed) return;
            size = measure.getBoundingClientRect().height || 58;
            const top =
                scroller.getBoundingClientRect().top -
                node.getBoundingClientRect().top;
            start = Math.max(
                0,
                Math.min(count - 1, Math.floor(top / size) - overscan),
            );
            end = Math.min(
                count,
                Math.max(
                    start + 1,
                    Math.ceil((top + scroller.clientHeight) / size) + overscan,
                ),
            );
        };
        schedule = () => {
            if (!frame && !disposed) frame = requestAnimationFrame(update);
        };
        const observer = new ResizeObserver(schedule);
        observer.observe(scroller);
        observer.observe(node);
        observer.observe(measure);
        scroller.addEventListener("scroll", schedule, { passive: true });
        window.addEventListener("resize", schedule);
        untrack(() => {
            update();
            if (initialIndex > 0) scrollToIndex(initialIndex, true);
        });
        return () => {
            disposed = true;
            navigation++;
            observer.disconnect();
            scroller.removeEventListener("scroll", schedule);
            window.removeEventListener("resize", schedule);
            cancelAnimationFrame(frame);
        };
    });

    $effect(() => {
        count;
        rowHeight;
        schedule();
    });
    $effect(() => {
        onRange?.(start, Math.min(count, end));
    });

    function focusChanged(event: FocusEvent) {
        const row = (event.target as HTMLElement).closest<HTMLElement>(
            "[data-virtual-index]",
        );
        focused = row ? Number(row.dataset.virtualIndex) : null;
    }
    function focusLeft(event: FocusEvent) {
        if (!host?.contains(event.relatedTarget as Node | null)) focused = null;
    }
    async function keydown(event: KeyboardEvent) {
        const target = event.target as HTMLElement;
        // Only the row's primary action owns list navigation, not its menus.
        if (
            !target.matches(".row-hit-area, .queue-row") ||
            event.altKey ||
            event.ctrlKey ||
            event.metaKey ||
            event.shiftKey
        )
            return;
        const current = Number(
            target.closest<HTMLElement>("[data-virtual-index]")?.dataset
                .virtualIndex,
        );
        let next: number;
        if (event.key === "ArrowDown") next = current + 1;
        else if (event.key === "ArrowUp") next = current - 1;
        else if (event.key === "Home") next = 0;
        else if (event.key === "End") next = count - 1;
        else return;
        event.preventDefault();
        const request = ++navigation;
        // Search inward from Home/End even when the endpoint is a heading.
        const step = event.key === "Home" || event.key === "ArrowDown" ? 1 : -1;
        // Skip group headings and disabled actions. Queue placeholders remain
        // focusable so navigating to them can load their page.
        while (next >= 0 && next < count) {
            focused = next;
            await tick();
            if (request !== navigation || !host?.isConnected) return;
            const action = host?.querySelector<HTMLButtonElement>(
                `[data-virtual-index="${next}"] .row-hit-area, [data-virtual-index="${next}"] .queue-row:not(:disabled)`,
            );
            if (action) {
                scrollToIndex(next, true);
                action.focus({ preventScroll: true });
                return;
            }
            next += step;
        }
        focused = current;
    }
</script>

<!-- Keyboard events are delegated from the row buttons; the list itself is not focusable. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<ul
    class="virtual-list"
    bind:this={host}
    aria-label={ariaLabel}
    style:height={`${count * size}px`}
    style:--virtual-row-height={`${rowHeight}rem`}
    onfocusin={focusChanged}
    onfocusout={focusLeft}
    onkeydown={keydown}
>
    <li class="measure" bind:this={probe} aria-hidden="true"></li>
    {#each indices as index (index)}
        <li
            class="virtual-row"
            data-virtual-index={index}
            aria-posinset={index + 1}
            aria-setsize={count}
            style:top={`${index * size}px`}
        >
            {@render children(index)}
        </li>
    {/each}
</ul>

<style>
    .virtual-list {
        position: relative;
        list-style: none;
        padding: 0;
        margin: 0;
        flex: none;
        overflow-anchor: none;
    }
    .measure {
        position: absolute;
        height: var(--virtual-row-height);
        width: 0;
        visibility: hidden;
        pointer-events: none;
    }
    .virtual-row {
        position: absolute;
        left: 0;
        right: 0;
        height: var(--virtual-row-height);
        padding-bottom: 2px;
        box-sizing: border-box;
    }
    .virtual-row:focus-within {
        z-index: 2;
    }
    .virtual-row :global(.track-row) {
        height: 100%;
        min-width: 0;
        box-sizing: border-box;
    }
</style>
