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
        getArtistImage,
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
    import ArtistCredits from "$lib/components/ArtistCredits.svelte";
    import ArtistLinks from "$lib/components/ArtistLinks.svelte";
    import { nowPlayingLayout } from "$lib/stores/uiPrefs";

    let lyrics = $state<Lyrics | null>(null);
    let loading = $state(false);
    let error = $state<string | null>(null);
    let lastTrackId = $state<number | null>(null);
    let lyricsRequest = 0;
    let lastAlbumId = $state<number | null>(null);
    let artUrl = $state("");
    let lastArtistId = $state<number | null>(null);
    let artistArtUrl = $state("");
    let backdropUrl = $derived(
        $nowPlayingLayout === "artist" && artistArtUrl ? artistArtUrl : artUrl,
    );
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

    async function loadArtistArt(artistId: number | null) {
        if (artistId === lastArtistId) return;
        lastArtistId = artistId;
        if (!artistId) {
            artistArtUrl = "";
            return;
        }
        const requestedArtistId = artistId;
        try {
            const art = await getArtistImage(requestedArtistId, "background");
            if (requestedArtistId !== lastArtistId) return;
            artistArtUrl = cachedImageToUrl(art, "");
        } catch {
            if (requestedArtistId !== lastArtistId) return;
            artistArtUrl = "";
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
                loadArtistArt(
                    $nowPlayingLayout === "artist"
                        ? ($playback.current_track.artist_ids?.[0] ?? null)
                        : null,
                );
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
            loadArtistArt(
                $nowPlayingLayout === "artist"
                    ? (track.artist_ids?.[0] ?? null)
                    : null,
            );
        } else {
            lyricsRequest += 1;
            lyrics = null;
            loading = false;
            lastTrackId = null;
            lastAlbumId = null;
            artUrl = "";
            lastArtistId = null;
            artistArtUrl = "";
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

<div class="now-playing" data-layout={$nowPlayingLayout}>
    <div class="np-clip" aria-hidden="true">
        <div
            class="np-backdrop"
            style:background-image={backdropUrl
                ? `url(${backdropUrl})`
                : "none"}
        ></div>
    </div>
    <div class="art-panel">
        {#if $playback.current_track}
            {@const track = $playback.current_track}
            {@const primaryArtistId = track.artist_ids?.[0] ?? null}
            {@const primaryArtistName = track.artist_names?.[0] ?? "Artist"}
            <div class="visual-stage">
                {#if primaryArtistId}
                    <a
                        class="artist-portrait"
                        href={`/artists/${primaryArtistId}`}
                        aria-label={`Open ${primaryArtistName}`}
                    >
                        {#if artistArtUrl}
                            <img
                                src={artistArtUrl}
                                alt={primaryArtistName}
                                decoding="async"
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
                                <path
                                    d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"
                                />
                                <circle cx="12" cy="7" r="4" />
                            </svg>
                        {/if}
                    </a>
                {:else}
                    <div class="artist-portrait" aria-hidden="true">
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
                    </div>
                {/if}
                <div class="art album-art">
                    {#if artUrl}
                        <img
                            src={artUrl}
                            alt={track.album_title ?? "Album art"}
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
                            <path
                                d="M9 18V5l12-2v13M6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zm12-2a3 3 0 1 0 0-6 3 3 0 0 0 0 6z"
                            />
                        </svg>
                    {/if}
                </div>
            </div>
            <div class="track-info">
                <h1 class="page-title">{track.title ?? "Unknown"}</h1>
                {#if $nowPlayingLayout === "artist" && (track.artist_names?.length ?? 0) < 2}
                    <p class="artist">
                        <ArtistLinks
                            names={track.artist_names}
                            ids={track.artist_ids}
                            linkClass="artist-link"
                        />
                    </p>
                {:else}
                    <ArtistCredits
                        names={track.artist_names}
                        ids={track.artist_ids}
                        size="regular"
                        align={$nowPlayingLayout === "lyrics"
                            ? "start"
                            : "center"}
                    />
                {/if}
                {#if track.album_id && track.album_title}
                    <p class="album">
                        <a class="album-link" href={`/albums/${track.album_id}`}
                            >{track.album_title}</a
                        >
                    </p>
                {/if}
            </div>
        {:else}
            <div class="visual-stage">
                <div class="artist-portrait" aria-hidden="true">
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.5"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
                        <circle cx="12" cy="7" r="4" />
                    </svg>
                </div>
                <div class="art album-art">
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
        --np-lyrics-align: center;
        --np-lyrics-line-align: center;
        --np-lyrics-line-size: var(--font-size-2xl);
        --np-lyrics-line-height: 1.5;
        --np-lyrics-line-color: var(--color-text-secondary);
        --np-lyrics-active-color: var(--color-text);
        --np-lyrics-inactive-scale: 0.833333;
        --np-lyrics-active-scale: 1.05;
        --np-lyrics-transform-origin: center;
        --np-lyrics-max-height: 70vh;
        --np-lyrics-container-padding: var(--spacing-xl);
        --np-lyrics-lines-padding: 30vh 0;
        --np-lyrics-lines-gap: var(--spacing-md);
        --np-lyrics-active-shadow: 0 2px 16px rgba(0, 0, 0, 0.6);
        position: relative;
        display: grid;
        grid-template-columns: minmax(18rem, 0.9fr) minmax(22rem, 1.1fr);
        gap: clamp(2.5rem, 5vw, 6rem);
        align-items: center;
        width: 100%;
        max-width: 90rem;
        min-height: calc(
            100vh - var(--player-height) - var(--spacing-2xl) -
                var(--spacing-2xl)
        );
        margin: 0 auto;
        padding-top: 2.5rem;
        isolation: isolate;
    }

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
        opacity: 0.9;
        transition:
            filter 700ms ease,
            opacity 700ms ease,
            transform 900ms cubic-bezier(0.16, 1, 0.3, 1);
    }

    .np-backdrop::after {
        content: "";
        position: absolute;
        inset: 0;
        background:
            radial-gradient(
                circle at 50% 32%,
                transparent 0%,
                color-mix(in srgb, var(--color-background) 22%, transparent) 54%,
                color-mix(in srgb, var(--color-background) 68%, transparent)
                    100%
            ),
            linear-gradient(
                to bottom,
                color-mix(in srgb, var(--color-background) 8%, transparent) 0%,
                var(--color-background) 94%
            );
    }

    .art-panel {
        position: relative;
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xl);
        min-width: 0;
        transition:
            transform var(--transition-slow),
            background-color var(--transition-slow),
            border-color var(--transition-slow),
            border-radius var(--transition-slow),
            padding var(--transition-slow);
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
        transition:
            transform 500ms cubic-bezier(0.16, 1, 0.3, 1),
            border-radius var(--transition-slow),
            box-shadow 500ms ease,
            width var(--transition-slow),
            max-width var(--transition-slow);
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
        position: relative;
        z-index: 2;
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
        text-align: center;
        transition:
            color var(--transition-slow),
            padding var(--transition-slow),
            text-align var(--transition-slow);
    }

    .track-info .page-title {
        letter-spacing: -0.02em;
        line-height: 1.04;
        text-wrap: balance;
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
        position: relative;
        display: flex;
        flex-direction: column;
        gap: var(--spacing-lg);
        min-width: 0;
        transition:
            background-color var(--transition-slow),
            border-color var(--transition-slow),
            border-radius var(--transition-slow),
            padding var(--transition-slow),
            box-shadow var(--transition-slow);
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
        background-color: color-mix(in srgb, var(--color-text) 8%, transparent);
        font-size: var(--font-size-xs);
        color: var(--color-text-secondary);
        transition:
            color var(--transition-fast),
            border-color var(--transition-fast),
            background-color var(--transition-fast);
    }

    .provider-tag:hover {
        background-color: color-mix(
            in srgb,
            var(--color-text) 12%,
            transparent
        );
        border-color: color-mix(in srgb, var(--color-text) 18%, transparent);
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
        background-color: color-mix(in srgb, var(--color-text) 8%, transparent);
        border: 1px solid var(--color-border);
        color: var(--color-text-secondary);
        font-size: var(--font-size-xs);
        transition:
            background-color var(--transition-fast),
            color var(--transition-fast),
            border-color var(--transition-fast);
    }

    .offset-btn:hover {
        background-color: color-mix(
            in srgb,
            var(--color-text) 12%,
            transparent
        );
        border-color: color-mix(in srgb, var(--color-text) 18%, transparent);
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

    /* Playback layouts are content-led: album art stays square, artist imagery
       stays circular, and the reading layout gives lyrics the primary axis. */
    .now-playing[data-layout] {
        grid-template-columns: minmax(19rem, 1fr) minmax(24rem, 1fr);
        align-items: center;
        max-width: 90rem;
        padding-top: 0;
        --np-lyrics-align: center;
        --np-lyrics-line-align: center;
        --np-lyrics-line-size: 1.625rem;
        --np-lyrics-line-height: 1.5;
        --np-lyrics-line-color: var(--color-text-secondary);
        --np-lyrics-active-color: var(--color-text);
        --np-lyrics-inactive-scale: 0.833333;
        --np-lyrics-active-scale: 1.05;
        --np-lyrics-transform-origin: center;
        --np-lyrics-max-height: 68vh;
        --np-lyrics-container-padding: var(--spacing-xl);
        --np-lyrics-lines-padding: 28vh 0;
        --np-lyrics-lines-gap: var(--spacing-md);
        --np-lyrics-active-shadow: 0 2px 16px rgba(0, 0, 0, 0.6);
    }

    .visual-stage {
        position: relative;
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        min-width: 0;
    }

    .artist-portrait {
        display: none;
        align-items: center;
        justify-content: center;
        overflow: hidden;
        border-radius: 50%;
        background: var(--color-surface-elevated);
        color: var(--color-text-muted);
    }

    .artist-portrait img {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }

    .artist-portrait svg {
        width: 42%;
        height: 42%;
    }

    .now-playing[data-layout] .album-art {
        transition:
            transform 500ms cubic-bezier(0.16, 1, 0.3, 1),
            box-shadow 500ms ease,
            width var(--transition-slow),
            max-width var(--transition-slow);
    }

    /* Album: the release is the identity. One large square, no visual fiction. */
    .now-playing[data-layout="album"] .album-art {
        max-width: 27.5rem;
        box-shadow:
            0 30px 72px rgba(0, 0, 0, 0.42),
            0 0 0 1px color-mix(in srgb, var(--color-text) 8%, transparent),
            0 0 96px
                color-mix(in srgb, var(--color-accent-seed) 13%, transparent);
    }

    .now-playing[data-layout="album"] .album-art:hover {
        transform: translateY(-5px) scale(1.012);
        box-shadow:
            0 38px 86px rgba(0, 0, 0, 0.48),
            0 0 0 1px color-mix(in srgb, var(--color-text) 11%, transparent),
            0 0 112px
                color-mix(in srgb, var(--color-accent-seed) 18%, transparent);
    }

    .now-playing[data-layout="album"] .track-info .page-title {
        font-size: clamp(var(--font-size-3xl), 3vw, var(--font-size-4xl));
    }

    /* Artist: a portrait is allowed to be a portrait. The square release sits
       above it as a separate object instead of being cropped into a circle. */
    .now-playing[data-layout="artist"] {
        grid-template-columns: minmax(22rem, 1fr) minmax(24rem, 1fr);
    }

    .now-playing[data-layout="artist"] .np-backdrop {
        filter: blur(125px) saturate(1.25) brightness(0.43);
        transform: scale(1.72);
    }

    .now-playing[data-layout="artist"] .visual-stage {
        width: min(32vw, 29rem);
        aspect-ratio: 1;
        margin: 0 auto;
    }

    .now-playing[data-layout="artist"] .artist-portrait {
        position: absolute;
        top: 2%;
        left: 2%;
        z-index: 1;
        display: flex;
        width: 84%;
        aspect-ratio: 1;
        border: 1px solid color-mix(in srgb, var(--color-text) 13%, transparent);
        box-shadow:
            0 30px 72px rgba(0, 0, 0, 0.42),
            0 0 72px
                color-mix(in srgb, var(--color-accent-seed) 12%, transparent);
        transition:
            transform var(--transition-slow),
            box-shadow var(--transition-slow);
    }

    .now-playing[data-layout="artist"] .artist-portrait:hover {
        transform: translateY(-4px) scale(1.01);
        box-shadow: 0 38px 82px rgba(0, 0, 0, 0.48);
    }

    .now-playing[data-layout="artist"] .album-art {
        position: absolute;
        right: 2%;
        bottom: 2%;
        z-index: 2;
        width: 34%;
        max-width: none;
        border-radius: var(--radius-lg);
        box-shadow:
            0 18px 44px rgba(0, 0, 0, 0.56),
            0 0 0 1px color-mix(in srgb, var(--color-text) 14%, transparent);
    }

    .now-playing[data-layout="artist"] .album-art:hover {
        transform: translateY(-4px) scale(1.035);
    }

    .now-playing[data-layout="artist"] .track-info .page-title {
        font-size: clamp(var(--font-size-3xl), 3vw, var(--font-size-4xl));
    }

    /* Lyrics: artwork becomes compact context and the words own the page. */
    .now-playing[data-layout="lyrics"] {
        grid-template-columns: minmax(14rem, 18rem) minmax(28rem, 1fr);
        align-items: start;
        max-width: 78rem;
        gap: clamp(3rem, 7vw, 6rem);
        --np-lyrics-align: left;
        --np-lyrics-line-align: left;
        --np-lyrics-line-size: clamp(1.7rem, 3.2vw, 2.85rem);
        --np-lyrics-line-height: 1.24;
        --np-lyrics-line-color: color-mix(
            in srgb,
            var(--color-text) 38%,
            transparent
        );
        --np-lyrics-active-color: var(--color-accent-content);
        --np-lyrics-line-weight: var(--font-weight-bold);
        --np-lyrics-active-weight: var(--font-weight-bold);
        --np-lyrics-inactive-scale: 0.94;
        --np-lyrics-active-scale: 1;
        --np-lyrics-transform-origin: left center;
        --np-lyrics-max-height: none;
        --np-lyrics-container-padding: 0 0 30vh;
        --np-lyrics-lines-padding: 14vh 0 28vh;
        --np-lyrics-lines-gap: clamp(1.35rem, 3vh, 2.35rem);
        --np-lyrics-active-shadow: none;
    }

    .now-playing[data-layout="lyrics"] .np-backdrop {
        filter: blur(155px) saturate(0.35) brightness(0.28);
        transform: scale(1.9);
        opacity: 0.28;
    }

    .now-playing[data-layout="lyrics"] .np-backdrop::after {
        background: linear-gradient(
            90deg,
            color-mix(in srgb, var(--color-background) 64%, transparent),
            var(--color-background) 74%
        );
    }

    .now-playing[data-layout="lyrics"] .art-panel {
        position: sticky;
        top: 0;
        align-items: flex-start;
        padding-top: var(--spacing-xl);
    }

    .now-playing[data-layout="lyrics"] .visual-stage {
        width: 13rem;
        max-width: 100%;
        aspect-ratio: 1;
        justify-content: flex-start;
    }

    .now-playing[data-layout="lyrics"] .album-art {
        width: 100%;
        max-width: none;
        align-self: auto;
        border-radius: var(--radius-lg);
        box-shadow:
            0 20px 50px rgba(0, 0, 0, 0.36),
            0 0 0 1px color-mix(in srgb, var(--color-text) 8%, transparent);
    }

    .now-playing[data-layout="lyrics"] .track-info {
        align-items: flex-start;
        text-align: left;
    }

    .now-playing[data-layout="lyrics"] .track-info .page-title {
        font-size: clamp(var(--font-size-3xl), 3vw, var(--font-size-4xl));
    }

    .now-playing[data-layout="lyrics"] .lyrics-panel {
        padding-left: clamp(2.5rem, 5vw, 4.5rem);
        border-left: 1px solid var(--color-border);
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
            min-height: calc(100vh - var(--player-height) - var(--spacing-2xl));
        }

        .np-clip {
            left: 0;
        }

        .lyrics-header {
            align-items: flex-start;
            flex-direction: column;
        }

        .offset-controls {
            flex-wrap: wrap;
        }

        .art {
            max-width: 20rem;
        }
    }

    @media (max-width: 1100px) {
        .now-playing[data-layout] {
            grid-template-columns: 1fr;
            align-items: start;
            max-width: 46rem;
        }

        .now-playing[data-layout="album"] .album-art {
            max-width: 23.5rem;
        }

        .now-playing[data-layout="artist"] .visual-stage {
            width: min(72vw, 28rem);
        }

        .now-playing[data-layout="lyrics"] .art-panel {
            position: static;
            display: grid;
            grid-template-columns: 11rem minmax(0, 1fr);
            align-items: center;
            gap: var(--spacing-xl);
            padding-top: var(--spacing-md);
        }

        .now-playing[data-layout="lyrics"] .visual-stage {
            width: 11rem;
        }

        .now-playing[data-layout="lyrics"] .lyrics-panel {
            padding-top: var(--spacing-xl);
            padding-left: 0;
            border-top: 1px solid var(--color-border);
            border-left: 0;
        }
    }

    @media (max-width: 767px) {
        .now-playing[data-layout] {
            padding-top: var(--spacing-md);
        }

        .now-playing[data-layout="album"] .album-art {
            max-width: 19rem;
        }

        .now-playing[data-layout="artist"] .visual-stage {
            width: min(82vw, 24rem);
        }

        .now-playing[data-layout="lyrics"] {
            --np-lyrics-line-size: clamp(1.5rem, 8vw, 2.25rem);
        }

        .now-playing[data-layout="lyrics"] .art-panel {
            grid-template-columns: 8.5rem minmax(0, 1fr);
            gap: var(--spacing-lg);
        }

        .now-playing[data-layout="lyrics"] .visual-stage {
            width: 8.5rem;
        }
    }

    @media (max-width: 480px) {
        .now-playing[data-layout="lyrics"] .art-panel {
            grid-template-columns: 1fr;
        }

        .now-playing[data-layout="lyrics"] .visual-stage {
            width: 9.5rem;
        }
    }
</style>
