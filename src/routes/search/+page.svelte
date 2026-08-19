<script lang="ts">
    import {
        search,
        getTracks,
        type SearchResults,
        type Track,
    } from "$lib/api";
    import { plural } from "$lib/utils/text";
    import { loadQueue } from "$lib/stores/playback";
    import Loading from "$lib/components/Loading.svelte";
    import TrackRow from "$lib/components/TrackRow.svelte";
    import Artwork from "$lib/components/Artwork.svelte";
    import ArtistAvatar from "$lib/components/ArtistAvatar.svelte";
    import { addRecentSearch } from "$lib/utils/searchHistory";
    import { onMount } from "svelte";
    import { page } from "$app/stores";

    let query = $state("");
    let results = $state<SearchResults | null>(null);
    let searching = $state(false);
    let inputRef = $state<HTMLInputElement | null>(null);
    let debounceTimer: ReturnType<typeof setTimeout> | null = null;
    let searchRequestId = 0;
    let history = $state<string[]>([]);

    const HISTORY_KEY = "sparkle.searchHistory";
    const HISTORY_MAX = 10;

    function loadHistory() {
        try {
            history = JSON.parse(
                window.localStorage.getItem(HISTORY_KEY) ?? "[]",
            );
        } catch {
            history = [];
        }
    }

    function pushHistory(q: string) {
        const next = addRecentSearch(history, q, HISTORY_MAX);
        history = next;
        try {
            window.localStorage.setItem(HISTORY_KEY, JSON.stringify(next));
        } catch {}
    }

    function clearHistory() {
        history = [];
        try {
            window.localStorage.removeItem(HISTORY_KEY);
        } catch {}
    }

    function applyHistory(q: string) {
        query = q;
        inputRef?.focus();
    }

    onMount(() => {
        inputRef?.focus();
        loadHistory();
        const initialQuery = $page.url.searchParams.get("q");
        if (initialQuery) query = initialQuery;
    });

    $effect(() => {
        const q = query.trim();
        const requestId = ++searchRequestId;
        if (debounceTimer) clearTimeout(debounceTimer);
        if (!q) {
            results = null;
            searching = false;
            return;
        }
        results = null;
        searching = true;
        debounceTimer = setTimeout(async () => {
            try {
                const nextResults = await search(q);
                // A slow provider or database read must never let an older
                // query replace the results for the text currently in the box.
                if (requestId !== searchRequestId || query.trim() !== q) return;
                results = nextResults;
            } catch (e) {
                console.error("Search failed:", e);
                if (requestId !== searchRequestId || query.trim() !== q) return;
                results = {
                    artists: [],
                    albums: [],
                    tracks: [],
                    lyric_tracks: [],
                };
            } finally {
                if (requestId === searchRequestId) searching = false;
            }
        }, 250);
    });

    // History records deliberate searches: pressing Enter or clicking a result.
    function commitSearch() {
        const q = query.trim();
        if (q) pushHistory(q);
    }

    function handleSearchKeydown(e: KeyboardEvent) {
        if (e.key === "Enter") {
            commitSearch();
        }
    }

    let hasResults = $derived(
        results !== null &&
            (results.artists.length > 0 ||
                results.albums.length > 0 ||
                results.tracks.length > 0 ||
                results.lyric_tracks.length > 0),
    );

    function playTrack(index: number) {
        if (!results) return;
        commitSearch();
        loadQueue(
            results.tracks.map((t: Track) => t.id),
            index,
            undefined,
            { kind: "search" },
        );
    }

    function playLyricTrack(index: number) {
        if (!results) return;
        commitSearch();
        loadQueue(
            results.lyric_tracks.map((m) => m.track.id),
            index,
            undefined,
            { kind: "search" },
        );
    }

    async function playAlbum(albumId: number, event: MouseEvent) {
        event.preventDefault();
        event.stopPropagation();
        commitSearch();
        const tracks = await getTracks(albumId);
        if (tracks.length > 0) {
            loadQueue(
                tracks.map((t) => t.id),
                0,
                undefined,
                { kind: "search" },
            );
        }
    }
</script>

