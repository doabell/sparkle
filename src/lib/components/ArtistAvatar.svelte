<script lang="ts">
    import { getArtistImage } from "$lib/api";
    import { cachedImageToUrl } from "$lib/utils/base64";

    interface Props {
        artistId: number;
        alt?: string;
        class?: string;
    }

    let { artistId, alt = "", class: cls = "" }: Props = $props();
</script>

{#await getArtistImage(artistId, "background")}
    <div class="{cls} avatar-fallback" aria-hidden="true">
        <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
            stroke-linejoin="round"
        >
            <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
            <circle cx="12" cy="7" r="4" />
        </svg>
    </div>
{:then image}
    {#if image.file_path}
        <img
            class={cls}
            src={cachedImageToUrl(image, "")}
            loading="lazy"
            decoding="async"
            {alt}
        />
    {:else}
        <div class="{cls} avatar-fallback" aria-hidden="true">
            <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
                stroke-linejoin="round"
            >
                <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
                <circle cx="12" cy="7" r="4" />
            </svg>
        </div>
    {/if}
{:catch}
    <div class="{cls} avatar-fallback" aria-hidden="true">
        <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
            stroke-linejoin="round"
        >
            <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
            <circle cx="12" cy="7" r="4" />
        </svg>
    </div>
{/await}

<style>
    .avatar-fallback {
        display: flex;
        align-items: center;
        justify-content: center;
        background-color: var(--color-surface-elevated);
        color: var(--color-text-muted);
        overflow: hidden;
    }

    .avatar-fallback svg {
        width: 45%;
        height: 45%;
    }
</style>
