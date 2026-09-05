// @ts-nocheck
import { strict as assert } from "node:assert";
import { test } from "bun:test";
import {
    createGroupScrollIndexEntries,
    createScrollIndexEntries,
    getScrollIndexLabel,
    resolveSongIndexLanguage,
    getSongCollator,
    resolveScrollIndexMode,
} from "../src/lib/utils/songIndex.ts";

test("automatic indexing follows locale and sorts track numbers naturally", () => {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, "navigator");
    try {
        delete globalThis.navigator;
        assert.equal(resolveSongIndexLanguage("auto"), "en");
        Object.defineProperty(globalThis, "navigator", {
            configurable: true,
            value: { language: "ja-JP" },
        });
        assert.equal(resolveSongIndexLanguage("auto"), "ja");
        assert.equal(resolveSongIndexLanguage("en"), "en");
        assert.ok(getSongCollator("en").compare("Track 2", "Track 10") < 0);
        assert.equal(getScrollIndexLabel("カタカナ", "text", "ja"), "か");
        assert.equal(getScrollIndexLabel("ｶﾀｶﾅ", "text", "ja"), "か");
        assert.equal(getScrollIndexLabel("😀", "text", "ja"), "#");
        for (const [group, sort, expected] of [
            ["year", "title", "year"],
            ["none", "year", "year"],
            ["artist", "year", "text"],
            ["none", "title", "text"],
        ])
            assert.equal(resolveScrollIndexMode(group, sort), expected);
    } finally {
        if (descriptor)
            Object.defineProperty(globalThis, "navigator", descriptor);
        else delete globalThis.navigator;
    }
});

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
