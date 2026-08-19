// @ts-nocheck
import { strict as assert } from "node:assert";
import { test } from "bun:test";
import {
    createGroupScrollIndexEntries,
    createScrollIndexEntries,
    getScrollIndexLabel,
} from "../src/lib/utils/songIndex.ts";

test("uses two-digit labels only for year indexes", () => {
    assert.equal(getScrollIndexLabel("1998", "year", "en"), "98");
    assert.equal(getScrollIndexLabel("1998", "text", "en"), "#");
    assert.equal(getScrollIndexLabel("Álbum", "text", "en"), "A");
    assert.equal(getScrollIndexLabel("かな", "text", "ja"), "か");
    assert.equal(getScrollIndexLabel("漢字", "text", "ja"), "漢");
});

test("groups use the same text bucket policy as ungrouped lists", () => {
    const groups = createGroupScrollIndexEntries(
        [
            { key: "Rock", offset: 0 },
            { key: "Ambient", offset: 10 },
            { key: "Jazz", offset: 20 },
        ],
        "text",
        "en",
    );

    assert.deepEqual(
        groups.map((entry) => entry.label),
        ["R", "A", "J"],
    );
});

test("numeric non-year sorting falls into the other bucket", () => {
    const entries = createScrollIndexEntries(
        [{ value: 12 }, { value: 34 }],
        () => null,
        "en",
    );

    assert.deepEqual(
        entries.map((entry) => entry.label),
        ["#"],
    );
});
