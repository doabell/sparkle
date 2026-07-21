export interface ArtistDisplayEntry {
    name: string;
    id: number | null;
}

export function getArtistDisplayEntries(
    names?: readonly string[] | null,
    ids?: readonly number[] | null,
): ArtistDisplayEntry[] {
    return (names ?? []).map((name, index) => ({
        name,
        id: ids?.[index] ?? null,
    }));
}
