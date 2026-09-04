<script lang="ts">
    import { onMount } from "svelte";
    import {
        playback,
        interpolatedPositionMs,
        seek,
        updateCurrentTrackLrcOffset,
        updateCurrentTrackLyricsSource,
    } from "$lib/stores/playback";
    import {
        getLyrics,
        getAlbumArt,
        setLrcOffset,
        getOnlineSettings,
        setTrackLyricsSource,
        setTrackCustomLyrics,
        clearTrackCustomLyrics,
        searchLyricsOnline,
        setTrackLyricsChoice,
        pickLyricsFile,
        LYRICS_CHANGED_EVENT,
        type Lyrics,
        type LyricCandidate,
        type LyricSearchResults,
    } from "$lib/api";
    import { cachedImageToUrl } from "$lib/utils/base64";
    import { getFontStack } from "$lib/utils/fonts";
    import { addToast } from "$lib/stores/toast";
    import { listen } from "@tauri-apps/api/event";
    import SyncedLyrics from "$lib/components/SyncedLyrics.svelte";
    import Select from "$lib/components/Select.svelte";
    import ArtistLinks from "$lib/components/ArtistLinks.svelte";

    let lyrics = $state<Lyrics | null>(null);
    let loading = $state(false);
    let error = $state<string | null>(null);
    let lastTrackId = $state<number | null>(null);
    let lyricsRequest = 0;
    let lastAlbumId = $state<number | null>(null);
    let artUrl = $state("");
    let lyricsFont = $state(getFontStack("Monospace"));
    let providerDialogOpen = $state(false);
    let providerChoice = $state("default");

    const PROVIDER_OPTIONS = [
        { value: "default", label: "Default (settings order)" },
        { value: "custom", label: "Custom lyrics" },
        { value: "embedded", label: "Embedded" },
        { value: "lrc", label: "Sidecar .lrc file" },
        { value: "lrclib", label: "LRCLIB" },
        { value: "netease", label: "NetEase" },
        { value: "kashinavi", label: "KashiNavi" },
        { value: "qq", label: "QQ Music" },
        { value: "none", label: "No lyrics" },
    ];

    const PROVIDER_LABELS: Record<string, string> = {
        embedded: "Embedded",
        lrc: "Sidecar .lrc",
        lrclib: "LRCLIB",
        netease: "NetEase",
        kashinavi: "KashiNavi",
        qq: "QQ Music",
        custom: "Custom lyrics",
        none: "No lyrics",
    };

    let providerLabel = $derived(
        lyrics?.source
            ? (PROVIDER_LABELS[lyrics.source] ?? lyrics.source)
            : null,
    );

    async function loadLyrics(trackId: number, force = false) {
        if (trackId === lastTrackId && !force) return;
        const request = ++lyricsRequest;
        lastTrackId = trackId;
        loading = true;
        error = null;
        lyrics = null;
        offsetMs =
            $playback.current_track?.id === trackId
                ? ($playback.current_track.lrc_offset_ms ?? 0)
                : 0;
        try {
            const loadedLyrics = await getLyrics(trackId);
            if (
                request !== lyricsRequest ||
                $playback.current_track?.id !== trackId
            ) {
                return;
            }
            lyrics = loadedLyrics;
        } catch (e) {
            if (
                request !== lyricsRequest ||
                $playback.current_track?.id !== trackId
            ) {
                return;
            }
            error = String(e);
            lyrics = null;
        } finally {
            if (request === lyricsRequest) loading = false;
        }
    }

    async function loadArt(albumId: number | null) {
        if (albumId === lastAlbumId) return;
        lastAlbumId = albumId;
        if (!albumId) {
            artUrl = "";
            return;
        }
        const requestedAlbumId = albumId;
        try {
            const art = await getAlbumArt(requestedAlbumId);
            if (requestedAlbumId !== lastAlbumId) return;
            artUrl = cachedImageToUrl(art, "");
        } catch {
            if (requestedAlbumId !== lastAlbumId) return;
            artUrl = "";
        }
    }

    onMount(() => {
        let unlisten: (() => void) | undefined;
        const handleLyricsChanged = (event: Event) => {
            const trackId = (event as CustomEvent<{ trackId: number }>).detail
                ?.trackId;
            const track = $playback.current_track;
            if (track && track.id === trackId) {
                loadLyrics(track.id, true);
            }
        };
        window.addEventListener(LYRICS_CHANGED_EVENT, handleLyricsChanged);
        (async () => {
            try {
                const onlineSettings = await getOnlineSettings();
                lyricsFont = getFontStack(
                    onlineSettings.lyrics_font || "Monospace",
                );
            } catch {
                lyricsFont = getFontStack("Monospace");
            }
            if ($playback.current_track) {
                loadLyrics($playback.current_track.id);
                loadArt($playback.current_track.album_id);
            }
            unlisten = await listen("online-settings-changed", () => {
                const track = $playback.current_track;
                if (track) {
                    lyrics = null;
                    loadLyrics(track.id, true);
                }
            });
        })();
        return () => {
            unlisten?.();
            window.removeEventListener(
                LYRICS_CHANGED_EVENT,
                handleLyricsChanged,
            );
        };
    });

    $effect(() => {
        const track = $playback.current_track;
        if (track) {
            loadLyrics(track.id);
            loadArt(track.album_id);
        } else {
            lyricsRequest += 1;
            lyrics = null;
            loading = false;
            lastTrackId = null;
            lastAlbumId = null;
            artUrl = "";
            offsetMs = 0;
        }
    });

    let offsetMs = $state(0);
    let offsetSaveTimeout = $state<ReturnType<typeof setTimeout> | null>(null);

    function saveOffset(trackId: number, value: number) {
        if (offsetSaveTimeout) clearTimeout(offsetSaveTimeout);
        offsetSaveTimeout = setTimeout(async () => {
            try {
                await setLrcOffset(trackId, value);
                updateCurrentTrackLrcOffset(trackId, value);
            } catch (e) {
                console.error("Failed to save LRC offset:", e);
            }
        }, 500);
    }

    function adjustOffset(delta: number) {
        const track = $playback.current_track;
        if (!track) return;
        offsetMs = Math.max(-5000, Math.min(5000, offsetMs + delta));
        saveOffset(track.id, offsetMs);
    }

    function handleSeek(timeMs: number) {
        seek(timeMs);
    }

    function openProviderDialog() {
        providerChoice = $playback.current_track?.lyrics_source ?? "default";
        lyricCandidates = [];
        lyricSearchQuery = defaultLyricQuery();
        providerDialogOpen = true;
    }

    function defaultLyricQuery(): string {
        const track = $playback.current_track;
        if (!track) return "";
        return [track.title ?? "", track.artist_names?.join(" ") ?? ""]
            .join(" ")
            .trim();
    }

    async function applyProviderChoice() {
        const track = $playback.current_track;
        if (!track) return;
        try {
            if (providerChoice === "custom") {
                await setTrackLyricsSource(track.id, "custom");
                updateCurrentTrackLyricsSource(track.id, "custom");
                addToast("Custom lyrics selected", "success");
            } else {
                await setTrackLyricsSource(
                    track.id,
                    providerChoice === "default" ? undefined : providerChoice,
                );
                updateCurrentTrackLyricsSource(
                    track.id,
                    providerChoice === "default" ? null : providerChoice,
                );
                addToast("Lyrics source updated", "success");
            }
            providerDialogOpen = false;
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    async function chooseCustomLyrics() {
        const track = $playback.current_track;
        if (!track) return;
        try {
            const path = await pickLyricsFile();
            if (!path) return;
            await setTrackCustomLyrics(track.id, path);
            updateCurrentTrackLyricsSource(track.id, "custom");
            addToast("Custom lyrics saved", "success");
            providerDialogOpen = false;
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    async function removeCustomLyrics() {
        const track = $playback.current_track;
        if (!track) return;
        try {
            await clearTrackCustomLyrics(track.id);
            const fallbackSource =
                track.lyrics_source === "custom"
                    ? null
                    : (track.lyrics_source ?? null);
            updateCurrentTrackLyricsSource(track.id, fallbackSource);
            providerChoice = fallbackSource ?? "default";
            addToast("Custom lyrics deleted", "success");
            providerDialogOpen = false;
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    // --- Manual lyrics search ------------------------------------------------
    let lyricSearchResults = $state<LyricSearchResults | null>(null);
    let lyricCandidates = $derived(lyricSearchResults?.candidates ?? []);
    let lyricSearchQuery = $state("");
    let lyricSearching = $state(false);
    let lyricApplying = $state(false);

    async function runLyricSearch() {
        const track = $playback.current_track;
        if (!track || lyricSearching) return;
        lyricSearching = true;
        lyricSearchResults = null;
        try {
            lyricSearchResults = await searchLyricsOnline(
                track.id,
                lyricSearchQuery,
            );
            if (lyricCandidates.length === 0) {
                addToast("No lyrics found online", "error");
            }
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            lyricSearching = false;
        }
    }

    async function applyLyricCandidate(candidate: LyricCandidate) {
        const track = $playback.current_track;
        if (!track || lyricApplying) return;
        lyricApplying = true;
        try {
            await setTrackLyricsChoice(track.id, {
                source: candidate.source,
                syncedText: candidate.synced_text,
                plainText: candidate.plain_text,
            });
            addToast("Lyrics updated", "success");
            providerDialogOpen = false;
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            lyricApplying = false;
        }
    }
</script>

<div class="now-playing">
    <div class="np-clip" aria-hidden="true">
        <div
            class="np-backdrop"
            style:background-image={artUrl ? `url(${artUrl})` : "none"}
        ></div>
    </div>
    <div class="art-panel">
        {#if $playback.current_track}
            {@const track = $playback.current_track}
            <div class="art">
                {#if artUrl}
                    <img src={artUrl} alt={track.album_title ?? "Album art"} />
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
                        <path
                            d="M9 18V5l12-2v13M6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zm12-2a3 3 0 1 0 0-6 3 3 0 0 0 0 6z"
                        />
                    </svg>
                {/if}
            </div>
            <div class="track-info">
                <h1 class="page-title">{track.title ?? "Unknown"}</h1>
                <p class="artist">
                    <ArtistLinks
                        names={track.artist_names}
                        ids={track.artist_ids}
                        linkClass="artist-link"
                    />
                </p>
                {#if track.album_id && track.album_title}
                    <p class="album">
                        <a class="album-link" href={`/albums/${track.album_id}`}
                            >{track.album_title}</a
                        >
                    </p>
                {/if}
            </div>
        {:else}
            <div class="art">
                <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.5"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    <path
                        d="M9 18V5l12-2v13M6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zm12-2a3 3 0 1 0 0-6 3 3 0 0 0 0 6z"
                    />
                </svg>
            </div>
            <div class="track-info">
                <h1 class="page-title">No track selected</h1>
                <p class="artist">
                    Choose a song from your library to start listening.
                </p>
            </div>
        {/if}
    </div>

    <div class="lyrics-panel" style:font-family={lyricsFont}>
        <div class="lyrics-header">
            <div class="lyrics-title-row">
                <h2 class="section-title">Lyrics</h2>
                {#if providerLabel}
                    <button
                        class="provider-tag"
                        onclick={openProviderDialog}
                        title="Change lyrics source"
                    >
                        From {providerLabel}
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
                {:else if $playback.current_track}
                    <button
                        class="provider-tag"
                        onclick={openProviderDialog}
                        title="Change lyrics source"
                    >
                        Source
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
                {/if}
            </div>
            {#if lyrics?.synced_text}
                <div class="offset-controls" aria-label="Lyrics timing offset">
                    <button
                        class="offset-btn"
                        onclick={() => adjustOffset(-50)}
                        aria-label="Shift lyrics earlier">−50ms</button
                    >
                    <span class="offset-value"
                        >{offsetMs > 0
                            ? `+${offsetMs}ms`
                            : `${offsetMs}ms`}</span
                    >
                    <button
                        class="offset-btn"
                        onclick={() => adjustOffset(50)}
                        aria-label="Shift lyrics later">+50ms</button
                    >
                    <button
                        class="offset-btn reset"
                        onclick={() => {
                            offsetMs = 0;
                            const track = $playback.current_track;
                            if (track) saveOffset(track.id, 0);
                        }}
                        aria-label="Reset lyrics offset">Reset</button
                    >
                </div>
            {/if}
        </div>
        {#if loading}
            <p class="status">Loading lyrics...</p>
        {:else if error}
            <p class="status error">{error}</p>
        {:else if lyrics}
            <SyncedLyrics
                syncedText={lyrics.synced_text}
                plainText={lyrics.plain_text}
                currentTimeMs={$interpolatedPositionMs}
                {offsetMs}
                onSeek={handleSeek}
            />
        {:else}
            <p class="status">No lyrics found.</p>
        {/if}
    </div>
</div>

{#if providerDialogOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
        class="dialog-overlay"
        role="presentation"
        tabindex="-1"
        onclick={() => (providerDialogOpen = false)}
        onkeydown={(e: KeyboardEvent) => {
            if (e.key === "Escape") providerDialogOpen = false;
        }}
    >
        <div
            class="dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="lyrics-provider-title"
            tabindex="-1"
            onclick={(e: MouseEvent) => e.stopPropagation()}
        >
            <h2 id="lyrics-provider-title" class="dialog-title">
                Lyrics source
            </h2>
            <div class="dialog-body">
                <Select
                    options={PROVIDER_OPTIONS}
                    value={providerChoice}
                    onchange={(v) => (providerChoice = v)}
                    ariaLabel="Lyrics source"
                />
                <p class="hint">
                    Choose this song's lyrics provider. Custom lyrics are saved
                    for this song; a sidecar .lrc file is read from beside the
                    audio file. No lyrics disables lookup for this song.
                </p>

                {#if providerChoice === "custom"}
                    <div class="custom-lyrics-actions">
                        <button
                            class="btn-pill btn-secondary"
                            onclick={chooseCustomLyrics}
                        >
                            {$playback.current_track?.lyrics_source === "custom"
                                ? "Replace custom lyrics file"
                                : "Choose custom lyrics file"}
                        </button>
                        <button
                            class="btn-pill btn-secondary"
                            onclick={removeCustomLyrics}
                        >
                            Delete custom lyrics
                        </button>
                    </div>
                    <p class="hint custom-lyrics-hint">
                        Custom lyrics stay with this song. Picked files are
                        copied into Sparkle's cache, so the original can move or
                        be deleted.
                    </p>
                {/if}

                <div class="lyric-search">
                    <p class="hint">
                        Searches your enabled online providers together and
                        keeps each result separate.
                    </p>
                    <div class="lyric-search-row">
                        <input
                            type="text"
                            bind:value={lyricSearchQuery}
                            placeholder="Search lyrics…"
                            spellcheck="false"
                            aria-label="Lyrics search query"
                            onkeydown={(e) => {
                                if (e.key === "Enter") runLyricSearch();
                            }}
                        />
                        <button
                            class="btn-pill btn-secondary"
                            onclick={runLyricSearch}
                            disabled={lyricSearching}
                        >
                            {lyricSearching ? "Searching…" : "Search"}
                        </button>
                    </div>
                    {#if lyricSearchResults}
                        <p class="provider-status">
                            Searched {lyricSearchResults.enabled_sources
                                .map(
                                    (source) =>
                                        PROVIDER_LABELS[source] ?? source,
                                )
                                .join(", ") || "no enabled providers"}.
                            {#if lyricSearchResults.failed_sources.length > 0}
                                Failed: {lyricSearchResults.failed_sources
                                    .map(
                                        (source) =>
                                            PROVIDER_LABELS[source] ?? source,
                                    )
                                    .join(", ")}.
                            {/if}
                            {#if lyricSearchResults.timed_out_sources.length > 0}
                                Timed out: {lyricSearchResults.timed_out_sources
                                    .map(
                                        (source) =>
                                            PROVIDER_LABELS[source] ?? source,
                                    )
                                    .join(", ")}.
                            {/if}
                        </p>
                    {/if}
                    {#if lyricCandidates.length > 0}
                        <ul class="lyric-candidates">
                            {#each lyricCandidates as candidate, i (i)}
                                <li>
                                    <button
                                        class="lyric-candidate"
                                        onclick={() =>
                                            applyLyricCandidate(candidate)}
                                        disabled={lyricApplying}
                                    >
                                        <span class="candidate-source-tag"
                                            >{PROVIDER_LABELS[
                                                candidate.source
                                            ] ?? candidate.source}</span
                                        >
                                        <span class="candidate-preview"
                                            >{candidate.preview}</span
                                        >
                                    </button>
                                </li>
                            {/each}
                        </ul>
                        <p class="hint">
                            Click a result to use it for this song.
                        </p>
                    {/if}
                </div>
            </div>
            <div class="dialog-actions">
                <button
                    class="btn-pill btn-secondary"
                    onclick={() => (providerDialogOpen = false)}>Cancel</button
                >
                <button
                    class="btn-pill btn-primary"
                    onclick={applyProviderChoice}>Apply</button
                >
            </div>
        </div>
    </div>
{/if}

<style>
    .now-playing {
        position: relative;
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: var(--spacing-2xl);
        align-items: start;
        min-height: 60vh;
        isolation: isolate;
    }

    /* The album cover, zoomed hard and blurred to an ambient wash. The clip
     wrapper keeps the spill out of the sidebar. */
    .np-clip {
        position: fixed;
        top: 0;
        left: var(--sidebar-width);
        right: 0;
        bottom: 0;
        z-index: -1;
        overflow: hidden;
        pointer-events: none;
    }

    .np-backdrop {
        position: absolute;
        inset: 0;
        background-size: cover;
        background-position: center;
        filter: blur(140px) saturate(1.6) brightness(0.5);
        transform: scale(1.8);
    }

    .np-backdrop::after {
        content: "";
        position: absolute;
        inset: 0;
        background: linear-gradient(
            to bottom,
            rgba(18, 18, 18, 0.2) 0%,
            var(--color-background) 90%
        );
    }

    .art-panel {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xl);
    }

    .art {
        width: 100%;
        max-width: 28rem;
        aspect-ratio: 1;
        border-radius: var(--radius-xl);
        overflow: hidden;
        background-color: var(--color-surface-elevated);
        color: var(--color-text-muted);
        display: flex;
        align-items: center;
        justify-content: center;
        box-shadow: var(--shadow-md);
        align-self: center;
    }

    .art img {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }

    .art svg {
        width: 40%;
        height: 40%;
    }

    .track-info {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
        text-align: center;
    }

    .track-info .page-title {
        letter-spacing: -0.02em;
    }

    .artist {
        color: var(--color-text-secondary);
        font-size: var(--font-size-lg);
    }

    :global(.artist-link),
    .album-link {
        color: inherit;
        transition: color var(--transition-fast);
    }

    :global(.artist-link:hover) {
        color: var(--color-text);
        text-decoration: underline;
    }

    .album-link:hover {
        color: var(--color-text-secondary);
        text-decoration: underline;
    }

    .album {
        color: var(--color-text-muted);
        font-size: var(--font-size-base);
    }

    .lyrics-panel {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-lg);
    }

    .lyrics-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
    }

    .lyrics-title-row {
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
    }

    .provider-tag {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-xs);
        padding: var(--spacing-xs) var(--spacing-sm);
        border-radius: var(--radius-full);
        border: 1px solid var(--color-border);
        background-color: rgba(255, 255, 255, 0.08);
        font-size: var(--font-size-xs);
        color: var(--color-text-secondary);
        transition:
            color var(--transition-fast),
            border-color var(--transition-fast),
            background-color var(--transition-fast);
    }

    .provider-tag:hover {
        background-color: rgba(255, 255, 255, 0.12);
        border-color: rgba(255, 255, 255, 0.18);
        color: var(--color-text);
    }

    .provider-tag svg {
        width: 0.625rem;
        height: 0.625rem;
    }

    .offset-controls {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        font-size: var(--font-size-sm);
    }

    .offset-btn {
        padding: var(--spacing-xs) var(--spacing-sm);
        border-radius: var(--radius);
        background-color: rgba(255, 255, 255, 0.08);
        border: 1px solid var(--color-border);
        color: var(--color-text-secondary);
        font-size: var(--font-size-xs);
        transition:
            background-color var(--transition-fast),
            color var(--transition-fast),
            border-color var(--transition-fast);
    }

    .offset-btn:hover {
        background-color: rgba(255, 255, 255, 0.12);
        border-color: rgba(255, 255, 255, 0.18);
        color: var(--color-text);
    }

    .offset-btn.reset {
        margin-left: var(--spacing-sm);
    }

    .offset-value {
        min-width: 4rem;
        text-align: center;
        color: var(--color-text-muted);
        font-variant-numeric: tabular-nums;
    }

    .status {
        color: var(--color-text-muted);
    }

    .status.error {
        background-color: var(--color-error);
        color: var(--color-text);
        padding: var(--spacing-md);
        border-radius: var(--radius-lg);
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
        max-width: 380px;
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

    .hint {
        margin: 0;
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
        line-height: var(--line-height);
    }

    .lyric-search {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
        padding-top: var(--spacing-md);
        border-top: 1px solid var(--color-border);
    }

    .lyric-search-row {
        display: flex;
        gap: var(--spacing-sm);
    }

    .lyric-search-row input {
        flex: 1;
        min-width: 0;
    }

    .lyric-candidates {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        max-height: 14rem;
        overflow-y: auto;
    }

    .lyric-candidate {
        display: flex;
        flex-direction: column;
        gap: 2px;
        width: 100%;
        text-align: left;
        padding: var(--spacing-sm) var(--spacing-md);
        border-radius: var(--radius);
        background: rgba(255, 255, 255, 0.08);
        border: 1px solid var(--color-border);
        transition:
            border-color var(--transition-fast),
            background-color var(--transition-fast);
    }

    .lyric-candidate:hover:not(:disabled) {
        background: rgba(255, 255, 255, 0.12);
        border-color: rgba(255, 255, 255, 0.18);
    }

    .candidate-source-tag {
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-semibold);
        color: var(--color-text-muted);
        text-transform: uppercase;
        letter-spacing: 0.05em;
    }

    .candidate-preview {
        font-size: var(--font-size-sm);
        color: var(--color-text);
        line-height: var(--line-height);
        /* See SyncedLyrics — East Asian punctuation must be breakable. */
        line-break: anywhere;
        display: -webkit-box;
        line-clamp: 2;
        -webkit-line-clamp: 2;
        -webkit-box-orient: vertical;
        overflow: hidden;
    }

    .dialog-actions {
        display: flex;
        justify-content: flex-end;
        gap: var(--spacing-md);
    }

    .custom-lyrics-actions {
        display: flex;
        flex-wrap: wrap;
        gap: var(--spacing-md);
        margin-top: var(--spacing-md);
    }

    .custom-lyrics-hint {
        margin-top: var(--spacing-sm);
    }

    @media (max-width: 767px) {
        .now-playing {
            grid-template-columns: 1fr;
        }

        .np-clip {
            left: 0;
        }

        .art-panel {
            position: static;
        }

        .art {
            max-width: 20rem;
        }
    }
</style>
