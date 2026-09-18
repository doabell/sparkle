// @ts-nocheck
import { expect, test } from "bun:test";
import { plugin } from "bun";
import { compile } from "svelte/compiler";
import { render } from "svelte/server";

plugin({
    name: "svelte-lyrics-render-test",
    setup(build) {
        build.onLoad({ filter: /\.svelte$/ }, async ({ path }) => ({
            contents: compile(await Bun.file(path).text(), {
                filename: path,
                generate: "server",
            }).js.code,
            loader: "js",
        }));
    },
});

const { default: SyncedLyrics } =
    await import("../src/lib/components/SyncedLyrics.svelte");

const textContent = (html: string) => html.replace(/<!--.*?-->|<[^>]*>/gs, "");

test("renders saved plain lyrics when native synced text is null", () => {
    const { body } = render(SyncedLyrics, {
        props: {
            syncedText: null,
            plainText: "Corrected first line\nCorrected second line",
            currentTimeMs: 15000,
        },
    });
    expect(body).toContain('class="lines plain ');
    expect(textContent(body)).toContain("Corrected first line");
    expect(textContent(body)).toContain("Corrected second line");
    expect(body).not.toContain("No lyrics found.");
    expect(body).not.toContain("<button");
});

test("renders untimed text from older synchronized payloads", () => {
    const { body } = render(SyncedLyrics, {
        props: {
            syncedText: "[ar:Artist]\nFirst line\nSecond line",
            plainText: null,
            currentTimeMs: 0,
        },
    });
    expect(body).toContain('class="lines plain ');
    expect(textContent(body)).toContain("First line");
    expect(textContent(body)).toContain("Second line");
    expect(body).not.toContain("[ar:Artist]");
});

test("renders empty and timed native lyric payloads", () => {
    const empty = render(SyncedLyrics, {
        props: { syncedText: null, plainText: null, currentTimeMs: 0 },
    });
    expect(empty.body).toContain("No lyrics found.");
    const timed = render(SyncedLyrics, {
        props: {
            syncedText: "[00:01]Timed line",
            plainText: null,
            currentTimeMs: 1000,
        },
    });
    expect(timed.body).toContain('class="lines synced ');
    expect(textContent(timed.body)).toContain("Timed line");
});
