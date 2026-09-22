// @ts-nocheck
import { expect, test } from "bun:test";
import {
    existsSync,
    mkdirSync,
    mkdtempSync,
    readFileSync,
    rmSync,
    statSync,
    unlinkSync,
    writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { buildFrontend } from "../scripts/lib/frontend-build-cache.mjs";

function fixture() {
    const root = mkdtempSync(join(tmpdir(), "sparkle-frontend-cache-"));
    const write = (path, bytes) => {
        mkdirSync(dirname(join(root, path)), { recursive: true });
        writeFileSync(join(root, path), bytes);
    };
    for (const path of [
        "src/App.svelte",
        "static/icon.svg",
        "scripts/helper.mjs",
        "licenses/dependencies.json",
        "LICENSE",
        "THIRD_PARTY_NOTICES.md",
        "package.json",
        "bun.lock",
        "vite.config.js",
        "svelte.config.js",
        "tsconfig.json",
        ".env.production",
        "node_modules/vite/package.json",
        "node_modules/vite/dist/build.js",
    ])
        write(path, "initial");
    let builds = 0;
    const options = {
        root,
        environment: {
            PUBLIC_API_URL: "https://example.test",
            SECRET: "private-value",
        },
        runtime: "fixture-runtime",
        build() {
            builds++;
            write(
                "build/index.html",
                `<script src="app.js"></script>${builds}`,
            );
            write("build/app.js", `console.log(${builds})`);
            write(".tmp/frontend-dependencies.json", "[]");
        },
    };
    return {
        root,
        write,
        options,
        builds: () => builds,
        cache: join(root, ".tmp/frontend-build-cache.json"),
        cleanup: () => rmSync(root, { recursive: true, force: true }),
    };
}

test("unchanged frontend reuses verified assets and preserves timestamps", () => {
    const f = fixture();
    try {
        expect(buildFrontend(f.options)).toEqual({ reused: false });
        const before = statSync(join(f.root, "build/index.html")).mtimeMs;
        expect(buildFrontend(f.options)).toEqual({ reused: true });
        expect(f.builds()).toBe(1);
        expect(statSync(join(f.root, "build/index.html")).mtimeMs).toBe(before);
        expect(readFileSync(f.cache, "utf8")).not.toContain("private-value");
        // Rust-only edits and generated tool caches do not rebuild the frontend.
        f.write("src-tauri/src/lib.rs", "changed native source");
        f.write("node_modules/.vite/deps/cache.js", "new tool cache");
        f.write("node_modules/.old-1234/package.json", "old install");
        expect(buildFrontend(f.options)).toEqual({ reused: true });
    } finally {
        f.cleanup();
    }
});

test("source, asset, config, license, lockfile and installed-package edits rebuild", () => {
    const f = fixture();
    try {
        buildFrontend(f.options);
        for (const path of [
            "src/App.svelte",
            "static/icon.svg",
            "scripts/helper.mjs",
            "licenses/dependencies.json",
            "LICENSE",
            "THIRD_PARTY_NOTICES.md",
            "package.json",
            "bun.lock",
            "vite.config.js",
            "svelte.config.js",
            "tsconfig.json",
            ".env.production",
            "node_modules/vite/dist/build.js",
        ]) {
            f.write(path, "changed");
            expect(buildFrontend(f.options)).toEqual({ reused: false });
            expect(buildFrontend(f.options)).toEqual({ reused: true });
        }
        f.write("static/new.svg", "new asset");
        expect(buildFrontend(f.options)).toEqual({ reused: false });
        unlinkSync(join(f.root, "static/new.svg"));
        expect(buildFrontend(f.options)).toEqual({ reused: false });
    } finally {
        f.cleanup();
    }
});

test("environment, runtime and explicit force invalidate reuse", () => {
    const f = fixture();
    try {
        buildFrontend(f.options);
        f.options.environment.PUBLIC_API_URL = "https://changed.test";
        expect(buildFrontend(f.options)).toEqual({ reused: false });
        f.options.runtime = "updated-runtime";
        expect(buildFrontend(f.options)).toEqual({ reused: false });
        expect(buildFrontend({ ...f.options, force: true })).toEqual({
            reused: false,
        });
        expect(buildFrontend(f.options)).toEqual({ reused: true });
    } finally {
        f.cleanup();
    }
});

test("missing, changed and extra output files or license reports cannot be reused", () => {
    const f = fixture();
    try {
        buildFrontend(f.options);
        for (const path of [
            "build/index.html",
            "build/app.js",
            ".tmp/frontend-dependencies.json",
        ]) {
            f.write(path, "damaged");
            expect(buildFrontend(f.options)).toEqual({ reused: false });
            unlinkSync(join(f.root, path));
            expect(buildFrontend(f.options)).toEqual({ reused: false });
        }
        f.write("build/stale.js", "stale output");
        expect(buildFrontend(f.options)).toEqual({ reused: false });
        f.write(".tmp/frontend-build-cache.json", "malformed cache");
        expect(buildFrontend(f.options)).toEqual({ reused: false });
    } finally {
        f.cleanup();
    }
});

test("failed builds and inputs changed during a build never receive a cache record", () => {
    const f = fixture();
    try {
        buildFrontend(f.options);
        expect(() =>
            buildFrontend({
                ...f.options,
                force: true,
                build() {
                    throw new Error("Vite failed");
                },
            }),
        ).toThrow("Vite failed");
        expect(existsSync(f.cache)).toBe(false);
        expect(buildFrontend(f.options)).toEqual({ reused: false });
        buildFrontend({
            ...f.options,
            force: true,
            build() {
                f.options.build();
                f.write("src/App.svelte", "changed during build");
            },
        });
        expect(existsSync(f.cache)).toBe(false);
        expect(buildFrontend(f.options)).toEqual({ reused: false });
    } finally {
        f.cleanup();
    }
});

test("a build reporting success without required outputs fails without caching", () => {
    const f = fixture();
    try {
        expect(() => buildFrontend({ ...f.options, build() {} })).toThrow(
            "missing",
        );
        expect(existsSync(f.cache)).toBe(false);
    } finally {
        f.cleanup();
    }
});
