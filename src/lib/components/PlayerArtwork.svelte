<script lang="ts">
    import { untrack, type Snippet } from "svelte";
    import { cachedImageToUrl } from "$lib/utils/base64";
    import type { CachedImage } from "$lib/api";

    let {
        image,
        alt,
        children,
    }: { image: CachedImage | null; alt: string; children: Snippet } = $props();

    const source = $derived(image ? cachedImageToUrl(image, "") : "");
    let shown = $state("");
    let images = $state<string[]>([]);
    let generation = 0;

    $effect(() => {
        const next = source;
        generation++;
        untrack(() => {
            if (!next) {
                shown = "";
                images = [];
            } else {
                images = shown && shown !== next ? [shown, next] : [next];
            }
        });
        return () => {
            generation++;
        };
    });

    async function ready(event: Event, url: string) {
        const request = generation;
        try {
            // A file path (and even a load event) does not mean its pixels are
            // ready. Keep the displayed element until this image is decoded.
            await (event.currentTarget as HTMLImageElement).decode();
            if (request !== generation || url !== source) return;
            shown = url;
            images = [url];
        } catch {
            if (request === generation) failed(url);
        }
    }

    function failed(url: string) {
        if (url !== source) return;
        shown = "";
        images = [];
    }
</script>

<span class="player-artwork">
    {#each images as url (url)}
        <img
            src={url}
            {alt}
            decoding="async"
            aria-hidden={url !== shown}
            style:visibility={url === shown ? "visible" : "hidden"}
            onload={(event) => ready(event, url)}
            onerror={() => failed(url)}
        />
    {/each}
    {#if !shown}
        <span class="fallback">{@render children()}</span>
    {/if}
</span>

<style>
    .player-artwork,
    .fallback {
        display: grid;
        grid-template: minmax(0, 1fr) / minmax(0, 1fr);
        place-items: center;
        width: 100%;
        height: 100%;
    }

    img,
    .fallback {
        grid-area: 1 / 1;
    }

    img {
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        object-fit: cover;
    }
</style>
