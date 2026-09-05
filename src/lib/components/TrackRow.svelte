<script lang="ts">
    import { playback, playNext } from "$lib/stores/playback";
    import {
        getPlaylists,
        addTracksToPlaylist,
        setTrackCustomLyrics,
        clearTrackCustomLyrics,
        pickLyricsFile,
        revealInExplorer,
        type Playlist,
    } from "$lib/api";
    import Artwork from "$lib/components/Artwork.svelte";
    import ArtistLinks from "$lib/components/ArtistLinks.svelte";
    import { addToast } from "$lib/stores/toast";
    import { goto } from "$app/navigation";

    interface TrackData {
        id: number;
        file_path?: string;
        title: string | null;
        artist_names?: string[] | null;
        artist_ids?: number[] | null;
        album_title?: string | null;
        album_id?: number | null;
        duration_ms?: number | null;
        track_number?: number | null;
        lyrics_source?: string | null;
    }

    interface Props {
        track: TrackData;
        index: number;
        variant?: "songs" | "album" | "artist";
        onPlay: (index: number) => void;
        showAddToPlaylist?: boolean;
        snippet?: string;
        anchorId?: string;
    }

    let {
        track,
        index,
        variant = "album",
        onPlay,
        showAddToPlaylist = false,
        snippet,
        anchorId,
    }: Props = $props();

    let isCurrent = $derived($playback.current_track?.id === track.id);
    let isPlaying = $derived(isCurrent && $playback.is_playing);

    let menuOpen = $state(false);
    let playlists = $state<Playlist[]>([]);
    let playlistsLoading = $state(false);
    let menuButtonRef: HTMLButtonElement | undefined = $state();
    let menuRef: HTMLDivElement | undefined = $state();

    let playlistsPromise: Promise<Playlist[]> | null = null;

    function formatDuration(ms = 0): string {
        const totalSeconds = Math.round(ms / 1000);
        const m = Math.floor(totalSeconds / 60);
        const s = totalSeconds % 60;
        return `${m}:${s.toString().padStart(2, "0")}`;
    }

    function stopPropagation(event: MouseEvent) {
        event.stopPropagation();
    }

    function loadManualPlaylists() {
        if (!playlistsPromise) {
            playlistsPromise = getPlaylists().then((all) =>
                all.filter((p) => !p.folder_path && !p.live_mix),
            );
        }
        return playlistsPromise;
    }

    async function openMenu() {
        menuOpen = true;
        if (playlists.length === 0 && !playlistsLoading) {
            playlistsLoading = true;
            try {
                playlists = await loadManualPlaylists();
            } catch (e) {
                addToast(String(e), "error");
            } finally {
                playlistsLoading = false;
            }
        }
    }

    function toggleMenu(event: MouseEvent) {
        event.stopPropagation();
        if (menuOpen) {
            menuOpen = false;
        } else {
            openMenu();
        }
    }

    async function addToPlaylist(playlist: Playlist) {
        try {
            await addTracksToPlaylist(playlist.id, [track.id]);
            addToast(`Added to ${playlist.name}`, "success");
            menuOpen = false;
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    async function handlePlayNext() {
        try {
            await playNext(track.id);
            addToast("Will play next", "success");
            menuOpen = false;
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    async function copyTrackInfo() {
        const artist = track.artist_names?.join(", ") || "Unknown artist";
        const info = `${track.title ?? "Unknown"} — ${artist}${track.album_title ? ` · ${track.album_title}` : ""}`;
        try {
            await navigator.clipboard.writeText(info);
            addToast("Song info copied", "success");
            menuOpen = false;
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    function openAlbum() {
        if (!track.album_id) return;
        menuOpen = false;
        goto(`/albums/${track.album_id}`);
    }

    async function pickCustomLyrics() {
        try {
            const path = await pickLyricsFile();
            if (!path) return;
            await setTrackCustomLyrics(track.id, path);
            addToast("Custom lyrics saved", "success");
            menuOpen = false;
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    async function removeCustomLyrics() {
        try {
            await clearTrackCustomLyrics(track.id);
            addToast("Custom lyrics deleted", "success");
            menuOpen = false;
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    function handleWindowClick(event: MouseEvent) {
        if (!menuOpen) return;
        const target = event.target as Node;
        if (menuRef?.contains(target) || menuButtonRef?.contains(target))
            return;
        menuOpen = false;
    }

    function handleKeydown(event: KeyboardEvent) {
        if (event.key === "Escape" && menuOpen) {
            event.stopPropagation();
            menuOpen = false;
        }
    }
</script>

<svelte:window onclick={handleWindowClick} onkeydown={handleKeydown} />

<li
    class="track-row"
    class:current={isCurrent}
    class:playing={isPlaying}
    data-variant={variant}
    id={anchorId}
    aria-label="Play {track.title ?? 'Unknown'}"
>
    <button
        class="row-hit-area"
        aria-label="Play {track.title ?? 'Unknown'}"
        onclick={() => onPlay(index)}
    ></button>

    {#if variant === "album"}
        <div class="track-index">
            <span class="index-number">{track.track_number ?? index + 1}</span>
            <button
                class="play-button"
                aria-label="Play"
                onclick={(e) => {
                    stopPropagation(e);
                    onPlay(index);
                }}
            >
                <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                    <path d="M8 5v14l11-7z" />
                </svg>
            </button>
        </div>
    {:else}
        <div class="track-cover">
            <Artwork albumId={track.album_id} alt="" class="cover-img" />
            <button
                class="cover-play"
                aria-label="Play"
                onclick={(e) => {
                    stopPropagation(e);
                    onPlay(index);
                }}
            >
                <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                    <path d="M8 5v14l11-7z" />
                </svg>
            </button>
        </div>
    {/if}

    <div class="track-main">
        <div class="track-info">
            <span class="track-title ellipsis" class:current={isCurrent}>
                {track.title ?? "Unknown"}
            </span>
            {#if variant === "artist"}
                {#if track.album_title}
                    <a
                        class="track-artist ellipsis track-album-link"
                        href={`/albums/${track.album_id}`}
                        onclick={(e) => e.stopPropagation()}
                    >
                        {track.album_title}
                    </a>
                {/if}
            {:else if variant === "songs" || (variant === "album" && track.artist_names && track.artist_names.length > 0)}
                <span class="track-artist ellipsis">
                    <ArtistLinks
                        names={track.artist_names}
                        ids={track.artist_ids}
                    />
                </span>
            {/if}
            {#if snippet}
                <span class="track-snippet ellipsis">"{snippet}"</span>
            {/if}
        </div>
    </div>

    {#if variant === "songs"}
        <a
            class="track-album ellipsis"
            href={track.album_id ? `/albums/${track.album_id}` : undefined}
            onclick={(e) => e.stopPropagation()}
        >
            {track.album_title ?? ""}
        </a>
    {/if}

    {#if showAddToPlaylist}
        <div class="track-actions" class:open={menuOpen}>
            <button
                bind:this={menuButtonRef}
                class="more-button"
                aria-label="More"
                aria-haspopup="true"
                aria-expanded={menuOpen}
                onclick={toggleMenu}
            >
                <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                    <circle cx="12" cy="6" r="1.5" />
                    <circle cx="12" cy="12" r="1.5" />
                    <circle cx="12" cy="18" r="1.5" />
                </svg>
            </button>
            {#if menuOpen}
                <div
                    bind:this={menuRef}
                    class="popover"
                    role="menu"
                    tabindex="-1"
                    onclick={(e) => e.stopPropagation()}
                    onkeydown={(e) => e.stopPropagation()}
                >
                    <ul class="popover-list">
                        <li>
                            <button
                                class="popover-item"
                                role="menuitem"
                                onclick={handlePlayNext}
                            >
                                Play next
                            </button>
                        </li>
                        {#if track.album_id}
                            <li>
                                <button
                                    class="popover-item"
                                    role="menuitem"
                                    onclick={openAlbum}
                                >
                                    Go to album
                                </button>
                            </li>
                        {/if}
                        {#if track.file_path}
                            <li>
                                <button
                                    class="popover-item"
                                    role="menuitem"
                                    onclick={() => {
                                        revealInExplorer(track.file_path!);
                                        menuOpen = false;
                                    }}
                                >
                                    Show in Explorer
                                </button>
                            </li>
                        {/if}
                        <li>
                            <button
                                class="popover-item"
                                role="menuitem"
                                onclick={copyTrackInfo}
                            >
                                Copy song info
                            </button>
                        </li>
                    </ul>
                    <div class="popover-divider" aria-hidden="true"></div>
                    <div class="popover-heading">Add to Playlist</div>
                    {#if playlistsLoading}
                        <div class="popover-loading">Loading playlists...</div>
                    {:else if playlists.length === 0}
                        <div class="popover-empty">No playlists available</div>
                    {:else}
                        <ul class="popover-list">
                            {#each playlists as playlist (playlist.id)}
                                <li>
                                    <button
                                        class="popover-item"
                                        role="menuitem"
                                        onclick={() => addToPlaylist(playlist)}
                                    >
                                        {playlist.name}
                                    </button>
                                </li>
                            {/each}
                        </ul>
                    {/if}
                    <div class="popover-divider" aria-hidden="true"></div>
                    <div class="popover-heading">Lyrics File</div>
                    <ul class="popover-list">
                        <li>
                            <button
                                class="popover-item"
                                role="menuitem"
                                onclick={pickCustomLyrics}
                            >
                                Choose custom lyrics...
                            </button>
                        </li>
                        <li>
                            <button
                                class="popover-item"
                                role="menuitem"
                                onclick={removeCustomLyrics}
                            >
                                Delete custom lyrics
                            </button>
                        </li>
                    </ul>
                </div>
            {/if}
        </div>
    {/if}

    <span class="track-duration">{formatDuration(track.duration_ms ?? 0)}</span>
</li>

<style>
    .row-hit-area {
        position: absolute;
        inset: 0;
        background: transparent;
        border: none;
        padding: 0;
        margin: 0;
        cursor: pointer;
        z-index: 0;
        border-radius: var(--radius);
    }

    .track-index,
    .track-cover,
    .track-main,
    .track-album,
    .track-duration {
        position: relative;
        z-index: 1;
    }

    .track-index {
        position: relative;
        display: flex;
        align-items: center;
        justify-content: center;
        width: 2.5rem;
        height: 2.5rem;
        flex-shrink: 0;
    }

    .index-number {
        font-size: var(--font-size-sm);
        color: var(--color-text-muted);
        text-align: center;
        font-variant-numeric: tabular-nums;
    }

    .track-row.current .index-number {
        color: var(--color-accent-content);
    }

    .track-row:hover .index-number,
    .track-row:focus-visible .index-number {
        display: none;
    }

    .play-button {
        position: relative;
        z-index: 1;
        display: none;
        align-items: center;
        justify-content: center;
        width: 1.5rem;
        height: 1.5rem;
        color: var(--color-text);
        background: none;
        border: none;
        padding: 0;
        cursor: pointer;
        transition:
            transform var(--transition-fast),
            color var(--transition-fast);
    }

    .track-row:hover .play-button,
    .track-row:focus-visible .play-button {
        display: flex;
    }

    .play-button:hover {
        color: var(--color-accent-graphic);
        transform: scale(var(--motion-hover-scale));
    }

    .play-button svg {
        width: 100%;
        height: 100%;
    }

    .track-cover {
        position: relative;
        width: 2.5rem;
        height: 2.5rem;
        flex-shrink: 0;
        border-radius: var(--radius-sm);
        overflow: hidden;
        background-color: var(--color-surface-elevated);
        color: var(--color-text-muted);
    }

    .track-cover :global(.cover-img) {
        width: 100%;
        height: 100%;
        object-fit: cover;
        border-radius: var(--radius-sm);
    }

    .cover-play {
        position: absolute;
        inset: 0;
        z-index: 1;
        display: flex;
        align-items: center;
        justify-content: center;
        background-color: rgba(0, 0, 0, 0.55);
        color: #fff;
        border: none;
        padding: 0;
        cursor: pointer;
        opacity: 0;
        transition: opacity var(--transition-fast);
    }

    .track-row:hover .cover-play,
    .track-row:focus-visible .cover-play {
        opacity: 1;
    }

    .cover-play svg {
        width: 1.25rem;
        height: 1.25rem;
    }

    .track-main {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        min-width: 0;
    }

    .track-info {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        min-width: 0;
    }

    .track-title {
        font-size: var(--font-size-sm);
        color: var(--color-text);
        font-weight: var(--font-weight-medium);
        transition: color var(--transition-fast);
    }

    .track-title.current {
        color: var(--color-accent-content);
    }

    .track-artist {
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
    }

    .track-album-link {
        display: block;
        transition: color var(--transition-fast);
    }

    .track-album-link:hover {
        color: var(--color-text);
    }

    .track-snippet {
        font-size: var(--font-size-xs);
        color: var(--color-accent-content);
        font-style: italic;
    }

    :global(.track-artist a) {
        color: inherit;
        transition: color var(--transition-fast);
    }

    :global(.track-artist a:hover) {
        color: var(--color-text);
    }

    .track-album {
        font-size: var(--font-size-sm);
        color: var(--color-text-muted);
        transition: color var(--transition-fast);
    }

    .track-album:hover {
        color: var(--color-text);
    }

    .track-actions {
        position: absolute;
        top: 50%;
        right: var(--spacing-md);
        transform: translateY(-50%);
        z-index: 2;
        display: none;
        align-items: center;
        justify-content: center;
    }

    .track-row:hover .track-actions,
    .track-row:focus-visible .track-actions,
    .track-actions.open {
        display: flex;
    }

    .track-row:hover .track-duration,
    .track-row:focus-visible .track-duration,
    .track-actions.open ~ .track-duration {
        opacity: 0;
        pointer-events: none;
    }

    .more-button {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 1.75rem;
        height: 1.75rem;
        border-radius: var(--radius-full);
        background: transparent;
        color: var(--color-text-muted);
        border: none;
        cursor: pointer;
        transition:
            color var(--transition-fast),
            background-color var(--transition-fast);
    }

    .more-button:hover,
    .more-button:focus-visible {
        color: var(--color-text);
        background-color: var(--interactive-hover);
    }

    .more-button svg {
        width: 1.125rem;
        height: 1.125rem;
    }

    .popover {
        position: absolute;
        top: calc(100% + 0.5rem);
        right: 0;
        min-width: 12rem;
        max-width: 16rem;
        max-height: 16rem;
        overflow-y: auto;
        background-color: var(--color-surface-elevated);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-lg);
        box-shadow: var(--shadow-lg);
        padding: var(--spacing-xs) 0;
        z-index: 10;
    }

    .popover-list {
        display: flex;
        flex-direction: column;
    }

    .popover-divider {
        height: 1px;
        background-color: var(--color-border);
        margin: var(--spacing-xs) 0;
    }

    .popover-heading {
        padding: var(--spacing-xs) var(--spacing-md);
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-semibold);
        letter-spacing: normal;
        color: var(--color-text-muted);
    }

    .popover-item,
    .popover-empty,
    .popover-loading {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-sm);
        width: 100%;
        text-align: left;
        padding: var(--spacing-sm) var(--spacing-md);
        font-size: var(--font-size-sm);
        color: var(--color-text);
        background: transparent;
        border: none;
        cursor: pointer;
        border-radius: 0;
    }

    .popover-item:hover,
    .popover-item:focus-visible {
        background-color: var(--interactive-hover);
    }

    .popover-empty,
    .popover-loading {
        color: var(--color-text-muted);
        cursor: default;
    }

    .track-duration {
        font-size: var(--font-size-sm);
        color: var(--color-text-muted);
        text-align: right;
        font-variant-numeric: tabular-nums;
        transition: opacity var(--transition-fast);
    }

    @media (max-width: 767px) {
        .track-row[data-variant="songs"] .track-album {
            display: none;
        }
    }

    @media (max-width: 480px) {
        .track-row[data-variant="songs"] .track-artist,
        .track-row[data-variant="album"] .track-artist,
        .track-row[data-variant="artist"] .track-artist {
            display: none;
        }
    }
</style>
