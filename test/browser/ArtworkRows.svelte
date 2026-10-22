<script lang="ts">
    import Artwork from "../../src/lib/components/Artwork.svelte";
    import ArtistAvatar from "../../src/lib/components/ArtistAvatar.svelte";
    import VirtualList from "../../src/lib/components/VirtualList.svelte";
</script>

<div class="content" style="height:300px;overflow:auto;width:500px">
    <VirtualList count={1000} ariaLabel="Artwork scroll regression">
        {#snippet children(index)}
            <div style="display:flex;height:48px">
                <Artwork albumId={8000 + index} alt={`Album ${index}`} />
                <ArtistAvatar artistId={8000 + index} alt={`Artist ${index}`} />
            </div>
        {/snippet}
    </VirtualList>
</div>
