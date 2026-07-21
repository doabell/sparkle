<script lang="ts">
    import { getAlbums, getTracks, type Album } from "$lib/api";
    import { loadQueue } from "$lib/stores/playback";
    import { uiPref } from "$lib/stores/uiPrefs";
    import Loading from "$lib/components/Loading.svelte";
    import ScrollIndex, {
        type ScrollIndexEntry,
    } from "$lib/components/ScrollIndex.svelte";
    import Select from "$lib/components/Select.svelte";
    import SortDirButton from "$lib/components/SortDirButton.svelte";
    import Artwork from "$lib/components/Artwork.svelte";
    import { songIndexLanguage } from "$lib/stores/songIndex";
    import {
        createScrollIndexEntries,
        createGroupScrollIndexEntries,
        resolveScrollIndexMode,
    } from "$lib/utils/songIndex";
    import { intersect } from "$lib/utils/intersect";
    import { onDestroy, onMount } from "svelte";
    import { scrollbackRegistry } from "$lib/utils/scrollback";

    const SORT_OPTIONS = [
        { value: "title", label: "Title" },
        { value: "artist", label: "Artist" },
        { value: "year", label: "Year" },
    ];

    const GROUP_OPTIONS = [
        { value: "none", label: "None" },
        { value: "year", label: "Year" },
        { value: "artist", label: "Artist" },
    ];

    const PAGE_SIZE = 120;

    let albums = $state<Album[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);
    const sortBy = uiPref<string>("albums.sortBy", "title");
    const sortAsc = uiPref("albums.sortAsc", true);
    const groupBy = uiPref<string>("albums.groupBy", "none");
    const groupAsc = uiPref("albums.groupAsc", true);
    const viewMode = uiPref<string>("albums.viewMode", "grid");
    let visibleCount = $state(PAGE_SIZE);

    const unregisterScrollback = scrollbackRegistry.register({
        key: "albums",
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

    function chooseGroup(v: string) {
        $groupBy = v;
        resetVisibleCount();
    }

    function toggleSortDirection() {
        $sortAsc = !$sortAsc;
        resetVisibleCount();
    }

    function toggleGroupDirection() {
        $groupAsc = !$groupAsc;
        resetVisibleCount();
    }

    function compareAlbums(a: Album, b: Album, dir: number): number {
        switch ($sortBy) {
            case "artist":
                return (
                    dir *
                        (a.artist_names?.[0] ?? "").localeCompare(
                            b.artist_names?.[0] ?? "",
                        ) || a.title.localeCompare(b.title)
                );
            case "year":
                return (
                    dir * ((a.year ?? 0) - (b.year ?? 0)) ||
                    a.title.localeCompare(b.title)
                );
            default:
                return dir * a.title.localeCompare(b.title);
        }
    }

    function groupKeyOf(album: Album): string {
        return $groupBy === "year"
            ? String(album.year ?? "?")
            : (album.artist_names?.[0] ?? "Unknown Artist");
    }

    // Group order has its own direction, independent of the item sort.
    // "Unknown" buckets always go last.
    function compareGroupKeys(a: string, b: string): number {
        const aUnknown = a === "?" || a.startsWith("Unknown");
        const bUnknown = b === "?" || b.startsWith("Unknown");
        if (aUnknown !== bUnknown) return aUnknown ? 1 : -1;
        const dir = $groupAsc ? 1 : -1;
        if ($groupBy === "year") {
            return dir * ((parseInt(a) || 0) - (parseInt(b) || 0));
        }
        return dir * a.localeCompare(b);
    }

    interface AlbumGroup {
        key: string;
        items: Album[];
        offset: number;
    }

    // Group first, then sort within each group: group order wins over sort.
    let groupedAlbums = $derived.by<AlbumGroup[]>(() => {
        const dir = $sortAsc ? 1 : -1;
        const sortItems = (list: Album[]) =>
            [...list].sort((a, b) => compareAlbums(a, b, dir));
        if ($groupBy === "none") {
            return [{ key: "", items: sortItems(albums), offset: 0 }];
        }
        const groups = new Map<string, Album[]>();
        for (const album of albums) {
            const key = groupKeyOf(album);
            if (!groups.has(key)) {
                groups.set(key, []);
            }
            groups.get(key)!.push(album);
        }
        const keys = [...groups.keys()].sort(compareGroupKeys);
        let offset = 0;
        return keys.map((key) => {
            const items = sortItems(groups.get(key)!);
            const group = { key, items, offset };
            offset += items.length;
            return group;
        });
    });

    let visibleGroups = $derived.by<AlbumGroup[]>(() => {
        let remaining = visibleCount;
        const out: AlbumGroup[] = [];
        for (const group of groupedAlbums) {
            if (remaining <= 0) break;
            out.push({
                key: group.key,
                items: group.items.slice(0, remaining),
                offset: group.offset,
            });
            remaining -= group.items.length;
        }
        return out;
    });

    let hasMore = $derived(visibleCount < albums.length);
    let orderedAlbums = $derived(groupedAlbums.flatMap((g) => g.items));

    function albumScrollValue(album: Album): string | null {
        switch ($sortBy) {
            case "artist":
                return album.artist_names?.[0] ?? null;
            case "year":
                return String(album.year ?? "?");
            default:
                return album.title;
        }
    }

    let scrollEntries = $derived.by<ScrollIndexEntry[]>(() => {
        const mode = resolveScrollIndexMode($groupBy, $sortBy);
        if ($groupBy !== "none") {
            return createGroupScrollIndexEntries(
                groupedAlbums,
                mode,
                $songIndexLanguage,
            );
        }
        return createScrollIndexEntries(
            orderedAlbums,
            albumScrollValue,
            $songIndexLanguage,
            mode,
        );
    });
    let scrollAnchorIndices = $derived(
        new Set(scrollEntries.map((entry) => entry.index)),
    );

    function scrollAnchorId(index: number): string {
        return `albums-scroll-${index}`;
    }

    function scrollToEntry(entry: ScrollIndexEntry) {
        if (visibleCount < entry.index + 1) {
            visibleCount = Math.min(
                albums.length,
                Math.ceil((entry.index + 1) / PAGE_SIZE) * PAGE_SIZE,
            );
        }
    }

    onMount(async () => {
        loading = true;
        try {
            albums = await getAlbums();
        } catch (e) {
            error = String(e);
        } finally {
            loading = false;
        }
    });

    async function playAlbum(album: Album, event?: MouseEvent) {
        event?.preventDefault();
        event?.stopPropagation();
        try {
            const tracks = await getTracks(album.id);
            if (tracks.length > 0) {
                // Card play is an explicit "play this album" — in order.
                loadQueue(
                    tracks.map((track) => track.id),
                    0,
                    false,
                );
            }
        } catch (e) {
            console.error("Failed to play album:", e);
        }
    }
</script>

<div class="albums-page page-enter">
    <div class="header">
        <h1 class="page-title">Albums</h1>
        <div class="controls">
            <div class="sort-field">
                <span class="sort-label">Sort</span>
                <Select
                    options={SORT_OPTIONS}
                    value={$sortBy}
                    onchange={chooseSort}
                    ariaLabel="Sort albums"
                />
                <SortDirButton
                    ascending={$sortAsc}
                    ontoggle={toggleSortDirection}
                />
            </div>
            <div class="sort-field">
                <span class="sort-label">Group</span>
                <Select
                    options={GROUP_OPTIONS}
                    value={$groupBy}
                    onchange={chooseGroup}
                    ariaLabel="Group albums"
                />
                <SortDirButton
                    ascending={$groupAsc}
                    ontoggle={toggleGroupDirection}
                    ariaLabel="Toggle group direction"
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
    {:else if albums.length === 0}
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
                    ><path
                        d="M9 18V5l12-2v13M6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zm12-2a3 3 0 1 0 0-6 3 3 0 0 0 0 6z"
                    />
                </svg>
            </div>
            <p class="empty-title">No albums found</p>
            <p class="empty-text">
                Add folders from the Folders page to start building your
                library.
            </p>
        </div>
    {:else}
        <ScrollIndex
            entries={scrollEntries}
            anchorIdForEntry={(entry) => scrollAnchorId(entry.index)}
            ariaLabel="Albums index"
            onSelect={scrollToEntry}
        />
        {#each visibleGroups as group (group.key)}
            {#if group.key}
                <h2 id={scrollAnchorId(group.offset)} class="group-header">
                    {group.key}
                </h2>
            {/if}
            {#if $viewMode === "grid"}
                <ul class="card-grid">
                    {#each group.items as album, index (album.id)}
                        <li
                            id={$groupBy === "none" &&
                            scrollAnchorIndices.has(group.offset + index)
                                ? scrollAnchorId(group.offset + index)
                                : undefined}
                            class="card-grid-item split card-enter"
                            style="animation-delay: {(index % PAGE_SIZE) *
                                20}ms"
                        >
                            <div class="card-grid-thumb-wrap">
                                <a
                                    class="thumb-link"
                                    href={`/albums/${album.id}`}
                                    aria-label="Open {album.title}"
                                    tabindex="-1"
                                >
                                    <Artwork
                                        albumId={album.id}
                                        alt={album.title}
                                        class="card-grid-thumb"
                                    />
                                </a>
                                <button
                                    class="card-play-button"
                                    type="button"
                                    aria-label="Play album"
                                    onclick={(e) => playAlbum(album, e)}
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
                            <a
                                class="card-text-link"
                                href={`/albums/${album.id}`}
                            >
                                <div class="card-grid-title ellipsis">
                                    {album.title}
                                </div>
                                <div class="card-grid-meta ellipsis">
                                    {#if album.year}{album.year} ·
                                    {/if}
                                    {album.artist_names?.join(", ") ?? ""}
                                </div>
                            </a>
                        </li>
                    {/each}
                </ul>
            {:else}
                <ul class="list-view">
                    {#each group.items as album, index (album.id)}
                        <li
                            id={$groupBy === "none" &&
                            scrollAnchorIndices.has(group.offset + index)
                                ? scrollAnchorId(group.offset + index)
                                : undefined}
                            class="list-row card-enter"
                            style="animation-delay: {(index % PAGE_SIZE) *
                                20}ms"
                        >
                            <a
                                class="list-row-hit"
                                href={`/albums/${album.id}`}
                                aria-label="Open {album.title}"
                            ></a>
                            <div class="list-row-thumb">
                                <Artwork
                                    albumId={album.id}
                                    alt={album.title}
                                    class="list-row-art"
                                />
                                <button
                                    class="list-row-play"
                                    type="button"
                                    aria-label="Play album"
                                    onclick={(e) => playAlbum(album, e)}
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
                                <div class="list-row-title ellipsis">
                                    {album.title}
                                </div>
                                <div class="list-row-meta ellipsis">
                                    {#if album.year}{album.year} ·
                                    {/if}
                                    {album.artist_names?.join(", ") ?? ""}
                                </div>
                            </div>
                        </li>
                    {/each}
                </ul>
            {/if}
        {/each}
        {#if hasMore}
            <div
                class="lazy-sentinel"
                use:intersect={() => (visibleCount += PAGE_SIZE)}
            >
                <Loading variant="inline" />
            </div>
        {/if}
    {/if}
</div>

<style>
    .albums-page {
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
