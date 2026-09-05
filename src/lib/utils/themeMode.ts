export type ThemeMode = "system" | "light" | "dark";

export const THEME_MODE_CACHE_KEY = "sparkle.themeMode.v1";

export function normalizeThemeMode(value: unknown): ThemeMode {
    return value === "light" || value === "dark" ? value : "system";
}

export function applyThemeMode(value: unknown): void {
    // CSS owns system detection, so OS changes remain live without a listener.
    document.documentElement.dataset.theme = normalizeThemeMode(value);
}

export function cacheThemeMode(value: unknown): void {
    try {
        localStorage.setItem(THEME_MODE_CACHE_KEY, normalizeThemeMode(value));
    } catch {
        // Settings in the database remain authoritative if storage is blocked.
    }
}

export function applyCachedThemeMode(): void {
    let mode: unknown;
    try {
        mode = localStorage.getItem(THEME_MODE_CACHE_KEY);
    } catch {
        // A new installation or unavailable cache follows the OS.
    }
    applyThemeMode(mode);
}
