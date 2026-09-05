<script lang="ts">
    import {
        activeLineIndex,
        anticipatedLineIndex,
        LYRIC_TRANSITION_DURATION_MS,
        normalizeLyricSpacing,
        parseLrc,
    } from "$lib/utils/lrc";
    import { onMount } from "svelte";
    import type { Action } from "svelte/action";

    interface Props {
        fontFamily?: string;
        syncedText?: string;
        plainText?: string;
        currentTimeMs: number;
        offsetMs?: number;
        onSeek?: (timeMs: number) => void;
    }

    let {
        fontFamily = "inherit",
        syncedText = "",
        plainText = "",
        currentTimeMs = 0,
        offsetMs = 0,
        onSeek,
    }: Props = $props();

    let parsedLines = $derived(parseLrc(syncedText));
    let hasTimestamps = $derived(parsedLines.length > 0);
    // Stay conservative until the client preference is known. This prevents a
    // reduced-motion user from seeing the upcoming line change early on mount.
    let reducedMotion = $state(true);
    let hasCenteredLine = false;
    let lyricsContainer: HTMLDivElement;

    function centerLine(node: HTMLElement, animate: boolean) {
        const container = node.closest<HTMLElement>(".lyrics-container");
        if (!container) return;
        // Scroll only the lyric viewport. scrollIntoView can also move the
        // surrounding page and dislodge the artwork or window chrome.
        container.scrollTo({
            top:
                node.offsetTop +
                node.offsetHeight / 2 -
                container.clientHeight / 2,
            behavior: animate ? "smooth" : "auto",
        });
    }

    let activeIndex = $derived.by(() => {
        if (!hasTimestamps) return -1;
        const adjusted = currentTimeMs - offsetMs;
        return reducedMotion
            ? activeLineIndex(parsedLines, adjusted)
            : anticipatedLineIndex(parsedLines, adjusted);
    });

    $effect.pre(() => {
        syncedText;
        hasCenteredLine = false;
    });

    onMount(() => {
        const mediaQuery = window.matchMedia(
            "(prefers-reduced-motion: reduce)",
        );
        const root = document.documentElement;
        const updateReducedMotion = () => {
            reducedMotion =
                mediaQuery.matches || root.dataset.motion !== "full";
        };
        const motionSettingObserver = new MutationObserver(updateReducedMotion);

        updateReducedMotion();
        mediaQuery.addEventListener("change", updateReducedMotion);
        motionSettingObserver.observe(root, {
            attributes: true,
            attributeFilter: ["data-motion"],
        });

        const sizeObserver = new ResizeObserver(() => {
            const active = lyricsContainer.querySelector<HTMLElement>(
                ".lyrics-line.active",
            );
            if (active) centerLine(active, false);
        });
        sizeObserver.observe(lyricsContainer);
        const lines = lyricsContainer.querySelector(".lines");
        if (lines) sizeObserver.observe(lines);

        return () => {
            mediaQuery.removeEventListener("change", updateReducedMotion);
            motionSettingObserver.disconnect();
            sizeObserver.disconnect();
        };
    });

    const scrollIntoCenter: Action<
        HTMLElement,
        { active: boolean; animate: boolean; source: string }
    > = (node, params) => {
        let wasActive = false;
        let wasAnimated = false;
        let previousSource = params.source;

        function update(p: typeof params) {
            if (
                p.active &&
                (!wasActive ||
                    previousSource !== p.source ||
                    (wasAnimated && !p.animate))
            ) {
                centerLine(node, p.animate && hasCenteredLine);
                hasCenteredLine = true;
            }
            wasActive = p.active;
            wasAnimated = p.animate;
            previousSource = p.source;
        }

        update(params);
        return { update };
    };

    function handleLineClick(timeMs: number) {
        onSeek?.(timeMs + offsetMs);
    }
</script>

<div
    bind:this={lyricsContainer}
    class="lyrics-container"
    style:font-family={fontFamily}
    style:--lyrics-transition-duration={`${LYRIC_TRANSITION_DURATION_MS}ms`}
