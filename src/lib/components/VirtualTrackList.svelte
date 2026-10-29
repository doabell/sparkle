<script lang="ts" generics="T">
    import type { Snippet } from "svelte";
    import VirtualList from "#lib/components/VirtualList.svelte";
    import ScrollIndex, {
        type ScrollIndexEntry,
    } from "#lib/components/ScrollIndex.svelte";

    interface Props {
        groups: { key: string; items: T[]; offset: number }[];
        entries: ScrollIndexEntry[];
        ariaLabel: string;
        children: Snippet<[T, number]>;
    }
    let { groups, entries, ariaLabel, children: renderTrack }: Props = $props();
    let list = $state<VirtualList>();
    let rows = $derived.by(() =>
        groups.flatMap((group) => {
            const items: (
                | { kind: "track"; item: T; index: number }
                | { kind: "header"; title: string }
            )[] = group.items.map((item, index) => ({
                kind: "track",
                item,
                index: group.offset + index,
            }));
            if (group.key) items.unshift({ kind: "header", title: group.key });
            return items;
        }),
    );
    let rowByTrack = $derived.by(() => {
        // Only index buckets need offsets; avoid a second map of every track.
        const positions = new Map<number, number>();
        const anchors = [...entries].sort((a, b) => a.index - b.index);
        let anchor = 0;
        let rowOffset = 0;
        for (const group of groups) {
            const header = group.key ? 1 : 0;
            while (
                anchor < anchors.length &&
                anchors[anchor].index < group.offset + group.items.length
            ) {
                const index = anchors[anchor++].index;
                positions.set(
                    index,
                    rowOffset +
                        index -
                        group.offset +
                        (index === group.offset ? 0 : header),
                );
            }
            rowOffset += group.items.length + header;
        }
        return positions;
    });
</script>

<ScrollIndex
    {entries}
    anchorIdForEntry={() => ""}
    anchorTopForEntry={(entry) =>
        list?.topForIndex(rowByTrack.get(entry.index) ?? 0) ?? null}
    {ariaLabel}
/>
<VirtualList count={rows.length} bind:this={list} {ariaLabel}>
    {#snippet children(index)}
        {@const row = rows[index]}
        {#if row.kind === "header"}
            <h2 class="group-header" title={row.title}>{row.title}</h2>
        {:else}
            {#key row.item}
                {@render renderTrack(row.item, row.index)}
            {/key}
        {/if}
    {/snippet}
</VirtualList>

<style>
    .group-header {
        height: 100%;
        display: flex;
        align-items: center;
        margin: 0;
        padding: 0 var(--spacing-md);
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
    }
</style>
