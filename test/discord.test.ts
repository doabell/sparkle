// @ts-nocheck
import { describe, expect, test } from "bun:test";
import {
    defaultDiscordLayout,
    renderDiscordTemplate,
} from "../src/lib/utils/discord";

const track = {
    title: "A {lyrics} title",
    artist: "Artist",
    album: "Album",
    lyrics: "Current line",
};

describe("Discord layout preview", () => {
    test("substitutes once and preserves unknown tokens", () => {
        expect(
            renderDiscordTemplate(
                "{title} / {artist} / {album} / {lyrics} / {unknown}",
                track,
            ),
        ).toBe("A {lyrics} title / Artist / Album / Current line / {unknown}");
        expect(renderDiscordTemplate("unfinished {", track)).toBe(
            "unfinished {",
        );
    });
    test("uses album text when synced lyrics are unavailable", () => {
        expect(
            renderDiscordTemplate("{lyrics}", { ...track, lyrics: undefined }),
        ).toBe("Album");
        expect(
            renderDiscordTemplate("{lyrics}", { ...track, lyrics: "  " }),
        ).toBe("Album");
        expect(renderDiscordTemplate(" \n ", track)).toBe("");
    });
    test("limits text by UTF-8 bytes without breaking Unicode", () => {
        const result = renderDiscordTemplate("{lyrics}", {
            ...track,
            lyrics: "🎵".repeat(40),
        });
        expect(result).toBe("🎵".repeat(32));
        expect(new TextEncoder().encode(result).length).toBe(128);
    });
    test("layout reset does not reuse a previously edited object", () => {
        const edited = defaultDiscordLayout();
        edited.name = "Other";
        expect(defaultDiscordLayout().name).toBe("Sparkle");
        expect(defaultDiscordLayout().status_display).toBe("name");
    });
});
