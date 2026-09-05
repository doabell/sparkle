<script lang="ts">
    import {
        scrollbarGeometry,
        scrollbarKeyTarget,
        scrollTopFromThumb,
    } from "$lib/utils/scrollbar";

    let {
        target,
        enabled = true,
    }: { target: HTMLElement | null; enabled?: boolean } = $props();
    let rail = $state<HTMLDivElement | null>(null);
    let metrics = $state(scrollbarGeometry(0, 0, 0, 0));
    let dragging = $state(false);
    let grabOffset = 0;

    function measure() {
        if (!target || !rail || !enabled) return;
        metrics = scrollbarGeometry(
            target.clientHeight,
            target.scrollHeight,
            target.scrollTop,
            rail.clientHeight,
        );
    }

    $effect(() => {
        const scroller = target;
        const track = rail;
        if (!scroller || !track || !enabled) {
            metrics = scrollbarGeometry(0, 0, 0, 0);
            return;
        }
        let frame: number | null = null;
        const schedule = () => {
            if (frame !== null) return;
            frame = requestAnimationFrame(() => {
                frame = null;
                measure();
            });
        };
        const sizes = new ResizeObserver(schedule);
        sizes.observe(scroller);
        sizes.observe(track);
        const children = new Set<Element>();
        const observeContent = () => {
            for (const child of children) {
                if (child.parentElement !== scroller) {
                    sizes.unobserve(child);
                    children.delete(child);
                }
            }
            for (const child of scroller.children) {
                if (!children.has(child)) {
                    children.add(child);
                    sizes.observe(child);
                }
            }
            schedule();
        };
        const mutations = new MutationObserver(observeContent);
        mutations.observe(scroller, {
            childList: true,
            subtree: true,
            characterData: true,
            attributes: true,
            attributeFilter: ["class", "style", "hidden"],
        });
        const wheel = (event: WheelEvent) => {
            if (event.ctrlKey) return;
            const unit =
                event.deltaMode === 1
                    ? 16
                    : event.deltaMode === 2
                      ? scroller.clientHeight
                      : 1;
            event.preventDefault();
            scroller.scrollBy({
                top: event.deltaY * unit,
                left: event.deltaX * unit,
                behavior: "instant",
            });
        };
        scroller.addEventListener("scroll", schedule, { passive: true });
        scroller.addEventListener("load", schedule, true);
        track.addEventListener("wheel", wheel, { passive: false });
        observeContent();
        return () => {
            if (frame !== null) cancelAnimationFrame(frame);
            mutations.disconnect();
            sizes.disconnect();
            scroller.removeEventListener("scroll", schedule);
            scroller.removeEventListener("load", schedule, true);
            track.removeEventListener("wheel", wheel);
            dragging = false;
        };
    });

    function pointerDown(event: PointerEvent) {
        if (event.button !== 0 || !target || !rail || metrics.maxScroll <= 0)
            return;
        event.preventDefault();
        measure();
        rail.focus({ preventScroll: true });
        const y = event.clientY - rail.getBoundingClientRect().top;
        if (
            y < metrics.thumbTop ||
            y > metrics.thumbTop + metrics.thumbHeight
        ) {
            target.scrollBy({
                top:
                    (y < metrics.thumbTop ? -1 : 1) * target.clientHeight * 0.9,
                behavior: "instant",
            });
            measure();
            return;
        }
        grabOffset = y - metrics.thumbTop;
        dragging = true;
        rail.setPointerCapture(event.pointerId);
    }

    function pointerMove(event: PointerEvent) {
        if (!dragging || !target || !rail) return;
        target.scrollTop = scrollTopFromThumb(
            event.clientY - rail.getBoundingClientRect().top - grabOffset,
            metrics.travel,
            metrics.maxScroll,
        );
        measure();
    }

    function pointerUp(event: PointerEvent) {
        dragging = false;
        if (rail?.hasPointerCapture(event.pointerId))
            rail.releasePointerCapture(event.pointerId);
    }

    function keyDown(event: KeyboardEvent) {
        if (!target) return;
        const next = scrollbarKeyTarget(
            event.key,
            target.scrollTop,
            target.clientHeight,
            metrics.maxScroll,
            event.shiftKey,
        );
        if (next === null) return;
        event.preventDefault();
        event.stopPropagation();
        target.scrollTop = next;
        measure();
    }
</script>

<div
    bind:this={rail}
    class="page-scrollbar"
    class:scrollable={enabled && metrics.maxScroll > 0}
    class:dragging
    role="scrollbar"
    aria-label="Page scroll"
    aria-controls="page-content"
    aria-orientation="vertical"
    aria-valuemin={0}
    aria-valuemax={Math.round(metrics.maxScroll)}
    aria-valuenow={Math.round(metrics.position)}
    tabindex={enabled && metrics.maxScroll > 0 ? 0 : -1}
    onpointerdown={pointerDown}
    onpointermove={pointerMove}
    onpointerup={pointerUp}
    onpointercancel={pointerUp}
    onlostpointercapture={() => (dragging = false)}
    onkeydown={keyDown}
>
    <span
        class="thumb"
        aria-hidden="true"
        style:height={`${metrics.thumbHeight}px`}
        style:transform={`translateY(${metrics.thumbTop}px)`}
    ></span>
</div>

<style>
    .page-scrollbar {
        grid-area: content;
        position: relative;
        z-index: 60;
        justify-self: end;
        align-self: stretch;
        width: 0.875rem;
        min-height: 0;
        margin-top: calc(var(--window-chrome-height) + var(--spacing-sm));
        margin-bottom: var(--spacing-sm);
        visibility: hidden;
        pointer-events: none;
        touch-action: none;
        user-select: none;
    }

    .page-scrollbar.scrollable {
        visibility: visible;
        pointer-events: auto;
    }

    .thumb {
        position: absolute;
        top: 0;
        right: 3px;
        width: 8px;
        pointer-events: none;
    }

    .thumb::before {
        content: "";
        position: absolute;
        inset: 0;
        border-radius: var(--radius-full);
        background: var(--color-text-secondary);
        opacity: 0.75;
        transform: scaleX(0.5);
        transition:
            opacity var(--transition-fast),
            transform var(--transition-fast);
    }

    .page-scrollbar:hover .thumb::before,
    .page-scrollbar:focus-visible .thumb::before,
    .page-scrollbar.dragging .thumb::before {
        opacity: 1;
        transform: scaleX(0.75);
    }

    .page-scrollbar:focus-visible .thumb::before {
        outline: 2px solid var(--color-accent-focus);
        outline-offset: 2px;
    }

    @media (forced-colors: active) {
        .thumb::before {
            background: CanvasText;
            opacity: 1;
            forced-color-adjust: none;
        }
    }
</style>
