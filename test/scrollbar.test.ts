// @ts-nocheck
import assert from "node:assert/strict";
import { test } from "bun:test";
import {
    scrollbarGeometry,
    scrollbarKeyTarget,
    scrollTopFromThumb,
} from "../src/lib/utils/scrollbar.ts";

test("overlay thumb reflects the native scroll position and visible fraction", () => {
    assert.deepEqual(scrollbarGeometry(600, 2400, 900, 500), {
        maxScroll: 1800,
        position: 900,
        thumbHeight: 125,
        travel: 375,
        thumbTop: 187.5,
    });
    assert.equal(scrollbarGeometry(600, 2400, -50, 500).thumbTop, 0);
    assert.equal(scrollbarGeometry(600, 2400, 9000, 500).thumbTop, 375);
});

test("no overflow or no track produces safe non-draggable geometry", () => {
    const fitting = scrollbarGeometry(600, 500, 10, 500);
    assert.equal(fitting.maxScroll, 0);
    assert.equal(fitting.position, 0);
    assert.equal(fitting.thumbHeight, 500);
    assert.equal(fitting.travel, 0);
    assert.equal(fitting.thumbTop, 0);

    const empty = scrollbarGeometry(0, 0, 0, 0);
    assert.equal(empty.thumbHeight, 0);
    assert.equal(empty.thumbTop, 0);
    assert.equal(scrollTopFromThumb(200, 0, 0), 0);
});

test("long libraries keep a usable thumb without exceeding short tracks", () => {
    assert.equal(scrollbarGeometry(600, 100000, 0, 500).thumbHeight, 28);
    const short = scrollbarGeometry(600, 100000, 0, 20);
    assert.equal(short.thumbHeight, 20);
    assert.equal(short.travel, 0);
});

test("dragging reaches both ends and preserves the grabbed scroll position", () => {
    for (const position of [0, 1, 300, 900, 1799, 1800]) {
        const geometry = scrollbarGeometry(600, 2400, position, 500);
        const result = scrollTopFromThumb(
            geometry.thumbTop,
            geometry.travel,
            geometry.maxScroll,
        );
        assert.ok(Math.abs(position - result) < 0.00001);
    }
    assert.equal(scrollTopFromThumb(-100, 375, 1800), 0);
    assert.equal(scrollTopFromThumb(999, 375, 1800), 1800);
});

test("keyboard scrolling supports arrows, pages, space, and endpoints", () => {
    const target = (key: string, shift = false) =>
        scrollbarKeyTarget(key, 700, 600, 1800, shift);
    assert.equal(target("ArrowUp"), 660);
    assert.equal(target("ArrowDown"), 740);
    assert.equal(target("PageUp"), 160);
    assert.equal(target("PageDown"), 1240);
    assert.equal(target(" "), 1240);
    assert.equal(target(" ", true), 160);
    assert.equal(target("Home"), 0);
    assert.equal(target("End"), 1800);
    assert.equal(target("Tab"), null);
    assert.equal(scrollbarKeyTarget("ArrowUp", 0, 600, 1800), 0);
    assert.equal(scrollbarKeyTarget("PageDown", 1700, 600, 1800), 1800);
});

test("thumb geometry follows growing content and resized viewports", () => {
    const before = scrollbarGeometry(600, 1200, 300, 500);
    const grown = scrollbarGeometry(600, 2400, 300, 500);
    assert.equal(before.maxScroll, 600);
    assert.equal(grown.maxScroll, 1800);
    assert.ok(grown.thumbHeight < before.thumbHeight);
    assert.ok(grown.thumbTop < before.thumbTop);
    const resized = scrollbarGeometry(900, 2400, 300, 800);
    assert.equal(resized.maxScroll, 1500);
    assert.equal(resized.thumbHeight, 300);
});
