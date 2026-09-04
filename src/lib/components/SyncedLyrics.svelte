<script lang="ts">
    import {
        activeLineIndex,
        anticipatedLineIndex,
        normalizeLyricSpacing,
        parseLrc,
    } from "$lib/utils/lrc";
    import { onMount } from "svelte";
    import type { Action } from "svelte/action";

    interface Props {
        syncedText?: string;
        plainText?: string;
        currentTimeMs: number;
        offsetMs?: number;
        onSeek?: (timeMs: number) => void;
    }

    let {
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

        return () => {
            mediaQuery.removeEventListener("change", updateReducedMotion);
            motionSettingObserver.disconnect();
        };
    });

    const scrollIntoCenter: Action<
        HTMLElement,
        { active: boolean; animate: boolean }
    > = (node, params) => {
        let wasActive = false;

        function update(p: typeof params) {
            if (p.active && !wasActive) {
                node.scrollIntoView({
                    behavior: p.animate && hasCenteredLine ? "smooth" : "auto",
                    block: "center",
                });
                hasCenteredLine = true;
            }
            wasActive = p.active;
        }

        update(params);
        return { update };
    };

    function handleLineClick(timeMs: number) {
        onSeek?.(timeMs + offsetMs);
    }
</script>

<div class="lyrics-container">
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
        max-height: 70vh;
        overflow-y: auto;
        padding: var(--spacing-xl);
        background: transparent;
        border: none;
        border-radius: var(--radius-lg);
        text-align: center;
    }

    .lines {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
        padding: 30vh 0;
    }

    .lyrics-line {
        display: block;
        width: 100%;
        background: transparent;
        border: none;
        padding: var(--spacing-sm) 0;
        color: var(--color-text-secondary);
        font-size: var(--font-size-2xl);
        line-height: 1.5;
        text-align: center;
        line-break: auto;
        overflow-wrap: anywhere;
        text-wrap: balance;
        transform: scale(0.833333);
        transition:
            color var(--transition-base),
            transform var(--transition-base),
            font-weight var(--transition-base),
            text-shadow var(--transition-base);
        cursor: pointer;
    }

    .lyrics-line:hover:not(:disabled) {
        color: var(--color-text);
    }

    .lyrics-line.active {
        color: var(--color-text);
        font-weight: var(--font-weight-bold);
        transform: scale(1.05);
        text-shadow: 0 2px 16px rgba(0, 0, 0, 0.6);
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
