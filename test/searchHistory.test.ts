// @ts-nocheck
import { strict as assert } from "node:assert";
import { test } from "node:test";
import { addRecentSearch } from "../src/lib/utils/searchHistory.ts";

test("records a deliberate search at the front and deduplicates it", () => {
    assert.deepEqual(addRecentSearch(["older", "Library"], "  library "), [
        "library",
        "older",
    ]);
});

test("does not add blank input and respects the history limit", () => {
    assert.deepEqual(addRecentSearch(["one"], "   "), ["one"]);
    assert.deepEqual(addRecentSearch(["a", "b"], "c", 2), ["c", "a"]);
});
