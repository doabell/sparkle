<script lang="ts">
    import { page } from "$app/stores";
    import { getTracksByGenre, type Track as ApiTrack } from "$lib/api";
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
    import { windowPageTitle } from "$lib/stores/windowPageTitle";
    import {
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
        { value: "year", label: "Year" },
    ];

    const PAGE_SIZE = 150;

    let tracks = $state<ApiTrack[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);
    let visibleCount = $state(PAGE_SIZE);
    const sortBy = uiPref<string>("genre-detail.sortBy", "album");
    const sortAsc = uiPref("genre-detail.sortAsc", true);
    const groupBy = uiPref<string>("genre-detail.groupBy", "album");
    const groupAsc = uiPref("genre-detail.groupAsc", true);

    const genreName = $derived(decodeURIComponent($page.params.name || ""));

    $effect(() => {
        windowPageTitle.set(genreName || null);
    });

    function resetVisibleCount() {
        visibleCount = PAGE_SIZE;
    }

    function chooseGroup(v: string) {
        // Direction is the user's choice — switching fields never touches it.
        $groupBy = v;
        resetVisibleCount();
    }

    function chooseSort(v: string) {
        $sortBy = v;
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

    onMount(async () => {
        try {
            loading = true;
            tracks = await getTracksByGenre(genreName);
        } catch (e) {
            error = String(e);
        } finally {
            loading = false;
        }
    });

    function compareTracks(a: ApiTrack, b: ApiTrack, dir: number): number {
        switch ($sortBy) {
            case "title":
                return dir * (a.title ?? "").localeCompare(b.title ?? "");
            case "artist":
                return (
                    dir *
                        (a.artist_names?.[0] ?? "").localeCompare(
                            b.artist_names?.[0] ?? "",
                        ) || (a.title ?? "").localeCompare(b.title ?? "")
                );
            case "duration":
                return dir * ((a.duration_ms ?? 0) - (b.duration_ms ?? 0));
            default:
                return (
                    dir *
                    ((a.album_title ?? "").localeCompare(b.album_title ?? "") ||
                        (a.track_number ?? 0) - (b.track_number ?? 0))
                );
        }
    }

    function groupKey(track: ApiTrack): string {
        switch ($groupBy) {
            case "artist":
                return track.artist_names?.[0] || "Unknown Artist";
            case "album":
                return track.album_title || "Unknown Album";
            case "year":
                return track.year ? String(track.year) : "?";
            default:
                return "";
        }
    }

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

    interface TrackGroup {
        key: string;
        items: ApiTrack[];
        offset: number;
    }

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
    let hasMore = $derived(visibleCount < tracks.length);

    function scrollIndexValue(track: ApiTrack): string | null {
        switch ($sortBy) {
            case "artist":
                return track.artist_names?.[0] ?? null;
            case "album":
                return track.album_title;
            case "duration":
                return null;
            default:
                return track.title;
        }
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
            scrollIndexValue,
            $songIndexLanguage,
            mode,
        );
    });
    let scrollAnchorIndices = $derived(
        new Set(scrollEntries.map((entry) => entry.index)),
    );

    function scrollAnchorId(index: number): string {
        return `genre-scroll-${index}`;
    }

    function scrollToEntry(entry: ScrollIndexEntry) {
        if (visibleCount < entry.index + 1) {
            visibleCount = Math.min(
                tracks.length,
                Math.ceil((entry.index + 1) / PAGE_SIZE) * PAGE_SIZE,
            );
        }
    }

    // Row clicks keep the player's current shuffle mode; the header buttons
    // are explicit context switches: Play = in order, Shuffle = shuffled.
    function playGenre(index = 0) {
        if (orderedTracks.length === 0) return;
        loadQueue(
            orderedTracks.map((t) => t.id),
            index,
            undefined,
            { kind: "genre" },
        );
    }

    function playGenreOrdered() {
        if (orderedTracks.length === 0) return;
        loadQueue(
            orderedTracks.map((t) => t.id),
            0,
            false,
            { kind: "genre" },
        );
    }

    function shuffleGenre() {
        if (orderedTracks.length === 0) return;
        const start = Math.floor(Math.random() * orderedTracks.length);
        loadQueue(
            orderedTracks.map((t) => t.id),
            start,
            true,
            { kind: "genre" },
        );
    }

    function totalDuration(): string {
        const totalMs = tracks.reduce(
            (sum, t) => sum + (t.duration_ms ?? 0),
            0,
        );
        const totalSeconds = Math.round(totalMs / 1000);
        const m = Math.floor(totalSeconds / 60);
        return `${m} min`;
    }
</script>

<div class="genre-detail page-enter">
    {#if loading}
        <Loading variant="full" />
    {:else}
        {#if error}
            <div class="error">{error}</div>
        {/if}

        <div class="header">
            <div class="header-info">
                <div class="genre-label">Genre</div>
                <h1 class="genre-title">{genreName}</h1>
                <div class="genre-meta">
                    {tracks.length} track{tracks.length === 1 ? "" : "s"}
                    · {totalDuration()}
                </div>
            </div>

            <div class="header-actions">
                <div class="sort-field">
                    <span class="sort-label">Sort</span>
                    <Select
                        options={SORT_OPTIONS}
                        value={$sortBy}
                        onchange={chooseSort}
                        ariaLabel="Sort tracks"
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
                        ariaLabel="Group tracks"
                    />
                    <SortDirButton
                        ascending={$groupAsc}
                        ontoggle={toggleGroupDirection}
                        ariaLabel="Toggle group direction"
                    />
                </div>
                <button
                    class="btn-pill btn-primary"
                    onclick={playGenreOrdered}
                    disabled={tracks.length === 0}
                >
                    <svg
                        viewBox="0 0 24 24"
                        fill="currentColor"
                        aria-hidden="true"
                    >
                        <path d="M8 5v14l11-7z" />
                    </svg>
                    Play
                </button>
                <button
                    class="btn-pill btn-secondary"
                    onclick={shuffleGenre}
                    disabled={tracks.length === 0}
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

        {#if tracks.length === 0}
            <div class="empty-state">
                <p class="empty-title">No tracks found</p>
                <p class="empty-text">No tracks are tagged with this genre.</p>
            </div>
        {:else}
            <ScrollIndex
                entries={scrollEntries}
                anchorIdForEntry={(entry) => scrollAnchorId(entry.index)}
                ariaLabel="Genre songs index"
                onSelect={scrollToEntry}
            />
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
                            onPlay={playGenre}
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
        {/if}
    {/if}
</div>

<style>
    .genre-detail {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xl);
    }

    .header {
        display: flex;
        align-items: flex-end;
        justify-content: space-between;
        gap: var(--spacing-xl);
        flex-wrap: wrap;
    }

    .header-info {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .genre-label {
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-semibold);
        letter-spacing: normal;
        color: var(--color-text-muted);
    }

    .genre-title {
        font-size: var(--font-size-3xl);
        font-weight: var(--font-weight-bold);
        line-height: var(--line-height-tight);
    }

    .genre-meta {
        font-size: var(--font-size-sm);
        color: var(--color-text-muted);
    }

    .header-actions {
        display: flex;
        gap: var(--spacing-md);
        flex-wrap: wrap;
        align-items: center;
    }

    .header-actions .btn-pill svg {
        width: 1rem;
        height: 1rem;
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

    .track-list {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .lazy-sentinel {
        display: flex;
        justify-content: center;
        padding: var(--spacing-md);
    }

    @media (max-width: 640px) {
        .header {
            flex-direction: column;
            align-items: flex-start;
        }
    }
</style>
