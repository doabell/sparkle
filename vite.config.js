import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { readFileSync } from "node:fs";
import { frontendLicenses } from "./scripts/lib/frontend-licenses.mjs";
import { publishStaticAssets } from "./scripts/lib/static-assets.mjs";

const host = process.env.TAURI_DEV_HOST;
const { version } = JSON.parse(
    readFileSync(new URL("./package.json", import.meta.url), "utf8"),
);
const staged = ".tmp/frontend-dist";
// Tauri embeds a static SPA, with index.html handling client-side routes.
const staticAdapter = adapter({
    pages: staged,
    assets: staged,
    fallback: "index.html",
});

/** @type {import('@sveltejs/kit').Adapter} */
const desktopAdapter = {
    ...staticAdapter,
    async adapt(builder) {
        await staticAdapter.adapt(builder);
        const { written, removed, reused } = publishStaticAssets(
            staged,
            "build",
        );
        builder.log(
            `Frontend assets: ${written} written, ${removed} removed, ${reused} unchanged.`,
        );
    },
};

// https://vite.dev/config/
export default defineConfig(async () => ({
    plugins: [
        sveltekit({
            preprocess: vitePreprocess(),
            adapter: desktopAdapter,
            // Velopack updates the embedded frontend with the app. Preserve
            // reproducible assets and disable deployment-version polling.
            version: { name: version, pollInterval: 0 },
        }),
        frontendLicenses(),
    ],

    // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
    //
    // 1. prevent Vite from obscuring rust errors
    clearScreen: false,
    // 2. tauri expects a fixed port, fail if that port is not available
    server: {
        port: 1420,
        strictPort: true,
        host: host || false,
        hmr: host
            ? {
                  protocol: "ws",
                  host,
                  port: 1421,
              }
            : undefined,
        watch: {
            // 3. tell Vite to ignore watching `src-tauri`
            ignored: ["**/src-tauri/**"],
        },
    },
}));
