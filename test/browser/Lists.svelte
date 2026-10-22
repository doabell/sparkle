<script lang="ts">
    import VirtualTrackList from "../../src/lib/components/VirtualTrackList.svelte";
    import VirtualList from "../../src/lib/components/VirtualList.svelte";

    let { large = false }: { large?: boolean } = $props();
    const groups = [0, 1, 2].map((group) => ({
        key: `Group ${group}`,
        offset: group * 3,
        items: [0, 1, 2].map((i) => ({ id: group * 3 + i })),
    }));
    // Exercise both endpoints when neither is a focusable row.
    groups.push({ key: "Empty group", offset: 9, items: [] });
</script>

<div class="content" style="height:300px;overflow:auto;width:500px">
    {#if large}
        <VirtualList count={100000} ariaLabel="Large queue">
            {#snippet children(index)}
                <button class="queue-row" aria-disabled={index > 99}
                    >Track {index}</button
                >
            {/snippet}
        </VirtualList>
    {:else}
        <VirtualTrackList {groups} entries={[]} ariaLabel="Grouped tracks">
            {#snippet children(item, index)}
                <button class="row-hit-area" data-track={item.id}
                    >Track {index}</button
                >
            {/snippet}
        </VirtualTrackList>
    {/if}
</div>
