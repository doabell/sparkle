<script lang="ts">
    import { getAlbumArt } from "$lib/api";
    import { cachedImageToUrl } from "$lib/utils/base64";

    interface Props {
        albumId: number | null | undefined;
        alt?: string;
        class?: string;
    }

    let { albumId, alt = "", class: cls = "" }: Props = $props();
</script>

{#if albumId}
    {#await getAlbumArt(albumId, "background")}
        <div class="{cls} artwork-fallback" aria-hidden="true">
            <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
                stroke-linejoin="round"
            >
                <path d="M9 18V5l12-2v13" />
                <circle cx="6" cy="18" r="3" />
                <circle cx="18" cy="16" r="3" />
            </svg>
        </div>
    {:then art}
        {#if art.file_path}
            <img
                class={cls}
                src={cachedImageToUrl(art, "")}
                loading="lazy"
                decoding="async"
                {alt}
            />
        {:else}
            <div class="{cls} artwork-fallback" aria-hidden="true">
                <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.5"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <path d="M9 18V5l12-2v13" />
                    <circle cx="6" cy="18" r="3" />
                    <circle cx="18" cy="16" r="3" />
                </svg>
            </div>
        {/if}
    {:catch}
        <div class="{cls} artwork-fallback" aria-hidden="true">
            <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
                stroke-linejoin="round"
            >
                <path d="M9 18V5l12-2v13" />
                <circle cx="6" cy="18" r="3" />
                <circle cx="18" cy="16" r="3" />
            </svg>
        </div>
    {/await}
{:else}
    <div class="{cls} artwork-fallback" aria-hidden="true">
        <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
            stroke-linejoin="round"
        >
            <path d="M9 18V5l12-2v13" />
            <circle cx="6" cy="18" r="3" />
            <circle cx="18" cy="16" r="3" />
        </svg>
    </div>
{/if}

<style>
    .artwork-fallback {
        display: flex;
        align-items: center;
        justify-content: center;
        background-color: var(--color-surface-elevated);
        color: var(--color-text-muted);
        overflow: hidden;
    }

    .artwork-fallback svg {
        width: 40%;
        height: 40%;
    }
</style>
