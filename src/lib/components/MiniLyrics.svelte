<script lang="ts">
    import { onMount } from "svelte";
    import { listen } from "@tauri-apps/api/event";
    import { getLyrics, LYRICS_CHANGED_EVENT } from "$lib/api";
    import {
        activeLineIndex,
        lrcPlainText,
        parseLrc,
        type LrcLine,
    } from "$lib/utils/lrc";

    let {
        trackId,
        positionMs,
        offsetMs = 0,
        firstLine = null,
    }: {
        trackId: number | null;
        positionMs: number;
        offsetMs?: number;
        firstLine?: string | null;
    } = $props();
    let lines = $state<LrcLine[]>([]);
    let plainText = $state("");
    let loading = $state(false);
    let revision = $state(0);
    let loadedTrackId = $state<number | null>(null);
    let index = $derived(activeLineIndex(lines, positionMs - offsetMs));
    let current = $derived(
        loadedTrackId === trackId ? (lines[index]?.text ?? "") : "",
    );

    let lyricText = $derived(
        current ||
            (loadedTrackId === trackId
                ? (plainText.split(/\r?\n/).find((line) => line.trim()) ?? "")
                : loading
                  ? (firstLine ?? "")
                  : ""),
    );

    $effect(() => {
        const id = trackId;
        revision;
        let disposed = false;
        loadedTrackId = null;
        lines = [];
        plainText = "";
        loading = id !== null;
        if (id !== null) {
            void getLyrics(id)
                .then((lyrics) => {
                    if (disposed) return;
                    lines = lyrics.synced_text
                        ? parseLrc(lyrics.synced_text)
                        : [];
                    plainText = lrcPlainText(
                        lyrics.plain_text || lyrics.synced_text || "",
                    );
                    loadedTrackId = id;
                })
                .catch(() => {
                    // Unavailable lyrics leave the metadata row empty.
                })
                .finally(() => {
                    if (!disposed) loading = false;
                });
        }
        return () => {
            disposed = true;
        };
    });

    onMount(() => {
        let disposed = false;
        let unlisten: (() => void) | undefined;
        const changed = (event: Event) => {
            if (
                (event as CustomEvent<{ trackId: number }>).detail?.trackId ===
                trackId
            )
                revision++;
        };
        window.addEventListener(LYRICS_CHANGED_EVENT, changed);
        void listen("online-settings-changed", () => {
            revision++;
        })
            .then((stop) => {
                if (disposed) stop();
                else unlisten = stop;
            })
            .catch(() => {
                /* Native settings events are unavailable in a web preview. */
            });
        return () => {
            disposed = true;
            unlisten?.();
            window.removeEventListener(LYRICS_CHANGED_EVENT, changed);
        };
    });
</script>

{#if lyricText}
    <span class="lyric-line ellipsis" title={lyricText}>{lyricText}</span>
{/if}

<style>
    .lyric-line {
        display: block;
        max-width: 100%;
        color: var(--color-text-secondary);
        font-size: 12px;
        line-height: 18px;
        font-weight: var(--font-weight-normal);
        text-align: left;
    }
</style>
