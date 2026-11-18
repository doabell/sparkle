import type { Album } from "#lib/api.ts";

export function getRecentlyAddedAlbums(albums: readonly Album[]): Album[] {
    return [...albums].sort(
        (a, b) => b.created_at - a.created_at || b.id - a.id,
    );
}
