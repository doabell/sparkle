// @ts-nocheck
import { strict as assert } from "node:assert";
import { test } from "bun:test";
import {
    activeLineIndex,
    anticipatedLineIndex,
    normalizeLyricSpacing,
    parseLrc,
    type LrcLine,
} from "../src/lib/utils/lrc.ts";

const lines: LrcLine[] = [
    { timeMs: 8_000, text: "Previous" },
    { timeMs: 10_000, text: "Current" },
    { timeMs: 12_000, text: "Next" },
];

test("parses repeated timestamps, fractions, CRLF and out-of-order lyrics", () => {
    assert.deepEqual(
        parseLrc(
            "[ar:Artist]\r\n\r\n[01:02.3456] Later \r\n[00:01][00:03.5] Chorus\n[00:02]\nplain text\n[bad]ignored",
        ),
        [
            { timeMs: 1000, text: "Chorus" },
            { timeMs: 3500, text: "Chorus" },
            { timeMs: 62346, text: "Later" },
        ],
    );
    assert.deepEqual(parseLrc(""), []);
    assert.equal(activeLineIndex([], 100), -1);
    assert.equal(activeLineIndex(lines, 0), -1);
    assert.equal(activeLineIndex(lines, 999999), 2);
    assert.equal(anticipatedLineIndex(lines, 9999, -100), 0);
});

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

test("normalizes lyric spaces for balanced wrapping", () => {
    assert.equal(
        normalizeLyricSpacing("  first　 second   third　"),
        "first second third",
    );
});
