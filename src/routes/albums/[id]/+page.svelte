<script lang="ts">
    import { page } from "$app/stores";
    import {
        getAlbum,
        getAlbums,
        getTracks,
        getAlbumArt,
        setAlbumArtFile,
        clearAlbumCustomArt,
        invalidateAlbumArt,
        pickImageFile,
        type Album,
        type Track as ApiTrack,
        type CachedImage,
    } from "$lib/api";
    import { cachedImageToUrl } from "$lib/utils/base64";
    import { plural } from "$lib/utils/text";
    import { loadQueue } from "$lib/stores/playback";
    import Loading from "$lib/components/Loading.svelte";
    import TrackRow from "$lib/components/TrackRow.svelte";
    import Artwork from "$lib/components/Artwork.svelte";
    import ArtistLinks from "$lib/components/ArtistLinks.svelte";
    import { addToast } from "$lib/stores/toast";
    import { windowPageTitle } from "$lib/stores/windowPageTitle";

    let album = $state<Album | null>(null);
    let tracks = $state<ApiTrack[]>([]);
    let moreAlbums = $state<Album[]>([]);
    let albumArt = $state<CachedImage | null>(null);
    let error = $state<string | null>(null);
    let loading = $state(true);
    let artDialogOpen = $state(false);

    const albumId = $derived(Number($page.params.id));

    $effect(() => {
        windowPageTitle.set(album?.title ?? null);
    });

    // Reload when the id changes — album-to-album navigation reuses this
    // component, so onMount alone would leave stale content behind.
    $effect(() => {
        load(albumId);
    });

    async function load(id: number) {
        loading = true;
        error = null;
        album = null;
        tracks = [];
        moreAlbums = [];
        albumArt = null;
        try {
            const [albumData, albumTracks, art] = await Promise.all([
                getAlbum(id),
                getTracks(id),
                getAlbumArt(id),
            ]);
            album = albumData;
            tracks = albumTracks;
            albumArt = art;
            loadMoreFromArtists(albumData);
        } catch (e) {
            error = String(e);
        } finally {
            loading = false;
        }
    }

    // Other albums by this album's artist(s) — "More from X".
    async function loadMoreFromArtists(current: Album) {
        const artistIds = current.artist_ids ?? [];
        if (artistIds.length === 0) return;
        const seen = new Set<number>([current.id]);
        const collected: Album[] = [];
        for (const id of artistIds) {
            try {
                for (const other of await getAlbums(id)) {
                    if (!seen.has(other.id)) {
                        seen.add(other.id);
                        collected.push(other);
                    }
                }
            } catch {
                // skip this artist
            }
        }
        moreAlbums = collected
            .sort(
                (a, b) =>
                    (b.year ?? 0) - (a.year ?? 0) ||
                    a.title.localeCompare(b.title),
            )
            .slice(0, 12);
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

    // Row clicks keep the player's current shuffle mode; the header buttons
    // are explicit context switches: Play = in order, Shuffle = shuffled.
    function playAlbum() {
        if (tracks.length === 0) return;
        loadQueue(
            tracks.map((track) => track.id),
            0,
            false,
        );
    }

    function playTrack(index: number) {
        loadQueue(
            tracks.map((track) => track.id),
            index,
        );
    }

    function shuffleAlbum() {
        if (tracks.length === 0) return;
        const start = Math.floor(Math.random() * tracks.length);
        loadQueue(
            tracks.map((t) => t.id),
            start,
            true,
        );
    }

    async function refreshArt() {
        invalidateAlbumArt(albumId);
        try {
            albumArt = await getAlbumArt(albumId);
        } catch {
            albumArt = null;
        }
    }

    async function handlePickArt() {
        const path = await pickImageFile();
        if (!path) return;
        try {
            await setAlbumArtFile(albumId, path);
            await refreshArt();
            addToast("Album artwork updated", "success");
            artDialogOpen = false;
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    async function handleClearArt() {
        try {
            await clearAlbumCustomArt(albumId);
            await refreshArt();
            addToast("Custom artwork removed", "success");
            artDialogOpen = false;
        } catch (e) {
            addToast(String(e), "error");
        }
    }
</script>

<div class="album-detail page-enter">
    {#if loading}
        <Loading variant="full" />
    {:else}
        {#if error}
            <div class="error">{error}</div>
        {/if}

        <section
            class="hero-section"
            style:--hero-image={albumArt?.file_path
                ? `url(${cachedImageToUrl(albumArt, "")})`
                : "none"}
        >
            <div class="hero-art">
                {#if albumArt?.file_path}
                    <img
                        src={cachedImageToUrl(albumArt, "")}
                        decoding="async"
                        alt={album?.title ?? "Album art"}
                    />
                {:else}
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
                {/if}
                <button
                    class="art-edit-btn"
                    aria-label="Edit album artwork"
                    title="Edit album artwork"
                    onclick={() => (artDialogOpen = true)}
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
                            d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"
                        />
                        <path d="m15 5 4 4" />
                    </svg>
                </button>
            </div>
            <div class="hero-info">
                <span class="hero-label">Album</span>
                <h1 class="page-title">{album?.title ?? "Album"}</h1>
                <p class="hero-meta">
                    <span class="artists">
                        <ArtistLinks
                            names={album?.artist_names}
                            ids={album?.artist_ids}
                            linkClass="hero-artist-link"
                        />
                    </span>
                    <span class="dot">·</span>
                    <span class="year">{album?.year ?? "?"}</span>
                    <span class="dot">·</span>
                    <span class="count"
                        >{plural(tracks.length, "song")}, {totalDuration()}</span
                    >
                </p>
                <div class="hero-actions">
                    <button
                        class="btn-pill btn-primary"
                        onclick={playAlbum}
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
                        onclick={shuffleAlbum}
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
                            <path
                                d="M22 18h-5.9c-1.3 0-2.6-.7-3.3-1.8l-.5-.8"
                            />
                            <path d="m18 14 4 4-4 4" />
                        </svg>
                        Shuffle
                    </button>
                </div>
            </div>
        </section>

        <section class="track-section">
            {#if tracks.length === 0}
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
                    <p class="empty-title">No tracks yet</p>
                    <p class="empty-text">
                        This album doesn't have any tracks in your library.
                    </p>
                </div>
            {:else}
                <div class="track-header album">
                    <span class="header-number">#</span>
                    <span class="header-title">Title</span>
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
                <ul class="track-list">
                    {#each tracks as track, index (track.id)}
                        <TrackRow
                            {track}
                            {index}
                            variant="album"
                            onPlay={playTrack}
                            showAddToPlaylist={true}
                        />
                    {/each}
                </ul>
            {/if}
        </section>

        {#if moreAlbums.length > 0}
            <section class="track-section">
                <h2 class="section-title">
                    More from {album?.artist_names?.join(", ") ?? "this artist"}
                </h2>
                <ul class="card-grid">
                    {#each moreAlbums as other, index (other.id)}
                        <li
                            class="card-grid-item card-enter"
                            style="animation-delay: {index * 40}ms"
                        >
                            <a href={`/albums/${other.id}`}>
                                <Artwork
                                    albumId={other.id}
                                    alt={other.title}
                                    class="card-grid-thumb"
                                />
                                <div class="card-grid-title ellipsis">
                                    {other.title}
                                </div>
                                <div class="card-grid-meta ellipsis">
                                    {#if other.year}{other.year} ·
                                    {/if}
                                    {other.artist_names?.join(", ") ?? ""}
                                </div>
                            </a>
                        </li>
                    {/each}
                </ul>
            </section>
        {/if}
    {/if}
</div>

{#if artDialogOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
        class="dialog-overlay"
        role="presentation"
        tabindex="-1"
        onclick={() => (artDialogOpen = false)}
        onkeydown={(e: KeyboardEvent) => {
            if (e.key === "Escape") artDialogOpen = false;
        }}
    >
        <div
            class="dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="art-dialog-title"
            tabindex="-1"
            onclick={(e: MouseEvent) => e.stopPropagation()}
        >
            <h2 id="art-dialog-title" class="dialog-title">Album artwork</h2>
            <div class="dialog-body">
                <div class="image-actions">
                    <button
                        class="btn-pill btn-secondary"
                        onclick={handlePickArt}
                    >
                        Choose image...
                    </button>
                    <button
                        class="btn-pill btn-secondary"
                        onclick={handleClearArt}
                    >
                        Use online art
                    </button>
                </div>
                <p class="hint">
                    Pick a local image file, or let the app fetch art from your
                    configured sources.
                </p>
            </div>
            <div class="dialog-actions">
                <button
                    class="btn-pill btn-secondary"
                    onclick={() => (artDialogOpen = false)}>Close</button
                >
            </div>
        </div>
    </div>
{/if}

<style>
    .album-detail {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-2xl);
    }

    .error {
        background-color: var(--color-error);
        color: var(--color-text);
        padding: var(--spacing-md);
        border-radius: var(--radius-lg);
        font-size: var(--font-size-sm);
    }

    .hero-meta .dot {
        margin: 0 var(--spacing-sm);
    }

    :global(.hero-artist-link) {
        color: inherit;
        transition: color var(--transition-fast);
    }

    :global(.hero-artist-link:hover) {
        color: var(--color-text);
        text-decoration: underline;
    }

    .hero-art {
        position: relative;
    }

    .art-edit-btn {
        position: absolute;
        bottom: var(--spacing-sm);
        right: var(--spacing-sm);
        display: flex;
        align-items: center;
        justify-content: center;
        width: 2rem;
        height: 2rem;
        border-radius: var(--radius-full);
        background-color: rgba(0, 0, 0, 0.65);
        color: rgba(255, 255, 255, 0.85);
        opacity: 0;
        transition:
            opacity var(--transition-fast),
            background-color var(--transition-fast);
    }

    .hero-art:hover .art-edit-btn,
    .art-edit-btn:focus-visible {
        opacity: 1;
    }

    .art-edit-btn:hover {
        background-color: rgba(0, 0, 0, 0.85);
    }

    .art-edit-btn svg {
        width: 0.875rem;
        height: 0.875rem;
    }

    .track-section {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
    }

    .dialog-overlay {
        position: fixed;
        inset: 0;
        z-index: 100;
        display: flex;
        align-items: center;
        justify-content: center;
        background-color: rgba(0, 0, 0, 0.6);
        backdrop-filter: blur(8px);
        -webkit-backdrop-filter: blur(8px);
        padding: var(--spacing-md);
    }

    .dialog {
        width: 100%;
        max-width: 400px;
        background-color: var(--color-surface);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-xl);
        padding: var(--spacing-xl);
        display: flex;
        flex-direction: column;
        gap: var(--spacing-lg);
        box-shadow: var(--shadow-lg);
    }

    .dialog-title {
        font-size: var(--font-size-xl);
        font-weight: var(--font-weight-bold);
        letter-spacing: -0.01em;
    }

    .dialog-body {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
    }

    .image-actions {
        display: flex;
        gap: var(--spacing-sm);
        flex-wrap: wrap;
    }

    .hint {
        margin: 0;
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
        line-height: var(--line-height);
    }

    .dialog-actions {
        display: flex;
        justify-content: flex-end;
        gap: var(--spacing-md);
    }
</style>
