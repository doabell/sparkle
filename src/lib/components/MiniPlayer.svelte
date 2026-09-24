<script lang="ts">
    import Artwork from "$lib/components/Artwork.svelte";
    import MiniLyrics from "$lib/components/MiniLyrics.svelte";
    import {
        playback,
        interpolatedPositionMs,
        play,
        pause,
        previousTrack,
        nextTrack,
        seek,
        setVolume,
        setVolumeLive,
        getUnmuteVolume,
        setShuffle,
        cycleRepeatMode,
    } from "$lib/stores/playback";
    import { formatTime } from "$lib/utils/formatTime";

    let track = $derived($playback.current_track);
    let artist = $derived(track?.artist_names?.join(", ") || "Unknown artist");
    let artistAndAlbum = $derived(
        track?.album_title ? artist + " · " + track.album_title : artist,
    );
    let trackId = $derived(track?.id);
    let repeatLabel = $derived(
        $playback.repeat_mode === "off"
            ? "Repeat off — repeat all"
            : $playback.repeat_mode === "all"
              ? "Repeat all — repeat one"
              : "Repeat one — turn repeat off",
    );
    let seekPreview = $state<number | null>(null);
    let seeking = $state(false);
    let seekStartValue = 0;
    let seekRequest = 0;
    let volumePreview = $state<number | null>(null);
    let volumeRequest = 0;
    let lastLiveVolumeAt = 0;
    let position = $derived(seekPreview ?? $interpolatedPositionMs);
    let duration = $derived($playback.duration_ms);
    let volume = $derived(volumePreview ?? $playback.volume);

    $effect(() => {
        trackId;
        seekRequest++;
        seeking = false;
        seekPreview = null;
    });

    // The shared store exposes playback failures. Catch them here so a failed
    // transport command does not become an unhandled event-handler rejection.
    async function control(action: () => Promise<unknown>) {
        try {
            await action();
        } catch {
            /* Shown below from playback.error. */
        }
    }

    async function commitSeek(event: Event) {
        if (!track || !duration) return;
        const request = ++seekRequest;
        const target = Number((event.currentTarget as HTMLInputElement).value);
        seeking = false;
        seekPreview = target;
        try {
            await seek(target);
        } catch {
            /* The store reports the error. */
        } finally {
            if (request === seekRequest) seekPreview = null;
        }
    }

    async function changeVolume(value: number) {
        const request = ++volumeRequest;
        volumePreview = value;
        try {
            await setVolume(value);
        } catch {
            /* The store reports the error. */
        } finally {
            if (request === volumeRequest) volumePreview = null;
        }
    }

    function previewVolume(value: number) {
        volumeRequest++;
        volumePreview = value;
        const now = performance.now();
        if (now - lastLiveVolumeAt >= 50) {
            lastLiveVolumeAt = now;
            void control(() => setVolumeLive(value));
        }
    }
</script>

