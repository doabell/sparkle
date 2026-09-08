<script lang="ts">
    import { lyricWrapSegments } from "$lib/utils/lrc";

    let { text }: { text: string } = $props();
    let segments = $derived(lyricWrapSegments(text));
</script>

{#each segments as segment, index (index)}{#if index > 0}<wbr
        />{/if}{segment}{/each}
