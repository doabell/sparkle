<script lang="ts">
    import { getAlbumArt } from "$lib/api";
    import { cachedImageToUrl } from "$lib/utils/base64";

    interface Props {
        albumIds: (number | null | undefined)[];
    }

    let { albumIds }: Props = $props();

    let urls = $state<string[]>([]);
    let lastKey = "";

    async function load(ids: (number | null | undefined)[]) {
        const unique: number[] = [];
        for (const id of ids) {
            if (id != null && !unique.includes(id)) unique.push(id);
            if (unique.length >= 4) break;
        }
        const key = unique.join(",");
        if (key === lastKey) return;
        lastKey = key;
        if (unique.length === 0) {
            urls = [];
            return;
        }
        const results = await Promise.all(
            unique.map(async (id) => {
                try {
                    const art = await getAlbumArt(id, "background");
                    return art.file_path ? cachedImageToUrl(art, "") : null;
                } catch {
                    return null;
                }
            }),
        );
        if (key !== lastKey) return;
        urls = results.filter((u): u is string => !!u);
    }

    $effect(() => {
        load(albumIds);
    });
</script>

{#if urls.length === 1}
    <img class="collage-single" src={urls[0]} alt="" />
{:else if urls.length > 1}
    <div class="collage-grid">
        {#each urls as url, i (i)}
            <img src={url} alt="" />
        {/each}
    </div>
{/if}

<style>
    .collage-single {
        position: absolute;
        inset: 0;
        width: 100%;
        height: 100%;
        object-fit: cover;
    }

    .collage-grid {
        position: absolute;
        inset: 0;
        display: grid;
        grid-template-columns: 1fr 1fr;
        grid-template-rows: 1fr 1fr;
    }

    .collage-grid img {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }
</style>
