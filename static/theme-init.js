// Apply only the cached preference before styles paint. CSS handles System.
// Keep the cache key and validation in sync with lib/utils/themeMode.ts.
(() => {
    let mode;
    try {
        mode = localStorage.getItem("sparkle.themeMode.v1");
    } catch {
        // Storage may be unavailable; the system theme still works.
    }
    document.documentElement.dataset.theme =
        mode === "light" || mode === "dark" ? mode : "system";
})();
