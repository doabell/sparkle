<script lang="ts">
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

    interface Line {
        timeMs: number;
        text: string;
    }

    function parseLrc(text: string): Line[] {
        const lines: Line[] = [];
        for (const raw of text.split(/\r?\n/)) {
            const trimmed = raw.trim();
            if (!trimmed) continue;
            const tagRegex = /\[(\d+):(\d+(?:\.\d+)?)\]/g;
            const times: number[] = [];
            let match: RegExpExecArray | null;
            while ((match = tagRegex.exec(trimmed)) !== null) {
                const minutes = parseInt(match[1], 10);
                const seconds = parseFloat(match[2]);
                times.push(Math.round((minutes * 60 + seconds) * 1000));
            }
            const textOnly = trimmed.replace(tagRegex, "").trim();
            if (times.length === 0 || textOnly.length === 0) continue;
            for (const timeMs of times) {
                lines.push({ timeMs, text: textOnly });
            }
        }
        return lines.sort((a, b) => a.timeMs - b.timeMs);
    }

    let parsedLines = $derived(parseLrc(syncedText));
    let hasTimestamps = $derived(parsedLines.length > 0);

    let activeIndex = $derived.by(() => {
        if (!hasTimestamps) return -1;
        const adjusted = currentTimeMs - offsetMs;
        let index = -1;
        for (let i = 0; i < parsedLines.length; i++) {
            if (parsedLines[i].timeMs <= adjusted) {
                index = i;
            } else {
                break;
            }
        }
        return index;
    });

    const scrollIntoCenter: Action<HTMLElement, { active: boolean }> = (
        node,
        params,
    ) => {
        let wasActive = false;

        function update(p: typeof params) {
            if (p.active && !wasActive) {
                node.scrollIntoView({ behavior: "auto", block: "center" });
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
                    use:scrollIntoCenter={{ active: index === activeIndex }}
                    onclick={() => handleLineClick(line.timeMs)}
                    disabled={!onSeek}
                >
                    {line.text}
                </button>
            {/each}
        </div>
    {:else if plainText}
        <div class="lines plain">
            {#each plainText.split(/\r?\n/) as line, index (index)}
                <p class="lyrics-line plain">{line}</p>
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
        font-size: var(--font-size-xl);
        line-height: 1.5;
        text-align: center;
        /* East Asian text: allow a soft wrap around every character, including
       full-width punctuation (、；。), instead of refusing to break near
       them (kinsoku prohibitions leave ugly gaps or overflow). */
        line-break: anywhere;
        transition: none;
        cursor: pointer;
    }

    .lyrics-line:hover:not(:disabled) {
        color: var(--color-text);
    }

    .lyrics-line.active {
        color: var(--color-text);
        font-weight: var(--font-weight-bold);
        font-size: var(--font-size-2xl);
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
    }

    .empty {
        color: var(--color-text-muted);
        text-align: center;
    }
</style>
