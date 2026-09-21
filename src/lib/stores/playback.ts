import { logger } from "$lib/logger";
import { readable, writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import {
    getPlaybackState,
    describePlaybackError,
    play as backendPlay,
    pause as backendPause,
    stop as backendStop,
    seek as backendSeek,
    seekLyrics as backendSeekLyrics,
    nextTrack as backendNextTrack,
    previousTrack as backendPreviousTrack,
    setVolume as backendSetVolume,
    setVolumeLive as backendSetVolumeLive,
    setShuffle as backendSetShuffle,
    cycleRepeatMode as backendCycleRepeatMode,
    playNext as backendPlayNext,
    playQueueIndex as backendPlayQueueIndex,
    loadQueue as backendLoadQueue,
    playTrack as backendPlayTrack,
    type Track as ApiTrack,
    type PlaybackState as ApiPlaybackState,
    type PlaybackProgress,
    type PlaybackActionSource,
    type PlaybackContext,
    type RepeatMode,
    LYRICS_CHANGED_EVENT,
    PLAYBACK_STATE_EVENT,
    PLAYBACK_PROGRESS_EVENT,
} from "$lib/api";

export interface Track extends ApiTrack {}

export interface PlaybackState extends ApiPlaybackState {
    error: string | null;
}

const initialState: PlaybackState = {
    revision: -1,
    is_playing: false,
    current_track: null,
    first_lyric_line: null,
    album_art: null,
    position_ms: 0,
    duration_ms: 0,
    volume: 0.8,
    shuffle: false,
    repeat_mode: "off" as RepeatMode,
    error: null,
};

export function createPlaybackStore() {
    const {
        subscribe,
        set: setState,
        update,
    } = writable<PlaybackState>({
        ...initialState,
    });
    const localOffsets = new Map<number, number>();
    let nativeRevision = -1;
    let progressSequence = -1;

    function withLocalOffset(track: Track | null): Track | null {
        if (!track || !localOffsets.has(track.id)) return track;
        const offset = localOffsets.get(track.id)!;
        if (track.lrc_offset_ms === offset) {
            localOffsets.delete(track.id);
            return track;
        }
        return { ...track, lrc_offset_ms: offset };
    }

    function set(state: PlaybackState) {
        setState({
            ...state,
            current_track: withLocalOffset(state.current_track),
        });
    }

    function acceptSnapshot(state: ApiPlaybackState) {
        // Equal revisions can be an older command reply that arrives after
        // progress for the already-published state. Do not rewind its clock.
        if (state.revision <= nativeRevision) return;
        nativeRevision = state.revision;
        progressSequence = -1;
        set({ ...state, error: null });
    }

    async function init() {
        // Subscribe before reading the snapshot so startup cannot miss a change.
        try {
            await listen<ApiPlaybackState>(PLAYBACK_STATE_EVENT, (event) => {
                acceptSnapshot(event.payload);
            });
        } catch (err) {
            void logger.error(
                "playback",
                "failed_to_listen_to_playback_state_changed",
                err,
            );
        }

        try {
            await listen<PlaybackProgress>(PLAYBACK_PROGRESS_EVENT, (event) => {
                update((state) => {
                    // A track ID alone cannot distinguish a seek, restart, or
                    // another listen to the same track. Future revisions wait
                    // for their complete snapshot; old revisions are discarded.
                    if (
                        event.payload.revision !== nativeRevision ||
                        event.payload.sequence <= progressSequence ||
                        state.current_track?.id !== event.payload.track_id
                    )
                        return state;
                    progressSequence = event.payload.sequence;
                    return {
                        ...state,
                        position_ms: event.payload.position_ms,
                        duration_ms: event.payload.duration_ms,
                    };
                });
            });
        } catch (err) {
            void logger.error(
                "playback",
                "failed_to_listen_to_playback_progress",
                err,
            );
        }

        try {
            acceptSnapshot(await getPlaybackState());
        } catch (err) {
            void logger.error(
                "playback",
                "failed_to_get_initial_playback_state",
                err,
            );
            update((s) => ({ ...s, error: String(err) }));
        }
    }

    if (typeof window !== "undefined") {
        window.addEventListener?.(LYRICS_CHANGED_EVENT, (event) => {
            const trackId = (event as CustomEvent<{ trackId: number }>).detail
                ?.trackId;
            if (trackId != null) updateCurrentTrackLrcOffset(trackId, 0);
        });
        init();
    }

    async function callCommand(fn: () => Promise<ApiPlaybackState>) {
        try {
            const state = await fn();
            acceptSnapshot(state);
            update((s) => ({ ...s, error: null }));
            return state;
        } catch (err) {
            const message = String(err);
            void logger.error(
                "playback",
                "playback_command_failed",
                describePlaybackError(err),
            );
            update((s) => ({ ...s, error: message }));
            throw err;
        }
    }

    function updateCurrentTrack(trackId: number, patch: Partial<Track>) {
        update((state) => {
            if (state.current_track?.id !== trackId) return state;
            return {
                ...state,
                current_track: {
                    ...state.current_track,
                    ...patch,
                },
            };
        });
    }

    function updateCurrentTrackLrcOffset(trackId: number, offsetMs: number) {
        localOffsets.set(trackId, offsetMs);
        updateCurrentTrack(trackId, { lrc_offset_ms: offsetMs });
    }

    function updateCurrentTrackLyricsSource(
        trackId: number,
        source: string | null,
    ) {
        updateCurrentTrack(trackId, { lyrics_source: source });
    }

    return {
        subscribe,
        set,
        play: (source: PlaybackActionSource = "ui") =>
            callCommand(() => backendPlay(source)),
        pause: (source: PlaybackActionSource = "ui") =>
            callCommand(() => backendPause(source)),
        stop: (source: PlaybackActionSource = "ui") =>
            callCommand(() => backendStop(source)),
        seek: (positionMs: number, source: PlaybackActionSource = "ui") =>
            callCommand(() => backendSeek(positionMs, source)),
        seekLyrics: (trackId: number, positionMs: number) =>
            callCommand(() => backendSeekLyrics(trackId, positionMs)),
        nextTrack: (source: PlaybackActionSource = "ui") =>
            callCommand(() => backendNextTrack(source)),
        previousTrack: (source: PlaybackActionSource = "ui") =>
            callCommand(() => backendPreviousTrack(source)),
        setVolume: (volume: number, source: PlaybackActionSource = "ui") =>
            callCommand(() => backendSetVolume(volume, source)),
        setVolumeLive: (volume: number, source: PlaybackActionSource = "ui") =>
            backendSetVolumeLive(volume, source).catch((err) => {
                void logger.error(
                    "playback",
                    "live_volume_update_failed",
                    describePlaybackError(err),
                );
            }),
        setShuffle: (shuffle: boolean, source: PlaybackActionSource = "ui") =>
            callCommand(() => backendSetShuffle(shuffle, source)),
        cycleRepeatMode: (source: PlaybackActionSource = "ui") =>
            callCommand(() => backendCycleRepeatMode(source)),
        playNext: (trackId: number, source: PlaybackActionSource = "ui") =>
            callCommand(() => backendPlayNext(trackId, source)),
        playQueueIndex: (
            orderPos: number,
            source: PlaybackActionSource = "ui",
        ) => callCommand(() => backendPlayQueueIndex(orderPos, source)),
        updateCurrentTrackLrcOffset,
        updateCurrentTrackLyricsSource,
        // shuffle = explicit context switch: page Play buttons pass false,
        // page Shuffle buttons pass true, individual track picks pass undefined
        // (the player's current mode is kept).
        loadQueue: (
            trackIds: number[],
            startIndex = 0,
            shuffle?: boolean,
            context?: PlaybackContext,
            source: PlaybackActionSource = "ui",
        ) =>
            callCommand(() =>
                backendLoadQueue(
                    trackIds,
                    startIndex,
                    shuffle,
                    context,
                    source,
                ),
            ),
        playTrack: (
            trackId: number,
            context?: PlaybackContext,
            source: PlaybackActionSource = "ui",
        ) => callCommand(() => backendPlayTrack(trackId, context, source)),
    };
}

export const playback = createPlaybackStore();

// The native engine intentionally publishes coarse progress updates to keep
// event traffic low. Lyrics need a clock that stays responsive between those
// corrections, so expose an interpolated position without changing the
// canonical playback state used by seeking, persistence, and controls.
export const interpolatedPositionMs = readable(0, (set) => {
    if (typeof window === "undefined") return () => {};

    let state: PlaybackState = initialState;
    let samplePositionMs = 0;
    let sampleAtMs = 0;

    const unsubscribe = playback.subscribe((next) => {
        state = next;
        samplePositionMs = next.position_ms;
        sampleAtMs = performance.now();
        set(next.position_ms);
    });

    let frame = 0;
    const update = (nowMs: number) => {
        const elapsedMs = state.is_playing
            ? Math.max(0, nowMs - sampleAtMs)
            : 0;
        const positionMs = samplePositionMs + elapsedMs;
        set(
            state.duration_ms > 0
                ? Math.min(positionMs, state.duration_ms)
                : positionMs,
        );
        frame = requestAnimationFrame(update);
    };
    frame = requestAnimationFrame(update);

    return () => {
        cancelAnimationFrame(frame);
        unsubscribe();
    };
});

export const {
    play,
    pause,
    stop,
    seek,
    seekLyrics,
    nextTrack,
    previousTrack,
    setVolume,
    setVolumeLive,
    setShuffle,
    cycleRepeatMode,
    playNext,
    playQueueIndex,
    loadQueue,
    playTrack,
    updateCurrentTrackLrcOffset,
    updateCurrentTrackLyricsSource,
} = playback;
