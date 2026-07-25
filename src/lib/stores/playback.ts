import { readable, writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import {
    getPlaybackState,
    play as backendPlay,
    pause as backendPause,
    stop as backendStop,
    seek as backendSeek,
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
    type RepeatMode,
} from "$lib/api";

export interface Track extends ApiTrack {}

export interface PlaybackState extends ApiPlaybackState {
    error: string | null;
}

const initialState: PlaybackState = {
    is_playing: false,
    current_track: null,
    position_ms: 0,
    duration_ms: 0,
    volume: 0.8,
    shuffle: false,
    repeat_mode: "off" as RepeatMode,
    error: null,
};

function createPlaybackStore() {
    const { subscribe, set, update } = writable<PlaybackState>({
        ...initialState,
    });

    async function init() {
        try {
            const state = await getPlaybackState();
            set({ ...state, error: null });
        } catch (err) {
            console.error("Failed to get initial playback state:", err);
            update((s) => ({ ...s, error: String(err) }));
        }

        try {
            await listen<{
                is_playing: boolean;
                current_track: Track | null;
                position_ms: number;
                duration_ms: number;
                shuffle: boolean;
                repeat_mode: RepeatMode;
            }>("playback-state-changed", (event) => {
                update((state) => ({
                    ...state,
                    is_playing: event.payload.is_playing,
                    current_track: event.payload.current_track,
                    position_ms: event.payload.position_ms,
                    duration_ms: event.payload.duration_ms,
                    shuffle: event.payload.shuffle,
                    repeat_mode: event.payload.repeat_mode,
                    error: null,
                }));
            });
        } catch (err) {
            console.error("Failed to listen to playback-state-changed:", err);
        }

        try {
            await listen<{
                track_id: number;
                position_ms: number;
                duration_ms: number;
            }>("playback-progress", (event) => {
                update((state) => ({
                    ...state,
                    position_ms: event.payload.position_ms,
                    duration_ms: event.payload.duration_ms,
                }));
            });
        } catch (err) {
            console.error("Failed to listen to playback-progress:", err);
        }
    }

    if (typeof window !== "undefined") {
        init();
    }

    async function callCommand(fn: () => Promise<ApiPlaybackState>) {
        try {
            const state = await fn();
            set({ ...state, error: null });
            return state;
        } catch (err) {
            const message = String(err);
            console.error("Playback command failed:", err);
            update((s) => ({ ...s, error: message, is_playing: false }));
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
        play: () => callCommand(backendPlay),
        pause: () => callCommand(backendPause),
        stop: () => callCommand(backendStop),
        seek: (positionMs: number) =>
            callCommand(() => backendSeek(positionMs)),
        nextTrack: () => callCommand(backendNextTrack),
        previousTrack: () => callCommand(backendPreviousTrack),
        setVolume: (volume: number) =>
            callCommand(() => backendSetVolume(volume)),
        setVolumeLive: (volume: number) =>
            backendSetVolumeLive(volume).catch((err) => {
                console.error("Live volume update failed:", err);
            }),
        setShuffle: (shuffle: boolean) =>
            callCommand(() => backendSetShuffle(shuffle)),
        cycleRepeatMode: () => callCommand(backendCycleRepeatMode),
        playNext: (trackId: number) =>
            callCommand(() => backendPlayNext(trackId)),
        playQueueIndex: (orderPos: number) =>
            callCommand(() => backendPlayQueueIndex(orderPos)),
        updateCurrentTrackLrcOffset,
        updateCurrentTrackLyricsSource,
        // shuffle = explicit context switch: page Play buttons pass false,
        // page Shuffle buttons pass true, individual track picks pass undefined
        // (the player's current mode is kept).
        loadQueue: (trackIds: number[], startIndex = 0, shuffle?: boolean) =>
            callCommand(() => backendLoadQueue(trackIds, startIndex, shuffle)),
        playTrack: (trackId: number) =>
            callCommand(() => backendPlayTrack(trackId)),
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
