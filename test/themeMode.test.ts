// @ts-nocheck
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import { test } from "bun:test";
import {
    THEME_MODE_CACHE_KEY,
    normalizeThemeMode,
    applyThemeMode,
    cacheThemeMode,
    applyCachedThemeMode,
} from "../src/lib/utils/themeMode.ts";

test("appearance defaults to System unless an explicit mode is valid", () => {
    for (const value of [undefined, null, "", "auto", "unknown", "LIGHT", 0]) {
        assert.equal(normalizeThemeMode(value), "system");
    }
    for (const value of ["system", "light", "dark"]) {
        assert.equal(normalizeThemeMode(value), value);
    }
});

test("appearance preferences persist and System clears an explicit override", () => {
    const storage = new Map();
    globalThis.document = { documentElement: { dataset: {} } };
    globalThis.localStorage = {
        getItem: (key) => storage.get(key) ?? null,
        setItem: (key, value) => storage.set(key, value),
    };
    try {
        for (const mode of ["light", "dark", "system"]) {
            applyThemeMode(mode);
            assert.equal(document.documentElement.dataset.theme, mode);
            cacheThemeMode(mode);
            assert.equal(storage.get(THEME_MODE_CACHE_KEY), mode);
            delete document.documentElement.dataset.theme;
            applyCachedThemeMode();
            assert.equal(document.documentElement.dataset.theme, mode);
        }
        storage.set(THEME_MODE_CACHE_KEY, "invalid");
        applyCachedThemeMode();
        assert.equal(document.documentElement.dataset.theme, "system");
    } finally {
        delete globalThis.document;
        delete globalThis.localStorage;
    }
});

test("unavailable appearance storage falls back to System without blocking UI", () => {
    globalThis.document = { documentElement: { dataset: {} } };
    globalThis.localStorage = {
        getItem() {
            throw new Error("blocked");
        },
        setItem() {
            throw new Error("blocked");
        },
    };
    try {
        assert.doesNotThrow(() => cacheThemeMode("dark"));
        assert.doesNotThrow(() => applyCachedThemeMode());
        assert.equal(document.documentElement.dataset.theme, "system");
    } finally {
        delete globalThis.document;
        delete globalThis.localStorage;
    }
});

test("pre-paint bootstrap uses the same validation and cache as Settings", () => {
    const script = fs.readFileSync("static/theme-init.js", "utf8");
    for (const mode of [null, "system", "light", "dark", "invalid"]) {
        const document = { documentElement: { dataset: {} } };
        vm.runInNewContext(script, {
            document,
            localStorage: {
                getItem(key) {
                    assert.equal(key, THEME_MODE_CACHE_KEY);
                    return mode;
                },
            },
        });
        assert.equal(
            document.documentElement.dataset.theme,
            normalizeThemeMode(mode),
        );
    }
    const document = { documentElement: { dataset: {} } };
    vm.runInNewContext(script, { document });
    assert.equal(document.documentElement.dataset.theme, "system");
    const html = fs.readFileSync("src/app.html", "utf8");
    assert.ok(html.indexOf("theme-init.js") < html.indexOf("%sveltekit.head%"));
});

test("system-light and explicit-light palettes stay identical and respect Dark", () => {
    const css = fs.readFileSync("src/app.css", "utf8");
    const systemRule = css.match(
        /@media \(prefers-color-scheme: light\)\s*\{\s*:root:not\(\[data-theme="dark"\]\)\s*\{([^}]+)\}/,
    )?.[1];
    const explicitRule = css.match(
        /:root\[data-theme="light"\]\s*\{([^}]+)\}/,
    )?.[1];
    assert.ok(systemRule);
    assert.ok(explicitRule);
    const declarations = (text) =>
        Object.fromEntries(
            [...text.matchAll(/([\w-]+):\s*([^;]+);/g)].map((match) => [
                match[1],
                match[2].trim(),
            ]),
        );
    assert.deepEqual(declarations(systemRule), declarations(explicitRule));
    assert.equal(declarations(explicitRule)["color-scheme"], "light");
    assert.match(css, /^:root\s*\{\s*color-scheme:\s*dark;/);
});
