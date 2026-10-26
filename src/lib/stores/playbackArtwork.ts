import { readable, type Readable } from "svelte/store";
import { getAlbumArt, getCachedAlbumArt, type CachedImage } from "$lib/api";
import { playback, type PlaybackState } from "$lib/stores/playback";

type ArtworkState = Pick<PlaybackState, "current_track" | "album_art">;

export function createPlaybackArtwork(source: Readable<ArtworkState>) {
    return readable<CachedImage | null>(null, (set) => {
        let trackId: number | null | undefined;
        let albumId: number | null | undefined;
        let generation = 0;
        const unsubscribe = source.subscribe((state) => {
            const nextTrackId = state.current_track?.id ?? null;
            const nextAlbumId = state.current_track?.album_id ?? null;
            if (nextTrackId === trackId && nextAlbumId === albumId) return;
            trackId = nextTrackId;
            albumId = nextAlbumId;
            const request = ++generation;
            if (!albumId) {
                set(null);
                return;
            }

            // Both player layouts use the snapshot/cache immediately. If the
            // next cover is cold, retain the current one until its request ends.
            const initial = state.album_art ?? getCachedAlbumArt(albumId);
            if (initial?.file_path) set(initial);
            void getAlbumArt(albumId).then(
                (image) => {
                    if (request === generation) set(image);
                },
                () => {
                    if (request === generation) set(initial ?? null);
                },
            );
        });
        return () => {
            generation++;
            unsubscribe();
        };
    });
}

export const playbackArtwork = createPlaybackArtwork(playback);
