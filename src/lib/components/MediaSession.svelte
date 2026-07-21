<script lang="ts">
    import { onMount } from "svelte";
    import { getAlbumArtData, type Track } from "$lib/api";
    import { bytesToBase64 } from "$lib/utils/base64";
    import { initializeMediaSessionOnce } from "$lib/utils/mediaSession";
    import { playback } from "$lib/stores/playback";
    import {
        mediaControls,
        PlaybackStatus,
        RepeatMode,
    } from "tauri-plugin-media-api";

    const POSITION_SYNC_INTERVAL_MS = 1000;

    let initialized = $state(false);
    let metadataSignature: string | null = null;
    let playbackInfoSignature: string | null = null;
    let metadataRequest = 0;
    let artworkTrackId: number | null = null;
    let artworkData: string | undefined;
    let artworkRequest: Promise<string | undefined> | undefined;
    let mediaQueue: Promise<void> = Promise.resolve();
    let positionTimer: ReturnType<typeof setTimeout> | undefined;
    let lastPositionTrackId: number | null = null;
    let lastPositionSeconds = -1;
    let lastPositionSentAt = 0;

    function queueMedia(operation: () => Promise<void>) {
        mediaQueue = mediaQueue.then(operation).catch((error) => {
            console.error("Failed to update native media controls:", error);
        });
    }

    function toSeconds(milliseconds: number | null | undefined): number {
        return Math.max(0, (milliseconds ?? 0) / 1000);
    }

    function durationSeconds(): number {
        return toSeconds($playback.duration_ms);
    }

    function positionSeconds(): number {
        const duration = durationSeconds();
        const position = toSeconds($playback.position_ms);
        return duration > 0 ? Math.min(position, duration) : position;
    }

    function nativeRepeatMode(): RepeatMode {
        switch ($playback.repeat_mode) {
            case "one":
                return RepeatMode.Track;
            case "all":
                return RepeatMode.List;
            default:
                return RepeatMode.None;
        }
    }

    function currentPlaybackInfo() {
        return {
            status: !$playback.current_track
                ? PlaybackStatus.Stopped
                : $playback.is_playing
                  ? PlaybackStatus.Playing
                  : PlaybackStatus.Paused,
            position: positionSeconds(),
            shuffle: $playback.shuffle,
            repeatMode: nativeRepeatMode(),
            playbackRate: 1,
        };
    }

    function metadataFor(track: Track, art?: string) {
        return {
            title: track.title || "Unknown",
            artist: track.artist_names?.join(", ") || undefined,
            album: track.album_title || undefined,
            duration: durationSeconds(),
            // The native plugin deserializes this as raw base64 image bytes;
            // browser blob URLs cannot be resolved by Windows.
            artworkData: art ?? "",
        };
    }

    function isCurrentMetadataRequest(
        trackId: number,
        request: number,
    ): boolean {
        return (
            initialized &&
            request === metadataRequest &&
            $playback.current_track?.id === trackId
        );
    }

    function fetchArtwork(track: Track): Promise<string | undefined> {
        if (artworkTrackId === track.id && artworkRequest) {
            return artworkRequest;
        }

        artworkTrackId = track.id;
        artworkData = undefined;
        artworkRequest = (async () => {
            if (!track.album_id) return undefined;
            try {
                const art = await getAlbumArtData(track.album_id);
                if (artworkTrackId !== track.id || !art.data?.length) {
                    return undefined;
                }
                artworkData = bytesToBase64(art.data);
                return artworkData;
            } catch {
                return undefined;
            }
        })();
        return artworkRequest;
    }

    function syncMetadata(track: Track | null) {
        const request = ++metadataRequest;
        if (!track) {
            artworkTrackId = null;
            artworkData = undefined;
            artworkRequest = undefined;
            queueMedia(async () => {
                if (
                    !initialized ||
                    request !== metadataRequest ||
                    $playback.current_track
                ) {
                    return;
                }
                await mediaControls.clearNowPlaying();
                await mediaControls.updatePlaybackStatus(
                    PlaybackStatus.Stopped,
                );
            });
            return;
        }

        const trackId = track.id;
        queueMedia(async () => {
            if (!isCurrentMetadataRequest(trackId, request)) return;
            await mediaControls.updateNowPlaying(
                metadataFor(track),
                currentPlaybackInfo(),
            );
        });

        void fetchArtwork(track).then((art) => {
            if (!art) return;
            queueMedia(async () => {
                if (!isCurrentMetadataRequest(trackId, request)) return;
                await mediaControls.updateNowPlaying(
                    metadataFor(track, art),
                    currentPlaybackInfo(),
                );
            });
        });
    }

    function schedulePositionSync(trackId: number, positionMs: number) {
        if (trackId !== lastPositionTrackId) {
            if (positionTimer) clearTimeout(positionTimer);
            positionTimer = undefined;
            lastPositionTrackId = trackId;
            lastPositionSeconds = -1;
            lastPositionSentAt = 0;
        }

        const candidatePosition = toSeconds(positionMs);
        const now = Date.now();
        const positionChanged =
            lastPositionSeconds < 0 ||
            Math.abs(candidatePosition - lastPositionSeconds) >= 1;
        if (!positionChanged || positionTimer) return;

        const delay = Math.max(
            0,
            POSITION_SYNC_INTERVAL_MS - (now - lastPositionSentAt),
        );
        positionTimer = setTimeout(() => {
            positionTimer = undefined;
            if (!initialized || $playback.current_track?.id !== trackId) return;
            const position = positionSeconds();
            lastPositionSeconds = position;
            lastPositionSentAt = Date.now();
            queueMedia(async () => {
                if (!initialized || $playback.current_track?.id !== trackId) {
                    return;
                }
                await mediaControls.updatePosition(position);
            });
        }, delay);
    }

    onMount(() => {
        let disposed = false;

        void (async () => {
            try {
                await initializeMediaSessionOnce();
                if (!disposed) initialized = true;
            } catch (error) {
                console.error("Failed to initialize media controls:", error);
            }
        })();

        return () => {
            disposed = true;
            metadataRequest += 1;
            if (positionTimer) clearTimeout(positionTimer);
        };
    });

    $effect(() => {
        const track = $playback.current_track;
        const signature = track
            ? [
                  track.id,
                  track.title ?? "",
                  track.artist_names?.join(",") ?? "",
                  track.album_id ?? "",
                  track.album_title ?? "",
                  $playback.duration_ms,
              ].join("\u0001")
            : "none";
        if (!initialized || signature === metadataSignature) return;
        metadataSignature = signature;
        syncMetadata(track);
    });

    $effect(() => {
        const trackId = $playback.current_track?.id ?? null;
        const signature = [
            trackId ?? "none",
            $playback.is_playing,
            $playback.shuffle,
            $playback.repeat_mode,
        ].join("\u0001");
        if (
            !initialized ||
            trackId === null ||
            signature === playbackInfoSignature
        ) {
            return;
        }
        playbackInfoSignature = signature;
        queueMedia(async () => {
            const currentTrack = $playback.current_track;
            if (!initialized || currentTrack?.id !== trackId) return;
            await mediaControls.updateNowPlaying(
                metadataFor(currentTrack),
                currentPlaybackInfo(),
            );
        });
    });

    $effect(() => {
        const trackId = $playback.current_track?.id;
        const positionMs = $playback.position_ms;
        if (!initialized || trackId === undefined || trackId === null) {
            if (positionTimer) clearTimeout(positionTimer);
            positionTimer = undefined;
            if (!trackId) lastPositionTrackId = null;
            return;
        }
        schedulePositionSync(trackId, positionMs);
    });
</script>
