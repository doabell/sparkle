<script lang="ts">
    import {
        playback,
        play,
        pause,
        nextTrack,
        previousTrack,
        setVolume,
        setVolumeLive,
        seek,
        setShuffle,
        cycleRepeatMode,
    } from "$lib/stores/playback";
    import { formatTime } from "$lib/utils/formatTime";
    import { smartGo } from "$lib/utils/nav";
    import {
        getAlbumArt,
        getLyrics,
        LYRICS_CHANGED_EVENT,
        type CachedImage,
    } from "$lib/api";
    import { cachedImageToUrl } from "$lib/utils/base64";
    import { parseLrc, activeLineIndex, type LrcLine } from "$lib/utils/lrc";
    import { listen } from "@tauri-apps/api/event";
    import { onMount } from "svelte";
    import QueuePanel from "$lib/components/QueuePanel.svelte";
    import ArtistLinks from "$lib/components/ArtistLinks.svelte";

    let art = $state<CachedImage | null>(null);
    let lastAlbumId = $state<number | null>(null);
    let queueOpen = $state(false);

    // --- Lyrics ticker -------------------------------------------------------
    // The current synced lyric line lives under the track info: lyrics are
    // first-class here, not buried in the now-playing page.
    let lyricLines = $state<LrcLine[]>([]);
    let lyricTrackId = $state<number | null>(null);
    let lyricRequest = 0;

    async function updateLyrics(trackId: number | null | undefined) {
        if (!trackId || trackId === lyricTrackId) return;
        const request = ++lyricRequest;
        lyricTrackId = trackId;
        lyricLines = [];
        try {
            const lyrics = await getLyrics(trackId);
            if (request !== lyricRequest || lyricTrackId !== trackId) return;
            lyricLines = lyrics.synced_text ? parseLrc(lyrics.synced_text) : [];
        } catch {
            if (request !== lyricRequest || lyricTrackId !== trackId) return;
            lyricLines = [];
        }
    }

    let currentLyricLine = $derived.by(() => {
        if (lyricLines.length === 0) return "";
        const track = $playback.current_track;
        if (!track || track.id !== lyricTrackId) return "";
        const adjusted = $playback.position_ms - (track.lrc_offset_ms ?? 0);
        const index = activeLineIndex(lyricLines, adjusted);
        return index >= 0 ? lyricLines[index].text : "";
    });

    async function updateArt(albumId: number | null | undefined) {
        if (albumId === lastAlbumId) return;
        lastAlbumId = albumId ?? null;
        if (!albumId) {
            art = null;
            return;
        }
        const requestedAlbumId = albumId;
        try {
            const fetched = await getAlbumArt(requestedAlbumId);
            if (requestedAlbumId !== lastAlbumId) return;
            art = fetched;
        } catch {
            if (requestedAlbumId !== lastAlbumId) return;
            art = null;
        }
    }

    onMount(() => {
        updateArt($playback.current_track?.album_id);
        updateLyrics($playback.current_track?.id);
        let unlisten: (() => void) | undefined;
        const handleLyricsChanged = (event: Event) => {
            const trackId = (event as CustomEvent<{ trackId: number }>).detail
                ?.trackId;
            if ($playback.current_track?.id !== trackId) return;
            lyricTrackId = null;
            updateLyrics(trackId);
        };
        window.addEventListener(LYRICS_CHANGED_EVENT, handleLyricsChanged);
        (async () => {
            unlisten = await listen("online-settings-changed", () => {
                // Bias or source list changed — refetch the ticker lyrics.
                lyricTrackId = null;
                updateLyrics($playback.current_track?.id);
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
        updateArt($playback.current_track?.album_id);
    });

    $effect(() => {
        updateLyrics($playback.current_track?.id);
    });

    // --- Volume ------------------------------------------------------------
    // volumeInput is the single display value. While dragging it follows the
    // mouse; after mouseup pendingVolume pins the bar to the committed value
    // until the store catches up, so the bar never snaps back.
    let volumeInput = $state($playback.volume);
    let pendingVolume = $state<number | null>(null);
    let isDraggingVolume = $state(false);
    let lastLiveVolumeSent = $state(0);
    // Mute is "volume 0 with memory": the speaker icon toggles between 0 and
    // the last non-zero volume. Any manual volume above 0 unmutes implicitly.
    let lastNonZeroVolume = $state(
        $playback.volume > 0 ? $playback.volume : 0.8,
    );

    $effect(() => {
        const storeVolume = $playback.volume;
        if (isDraggingVolume) return;
        if (pendingVolume !== null) {
            if (Math.abs(storeVolume - pendingVolume) < 0.01) {
                pendingVolume = null;
                volumeInput = storeVolume;
            }
            return;
        }
        if (Math.abs(storeVolume - volumeInput) > 0.01) {
            volumeInput = storeVolume;
        }
    });

    function recordNonZero(value: number) {
        if (value > 0) {
            lastNonZeroVolume = value;
        }
    }

    function toggleMute() {
        if (volumeInput > 0) {
            lastNonZeroVolume = volumeInput;
            volumeInput = 0;
            pendingVolume = 0;
            setVolume(0);
        } else {
            const target = Math.max(0.01, lastNonZeroVolume);
            volumeInput = target;
            pendingVolume = target;
            setVolume(target);
        }
    }

    function commitLiveVolume(value: number) {
        recordNonZero(value);
        const now = Date.now();
        if (now - lastLiveVolumeSent >= 50) {
            lastLiveVolumeSent = now;
            setVolumeLive(value);
        }
    }

    function handleVolumeMouseDown(e: MouseEvent) {
        if (!volumeBar) return;
        isDraggingVolume = true;
        pendingVolume = null;
        const fraction = getBarFraction(volumeBar, e.clientX);
        volumeInput = Math.max(
            0,
            Math.min(1, Math.round(fraction * 100) / 100),
        );
        commitLiveVolume(volumeInput);
    }

    function handleVolumeMove(e: MouseEvent) {
        if (!isDraggingVolume || !volumeBar) return;
        const fraction = getBarFraction(volumeBar, e.clientX);
        volumeInput = Math.max(
            0,
            Math.min(1, Math.round(fraction * 100) / 100),
        );
        commitLiveVolume(volumeInput);
    }

    function handleVolumeMouseUp() {
        if (!isDraggingVolume) return;
        recordNonZero(volumeInput);
        pendingVolume = volumeInput;
        isDraggingVolume = false;
        setVolume(volumeInput);
    }

    function handleVolumeWheel(e: WheelEvent) {
        e.preventDefault();
        const delta = e.deltaY > 0 ? -0.05 : 0.05;
        const value = Math.max(
            0,
            Math.min(1, Math.round((volumeInput + delta) * 100) / 100),
        );
        recordNonZero(value);
        pendingVolume = value;
        volumeInput = value;
        setVolume(value);
    }

    // --- Progress ------------------------------------------------------------
    let isDraggingProgress = $state(false);
    let dragProgressPercent = $state(0);
    let pendingSeekMs = $state<number | null>(null);
    let pendingSeekTrackId = $state<number | null>(null);

    let progressPercent = $derived.by(() => {
        if (isDraggingProgress) {
            return dragProgressPercent;
        }
        if (
            pendingSeekMs !== null &&
            pendingSeekTrackId === $playback.current_track?.id &&
            $playback.duration_ms > 0
        ) {
            return (pendingSeekMs / $playback.duration_ms) * 100;
        }
        return $playback.duration_ms > 0
            ? ($playback.position_ms / $playback.duration_ms) * 100
            : 0;
    });

    let displayPositionMs = $derived.by(() => {
        if (isDraggingProgress && $playback.duration_ms > 0) {
            return Math.round(
                (dragProgressPercent / 100) * $playback.duration_ms,
            );
        }
        if (
            pendingSeekMs !== null &&
            pendingSeekTrackId === $playback.current_track?.id
        ) {
            return pendingSeekMs;
        }
        return $playback.position_ms;
    });

    $effect(() => {
        if (pendingSeekMs === null) return;
        if (pendingSeekTrackId !== $playback.current_track?.id) {
            pendingSeekMs = null;
            return;
        }
        if (Math.abs($playback.position_ms - pendingSeekMs) < 600) {
            pendingSeekMs = null;
        }
    });

    let progressBar = $state<HTMLDivElement | null>(null);
    let volumeBar = $state<HTMLDivElement | null>(null);

    function getBarFraction(bar: HTMLDivElement, clientX: number): number {
        const rect = bar.getBoundingClientRect();
        const x = Math.max(0, Math.min(clientX - rect.left, rect.width));
        return rect.width > 0 ? x / rect.width : 0;
    }

    function handleProgressMouseDown(e: MouseEvent) {
        if (!progressBar || $playback.duration_ms <= 0) return;
        isDraggingProgress = true;
        pendingSeekMs = null;
        const fraction = getBarFraction(progressBar, e.clientX);
        dragProgressPercent = fraction * 100;
    }

    function handleProgressMove(e: MouseEvent) {
        if (!isDraggingProgress || !progressBar || $playback.duration_ms <= 0)
            return;
        const fraction = getBarFraction(progressBar, e.clientX);
        dragProgressPercent = fraction * 100;
    }

    function handleProgressMouseUp() {
        if (!isDraggingProgress) return;
        isDraggingProgress = false;
        if (!progressBar || $playback.duration_ms <= 0) return;
        const fraction = dragProgressPercent / 100;
        const targetMs = Math.round(fraction * $playback.duration_ms);
        pendingSeekMs = targetMs;
        pendingSeekTrackId = $playback.current_track?.id ?? null;
        seek(targetMs);
    }

    let volumePercent = $derived(volumeInput * 100);

    function volumeIconPath() {
        if (volumeInput <= 0) {
            return "M16.5 12c0-1.77-1.02-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51C20.63 14.91 21 13.5 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3L3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06c1.38-.31 2.63-.95 3.69-1.81L19.73 21 21 19.73 4.27 3zM12 4L9.91 6.09 12 8.18V4z";
        }
        if (volumeInput < 0.5) {
            return "M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02z";
        }
        return "M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z";
    }
</script>

<svelte:window
    onmousemove={(e) => {
        handleProgressMove(e);
        handleVolumeMove(e);
    }}
    onmouseup={() => {
        handleProgressMouseUp();
        handleVolumeMouseUp();
    }}
/>

<div class="player-bar">
    {#if $playback.error}
        <div class="error">{$playback.error}</div>
    {/if}

    <div class="left">
        <button
            class="art"
            onclick={() => smartGo("/now-playing")}
            aria-label="Open now playing"
        >
            {#if art?.file_path}
                <img
                    src={cachedImageToUrl(art, "")}
                    decoding="async"
                    alt={$playback.current_track?.album_title ?? "Album art"}
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
        </button>

        <div class="track-info">
            {#if $playback.current_track}
                {@const track = $playback.current_track}
                <button
                    class="title ellipsis"
                    onclick={() => smartGo("/now-playing")}
                >
                    {track.title ?? "Unknown"}
                </button>
                <span class="artist ellipsis">
                    <ArtistLinks
                        names={track.artist_names}
                        ids={track.artist_ids}
                    />
                    {#if track.album_id && track.album_title}
                        <span class="meta-sep">·</span><a
                            href={`/albums/${track.album_id}`}
                            onclick={(e) => e.stopPropagation()}
                            >{track.album_title}</a
                        >
                    {/if}
                </span>
                {#if currentLyricLine}
                    <button
                        class="lyric-line ellipsis"
                        onclick={() => smartGo("/now-playing")}
                        title="Open lyrics"
                    >
                        {currentLyricLine}
                    </button>
                {/if}
            {:else}
                <span class="title ellipsis text-muted">No track selected</span>
                <span class="artist">--</span>
            {/if}
        </div>
    </div>

    <div class="center">
        <div class="controls">
            <button
                class="mode-btn"
                class:active={$playback.shuffle}
                onclick={() => setShuffle(!$playback.shuffle)}
                aria-label="Shuffle"
                aria-pressed={$playback.shuffle}
                title="Shuffle"
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
            </button>

            <button
                class="control-btn"
                onclick={previousTrack}
                aria-label="Previous"
            >
                <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                    <path d="M6 6h2v12H6zm3.5 6 8.5 6V6z" />
                </svg>
            </button>

            <button
                class="play-btn"
                onclick={$playback.is_playing ? pause : play}
                aria-label="Play/Pause"
            >
                {#if $playback.is_playing}
                    <svg
                        viewBox="0 0 24 24"
                        fill="currentColor"
                        aria-hidden="true"
                    >
                        <path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z" />
                    </svg>
                {:else}
                    <svg
                        viewBox="0 0 24 24"
                        fill="currentColor"
                        aria-hidden="true"
                    >
                        <path d="M8 5v14l11-7z" />
                    </svg>
                {/if}
            </button>

            <button class="control-btn" onclick={nextTrack} aria-label="Next">
                <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                    <path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" />
                </svg>
            </button>

            <button
                class="mode-btn"
                class:active={$playback.repeat_mode !== "off"}
                onclick={cycleRepeatMode}
                aria-label="Repeat"
                aria-pressed={$playback.repeat_mode !== "off"}
                title={$playback.repeat_mode === "one"
                    ? "Repeat one"
                    : $playback.repeat_mode === "all"
                      ? "Loop all"
                      : "Repeat off"}
            >
                {#if $playback.repeat_mode === "one"}
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        aria-hidden="true"
                    >
                        <path d="m11 10 1-1v8" />
                        <path d="m17 2 4 4-4 4" />
                        <path d="M3 11v-1a4 4 0 0 1 4-4h14" />
                        <path d="m7 22-4-4 4-4" />
                        <path d="M21 13v1a4 4 0 0 1-4 4H3" />
                    </svg>
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
                        <path d="m17 2 4 4-4 4" />
                        <path d="M3 11v-1a4 4 0 0 1 4-4h14" />
                        <path d="m7 22-4-4 4-4" />
                        <path d="M21 13v1a4 4 0 0 1-4 4H3" />
                    </svg>
                {/if}
            </button>
        </div>

        <div class="progress">
            <span class="time">{formatTime(displayPositionMs)}</span>
            <div
                class="bar-track"
                bind:this={progressBar}
                role="slider"
                aria-label="Seek"
                aria-valuemin={0}
                aria-valuemax={$playback.duration_ms}
                aria-valuenow={$playback.position_ms}
                tabindex={$playback.duration_ms > 0 ? 0 : -1}
                onmousedown={handleProgressMouseDown}
            >
                <div class="bar-fill" style:width={`${progressPercent}%`}></div>
            </div>
            <span class="time">{formatTime($playback.duration_ms)}</span>
        </div>
    </div>

    <div class="right">
        <button
            class="queue-toggle"
            class:active={queueOpen}
            aria-label="Queue"
            aria-pressed={queueOpen}
            title="Queue"
            onclick={(e: MouseEvent) => {
                e.stopPropagation();
                queueOpen = !queueOpen;
            }}
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
                <path d="M21 15V6" />
                <path d="M18.5 18a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Z" />
                <path d="M12 12H3" />
                <path d="M16 6H3" />
                <path d="M12 18H3" />
            </svg>
        </button>

        <div class="volume">
            <button
                class="volume-icon"
                onclick={toggleMute}
                onwheel={handleVolumeWheel}
                aria-label={volumeInput <= 0 ? "Unmute" : "Mute"}
                title={volumeInput <= 0 ? "Unmute" : "Mute"}
            >
                <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                    <path d={volumeIconPath()} />
                </svg>
            </button>
            <div
                class="bar-track"
                bind:this={volumeBar}
                role="slider"
                aria-label="Volume"
                aria-valuemin={0}
                aria-valuemax={1}
                aria-valuenow={volumeInput}
                tabindex={0}
                onmousedown={handleVolumeMouseDown}
                onwheel={handleVolumeWheel}
            >
                <div class="bar-fill" style:width={`${volumePercent}%`}></div>
            </div>
        </div>
    </div>

    {#if queueOpen}
        <QueuePanel onClose={() => (queueOpen = false)} />
    {/if}
</div>

<style>
    .player-bar {
        grid-area: player;
        position: relative;
        height: var(--player-height);
        display: grid;
        grid-template-columns: 1fr 1.5fr 1fr;
        align-items: center;
        gap: var(--spacing-lg);
        padding: var(--spacing-sm) var(--spacing-lg);
        background: rgba(var(--color-surface-rgb), 0.65);
        backdrop-filter: blur(20px) saturate(1.8);
        -webkit-backdrop-filter: blur(20px) saturate(1.8);
        border-top: 1px solid rgba(255, 255, 255, 0.08);
        box-shadow: 0 -4px 24px rgba(0, 0, 0, 0.15);
    }

    .error {
        position: absolute;
        top: 0;
        left: 0;
        right: 0;
        transform: translateY(-100%);
        background-color: var(--color-error);
        color: var(--color-text);
        padding: var(--spacing-sm) var(--spacing-md);
        font-size: var(--font-size-sm);
    }

    .left {
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
        min-width: 0;
    }

    .art {
        width: 4rem;
        height: 4rem;
        flex-shrink: 0;
        border-radius: var(--radius-sm);
        overflow: hidden;
        background-color: rgba(var(--color-surface-rgb), 0.8);
        color: var(--color-text-muted);
        display: flex;
        align-items: center;
        justify-content: center;
        cursor: pointer;
        transition:
            box-shadow var(--transition-fast),
            transform var(--transition-fast);
    }

    .art:hover {
        box-shadow: 0 0 0 2px var(--color-accent-graphic);
        transform: scale(1.04);
    }

    .art img {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }

    .art svg {
        width: 1.25rem;
        height: 1.25rem;
    }

    .track-info {
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        text-align: left;
        min-width: 0;
        gap: var(--spacing-xs);
    }

    .track-info .title {
        font-weight: var(--font-weight-semibold);
        font-size: var(--font-size-base);
        color: var(--color-text);
        cursor: pointer;
        transition: color var(--transition-fast);
        background: none;
        border: none;
        padding: 0;
        max-width: 100%;
    }

    .track-info .title:hover {
        color: var(--color-accent-content);
    }

    .artist {
        font-size: var(--font-size-sm);
        color: var(--color-text-muted);
        max-width: 100%;
    }

    .artist a {
        color: inherit;
        transition: color var(--transition-fast);
    }

    .artist a:hover {
        color: var(--color-text);
        text-decoration: underline;
    }

    .meta-sep {
        margin: 0 var(--spacing-xs);
    }

    .lyric-line {
        font-size: var(--font-size-sm);
        color: var(--color-text-secondary);
        background: none;
        border: none;
        padding: 0;
        max-width: 100%;
        text-align: left;
        cursor: pointer;
        transition: color var(--transition-fast);
    }

    .lyric-line:hover {
        color: var(--color-text);
        text-decoration: underline;
    }

    .center {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: var(--spacing-xs);
        min-width: 0;
    }

    .controls {
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
    }

    .control-btn,
    .mode-btn,
    .play-btn {
        display: flex;
        align-items: center;
        justify-content: center;
        transition:
            transform var(--transition-fast),
            color var(--transition-fast),
            background-color var(--transition-fast);
    }

    .control-btn {
        width: 2rem;
        height: 2rem;
        color: var(--color-text);
    }

    .control-btn svg,
    .play-btn svg {
        width: 100%;
        height: 100%;
    }

    .control-btn:hover {
        color: var(--color-accent-graphic);
        transform: scale(1.08);
    }

    .mode-btn {
        position: relative;
        width: 1.25rem;
        height: 1.25rem;
        color: var(--color-text-muted);
    }

    .mode-btn:hover {
        color: var(--color-text);
        transform: scale(1.08);
    }

    .mode-btn.active {
        color: var(--color-accent-graphic);
    }

    .mode-btn.active::after {
        content: "";
        position: absolute;
        bottom: -0.375rem;
        left: 50%;
        transform: translateX(-50%);
        width: 4px;
        height: 4px;
        border-radius: var(--radius-full);
        background-color: var(--color-accent-graphic);
    }

    .mode-btn svg {
        width: 100%;
        height: 100%;
    }

    .queue-toggle {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 1.25rem;
        height: 1.25rem;
        color: var(--color-text-muted);
        transition:
            transform var(--transition-fast),
            color var(--transition-fast);
    }

    .queue-toggle:hover {
        color: var(--color-text);
        transform: scale(1.08);
    }

    .queue-toggle.active {
        color: var(--color-accent-graphic);
    }

    .queue-toggle svg {
        width: 100%;
        height: 100%;
    }

    .play-btn {
        width: 3rem;
        height: 3rem;
        border-radius: var(--radius-full);
        background-color: var(--color-text);
        color: var(--color-background);
        box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    }

    .play-btn svg {
        width: 1.375rem;
        height: 1.375rem;
    }

    .play-btn:hover {
        transform: scale(1.06);
        background-color: var(--color-accent-fill-hover);
        color: var(--color-on-accent-fill);
        box-shadow:
            inset 0 0 0 1px var(--color-accent-graphic),
            0 6px 20px
                color-mix(in srgb, var(--color-accent-seed) 35%, transparent);
    }

    .play-btn:active {
        transform: scale(0.98);
    }

    .progress {
        width: 100%;
        max-width: 28rem;
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
    }

    .time {
        font-size: var(--font-size-sm);
        line-height: 1;
        color: var(--color-text-muted);
        font-variant-numeric: tabular-nums;
        min-width: 2.5rem;
        text-align: center;
    }

    .right {
        display: flex;
        align-items: center;
        justify-content: flex-end;
        gap: var(--spacing-md);
        min-width: 0;
    }

    .volume {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        width: 100%;
        max-width: 8rem;
    }

    .volume-icon {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 1.5rem;
        height: 1.5rem;
        color: var(--color-text-muted);
        flex-shrink: 0;
        cursor: pointer;
        border-radius: var(--radius-full);
        transition: color var(--transition-fast);
    }

    .volume-icon svg {
        width: 1.25rem;
        height: 1.25rem;
    }

    .volume-icon:hover {
        color: var(--color-text);
    }

    .progress .bar-track,
    .volume .bar-track {
        height: 4px;
        transition: height 100ms ease;
    }

    .progress .bar-track:hover,
    .volume .bar-track:hover,
    .progress .bar-track:focus-visible,
    .volume .bar-track:focus-visible {
        height: 6px;
    }

    .progress .bar-fill,
    .volume .bar-fill {
        position: relative;
    }

    .progress .bar-fill::after,
    .volume .bar-fill::after {
        content: "";
        position: absolute;
        right: 0;
        top: 50%;
        transform: translate(50%, -50%) scale(0);
        width: 10px;
        height: 10px;
        border-radius: var(--radius-full);
        background-color: var(--color-text);
        transition:
            transform 100ms ease,
            background-color var(--transition-fast);
    }

    .progress .bar-track:hover .bar-fill::after,
    .volume .bar-track:hover .bar-fill::after,
    .progress .bar-track:focus-visible .bar-fill::after,
    .volume .bar-track:focus-visible .bar-fill::after {
        transform: translate(50%, -50%) scale(1);
    }

    .progress .bar-track:hover .bar-fill,
    .volume .bar-track:hover .bar-fill,
    .progress .bar-track:focus-visible .bar-fill,
    .volume .bar-track:focus-visible .bar-fill {
        background-color: var(--color-accent-graphic);
    }

    @media (max-width: 767px) {
        .player-bar {
            grid-template-columns: 1fr auto;
            grid-template-rows: auto auto;
            height: auto;
            gap: var(--spacing-sm) var(--spacing-md);
            padding: var(--spacing-md);
            backdrop-filter: blur(16px) saturate(1.8);
            -webkit-backdrop-filter: blur(16px) saturate(1.8);
        }

        .left {
            grid-column: 1;
            grid-row: 1;
        }

        .center {
            grid-column: 1 / -1;
            grid-row: 2;
            align-items: stretch;
        }

        .progress {
            max-width: none;
        }

        .right {
            grid-column: 2;
            grid-row: 1;
        }

        .volume {
            display: none;
        }
    }
</style>
