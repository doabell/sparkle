import type { DiscordLayout } from "$lib/api";

export function defaultDiscordLayout(): DiscordLayout {
    return {
        name: "Sparkle",
        details: "{title}",
        state: "{artist}",
        image_text: "{album}",
        status_display: "name",
        show_artwork: true,
        show_progress: true,
    };
}

/** Match the backend's single-pass substitution and UTF-8 field limit. */
export function renderDiscordTemplate(
    template: string,
    values: { title: string; artist: string; album: string; lyrics?: string },
): string {
    const rendered = template
        .replace(/\{[^}]*\}/g, (token) => {
            switch (token) {
                case "{title}":
                    return values.title;
                case "{artist}":
                    return values.artist;
                case "{album}":
                    return values.album;
                case "{lyrics}":
                    return values.lyrics?.trim() ? values.lyrics : values.album;
                default:
                    return token;
            }
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
