// @ts-nocheck
import assert from "node:assert/strict";
import { test } from "bun:test";
import { APP_TITLE, getWindowTitle } from "../src/lib/utils/windowTitle.ts";

test("uses the navigated page when nothing is playing", () => {
    assert.equal(
        getWindowTitle("/albums/42", null, false),
        `Album · ${APP_TITLE}`,
    );
});

test("uses the loaded entity name for detail pages", () => {
    assert.equal(
        getWindowTitle("/albums/42", null, false, "Kind of Blue"),
        `Kind of Blue · ${APP_TITLE}`,
    );
});

test("uses the playing song as the concise title", () => {
    assert.equal(
        getWindowTitle(
            "/now-playing",
            { title: "My Song", artist_names: ["The Band"] },
            true,
        ),
        `My Song — The Band · ${APP_TITLE}`,
    );
});

test("keeps every playing artist in the title", () => {
    assert.equal(
        getWindowTitle(
            "/now-playing",
            {
                title: "My Song",
                artist_names: ["A", "B", "C", "D", "E", "F", "G", "H"],
            },
            true,
        ),
        `My Song — A; B; C; D; E; F; G; H · ${APP_TITLE}`,
    );
});

test("falls back to the page after pausing", () => {
    assert.equal(
        getWindowTitle(
            "/songs",
            { title: "My Song", artist_names: ["The Band"] },
            false,
        ),
        `Songs · ${APP_TITLE}`,
    );
});

test("handles missing track metadata", () => {
    assert.equal(
        getWindowTitle("/artists", { title: null, artist_names: [] }, true),
        `Unknown · ${APP_TITLE}`,
    );
});
