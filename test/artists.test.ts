// @ts-nocheck
import { strict as assert } from "node:assert";
import { test } from "bun:test";
import { getArtistDisplayEntries } from "../src/lib/utils/artists.ts";

test("keeps every artist name when linkable ids are incomplete", () => {
    assert.deepEqual(
        getArtistDisplayEntries(["First Artist", "Second Artist"], [7]),
        [
            { name: "First Artist", id: 7 },
            { name: "Second Artist", id: null },
        ],
    );
});