<section class="mini-player" aria-label="Mini player">
    <div class="track">
        <div class="artwork">
            <Artwork
                albumId={track?.album_id}
                alt={track?.album_title ?? "Album art"}
            />
        </div>
        <div class="track-details">
            <div
                class="track-title ellipsis"
                title={track?.title ?? "No track selected"}
            >
                {track?.title ?? "No track selected"}
            </div>
            <div
                class="artist ellipsis"
                title={track
                    ? artistAndAlbum
                    : "Choose music in the full player"}
            >
                {track ? artistAndAlbum : "Choose music in the full player"}
            </div>
            {#if $playback.error}
                <div
                    class="error ellipsis"
                    role="status"
                    title={$playback.error}
                >
                    {$playback.error}
                </div>
            {:else}
                <MiniLyrics
                    trackId={trackId ?? null}
                    positionMs={$interpolatedPositionMs}
                    offsetMs={track?.lrc_offset_ms ?? 0}
                    firstLine={$playback.first_lyric_line}
                />
            {/if}
        </div>
    </div>
    <div class="transport">
        <button
            class="icon-button mode"
            class:active={$playback.shuffle}
            aria-label="Shuffle"
            aria-pressed={$playback.shuffle}
            title={$playback.shuffle ? "Turn shuffle off" : "Turn shuffle on"}
            onclick={() => control(() => setShuffle(!$playback.shuffle))}
        >
            <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
            >
                <path
                    d="M3 6h2c4 0 10 12 14 12h2m-4-4 4 4-4 4M3 18h2c2 0 4-3 6-6s6-6 8-6h2m-4-4 4 4-4 4"
                />
            </svg>
        </button>

        <button
            class="icon-button"
            onclick={() => control(() => previousTrack())}
            disabled={!track}
            aria-label="Previous track"
            title="Previous track"
        >
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"
                ><path d="M6 6h2v12H6zm3.5 6 8.5 6V6z" /></svg
            >
        </button>
        <button
            class="icon-button play"
            onclick={() =>
                control(() => ($playback.is_playing ? pause() : play()))}
            disabled={!track}
            aria-label={$playback.is_playing ? "Pause" : "Play"}
            title={$playback.is_playing ? "Pause" : "Play"}
        >
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"
                ><path
                    d={$playback.is_playing
                        ? "M6 19h4V5H6v14zm8-14v14h4V5h-4z"
                        : "M8 5v14l11-7z"}
                /></svg
            >
        </button>
        <button
            class="icon-button"
            onclick={() => control(() => nextTrack())}
            disabled={!track}
            aria-label="Next track"
            title="Next track"
        >
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"
                ><path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" /></svg
            >
        </button>
        <button
            class="icon-button mode"
            class:active={$playback.repeat_mode !== "off"}
            aria-label={repeatLabel}
            aria-pressed={$playback.repeat_mode !== "off"}
            title={repeatLabel}
            onclick={() => control(() => cycleRepeatMode())}
        >
            <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
            >
                <path
                    d="m17 2 4 4-4 4M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4m14-1v2a3 3 0 0 1-3 3H3"
                />
                {#if $playback.repeat_mode === "one"}<path
                        d="m10 10 2-1v6m-2 0h4"
                    />{/if}
            </svg>
        </button>
        <div class="volume">
            <button
                class="icon-button mute"
                onclick={() => changeVolume(volume > 0 ? 0 : getUnmuteVolume())}
                aria-label={volume > 0 ? "Mute" : "Unmute"}
                title={volume > 0 ? "Mute" : "Unmute"}
            >
                <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.7"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    <path d="M11 5 6 9H3v6h3l5 4V5Z" />
                    {#if volume > 0}<path
                            d="M15 8a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14"
                        />{:else}<path d="m16 9 6 6m0-6-6 6" />{/if}
                </svg>
            </button>
            <input
                type="range"
                min="0"
                max="1"
                step="0.01"
                value={volume}
                aria-label="Volume"
                aria-valuetext={`${Math.round(volume * 100)}%`}
                style:--fill={`${volume * 100}%`}
                oninput={(event) =>
                    previewVolume(Number(event.currentTarget.value))}
                onchange={(event) =>
                    changeVolume(Number(event.currentTarget.value))}
                onpointercancel={() => changeVolume(volume)}
            />
        </div>
    </div>
    <div class="timeline">
        <span>{formatTime(position)}</span>
        <input
            class="seek"
            type="range"
            min="0"
            max={duration || 1}
            step="1000"
            value={Math.min(position, duration)}
            disabled={!track || duration <= 0}
            aria-label="Seek"
            aria-valuetext={`${formatTime(position)} of ${formatTime(duration)}`}
            style:--fill={`${duration ? Math.min(100, (position / duration) * 100) : 0}%`}
            onpointerdown={(event) => {
                seekRequest++;
                seeking = true;
                seekStartValue = Number(event.currentTarget.value);
                seekPreview = null;
            }}
            onpointerup={(event) => {
                // Native ranges do not fire change when released unchanged.
                if (
                    seeking &&
                    Number(event.currentTarget.value) === seekStartValue
                ) {
                    seeking = false;
                    seekPreview = null;
                }
            }}
            oninput={(event) => {
                seekPreview = Number(event.currentTarget.value);
            }}
            onchange={commitSeek}
            onpointercancel={() => {
                seeking = false;
                seekPreview = null;
            }}
            onblur={() => {
                if (seeking) {
                    seeking = false;
                    seekPreview = null;
                }
            }}
        />
        <span>{formatTime(duration)}</span>
    </div>
</section>

<style>
    .mini-player {
        height: 100vh;
        padding: 8px 12px 10px;
        display: grid;
        grid-template-rows: 68px 36px 20px;
        gap: 6px;
        color: var(--color-text);
        background: var(--color-background);
        overflow: hidden;
    }
    .track {
        display: flex;
        gap: 12px;
        align-items: center;
        min-width: 0;
    }
    .artwork {
        width: 68px;
        height: 68px;
        flex: 0 0 68px;
        overflow: hidden;
        border-radius: var(--radius-sm);
        background: var(--color-surface-elevated);
    }
    .artwork :global(img),
    .artwork :global(.artwork-fallback) {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }
    .track-details {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        min-width: 0;
        flex: 1;
    }
    .track-title {
        /* The first line shares vertical space with the window controls. */
        max-width: calc(100% - 4 * var(--window-chrome-height) + 8px);
        font-size: 14px;
        line-height: 20px;
        font-weight: var(--font-weight-semibold);
    }
    .artist {
        color: var(--color-text-secondary);
        font-size: 12px;
        line-height: 18px;
    }
    .transport {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }
    .icon-button {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 32px;
        height: 32px;
        flex-shrink: 0;
        padding: 0;
        border-radius: var(--radius-full);
        background: transparent;
        color: var(--color-text);
    }
    .icon-button:hover {
        background: var(--interactive-hover);
    }
    .icon-button:active {
        background: var(--interactive-active);
    }
    .icon-button svg {
        width: 18px;
        height: 18px;
    }
    .icon-button.mode {
        position: relative;
        color: var(--color-text-muted);
        background: transparent;
    }
    .mode:hover {
        color: var(--color-text);
    }
    .mode svg {
        width: 16px;
        height: 16px;
        transition: transform var(--transition-transform);
    }
    .mode:hover svg {
        transform: scale(var(--motion-hover-scale));
    }
    .mode:active svg {
        transform: scale(var(--motion-press-scale));
    }
    .mode.active {
        color: var(--color-accent-graphic);
    }
    .mode.active::after {
        content: "";
        position: absolute;
        bottom: 2px;
        left: 50%;
        transform: translateX(-50%);
        width: 4px;
        height: 4px;
        border-radius: var(--radius-full);
        background-color: var(--color-accent-graphic);
        pointer-events: none;
    }
    .play {
        width: 36px;
        height: 36px;
        background: var(--color-accent-fill);
        color: var(--color-on-accent-fill);
        transition: transform var(--transition-transform);
    }
    .play:hover {
        background: var(--color-accent-fill-hover);
        transform: scale(var(--motion-hover-scale));
    }
    .play:active {
        background: var(--color-accent-fill-active);
        transform: scale(var(--motion-press-scale));
    }
    button:disabled {
        opacity: 0.45;
        cursor: default;
    }
    .timeline {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }
    .timeline span {
        font-size: 10px;
        font-variant-numeric: tabular-nums;
        color: var(--color-text-secondary);
        min-width: 28px;
    }
    .timeline span:last-child {
        text-align: right;
    }
    input[type="range"] {
        appearance: none;
        -webkit-appearance: none;
        min-width: 0;
        width: 100%;
        height: 20px;
        margin: 0;
        padding: 0;
        border: 0;
        border-radius: var(--radius-full);
        background: transparent;
        cursor: pointer;
    }
    input[type="range"]::-webkit-slider-runnable-track {
        height: 3px;
        border-radius: var(--radius-full);
        background: linear-gradient(
            to right,
            var(--color-accent-graphic) var(--fill),
            var(--color-surface-raised) var(--fill)
        );
    }
    input[type="range"]::-webkit-slider-thumb {
        appearance: none;
        width: 9px;
        height: 9px;
        border-radius: 50%;
        margin-top: -3px;
        background: var(--color-text);
    }
    input[type="range"]::-moz-range-track {
        height: 3px;
        border-radius: var(--radius-full);
        background: var(--color-surface-raised);
    }
    input[type="range"]::-moz-range-progress {
        height: 3px;
        background: var(--color-accent-graphic);
    }
    input[type="range"]::-moz-range-thumb {
        width: 9px;
        height: 9px;
        border: 0;
        border-radius: 50%;
        background: var(--color-text);
    }
    input:disabled {
        opacity: 0.45;
        cursor: default;
    }
    .error {
        color: var(--color-error);
        font-size: 12px;
        line-height: 18px;
    }
    .volume {
        display: flex;
        align-items: center;
        gap: 4px;
        width: 112px;
        margin-left: auto;
        flex-shrink: 0;
    }
    .mute {
        width: 24px;
        height: 24px;
        color: var(--color-text-secondary);
    }
    .mute svg {
        width: 15px;
        height: 15px;
    }
</style>
