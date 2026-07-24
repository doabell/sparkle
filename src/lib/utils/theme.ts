import {
    createAccentTheme,
    normalizeAccentForegroundPreference,
    normalizeHex,
    type AccentForegroundPreference,
    type AccentPalette,
    type AccentTheme,
} from "./accentPalette.ts";

export * from "./accentPalette.ts";

const PALETTE_CACHE_KEY = "sparkle.accent.v1";
const CSS_PALETTE_PROPERTIES: [keyof AccentPalette, string][] = [
    ["content", "content"],
    ["graphic", "graphic"],
    ["fill", "fill"],
    ["fillHover", "fill-hover"],
    ["fillActive", "fill-active"],
    ["fillDisabled", "fill-disabled"],
    ["onFill", "on-fill"],
    ["onFillDisabled", "on-fill-disabled"],
    ["subtle", "subtle"],
    ["onSubtle", "on-subtle"],
    ["selection", "selection"],
    ["onSelection", "on-selection"],
    ["focus", "focus"],
    ["native", "native"],
];

export function applyAccent(
    value: string,
    preference: AccentForegroundPreference = "auto",
): AccentTheme {
    const theme = createAccentTheme(value, preference);
    const root = document.documentElement;
    root.style.setProperty("--color-accent-seed", theme.seed);
    for (const mode of ["dark", "light"] as const) {
        for (const [property, cssName] of CSS_PALETTE_PROPERTIES) {
            root.style.setProperty(
                `--accent-${mode}-${cssName}`,
                theme[mode][property],
            );
        }
    }
    return theme;
}

export function cacheAccent(
    value: string,
    preference: AccentForegroundPreference = "auto",
): void {
    const seed = normalizeHex(value);
    if (!seed || typeof localStorage === "undefined") return;
    try {
        localStorage.setItem(
            PALETTE_CACHE_KEY,
            JSON.stringify({
                seed,
                preference: normalizeAccentForegroundPreference(preference),
            }),
        );
    } catch {
        // The database remains authoritative when storage is unavailable,
        // blocked, or full.
    }
}

export function applyCachedAccent(): boolean {
    if (typeof localStorage === "undefined") return false;
    try {
        const cached = JSON.parse(
            localStorage.getItem(PALETTE_CACHE_KEY) ?? "",
        );
        const seed =
            typeof cached?.seed === "string" ? normalizeHex(cached.seed) : null;
        if (!seed) return false;
        applyAccent(
            seed,
            normalizeAccentForegroundPreference(cached.preference),
        );
        return true;
    } catch {
        return false;
    }
}
