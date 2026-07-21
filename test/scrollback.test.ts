// @ts-nocheck
import { strict as assert } from "node:assert";
import { test } from "node:test";
import {
    createScrollbackRegistry,
    readScrollbackFromHistory,
    saveScrollbackToHistory,
} from "../src/lib/utils/scrollback.ts";

test("stores scrollback on the browser history entry", () => {
    const history = {
        state: { "sveltekit:history": 1 },
        replaceState(nextState: Record<string, unknown>) {
            this.state = nextState;
        },
    };
    const snapshot = {
        route: "/playlists",
        scroll: { top: 8_000, maxScrollTop: 10_000 },
        page: null,
    };

    saveScrollbackToHistory(history, snapshot, "/playlists");

    assert.equal(history.state["sveltekit:history"], 1);
    assert.deepEqual(
        readScrollbackFromHistory(history, "/playlists"),
        snapshot,
    );
    assert.equal(readScrollbackFromHistory(history, "/genres/rock"), null);
});

test("restores registered lazy-list state for the matching history route", () => {
    const registry = createScrollbackRegistry();
    let visibleCount = 150;

    registry.register({
        key: "songs",
        capture: () => visibleCount,
        restore: (value: number) => {
            visibleCount = value;
        },
    });

    visibleCount = 900;
    const snapshot = registry.capture("/songs", {
        top: 9_000,
        maxScrollTop: 10_000,
    });

    visibleCount = 150;
    const result = registry.restore("/songs", snapshot);

    assert.deepEqual(result.scroll, snapshot.scroll);
    assert.equal(result.pageRestored, true);
    assert.equal(visibleCount, 900);
});

test("does not apply page state to a different route", () => {
    const registry = createScrollbackRegistry();
    let visibleCount = 150;

    registry.register({
        key: "songs",
        capture: () => visibleCount,
        restore: (value: number) => {
            visibleCount = value;
        },
    });

    visibleCount = 900;
    const snapshot = registry.capture("/songs", {
        top: 9_000,
        maxScrollTop: 10_000,
    });

    visibleCount = 150;
    const result = registry.restore("/albums/42", snapshot);

    assert.equal(result.pageRestored, false);
    assert.equal(visibleCount, 150);
});

test("replays lazy-list state when the page registers after snapshot restore", () => {
    const registry = createScrollbackRegistry();
    let visibleCount = 150;
    const snapshot = {
        route: "/songs",
        scroll: { top: 9_000, maxScrollTop: 10_000 },
        page: { key: "songs", value: 900 },
    };

    const result = registry.restore("/songs", snapshot);
    assert.equal(result.pageRestored, false);

    registry.register({
        key: "songs",
        capture: () => visibleCount,
        restore: (value: number) => {
            visibleCount = value;
        },
    });

    assert.equal(visibleCount, 900);
});
