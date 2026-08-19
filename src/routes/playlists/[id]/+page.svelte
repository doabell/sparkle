<script lang="ts">
    import { page } from "$app/stores";
    import {
        getPlaylist,
        deletePlaylist,
        removeTrackFromPlaylist,
        type PlaylistDetail,
        type Track,
    } from "$lib/api";
    import { loadQueue } from "$lib/stores/playback";
    import { uiPref } from "$lib/stores/uiPrefs";
    import Loading from "$lib/components/Loading.svelte";
    import ScrollIndex, {
        type ScrollIndexEntry,
    } from "$lib/components/ScrollIndex.svelte";
    import TrackRow from "$lib/components/TrackRow.svelte";
    import CoverCollage from "$lib/components/CoverCollage.svelte";
    import Select from "$lib/components/Select.svelte";
    import SortDirButton from "$lib/components/SortDirButton.svelte";
    import { goto } from "$app/navigation";
    import { onMount } from "svelte";
    import { addToast } from "$lib/stores/toast";
    import { windowPageTitle } from "$lib/stores/windowPageTitle";
    import { songIndexLanguage } from "$lib/stores/songIndex";
    import {
        createScrollIndexEntries,
        createGroupScrollIndexEntries,
        resolveScrollIndexMode,
    } from "$lib/utils/songIndex";

    const SORT_OPTIONS = [
        { value: "position", label: "Playlist order" },
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

    let playlist = $state<PlaylistDetail | null>(null);
    let loading = $state(true);
    let error = $state<string | null>(null);
    let deleting = $state(false);
    let deletingTrack = $state<number | null>(null);

    const sortBy = uiPref<string>("playlist-detail.sortBy", "position");
    const sortAsc = uiPref("playlist-detail.sortAsc", true);
    const groupBy = uiPref<string>("playlist-detail.groupBy", "none");
    const groupAsc = uiPref("playlist-detail.groupAsc", true);

    const playlistId = $derived(Number($page.params.id));

    $effect(() => {
        windowPageTitle.set(playlist?.name ?? null);
    });

    onMount(async () => {
        try {
            loading = true;
            playlist = await getPlaylist(playlistId);
        } catch (e) {
            error = String(e);
        } finally {
            loading = false;
        }
    });

    async function refresh() {
        try {
            playlist = await getPlaylist(playlistId);
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    interface IndexedTrack {
        track: Track;
        pos: number;
    }

    interface TrackGroup {
        key: string;
        items: IndexedTrack[];
        offset: number;
    }

    function compareTracks(
        a: IndexedTrack,
        b: IndexedTrack,
        dir: number,
    ): number {
        switch ($sortBy) {
            case "title":
                return (
                    dir *
                    (a.track.title ?? "").localeCompare(b.track.title ?? "")
                );
            case "artist":
                return (
                    dir *
                        (a.track.artist_names?.[0] ?? "").localeCompare(
                            b.track.artist_names?.[0] ?? "",
                        ) || a.pos - b.pos
                );
            case "album":
                return (
                    dir *
                        (a.track.album_title ?? "").localeCompare(
                            b.track.album_title ?? "",
                        ) ||
                    (a.track.track_number ?? 0) - (b.track.track_number ?? 0)
                );
            case "duration":
                return (
                    dir *
                    ((a.track.duration_ms ?? 0) - (b.track.duration_ms ?? 0))
                );
            default:
                return dir * (a.pos - b.pos);
        }
    }

    function groupKey(item: IndexedTrack): string {
        switch ($groupBy) {
            case "artist":
                return item.track.artist_names?.[0] || "Unknown Artist";
            case "album":
                return item.track.album_title || "Unknown Album";
            case "genre":
                return item.track.genre || "Unknown Genre";
            case "year":
                return item.track.year ? String(item.track.year) : "?";
            default:
                return "";
        }
    }

    // Group order has its own direction; "Unknown" buckets always go last.
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

    // Group first, then sort within each group: group order wins over sort.
    let groupedTracks = $derived.by<TrackGroup[]>(() => {
        if (!playlist) return [];
        const dir = $sortAsc ? 1 : -1;
        const items = playlist.tracks.map((track, pos) => ({ track, pos }));
        const sortItems = (list: IndexedTrack[]) =>
            [...list].sort((a, b) => compareTracks(a, b, dir));
        if ($groupBy === "none") {
            return [{ key: "", items: sortItems(items), offset: 0 }];
        }
        const groups = new Map<string, IndexedTrack[]>();
        for (const item of items) {
            const key = groupKey(item);
            if (!groups.has(key)) groups.set(key, []);
            groups.get(key)!.push(item);
        }
        const keys = [...groups.keys()].sort(compareGroupKeys);
        let offset = 0;
        return keys.map((key) => {
            const groupItems = sortItems(groups.get(key)!);
            const group = { key, items: groupItems, offset };
            offset += groupItems.length;
            return group;
        });
    });

    let orderedTracks = $derived(groupedTracks.flatMap((g) => g.items));

    function scrollIndexValue(item: IndexedTrack): string | null {
        switch ($sortBy) {
            case "artist":
                return item.track.artist_names?.[0] ?? null;
            case "album":
                return item.track.album_title;
            case "duration":
                return null;
            case "position":
                return null;
            default:
                return item.track.title;
        }
    }

    let scrollEntries = $derived.by<ScrollIndexEntry[]>(() => {
        const mode = resolveScrollIndexMode($groupBy, $sortBy);
        if ($sortBy === "position" && $groupBy === "none") return [];
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
        return `playlist-scroll-${index}`;
    }

    // Row clicks keep the player's current shuffle mode; the header buttons
    // are explicit context switches: Play = in order, Shuffle = shuffled.
    function playPlaylist(index = 0) {
        if (orderedTracks.length === 0) return;
        loadQueue(
            orderedTracks.map((t) => t.track.id),
            index,
            undefined,
            { kind: "playlist", id: String(playlistId) },
        );
    }

    function playPlaylistOrdered() {
        if (orderedTracks.length === 0) return;
        loadQueue(
            orderedTracks.map((t) => t.track.id),
            0,
            false,
            { kind: "playlist", id: String(playlistId) },
        );
    }

    function shufflePlaylist() {
        if (orderedTracks.length === 0) return;
        const start = Math.floor(Math.random() * orderedTracks.length);
        loadQueue(
            orderedTracks.map((t) => t.track.id),
            start,
            true,
            { kind: "playlist", id: String(playlistId) },
        );
    }

    async function handleDelete() {
        deleting = true;
        try {
            await deletePlaylist(playlistId);
            addToast("Playlist deleted", "success");
            goto("/playlists");
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            deleting = false;
        }
    }

    async function handleRemoveTrack(trackId: number) {
        deletingTrack = trackId;
        try {
            await removeTrackFromPlaylist(playlistId, trackId);
            addToast("Track removed", "success");
            await refresh();
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            deletingTrack = null;
        }
    }

    function totalDuration(): string {
        if (!playlist) return "0 min";
        const totalMs = playlist.tracks.reduce(
            (sum, t) => sum + (t.duration_ms ?? 0),
            0,
        );
        const totalSeconds = Math.round(totalMs / 1000);
        const m = Math.floor(totalSeconds / 60);
        return `${m} min`;
    }
</script>

<div class="playlist-detail page-enter">
    {#if loading}
        <Loading variant="full" />
    {:else if playlist}
        <div class="header">
            <div class="header-main">
                <div class="playlist-thumb">
                    <svg
                        viewBox="0 0 24 24"
                        fill="currentColor"
                        aria-hidden="true"
                    >
                        <path
                            d="M15 6H3v2h12V6zm0 4H3v2h12v-2zm0 4H3v2h12v-2zm2-10v16l7-8-7-8z"
                        />
                    </svg>
                    <CoverCollage
                        albumIds={playlist.tracks.map((t) => t.album_id)}
                    />
                </div>
                <div class="header-info">
                    <div class="playlist-type">
                        {playlist.live_mix
                            ? "Live mix"
                            : playlist.folder_path
                              ? "Folder playlist"
                              : "Playlist"}
                    </div>
                    <h1 class="playlist-title">{playlist.name}</h1>
                    {#if playlist.description}
                        <p class="playlist-description">
                            {playlist.description}
                        </p>
                    {/if}
                    <div class="playlist-meta">
                        {playlist.tracks.length} track{playlist.tracks
                            .length === 1
                            ? ""
                            : "s"}
                        · {totalDuration()}
                        {#if playlist.folder_path}
                            · {playlist.folder_path}
                        {/if}
                    </div>
                </div>
            </div>

            <div class="header-actions">
                <button
                    class="btn-pill btn-primary"
                    onclick={playPlaylistOrdered}
                    disabled={playlist.tracks.length === 0}
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
                    onclick={shufflePlaylist}
                    disabled={playlist.tracks.length === 0}
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
                {#if !playlist.live_mix}
                    <button
                        class="delete-playlist-btn"
                        aria-label="Delete playlist"
                        onclick={handleDelete}
                        disabled={deleting}
                    >
                        {#if deleting}
                            <Loading variant="inline" />
                        {:else}
                            <svg
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                aria-hidden="true"
                            >
                                <path d="M3 6h18" />
                                <path
                                    d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"
                                />
                                <path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" />
                                <line x1="10" x2="10" y1="11" y2="17" />
                                <line x1="14" x2="14" y1="11" y2="17" />
                            </svg>
                        {/if}
                    </button>
                {/if}
            </div>
        </div>

        {#if playlist.tracks.length === 0}
            <div class="empty-state">
                <p class="empty-title">
                    {playlist.live_mix
                        ? "This mix is empty"
                        : playlist.folder_path
                          ? "No tracks in this folder"
                          : "This playlist is empty"}
                </p>
                <p class="empty-text">
                    {playlist.live_mix
                        ? "Refresh the mix after adding or playing more music."
                        : playlist.folder_path
                          ? "Add music to the monitored folder and rescan your library."
                          : "Add tracks from your library using the track menu."}
                </p>
            </div>
        {:else}
            <div class="toolbar">
                <div class="sort-field">
                    <span class="sort-label">Sort</span>
                    <Select
                        options={SORT_OPTIONS}
                        value={$sortBy}
                        onchange={(v) => ($sortBy = v)}
                        ariaLabel="Sort playlist"
                    />
                    <SortDirButton
                        ascending={$sortAsc}
                        ontoggle={() => ($sortAsc = !$sortAsc)}
                    />
                </div>
                <div class="sort-field">
                    <span class="sort-label">Group</span>
                    <Select
                        options={GROUP_OPTIONS}
                        value={$groupBy}
                        onchange={(v) => ($groupBy = v)}
                        ariaLabel="Group playlist"
                    />
                    <SortDirButton
                        ascending={$groupAsc}
                        ontoggle={() => ($groupAsc = !$groupAsc)}
                        ariaLabel="Toggle group direction"
                    />
                </div>
            </div>
            <ScrollIndex
                entries={scrollEntries}
                anchorIdForEntry={(entry) => scrollAnchorId(entry.index)}
                ariaLabel="Playlist songs index"
            />
            {#each groupedTracks as group (group.key)}
                {#if group.key}
                    <h2 id={scrollAnchorId(group.offset)} class="group-header">
                        {group.key}
                    </h2>
                {/if}
                <ul class="track-list">
                    {#each group.items as item, index (item.track.id)}
                        <li class="track-row-wrapper">
                            <TrackRow
                                track={item.track}
                                index={group.offset + index}
                                anchorId={$groupBy === "none" &&
                                scrollAnchorIndices.has(group.offset + index)
                                    ? scrollAnchorId(group.offset + index)
                                    : undefined}
                                variant="songs"
                                onPlay={playPlaylist}
                                showAddToPlaylist={true}
                            />
                            {#if !playlist.folder_path && !playlist.live_mix}
                                <button
                                    class="remove-track"
                                    aria-label="Remove track"
                                    onclick={() =>
                                        handleRemoveTrack(item.track.id)}
                                    disabled={deletingTrack === item.track.id}
                                >
                                    {#if deletingTrack === item.track.id}
                                        <Loading variant="inline" />
                                    {:else}
                                        <svg
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="2"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                            aria-hidden="true"
                                        >
                                            <path d="M18 6 6 18" />
                                            <path d="m6 6 12 12" />
                                        </svg>
                                    {/if}
                                </button>
                            {/if}
                        </li>
                    {/each}
                </ul>
            {/each}
        {/if}
    {:else if error}
        <div class="error">{error}</div>
    {/if}
</div>

<style>
    .playlist-detail {
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

    .header-main {
        display: flex;
        align-items: flex-end;
        gap: var(--spacing-lg);
        flex: 1;
    }

    .playlist-thumb {
        position: relative;
        width: 8rem;
        height: 8rem;
        flex-shrink: 0;
        border-radius: var(--radius-lg);
        background-color: var(--color-surface);
        display: flex;
        align-items: center;
        justify-content: center;
        color: var(--color-text-muted);
        overflow: hidden;
        box-shadow: var(--shadow-md);
    }

    .playlist-thumb svg {
        width: 50%;
        height: 50%;
    }

    .header-info {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .playlist-type {
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-semibold);
        text-transform: uppercase;
        letter-spacing: 0.05em;
        color: var(--color-text-muted);
    }

    .playlist-title {
        font-size: var(--font-size-3xl);
        font-weight: var(--font-weight-bold);
        line-height: var(--line-height-tight);
    }

    .playlist-description {
        margin: 0;
        color: var(--color-text-secondary);
        font-size: var(--font-size-sm);
    }

    .playlist-meta {
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

    .delete-playlist-btn {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 2.25rem;
        height: 2.25rem;
        border-radius: var(--radius-full);
        background-color: rgba(255, 255, 255, 0.08);
        color: var(--color-text-secondary);
        border: 1px solid var(--color-border);
        transition:
            color var(--transition-fast),
            background-color var(--transition-fast),
            border-color var(--transition-fast),
            transform var(--transition-fast);
    }

    .delete-playlist-btn:hover:not(:disabled) {
        color: var(--color-error);
        border-color: var(--color-error);
        background-color: rgba(226, 33, 52, 0.12);
        transform: scale(1.04);
    }

    .delete-playlist-btn:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }

    .delete-playlist-btn svg {
        width: 1.125rem;
        height: 1.125rem;
    }

    .track-list {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .toolbar {
        display: flex;
        align-items: center;
        justify-content: flex-end;
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

    .track-row-wrapper {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
    }

    .track-row-wrapper :global(.track-row) {
        flex: 1;
    }

    .remove-track {
        width: 2rem;
        height: 2rem;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 0.25rem;
        border-radius: var(--radius-full);
        background-color: rgba(255, 255, 255, 0.08);
        border: 1px solid var(--color-border);
        color: var(--color-text-secondary);
        flex-shrink: 0;
        transition:
            color var(--transition-fast),
            background-color var(--transition-fast),
            border-color var(--transition-fast);
    }

    .remove-track:hover:not(:disabled) {
        color: var(--color-error);
        border-color: var(--color-error);
        background-color: rgba(226, 33, 52, 0.12);
    }

    .remove-track svg {
        width: 1rem;
        height: 1rem;
    }

    @media (max-width: 640px) {
        .header {
            flex-direction: column;
            align-items: flex-start;
        }

        .header-main {
            align-items: flex-start;
            flex-direction: column;
        }
    }
</style>
