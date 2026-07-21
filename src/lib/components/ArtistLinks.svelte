<script lang="ts">
    import { getArtistDisplayEntries } from "$lib/utils/artists";

    interface Props {
        names?: readonly string[] | null;
        ids?: readonly number[] | null;
        linkClass?: string;
    }

    let { names, ids, linkClass = "" }: Props = $props();
    let entries = $derived(getArtistDisplayEntries(names, ids));
</script>

{#each entries as entry, index (`${entry.id ?? "name"}-${index}`)}
    {#if index > 0}<span aria-hidden="true">, </span>{/if}
    {#if entry.id !== null}
        <a
            class={linkClass}
            href={`/artists/${entry.id}`}
            onclick={(event) => event.stopPropagation()}>{entry.name}</a
        >
    {:else}
        <span>{entry.name}</span>
    {/if}
{/each}
