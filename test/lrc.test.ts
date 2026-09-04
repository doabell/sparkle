// @ts-nocheck
import { strict as assert } from "node:assert";
import { test } from "bun:test";
import {
    activeLineIndex,
    anticipatedLineIndex,
    type LrcLine,
} from "../src/lib/utils/lrc.ts";

const lines: LrcLine[] = [
    { timeMs: 8_000, text: "Previous" },
    { timeMs: 10_000, text: "Current" },
    { timeMs: 12_000, text: "Next" },
];

test("starts the next lyric transition 200ms before its timestamp", () => {
    assert.equal(anticipatedLineIndex(lines, 9_799), 0);
    assert.equal(anticipatedLineIndex(lines, 9_800), 1);
    assert.equal(anticipatedLineIndex(lines, 9_999), 1);
    assert.equal(activeLineIndex(lines, 9_999), 0);
    assert.equal(activeLineIndex(lines, 10_000), 1);
});

test("does not anticipate a lyric when the transition lead is disabled", () => {
    assert.equal(anticipatedLineIndex(lines, 9_999, 0), 0);
    assert.equal(activeLineIndex(lines, 9_999), 0);
});

test("uses the lyric state at the end of the transition window", () => {
    const closeLines: LrcLine[] = [
        { timeMs: 10_000, text: "First" },
        { timeMs: 10_100, text: "Second" },
    ];

    assert.equal(anticipatedLineIndex(closeLines, 9_899), 0);
    assert.equal(anticipatedLineIndex(closeLines, 9_900), 1);
});