<div class="search-page page-enter">
    <div class="header">
        <h1 class="page-title">Search</h1>
    </div>

    <div class="search-box">
        <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
        >
            <circle cx="11" cy="11" r="8" />
            <path d="m21 21-4.3-4.3" />
        </svg>
        <input
            bind:this={inputRef}
            type="search"
            placeholder="Songs, artists, albums, lyrics..."
            bind:value={query}
            spellcheck="false"
            aria-label="Search library"
            onkeydown={handleSearchKeydown}
        />
    </div>

    {#if !query.trim() && history.length > 0}
        <section class="section history-section">
            <div class="history-head">
                <h2 class="section-title">Recent searches</h2>
                <button class="history-clear" onclick={clearHistory}
                    >Clear</button
                >
            </div>
            <div class="history-chips">
                {#each history as item (item)}
                    <button
                        class="history-chip"
                        onclick={() => applyHistory(item)}
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
                            <circle cx="12" cy="12" r="10" />
                            <polyline points="12 6 12 12 16 14" />
                        </svg>
                        {item}
                    </button>
                {/each}
            </div>
        </section>
    {/if}

    {#if searching && !results}
        <Loading />
    {:else if results && !hasResults}
        <div class="empty-state">
            <p class="empty-title">No results for "{query.trim()}"</p>
            <p class="empty-text">
                Try a different title, artist, album, genre, or lyric.
            </p>
        </div>
    {:else if results}
        {#if results.artists.length > 0}
            <section class="section">
                <h2 class="section-title">Artists</h2>
                <ul class="card-grid">
                    {#each results.artists as artist (artist.id)}
                        <li class="card-grid-item artist">
                            <a
                                href={`/artists/${artist.id}`}
                                onclick={commitSearch}
                            >
                                <ArtistAvatar
                                    artistId={artist.id}
                                    alt={artist.name}
                                    class="artist-avatar"
                                />
                                <div class="card-grid-title ellipsis">
                                    {artist.name}
                                </div>
                                <div class="card-grid-meta ellipsis">
                                    {plural(artist.track_count ?? 0, "track")} · {plural(
                                        artist.album_count ?? 0,
                                        "album",
                                    )}
                                </div>
                            </a>
                        </li>
                    {/each}
                </ul>
            </section>
        {/if}

        {#if results.albums.length > 0}
            <section class="section">
                <h2 class="section-title">Albums</h2>
                <ul class="card-grid">
                    {#each results.albums as album (album.id)}
                        <li class="card-grid-item split">
                            <div class="card-grid-thumb-wrap">
                                <a
                                    class="thumb-link"
                                    href={`/albums/${album.id}`}
                                    aria-label="Open {album.title}"
                                    tabindex="-1"
                                    onclick={commitSearch}
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
                                    onclick={(e) => playAlbum(album.id, e)}
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
                                onclick={commitSearch}
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
            </section>
        {/if}

        {#if results.lyric_tracks.length > 0}
            <section class="section">
                <h2 class="section-title">Lyrics</h2>
                <ul class="track-list">
                    {#each results.lyric_tracks as match, index (match.track.id)}
                        <TrackRow
                            track={match.track}
                            {index}
                            variant="songs"
                            onPlay={playLyricTrack}
                            showAddToPlaylist={true}
                            snippet={match.snippet}
                        />
                    {/each}
                </ul>
            </section>
        {/if}

        {#if results.tracks.length > 0}
            <section class="section">
                <h2 class="section-title">Songs</h2>
                <ul class="track-list">
                    {#each results.tracks as track, index (track.id)}
                        <TrackRow
                            {track}
                            {index}
                            variant="songs"
                            onPlay={playTrack}
                            showAddToPlaylist={true}
                        />
                    {/each}
                </ul>
            </section>
        {/if}
    {/if}
</div>

<style>
    .search-page {
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

    .search-box {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        padding: var(--spacing-sm) var(--spacing-md);
        border-radius: var(--radius-full);
        border: 1px solid var(--color-border);
        background-color: var(--color-surface-elevated);
        max-width: 32rem;
    }

    .search-box:focus-within {
        border-color: var(--color-accent-focus);
    }

    .search-box svg {
        width: 1rem;
        height: 1rem;
        color: var(--color-text-muted);
        flex-shrink: 0;
    }

    .search-box input {
        flex: 1;
        border: none;
        background: transparent;
        font-size: var(--font-size-sm);
        color: var(--color-text);
    }

    .search-box input:focus {
        outline: none;
    }

    .search-box input::-webkit-search-cancel-button {
        cursor: pointer;
    }

    .section {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-lg);
    }

    .history-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
    }

    .history-clear {
        font-size: var(--font-size-sm);
        color: var(--color-text-muted);
        transition: color var(--transition-fast);
    }

    .history-clear:hover {
        color: var(--color-text);
        text-decoration: underline;
    }

    .history-chips {
        display: flex;
        flex-wrap: wrap;
        gap: var(--spacing-sm);
    }

    .history-chip {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-xs);
        padding: var(--spacing-xs) var(--spacing-md);
        border-radius: var(--radius-full);
        border: 1px solid var(--color-border);
        background-color: var(--color-surface-elevated);
        font-size: var(--font-size-sm);
        color: var(--color-text);
        transition:
            background-color var(--transition-fast),
            border-color var(--transition-fast);
    }

    .history-chip:hover {
        background-color: var(--color-surface-raised);
        border-color: var(--color-accent-graphic);
    }

    .history-chip svg {
        width: 0.75rem;
        height: 0.75rem;
        color: var(--color-text-muted);
    }

    .track-list {
        display: flex;
        flex-direction: column;
        gap: 2px;
    }
</style>
