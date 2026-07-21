export const APP_TITLE = "Sparkle";

export interface WindowTitleTrack {
    title: string | null;
    artist_names?: string[];
}

const routeTitles: Array<[string, string]> = [
    ["/now-playing", "Now Playing"],
    ["/settings", "Settings"],
    ["/search", "Search"],
    ["/stats", "Listening"],
    ["/folders", "Folders"],
    ["/songs", "Songs"],
    ["/artists", "Artists"],
    ["/albums", "Albums"],
    ["/genres", "Genres"],
    ["/playlists", "Playlists"],
];

export function getRouteTitle(pathname: string): string {
    const path = pathname.replace(/\/$/, "") || "/";
    if (path === "/") return "Home";

    const exact = routeTitles.find(([route]) => route === path);
    if (exact) return exact[1];

    const detail = routeTitles.find(([route]) => path.startsWith(`${route}/`));
    if (detail) return detail[1].replace(/s$/, "");

    return "Sparkle";
}

export function getWindowTitle(
    pathname: string,
    track: WindowTitleTrack | null,
    isPlaying: boolean,
    pageName?: string | null,
): string {
    const page = pageName?.trim() || getRouteTitle(pathname);
    const trackTitle = track?.title?.trim() || "Unknown";
    const artist = track?.artist_names?.filter(Boolean).join("; ");
    if (track && isPlaying) {
        const nowPlaying = artist ? `${trackTitle} — ${artist}` : trackTitle;
        return `${nowPlaying} · ${APP_TITLE}`;
    }

    return `${page} · ${APP_TITLE}`;
}
