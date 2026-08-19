// @ts-nocheck
import assert from "node:assert/strict";
import { test } from "bun:test";
import { moveCommandSelection } from "../src/lib/utils/commandPalette.ts";

test("keeps command selection inside the available results", () => {
    assert.equal(moveCommandSelection(9, 1, 10), 9);
    assert.equal(moveCommandSelection(0, -1, 10), 0);
});

test("includes the search action only when it is in the item count", () => {
    assert.equal(moveCommandSelection(9, 1, 11), 10);
    assert.equal(moveCommandSelection(10, 1, 11), 10);
});

test("keeps an empty command list at the neutral index", () => {
    assert.equal(moveCommandSelection(0, 1, 0), 0);
});
