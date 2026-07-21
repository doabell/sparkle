<script lang="ts">
    import { getArtists, getTracksByArtist, type Artist } from "$lib/api";
    import { plural } from "$lib/utils/text";
    import { loadQueue } from "$lib/stores/playback";
    import { uiPref } from "$lib/stores/uiPrefs";
    import Loading from "$lib/components/Loading.svelte";
    import ScrollIndex, {
        type ScrollIndexEntry,
    } from "$lib/components/ScrollIndex.svelte";
    import Select from "$lib/components/Select.svelte";
    import SortDirButton from "$lib/components/SortDirButton.svelte";
    import ArtistAvatar from "$lib/components/ArtistAvatar.svelte";
    import { songIndexLanguage } from "$lib/stores/songIndex";
    import { createScrollIndexEntries } from "$lib/utils/songIndex";
    import { intersect } from "$lib/utils/intersect";
    import { onDestroy, onMount } from "svelte";
    import { scrollbackRegistry } from "$lib/utils/scrollback";

    const SORT_OPTIONS = [
        { value: "name", label: "Name" },
        { value: "tracks", label: "Tracks" },
        { value: "albums", label: "Albums" },
    ];

    const PAGE_SIZE = 120;

    let artists = $state<Artist[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);
    const sortBy = uiPref<string>("artists.sortBy", "name");
    const sortAsc = uiPref("artists.sortAsc", true);
    const viewMode = uiPref<string>("artists.viewMode", "grid");
    let visibleCount = $state(PAGE_SIZE);

    const unregisterScrollback = scrollbackRegistry.register({
        key: "artists",
        capture: () => visibleCount,
        restore: (value: number) => {
            visibleCount = Math.max(PAGE_SIZE, value);
        },
    });
    onDestroy(unregisterScrollback);

    function resetVisibleCount() {
        visibleCount = PAGE_SIZE;
    }

    function chooseSort(v: string) {
        // Direction is the user's choice — switching fields never touches it.
        $sortBy = v;
        resetVisibleCount();
    }

    function toggleSortDirection() {
        $sortAsc = !$sortAsc;
        resetVisibleCount();
    }

    let sortedArtists = $derived.by(() => {
        const dir = $sortAsc ? 1 : -1;
        const sorted = [...artists];
        switch ($sortBy) {
            case "name":
                return sorted.sort(
                    (a, b) => dir * a.name.localeCompare(b.name),
                );
            case "tracks":
                return sorted.sort(
                    (a, b) =>
                        dir * ((a.track_count ?? 0) - (b.track_count ?? 0)) ||
                        a.name.localeCompare(b.name),
                );
            case "albums":
                return sorted.sort(
                    (a, b) =>
                        dir * ((a.album_count ?? 0) - (b.album_count ?? 0)) ||
                        a.name.localeCompare(b.name),
                );
            default:
                return sorted;
        }
    });

    let visibleArtists = $derived(sortedArtists.slice(0, visibleCount));
    let hasMore = $derived(visibleCount < artists.length);

    function artistScrollValue(artist: Artist): string | null {
        return $sortBy === "name" ? artist.name : null;
    }

    let scrollEntries = $derived(
        createScrollIndexEntries(
            sortedArtists,
            artistScrollValue,
            $songIndexLanguage,
        ),
    );
    let scrollAnchorIndices = $derived(
        new Set(scrollEntries.map((entry) => entry.index)),
    );

    function scrollAnchorId(index: number): string {
        return `artists-scroll-${index}`;
    }

    function scrollToEntry(entry: ScrollIndexEntry) {
        if (visibleCount < entry.index + 1) {
            visibleCount = Math.min(
                artists.length,
                Math.ceil((entry.index + 1) / PAGE_SIZE) * PAGE_SIZE,
            );
        }
    }

    onMount(async () => {
        loading = true;
        try {
            artists = await getArtists();
        } catch (e) {
            error = String(e);
        } finally {
            loading = false;
        }
    });

    async function playArtist(artist: Artist, event?: MouseEvent) {
        event?.preventDefault();
        event?.stopPropagation();
        try {
            const tracks = await getTracksByArtist(artist.id);
            if (tracks.length > 0) {
                // Card play is an explicit "play this artist" — in order.
                loadQueue(
                    tracks.map((track) => track.id),
                    0,
                    false,
                );
            }
        } catch (e) {
            console.error("Failed to play artist:", e);
        }
    }
</script>

