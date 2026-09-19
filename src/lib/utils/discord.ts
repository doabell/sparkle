import type { DiscordLayout } from "$lib/api";

export const discordTemplateFields = [
    { key: "title", label: "Title" },
    { key: "artist", label: "Artist" },
    { key: "album", label: "Album" },
    { key: "album_artist", label: "Album artist" },
    { key: "lyrics", label: "Lyrics" },
    { key: "year", label: "Year" },
    { key: "genre", label: "Genre" },
    { key: "track", label: "Track" },
    { key: "disc", label: "Disc" },
    { key: "duration", label: "Duration" },
    { key: "format", label: "Format" },
    { key: "bitrate", label: "Bitrate" },
    { key: "sample_rate", label: "Sample rate" },
    { key: "bit_depth", label: "Bit depth" },
    { key: "channels", label: "Channels" },
] as const;

export type DiscordTemplateValues = Partial<
    Record<(typeof discordTemplateFields)[number]["key"], string>
>;

export function defaultDiscordLayout(): DiscordLayout {
    return {
        name: "Sparkle",
        details: "{title}",
        state: "{artist}",
        image_text: "{album}",
        status_display: "state",
        show_artwork: true,
        show_progress: true,
    };
}

/** Match the backend's single-pass substitution and UTF-8 field limit. */
export function renderDiscordTemplate(
    template: string,
    values: DiscordTemplateValues,
): string {
    const rendered = template
        .replace(/\{[^}]*\}/g, (token) => {
            const field = discordTemplateFields.find(
                ({ key }) => token === `{${key}}`,
            );
            if (!field) return token;
            if (field.key === "lyrics") {
                return values.lyrics?.trim()
                    ? values.lyrics
                    : (values.album ?? "");
            }
            return values[field.key] ?? "";
        })
        .replace(/\s+/gu, " ")
        .trim();
    let result = "";
    let bytes = 0;
    const encoder = new TextEncoder();
    for (const character of rendered) {
        bytes += encoder.encode(character).length;
        if (bytes > 128) break;
        result += character;
    }
    return result;
}
