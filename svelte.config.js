// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { readFileSync } from "node:fs";
import { publishStaticAssets } from "./scripts/lib/static-assets.mjs";

const { version } = JSON.parse(
    readFileSync(new URL("./package.json", import.meta.url), "utf8"),
);
const staged = ".tmp/frontend-dist";
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

/** @type {import('@sveltejs/kit').Config} */
const config = {
    preprocess: vitePreprocess(),
    kit: {
        adapter: desktopAdapter,
        // The frontend is embedded in this app version and updated by Velopack.
        // A build timestamp would change identical assets and force Rust to relink.
        version: { name: version },
    },
};

export default config;