<div class="artists-page page-enter">
    <div class="header">
        <h1 class="page-title">Artists</h1>
        <div class="controls">
            <div class="sort-field">
                <span class="sort-label">Sort</span>
                <Select
                    options={SORT_OPTIONS}
                    value={$sortBy}
                    onchange={chooseSort}
                    ariaLabel="Sort artists"
                />
                <SortDirButton
                    ascending={$sortAsc}
                    ontoggle={toggleSortDirection}
                />
            </div>
            <div class="view-toggle">
                <button
                    class="view-grid"
                    class:active={$viewMode === "grid"}
                    aria-label="Grid view"
                    onclick={() => ($viewMode = "grid")}
                >
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        aria-hidden="true"
                    >
                        <rect x="3" y="3" width="7" height="7" rx="1" />
                        <rect x="14" y="3" width="7" height="7" rx="1" />
                        <rect x="3" y="14" width="7" height="7" rx="1" />
                        <rect x="14" y="14" width="7" height="7" rx="1" />
                    </svg>
                </button>
                <button
                    class="view-list"
                    class:active={$viewMode === "row"}
                    aria-label="Row view"
                    onclick={() => ($viewMode = "row")}
                >
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        aria-hidden="true"
                    >
                        <line x1="3" y1="6" x2="21" y2="6" />
                        <line x1="3" y1="12" x2="21" y2="12" />
                        <line x1="3" y1="18" x2="21" y2="18" />
                    </svg>
                </button>
            </div>
        </div>
    </div>

    {#if error}
        <div class="error">{error}</div>
    {/if}

    {#if loading}
        <Loading />
    {:else if artists.length === 0}
        <div class="empty-state">
            <div class="empty-icon">
                <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.5"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
                    <circle cx="12" cy="7" r="4" />
                </svg>
            </div>
            <p class="empty-title">No artists found</p>
            <p class="empty-text">
                Add folders from the Folders page to start building your
                library.
            </p>
        </div>
    {:else if $viewMode === "grid"}
        <ScrollIndex
            entries={scrollEntries}
            anchorIdForEntry={(entry) => scrollAnchorId(entry.index)}
            ariaLabel="Artists index"
            onSelect={scrollToEntry}
        />
        <ul class="card-grid">
            {#each visibleArtists as artist, index (artist.id)}
                <li
                    id={scrollAnchorIndices.has(index)
                        ? scrollAnchorId(index)
                        : undefined}
                    class="card-grid-item split card-enter"
                    style="animation-delay: {(index % PAGE_SIZE) * 20}ms"
                >
                    <div class="card-grid-thumb-wrap">
                        <a
                            class="thumb-link"
                            href={`/artists/${artist.id}`}
                            aria-label="Open {artist.name}"
                            tabindex="-1"
                        >
                            <ArtistAvatar
                                artistId={artist.id}
                                alt={artist.name}
                                class="artist-avatar"
                            />
                        </a>
                        <button
                            class="card-play-button"
                            type="button"
                            aria-label="Play artist"
                            onclick={(e) => playArtist(artist, e)}
                        >
                            <svg
                                viewBox="0 0 24 24"
                                fill="currentColor"
                                aria-hidden="true"
                            >
                                <path d="M8 5v14l11-7z" />
                            </svg>
                        </button>
                    </div>
                    <a class="card-text-link" href={`/artists/${artist.id}`}>
                        <div class="card-grid-title ellipsis">
                            {artist.name}
                        </div>
                        <div class="card-grid-meta">
                            {plural(artist.track_count ?? 0, "track")} · {plural(
                                artist.album_count ?? 0,
                                "album",
                            )}
                        </div>
                    </a>
                </li>
            {/each}
        </ul>
    {:else}
        <ScrollIndex
            entries={scrollEntries}
            anchorIdForEntry={(entry) => scrollAnchorId(entry.index)}
            ariaLabel="Artists index"
            onSelect={scrollToEntry}
        />
        <ul class="list-view">
            {#each visibleArtists as artist, index (artist.id)}
                <li
                    id={scrollAnchorIndices.has(index)
                        ? scrollAnchorId(index)
                        : undefined}
                    class="list-row card-enter"
                    style="animation-delay: {(index % PAGE_SIZE) * 20}ms"
                >
                    <a
                        class="list-row-hit"
                        href={`/artists/${artist.id}`}
                        aria-label="Open {artist.name}"
                    ></a>
                    <div class="list-row-thumb artist">
                        <ArtistAvatar
                            artistId={artist.id}
                            alt={artist.name}
                            class="list-row-art"
                        />
                        <button
                            class="list-row-play"
                            type="button"
                            aria-label="Play artist"
                            onclick={(e) => playArtist(artist, e)}
                        >
                            <svg
                                viewBox="0 0 24 24"
                                fill="currentColor"
                                aria-hidden="true"
                            >
                                <path d="M8 5v14l11-7z" />
                            </svg>
                        </button>
                    </div>
                    <div class="list-row-content">
                        <div class="list-row-title ellipsis">{artist.name}</div>
                        <div class="list-row-meta">
                            {plural(artist.track_count ?? 0, "track")} · {plural(
                                artist.album_count ?? 0,
                                "album",
                            )}
                        </div>
                    </div>
                </li>
            {/each}
        </ul>
    {/if}
    {#if hasMore}
        <div
            class="lazy-sentinel"
            use:intersect={() => (visibleCount += PAGE_SIZE)}
        >
            <Loading variant="inline" />
        </div>
    {/if}
</div>

<style>
    .artists-page {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xl);
    }

    .header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
    }

    .controls {
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
    }

    .sort-field {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
    }

    .sort-label {
        font-size: var(--font-size-sm);
        color: var(--color-text-secondary);
        font-weight: var(--font-weight-medium);
    }

    .error {
        background-color: var(--color-error);
        color: var(--color-text);
        padding: var(--spacing-md);
        border-radius: var(--radius-lg);
        font-size: var(--font-size-sm);
    }

    .list-row-thumb :global(.list-row-art) {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }

    .lazy-sentinel {
        display: flex;
        justify-content: center;
        padding: var(--spacing-md);
    }
</style>
