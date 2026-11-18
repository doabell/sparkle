// @ts-nocheck
import { expect, test } from "bun:test";
import type { Album } from "../src/lib/api.ts";
import { getRecentlyAddedAlbums } from "../src/lib/utils/albums.ts";

test("recent albums follow library additions even without release dates", () => {
    const albums: Album[] = [
        { id: 3, title: "New release", year: 2026, created_at: 100 },
        { id: 2, title: "Old release", year: 1970, created_at: 300 },
        { id: 1, title: "Undated", year: null, created_at: 400 },
        { id: 4, title: "Missing year", created_at: 200 },
    ];

    expect(getRecentlyAddedAlbums(albums).map((album) => album.id)).toEqual([
        1, 2, 4, 3,
    ]);
});

test("albums added in the same second use insertion order, not title", () => {
    const albums: Album[] = [
        { id: 10, title: "A first", created_at: 100 },
        { id: 12, title: "Z last", created_at: 100 },
        { id: 11, title: "B second", created_at: 100 },
    ];

    expect(getRecentlyAddedAlbums(albums).map((album) => album.id)).toEqual([
        12, 11, 10,
    ]);
});

test("recent albums keep the full ranking for any row width without changing library order", () => {
    const albums: readonly Album[] = Object.freeze(
        Array.from({ length: 8 }, (_, index) => ({
            id: index + 1,
            title: `Album ${index + 1}`,
            created_at: 100 + index,
        })),
    );

    expect(getRecentlyAddedAlbums(albums).map((album) => album.id)).toEqual([
        8, 7, 6, 5, 4, 3, 2, 1,
    ]);
    expect(albums.map((album) => album.id)).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
    expect(getRecentlyAddedAlbums([])).toEqual([]);
});
