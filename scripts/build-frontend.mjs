import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { buildFrontend } from "./lib/frontend-build-cache.mjs";

const args = process.argv.slice(2);
if (args.some((arg) => arg !== "--force"))
    throw new Error("Usage: bun run build [--force]");
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const { reused } = buildFrontend({
    root,
    force: args.includes("--force"),
    build() {
        const result = spawnSync("bunx", ["--no-install", "vite", "build"], {
            cwd: root,
            env: process.env,
            stdio: "inherit",
            windowsHide: true,
        });
        if (result.error || result.status !== 0)
            throw new Error(
                result.error?.message ||
                    `Frontend build failed (${result.status}).`,
            );
    },
});
if (reused)
    console.log("Frontend inputs and outputs unchanged; reusing build.");
