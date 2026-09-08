// @ts-nocheck
import { strict as assert } from "node:assert";
import { test } from "bun:test";
import {
    activeLineIndex,
    anticipatedLineIndex,
    normalizeLyricSpacing,
    lyricWrapSegments,
    parseLrc,
    lrcPlainText,
    applyLrcOffset,
    type LrcLine,
} from "../src/lib/utils/lrc.ts";
import lrcContract from "./fixtures/lrc.json";

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
    assert.equal(activeLineIndex(lines, 0), 0);
    assert.equal(activeLineIndex(lines, 999999), 2);
    assert.equal(anticipatedLineIndex(lines, 9999, -100), 0);
});

test("shared LRC contract matches native parsing, timing edits and export", () => {
    for (const entry of lrcContract) {
        assert.deepEqual(
            parseLrc(entry.text),
            entry.lines,
            `${entry.name}: parse`,
        );
        assert.equal(
            lrcPlainText(entry.text),
            entry.plain,
            `${entry.name}: plain`,
        );
        assert.equal(
            applyLrcOffset(entry.text, entry.delayMs),
            entry.shifted,
            `${entry.name}: bake`,
        );
        assert.equal(
            applyLrcOffset(entry.shifted, 0),
            entry.shifted,
            `${entry.name}: no double shift`,
        );
    }
});

test("first sentence is visible throughout the intro and blank cues do not clear it", () => {
    const intro = parseLrc(
        "[00:15]First sentence\n[00:20]\n[00:30]Next sentence",
    );
    assert.equal(activeLineIndex(intro, 0), 0);
    assert.equal(anticipatedLineIndex(intro, 0), 0);
    assert.equal(activeLineIndex(intro, 25000), 0);
    assert.equal(activeLineIndex(intro, 30000), 1);
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

test("duplicate timestamps preserve every line and deterministic active timing", () => {
    const duplicate = parseLrc(
        "[00:00.00]Title\n[00:00.000]Artist\n[00:00]Opening\n[00:01]Next",
    );
    assert.deepEqual(
        duplicate.map((line) => line.text),
        ["Title", "Artist", "Opening", "Next"],
    );
    assert.equal(activeLineIndex(duplicate, 0), 2);
    assert.equal(activeLineIndex(duplicate, 1000), 3);
});

test("wrap points precede spaces and opening punctuation and follow sentence endings", () => {
    for (const separator of [
        " ",
        "　",
        '"',
        "“",
        "‘",
        "＂",
        "(",
        "（",
        "[",
        "［",
        "{",
        "｛",
        "「",
        "｢",
    ]) {
        assert.deepEqual(lyricWrapSegments(`One${separator}Two`), [
            "One",
            `${separator === "　" ? " " : separator}Two`,
        ]);
    }
    for (const separator of [
        ".",
        "．",
        "。",
        "｡",
        "?",
        "？",
        "!",
        "！",
        "/",
        "／",
    ]) {
        assert.deepEqual(lyricWrapSegments(`One${separator}Two`), [
            `One${separator}`,
            "Two",
        ]);
    }
    assert.deepEqual(lyricWrapSegments("UnbrokenEnglishWord"), [
        "UnbrokenEnglishWord",
    ]);
    assert.deepEqual(lyricWrapSegments(""), []);
});

test("CJK wrap points use word boundaries without dropping text or stranding closing punctuation", () => {
    for (const text of [
        "私は音楽が好きです。",
        "我喜欢听音乐。",
        "음악을 좋아합니다！",
        "「今日は音楽を聴く」",
    ]) {
        const segments = lyricWrapSegments(text);
        assert.equal(segments.join(""), text);
        assert.ok(segments.length > 1);
        assert.ok(segments.every((part) => !/^[」』】。、。！？]/u.test(part)));
    }
});
