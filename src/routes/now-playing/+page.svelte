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
    import LyricsOffsetControls from "$lib/components/LyricsOffsetControls.svelte";
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
                <div class="artwork-composition">
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
                <div class="artwork-composition">
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
            </div>
            <div class="track-info">
                <h1 class="page-title">No track selected</h1>
                <p class="artist">
                    Choose a song from your library to start listening.
                </p>
            </div>
        {/if}
    </div>

    <div class="lyrics-panel">
        <div class="lyrics-header">
            <div class="lyrics-title-row">
                <h2 class="section-title">Lyrics</h2>
                {#if providerLabel || $playback.current_track}
                    <div class="control-cluster">
                        <button
                            class="provider-tag"
                            type="button"
                            onclick={openProviderDialog}
                            title="Change lyrics source"
                        >
                            {providerLabel ? `From ${providerLabel}` : "Source"}
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
                {/if}
            </div>
            {#if lyrics?.synced_text}
                <LyricsOffsetControls
                    value={offsetMs}
                    onadjust={adjustOffset}
                    onreset={() => {
                        offsetMs = 0;
                        const track = $playback.current_track;
                        if (track) saveOffset(track.id, 0);
                    }}
                />
            {/if}
        </div>
        {#if loading}
            <p class="status">Loading lyrics...</p>
        {:else if error}
            <p class="status error">{error}</p>
        {:else if lyrics}
            <SyncedLyrics
                fontFamily={lyricsFont}
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
        --np-art-size: 27.5rem;
        position: relative;
        display: grid;
        grid-template-rows: minmax(0, 1fr);
        gap: clamp(1.5rem, 4vw, 4rem);
        align-items: center;
        flex: 1;
        width: 100%;
        max-width: 90rem;
        min-height: 0;
        margin: 0 auto;
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
            filter var(--transition-slow),
            opacity var(--transition-slow),
            transform var(--transition-slow);
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
        align-items: center;
        justify-content: center;
        height: 100%;
        min-height: 0;
        gap: clamp(1rem, 3vh, 2rem);
        min-width: 0;
        container-type: inline-size;
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
            transform var(--transition-slow),
            border-radius var(--transition-slow),
            box-shadow var(--transition-slow),
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
        flex: 0 0 auto;
        width: 100%;
        min-width: 0;
        max-height: 60%;
        overflow-y: auto;
        overscroll-behavior: contain;
        padding: var(--spacing-xs);
        text-align: center;
        transition:
            color var(--transition-slow),
            padding var(--transition-slow),
            text-align var(--transition-slow);
    }

    .track-info > :global(*) {
        flex-shrink: 0;
    }

    .track-info .page-title {
        letter-spacing: -0.02em;
        line-height: 1.04;
        text-wrap: balance;
        overflow-wrap: anywhere;
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
    }

    .album-link:hover {
        color: var(--color-text-secondary);
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
        min-height: 0;
        height: 100%;
        transition:
            background-color var(--transition-slow),
            border-color var(--transition-slow),
            border-radius var(--transition-slow),
            padding var(--transition-slow),
            box-shadow var(--transition-slow);
    }

    .lyrics-header {
        display: flex;
        flex-wrap: wrap;
        flex-shrink: 0;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-sm) var(--spacing-md);
    }

    .lyrics-title-row {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: var(--spacing-md);
    }

    .provider-tag {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-sm);
    }

    .provider-tag svg {
        width: 0.875rem;
        height: 0.875rem;
        flex-shrink: 0;
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
        grid-template-columns: repeat(2, minmax(0, 1fr));
        align-items: center;
        max-width: 90rem;
        --np-lyrics-align: center;
        --np-lyrics-line-align: center;
        --np-lyrics-line-size: 1.625rem;
        --np-lyrics-line-height: 1.5;
        --np-lyrics-line-color: var(--color-text-secondary);
        --np-lyrics-active-color: var(--color-text);
        --np-lyrics-inactive-scale: 0.833333;
        --np-lyrics-active-scale: 1.05;
        --np-lyrics-transform-origin: center;
        --np-lyrics-container-padding: 0 var(--spacing-md);
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
        min-height: 0;
        flex: 0 1 min(100cqw, var(--np-art-size));
        container-type: size;
    }

    /* Fit one square composition into the actual space left above the credits.
       The stage does not crop; only the portrait and cover mask their images. */
    .artwork-composition {
        position: relative;
        display: flex;
        align-items: center;
        justify-content: center;
        width: min(100cqw, 100cqh, var(--np-art-size));
        aspect-ratio: 1;
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
            transform var(--transition-slow),
            box-shadow var(--transition-slow),
            width var(--transition-slow),
            max-width var(--transition-slow);
    }

    /* Album: the release is the identity. One large square, no visual fiction. */
    .now-playing[data-layout="album"] .art-panel,
    .now-playing[data-layout="artist"] .art-panel {
        justify-content: flex-start;
    }

    .now-playing[data-layout="album"] .album-art {
        max-width: 27.5rem;
        box-shadow:
            0 30px 72px rgba(0, 0, 0, 0.42),
            0 0 0 1px color-mix(in srgb, var(--color-text) 8%, transparent),
            0 0 96px
                color-mix(in srgb, var(--color-accent-seed) 13%, transparent);
    }

    .now-playing[data-layout="album"] .track-info .page-title {
        font-size: clamp(var(--font-size-3xl), 3vw, var(--font-size-4xl));
    }

    /* Artist: a portrait is allowed to be a portrait. The square release sits
       above it as a separate object instead of being cropped into a circle. */
    .now-playing[data-layout="artist"] {
        --np-art-size: 29rem;
    }

    .now-playing[data-layout="artist"] .np-backdrop {
        filter: blur(125px) saturate(1.25) brightness(0.43);
        transform: scale(1.72);
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

    .now-playing[data-layout="artist"] a.artist-portrait:hover {
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

    .now-playing[data-layout="artist"] .track-info .page-title {
        font-size: clamp(var(--font-size-3xl), 3vw, var(--font-size-4xl));
    }

    /* Lyrics: artwork becomes compact context and the words own the page. */
    .now-playing[data-layout="lyrics"] {
        --np-art-size: 13rem;
        grid-template-columns: minmax(14rem, 18rem) minmax(0, 1fr);
        align-items: start;
        max-width: 78rem;
        gap: clamp(1.5rem, 4vw, 4rem);
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
        --np-lyrics-inactive-scale: 0.94;
        --np-lyrics-active-scale: 1;
        --np-lyrics-transform-origin: left center;
        --np-lyrics-container-padding: 0 var(--spacing-xs);
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
        align-items: flex-start;
    }

    .now-playing[data-layout="lyrics"] .visual-stage {
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
        padding-left: clamp(1.5rem, 3vw, 3rem);
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
        background: var(--interactive-hover);
    }

    .candidate-source-tag {
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-semibold);
        color: var(--color-text-muted);
        letter-spacing: normal;
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
        .np-clip {
            left: 0;
        }
    }

    @media (max-width: 1000px) {
        .now-playing[data-layout] {
            grid-template-columns: 1fr;
            grid-template-rows: minmax(0, auto) minmax(0, 1fr);
            max-width: 46rem;
            gap: var(--spacing-lg);
        }

        .now-playing[data-layout] .art-panel {
            display: grid;
            grid-template-columns: clamp(5rem, 15vw, 8rem) minmax(0, 1fr);
            align-items: center;
            gap: var(--spacing-md);
            height: auto;
            max-height: 28vh;
        }

        .now-playing[data-layout] .visual-stage {
            height: clamp(5rem, 15vw, 8rem);
            max-height: 28vh;
        }

        .now-playing[data-layout] .track-info {
            align-items: flex-start;
            text-align: left;
            max-height: 28vh;
        }

        .track-info :global(.artist-credits) {
            justify-content: flex-start;
        }

        .now-playing[data-layout] .track-info .page-title {
            font-size: clamp(var(--font-size-xl), 3vw, var(--font-size-3xl));
        }

        .now-playing[data-layout="lyrics"] .lyrics-panel {
            padding-left: 0;
            border-left: 0;
        }
    }

    @media (max-width: 767px) {
        .now-playing[data-layout="lyrics"] {
            --np-lyrics-line-size: clamp(1.5rem, 8vw, 2.25rem);
        }
    }
</style>
