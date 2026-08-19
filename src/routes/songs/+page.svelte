<script lang="ts">
    import { getTracks, type Track as ApiTrack } from "$lib/api";
    import { loadQueue } from "$lib/stores/playback";
    import { uiPref } from "$lib/stores/uiPrefs";
    import Loading from "$lib/components/Loading.svelte";
    import ScrollIndex, {
        type ScrollIndexEntry,
    } from "$lib/components/ScrollIndex.svelte";
    import TrackRow from "$lib/components/TrackRow.svelte";
    import Select from "$lib/components/Select.svelte";
    import SortDirButton from "$lib/components/SortDirButton.svelte";
    import { songIndexLanguage } from "$lib/stores/songIndex";
    import {
        getSongCollator,
        createScrollIndexEntries,
        createGroupScrollIndexEntries,
        resolveScrollIndexMode,
    } from "$lib/utils/songIndex";
    import { intersect } from "$lib/utils/intersect";
    import { onMount } from "svelte";

    const SORT_OPTIONS = [
        { value: "title", label: "Title" },
        { value: "artist", label: "Artist" },
        { value: "album", label: "Album" },
        { value: "duration", label: "Duration" },
    ];

    const GROUP_OPTIONS = [
        { value: "none", label: "None" },
        { value: "artist", label: "Artist" },
        { value: "album", label: "Album" },
        { value: "genre", label: "Genre" },
        { value: "year", label: "Year" },
    ];

    const PAGE_SIZE = 150;

    let tracks = $state<ApiTrack[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);
    const sortBy = uiPref<string>("songs.sortBy", "title");
    const sortAsc = uiPref("songs.sortAsc", true);
    const groupBy = uiPref<string>("songs.groupBy", "none");
    const groupAsc = uiPref("songs.groupAsc", true);
    let visibleCount = $state(PAGE_SIZE);
    let collator = $derived(getSongCollator($songIndexLanguage));

    function resetVisibleCount() {
        visibleCount = PAGE_SIZE;
    }

    function chooseGroup(v: string) {
        // Direction is the user's choice — switching fields never touches it.
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

    function getTrackSortValue(track: ApiTrack): string | number {
        switch ($sortBy) {
            case "artist":
                return track.artist_names?.[0] ?? "";
            case "album":
                return track.album_title ?? "";
            case "duration":
                return track.duration_ms ?? 0;
            default:
                return track.title ?? "";
        }
    }

    function compareTracks(a: ApiTrack, b: ApiTrack, dir: number): number {
        const aValue = getTrackSortValue(a);
        const bValue = getTrackSortValue(b);
        if (typeof aValue === "number" && typeof bValue === "number") {
            return dir * (aValue - bValue);
        }

        const primary = collator.compare(String(aValue), String(bValue));
        if (primary !== 0) return dir * primary;
        if ($sortBy === "album") {
            return dir * ((a.track_number ?? 0) - (b.track_number ?? 0));
        }
        if ($sortBy === "artist") {
            return dir * collator.compare(a.title ?? "", b.title ?? "");
        }
        return 0;
    }

    function groupKey(track: ApiTrack): string {
        switch ($groupBy) {
            case "artist":
                return track.artist_names?.[0] || "Unknown Artist";
            case "album":
                return track.album_title || "Unknown Album";
            case "genre":
                return track.genre || "Unknown Genre";
            case "year":
                return track.year ? String(track.year) : "?";
            default:
                return "";
        }
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
        return dir * collator.compare(a, b);
    }

    interface TrackGroup {
        key: string;
        items: ApiTrack[];
        offset: number;
    }

    // Group first, then sort within each group: group order wins over sort.
    let groupedTracks = $derived.by<TrackGroup[]>(() => {
        const dir = $sortAsc ? 1 : -1;
        const sortItems = (list: ApiTrack[]) =>
            [...list].sort((a, b) => compareTracks(a, b, dir));
        if ($groupBy === "none") {
            return [{ key: "", items: sortItems(tracks), offset: 0 }];
        }
        const groups = new Map<string, ApiTrack[]>();
        for (const track of tracks) {
            const key = groupKey(track);
            if (!groups.has(key)) groups.set(key, []);
            groups.get(key)!.push(track);
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

    // Progressive rendering: only the first `visibleCount` tracks are mounted.
    // Group offsets come from the full list so playback order is preserved.
    let visibleGroups = $derived.by<TrackGroup[]>(() => {
        let remaining = visibleCount;
        const out: TrackGroup[] = [];
        for (const group of groupedTracks) {
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

    let orderedTracks = $derived(groupedTracks.flatMap((g) => g.items));

    function getScrollIndexValue(track: ApiTrack): string | null {
        const value = getTrackSortValue(track);
        return typeof value === "string" ? value : null;
    }

    let scrollEntries = $derived.by<ScrollIndexEntry[]>(() => {
        const mode = resolveScrollIndexMode($groupBy, $sortBy);
        if ($groupBy !== "none") {
            return createGroupScrollIndexEntries(
                groupedTracks,
                mode,
                $songIndexLanguage,
            );
        }

        return createScrollIndexEntries(
            orderedTracks,
            getScrollIndexValue,
            $songIndexLanguage,
            mode,
        );
    });

    let scrollAnchorIndices = $derived(
        new Set(scrollEntries.map((entry) => entry.index)),
    );

    function scrollAnchorId(index: number): string {
        return `song-scroll-${index}`;
    }

    function scrollToEntry(entry: ScrollIndexEntry) {
        if (visibleCount < entry.index + 1) {
            visibleCount = Math.min(
                tracks.length,
                Math.ceil((entry.index + 1) / PAGE_SIZE) * PAGE_SIZE,
            );
        }
    }

    let hasMore = $derived(visibleCount < tracks.length);

    onMount(async () => {
        loading = true;
        try {
            tracks = await getTracks();
        } catch (e) {
            error = String(e);
        } finally {
            loading = false;
        }
    });

    // Row clicks keep the player's current shuffle mode; the header buttons
    // are explicit context switches: Play = in order, Shuffle = shuffled.
    function playTrack(index: number) {
        loadQueue(
            orderedTracks.map((track) => track.id),
            index,
            undefined,
            { kind: "songs" },
        );
    }

    function playAll() {
        if (orderedTracks.length === 0) return;
        loadQueue(
            orderedTracks.map((track) => track.id),
            0,
            false,
            { kind: "songs" },
        );
    }

    function shuffleAll() {
        if (orderedTracks.length === 0) return;
        const start = Math.floor(Math.random() * orderedTracks.length);
        loadQueue(
            orderedTracks.map((t) => t.id),
            start,
            true,
            { kind: "songs" },
        );
    }
</script>

<div class="songs-page page-enter">
    <div class="header">
        <h1 class="page-title">Songs</h1>
        <div class="actions">
            <div class="sort-field">
                <span class="sort-label">Sort</span>
                <Select
                    options={SORT_OPTIONS}
                    value={$sortBy}
                    onchange={(v) => {
                        $sortBy = v;
                        resetVisibleCount();
                    }}
                    ariaLabel="Sort songs"
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
                    ariaLabel="Group songs"
                />
                <SortDirButton
                    ascending={$groupAsc}
                    ontoggle={toggleGroupDirection}
                    ariaLabel="Toggle group direction"
                />
            </div>
            <button
                class="btn-pill btn-primary"
                onclick={playAll}
                disabled={orderedTracks.length === 0}
            >
                <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                    <path d="M8 5v14l11-7z" />
                </svg>
                Play
            </button>
            <button
                class="btn-pill btn-secondary"
                onclick={shuffleAll}
                disabled={orderedTracks.length === 0}
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
                    <path
                        d="M2 18h1.4c1.3 0 2.5-.6 3.3-1.7l6.1-8.6c.8-1.1 2-1.7 3.3-1.7H22"
                    />
                    <path d="m18 2 4 4-4 4" />
                    <path d="M2 6h1.9c1.5 0 2.9.9 3.6 2.2" />
                    <path d="M22 18h-5.9c-1.3 0-2.6-.7-3.3-1.8l-.5-.8" />
                    <path d="m18 14 4 4-4 4" />
                </svg>
                Shuffle
            </button>
        </div>
    </div>

    {#if error}
        <div class="error">{error}</div>
    {/if}

    {#if loading}
        <Loading />
    {:else if tracks.length === 0}
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
                    <path d="M9 18V5l12-2v13" />
                    <circle cx="6" cy="18" r="3" />
                    <circle cx="18" cy="16" r="3" />
                </svg>
            </div>
            <p class="empty-title">No songs found</p>
            <p class="empty-text">
                Add folders from the Folders page to start listening.
            </p>
        </div>
    {:else}
        <ScrollIndex
            entries={scrollEntries}
            anchorIdForEntry={(entry) => scrollAnchorId(entry.index)}
            ariaLabel="Songs index"
            onSelect={scrollToEntry}
        />
        <div class="track-section">
            <div class="track-header songs">
                <span class="header-cover"></span>
                <span class="header-title">Title</span>
                <span class="header-album">Album</span>
                <span class="header-duration">
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        aria-hidden="true"
                    >
                        <circle cx="12" cy="12" r="10" />
                        <polyline points="12 6 12 12 16 14" />
                    </svg>
                </span>
            </div>
            {#each visibleGroups as group (group.key)}
                {#if group.key}
                    <h2 id={scrollAnchorId(group.offset)} class="group-header">
                        {group.key}
                    </h2>
                {/if}
                <ul class="track-list">
                    {#each group.items as track, index (track.id)}
                        <TrackRow
                            {track}
                            index={group.offset + index}
                            anchorId={$groupBy === "none" &&
                            scrollAnchorIndices.has(group.offset + index)
                                ? scrollAnchorId(group.offset + index)
                                : undefined}
                            variant="songs"
                            onPlay={playTrack}
                            showAddToPlaylist={true}
                        />
                    {/each}
                </ul>
            {/each}
            {#if hasMore}
                <div
                    class="lazy-sentinel"
                    use:intersect={() => (visibleCount += PAGE_SIZE)}
                >
                    <Loading variant="inline" />
                </div>
            {/if}
        </div>
    {/if}
</div>

<style>
    .songs-page {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xl);
    }

    .header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
        flex-wrap: wrap;
    }

    .actions {
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
        flex-wrap: wrap;
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

    .btn-pill svg,
    .btn-secondary svg {
        width: 1.125rem;
        height: 1.125rem;
    }

    .error {
        background-color: var(--color-error);
        color: var(--color-text);
        padding: var(--spacing-md);
        border-radius: var(--radius-lg);
        font-size: var(--font-size-sm);
    }

    .track-section {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
    }

    .lazy-sentinel {
        display: flex;
        justify-content: center;
        padding: var(--spacing-md);
    }

    @media (max-width: 767px) {
        .track-header.songs .header-album {
            display: none;
        }
    }
</style>