>
    {#if hasTimestamps}
        <div class="lines synced">
            {#each parsedLines as line, index (line.timeMs)}
                <button
                    type="button"
                    class="lyrics-line"
                    class:active={index === activeIndex}
                    use:scrollIntoCenter={{
                        active: index === activeIndex,
                        animate: !reducedMotion,
                        source: syncedText,
                    }}
                    onclick={() => handleLineClick(line.timeMs)}
                    disabled={!onSeek}
                >
                    {normalizeLyricSpacing(line.text)}
                </button>
            {/each}
        </div>
    {:else if plainText}
        <div class="lines plain">
            {#each plainText.split(/\r?\n/) as line, index (index)}
                <p class="lyrics-line plain">{normalizeLyricSpacing(line)}</p>
            {/each}
        </div>
    {:else}
        <p class="empty">No lyrics found.</p>
    {/if}
</div>

<style>
    .lyrics-container {
        position: relative;
        flex: 1;
        min-height: 0;
        container-type: size;
        overflow-y: auto;
        overscroll-behavior: contain;
        scrollbar-gutter: stable both-edges;
        padding: var(--np-lyrics-container-padding, var(--spacing-xl));
        background: var(--np-lyrics-background, transparent);
        border: var(--np-lyrics-border, none);
        border-radius: var(--np-lyrics-radius, var(--radius-lg));
        text-align: var(--np-lyrics-align, center);
    }

    .lines {
        display: flex;
        flex-direction: column;
        gap: var(--np-lyrics-lines-gap, var(--spacing-md));
        padding: var(--spacing-md) 0;
    }

    .lines.synced {
        /* Enough space to center the first and last lines, based on this
           panel's height, not the full window behind the player. */
        padding: 50cqh 0;
    }

    .lyrics-line {
        display: block;
        width: 100%;
        background: transparent;
        border: none;
        padding: var(--spacing-sm) 0;
        color: var(--np-lyrics-line-color, var(--color-text-secondary));
        font-size: var(--np-lyrics-line-size, var(--font-size-2xl));
        line-height: var(--np-lyrics-line-height, 1.5);
        text-align: var(--np-lyrics-line-align, center);
        line-break: auto;
        overflow-wrap: anywhere;
        text-wrap: balance;
        transform: scale(var(--np-lyrics-inactive-scale, 0.833333));
        transform-origin: var(--np-lyrics-transform-origin, center);
        transition:
            color var(--lyrics-transition-duration) var(--motion-ease-standard),
            transform var(--lyrics-transition-duration)
                var(--motion-ease-standard),
            text-shadow var(--lyrics-transition-duration)
                var(--motion-ease-standard),
            -webkit-text-stroke-width var(--lyrics-transition-duration)
                var(--motion-ease-standard);
        cursor: pointer;
    }

    .lines.synced .lyrics-line {
        font-weight: var(--font-weight-medium);
        -webkit-text-stroke: 0 currentColor;
    }

    .lyrics-line:hover:not(:disabled) {
        color: var(--color-text);
    }

    .lines.synced .lyrics-line.active {
        color: var(--np-lyrics-active-color, var(--color-text));
        /* Visual weight without changing glyph advances: activation must
           never change a line's wrapping, even with custom lyric fonts. */
        -webkit-text-stroke-width: 0.025em;
        transform: scale(var(--np-lyrics-active-scale, 1.05));
        text-shadow: var(
            --np-lyrics-active-shadow,
            0 2px 16px rgba(0, 0, 0, 0.6)
        );
    }

    .lyrics-line:disabled {
        cursor: default;
    }

    .lyrics-line.plain {
        text-align: left;
        cursor: default;
        font-size: var(--font-size-lg);
        transform: none;
    }

    .empty {
        color: var(--color-text-muted);
        text-align: center;
    }

    @media (prefers-reduced-motion: reduce) {
        .lyrics-line {
            transition: none;
        }
    }

    :global(:root[data-motion="reduced"]) .lyrics-line {
        transition: none;
    }
</style>
