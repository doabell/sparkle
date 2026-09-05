<script lang="ts">
    import ArtistAvatar from "$lib/components/ArtistAvatar.svelte";
    import { getArtistDisplayEntries } from "$lib/utils/artists";

    interface Props {
        names?: readonly string[] | null;
        ids?: readonly number[] | null;
        size?: "regular" | "compact";
        align?: "start" | "center";
    }

    let { names, ids, size = "regular", align = "center" }: Props = $props();

    let entries = $derived(getArtistDisplayEntries(names, ids));
</script>

<div class="artist-credits {size} {align}" role="list" aria-label="Artists">
    {#each entries as entry, index (`${entry.id ?? "name"}-${index}`)}
        <span class="artist-credit-item" role="listitem">
            {#if entry.id !== null}
                <a
                    class="artist-credit"
                    href={`/artists/${entry.id}`}
                    onclick={(event) => event.stopPropagation()}
                >
                    <ArtistAvatar
                        artistId={entry.id}
                        alt=""
                        class="artist-credit-avatar"
                    />
                    <span class="artist-credit-name">{entry.name}</span>
                </a>
            {:else}
                <span class="artist-credit unlinked">
                    <span
                        class="artist-credit-avatar artist-credit-fallback"
                        aria-hidden="true"
                    >
                        <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="1.5"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                        >
                            <path
                                d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"
                            />
                            <circle cx="12" cy="7" r="4" />
                        </svg>
                    </span>
                    <span class="artist-credit-name">{entry.name}</span>
                </span>
            {/if}
        </span>
    {/each}
</div>

<style>
    .artist-credits {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        width: 100%;
        gap: 0.5rem 1rem;
    }

    .artist-credits.start {
        justify-content: flex-start;
    }

    .artist-credits.center {
        justify-content: center;
    }

    .artist-credits.compact {
        gap: 0.4375rem 0.75rem;
    }

    .artist-credit-item {
        display: inline-flex;
        min-width: 0;
        max-width: 100%;
    }

    .artist-credit {
        display: inline-flex;
        align-items: center;
        min-width: 0;
        max-width: 100%;
        gap: 0.4375rem;
        color: var(--color-text-secondary);
        font-size: var(--font-size-base);
        line-height: 1.25;
        transition: color var(--transition-fast);
    }

    .compact .artist-credit {
        gap: 0.375rem;
        font-size: var(--font-size-sm);
    }

    :global(.artist-credit-avatar) {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 1.75rem;
        height: 1.75rem;
        flex: 0 0 1.75rem;
        overflow: hidden;
        border: 1px solid color-mix(in srgb, var(--color-text) 12%, transparent);
        border-radius: 50%;
        background: var(--color-surface-elevated);
        color: var(--color-text-muted);
        object-fit: cover;
        box-shadow: var(--shadow-sm);
        transition:
            transform var(--transition-fast),
            border-color var(--transition-fast),
            box-shadow var(--transition-fast);
    }

    .compact :global(.artist-credit-avatar) {
        width: 1.5rem;
        height: 1.5rem;
        flex-basis: 1.5rem;
    }

    .artist-credit-fallback svg {
        width: 52%;
        height: 52%;
    }

    .artist-credit-name {
        min-width: 0;
        overflow-wrap: anywhere;
    }

    a.artist-credit:hover {
        color: var(--color-text);
    }

    a.artist-credit:hover :global(.artist-credit-avatar) {
        transform: scale(var(--motion-hover-scale));
        box-shadow: var(--shadow-md);
    }

    .artist-credit.unlinked {
        color: var(--color-text-secondary);
    }
</style>
