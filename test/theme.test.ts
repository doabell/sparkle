// @ts-nocheck
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import {
    DEFAULT_ACCENT_COLOR,
    applyAccent,
    applyCachedAccent,
    cacheAccent,
    contrastRatio,
    createAccentTheme,
    hexToRgb,
    normalizeAccentForegroundPreference,
    normalizeHex,
} from "../src/lib/utils/theme.ts";

const DARK_SURFACES = ["#0b0b0d", "#171719", "#242426", "#303034"];
const LIGHT_SURFACES = ["#f5f5f7", "#ffffff", "#e9e9ed", "#d8d8de"];
const PRESETS = [
    "#fa243c",
    "#fa5a24",
    "#fa24a8",
    "#a855f7",
    "#3b82f6",
    "#14b8a6",
    "#22c55e",
    "#eab308",
];

function rgb(hex: string) {
    const value = hexToRgb(hex);
    assert.ok(value, `Expected a valid color, received ${hex}`);
    return value;
}

function assertContrast(
    foreground: string,
    background: string,
    minimum: number,
    label: string,
) {
    const ratio = contrastRatio(rgb(foreground), rgb(background));
    assert.ok(
        ratio >= minimum,
        `${label}: ${foreground} on ${background} was ${ratio.toFixed(3)}:1`,
    );
}

function mix(first: string, second: string, amount: number) {
    const a = rgb(first);
    const b = rgb(second);
    return a.map((channel, index) =>
        Math.round(channel + (b[index] - channel) * amount),
    );
}

function assertPalette(seed: string) {
    for (const preference of ["auto", "light", "dark"]) {
        const theme = createAccentTheme(seed, preference);
        assert.equal(theme.seed, normalizeHex(seed));

        for (const [mode, surfaces] of [
            ["dark", DARK_SURFACES],
            ["light", LIGHT_SURFACES],
        ]) {
            const palette = theme[mode];
            for (const surface of surfaces) {
                assertContrast(
                    palette.content,
                    surface,
                    4.5,
                    `${seed} ${mode} content`,
                );
                assertContrast(
                    palette.graphic,
                    surface,
                    3,
                    `${seed} ${mode} graphic`,
                );
                assertContrast(
                    palette.focus,
                    surface,
                    3,
                    `${seed} ${mode} focus`,
                );
            }

            for (const state of [
                palette.fill,
                palette.fillHover,
                palette.fillActive,
            ]) {
                assertContrast(
                    palette.onFill,
                    state,
                    4.5,
                    `${seed} ${mode} filled control`,
                );
            }
            assertContrast(
                palette.onFillDisabled,
                palette.fillDisabled,
                4.5,
                `${seed} ${mode} disabled filled control`,
            );
            assertContrast(
                palette.onSubtle,
                palette.subtle,
                4.5,
                `${seed} ${mode} subtle pair`,
            );
            assert.ok(
                palette.onSubtle === "#000000" ||
                    palette.onSubtle === "#ffffff",
                `${seed} ${mode} subtle text must stay neutral`,
            );
            assertContrast(
                palette.content,
                palette.subtle,
                4.5,
                `${seed} ${mode} content on subtle`,
            );
            for (const surface of surfaces) {
                assertContrast(
                    palette.onSubtle,
                    surface,
                    4.5,
                    `${seed} ${mode} subtle foreground over gradient`,
                );
            }
            assertContrast(
                palette.onSelection,
                palette.selection,
                4.5,
                `${seed} ${mode} selection pair`,
            );

            assert.notEqual(
                palette.fillHover,
                palette.fill,
                `${seed} ${mode} hover must remain visible`,
            );
            assert.notEqual(
                palette.fillActive,
                palette.fillHover,
                `${seed} ${mode} active must remain distinct`,
            );
            for (const endpoint of [palette.fillHover, palette.fillActive]) {
                for (let step = 0; step <= 10; step += 1) {
                    const frame = mix(palette.fill, endpoint, step / 10);
                    assert.ok(
                        contrastRatio(rgb(palette.onFill), frame) >= 4.5,
                        `${seed} ${mode} transition frame lost contrast`,
                    );
                }
            }

            for (const value of Object.values(palette)) {
                assert.match(value, /^#[0-9a-f]{6}$/);
            }
        }
    }
}

test("normalizes only opaque six-digit sRGB colors", () => {
    assert.equal(normalizeHex(" FA243C "), DEFAULT_ACCENT_COLOR);
    assert.equal(normalizeHex("fa243c"), DEFAULT_ACCENT_COLOR);
    assert.equal(normalizeHex("#abc"), null);
    assert.equal(normalizeHex("#fa243cff"), null);
    assert.equal(normalizeHex("red"), null);
    assert.equal(normalizeHex("#fa24;drop"), null);
});

test("uses measured WCAG sRGB contrast without threshold rounding", () => {
    assert.equal(contrastRatio(rgb("#000000"), rgb("#ffffff")), 21);
    assert.ok(contrastRatio(rgb("#777777"), rgb("#ffffff")) < 4.5);
    assert.ok(contrastRatio(rgb("#767676"), rgb("#ffffff")) >= 4.5);
});

test("default Apple Music red keeps white text on an adjusted red fill", () => {
    const theme = createAccentTheme(DEFAULT_ACCENT_COLOR, "auto");
    assert.equal(theme.seed, "#fa243c");
    assert.equal(theme.dark.onFill, "#ffffff");
    assert.equal(theme.light.onFill, "#ffffff");
    assert.notEqual(theme.dark.fill, theme.seed);
    assertContrast(theme.dark.onFill, theme.dark.fill, 4.5, "default fill");
});

test("bright yellow remains close to the seed with dark text", () => {
    const theme = createAccentTheme("#eab308", "auto");
    assert.equal(theme.dark.fill, "#eab308");
    assert.equal(theme.dark.onFill, "#000000");
});

test("foreground preferences stay stable and accessible across states", () => {
    const light = createAccentTheme("#ffffff", "light");
    const dark = createAccentTheme("#000000", "dark");
    assert.equal(light.dark.onFill, "#ffffff");
    assert.equal(light.light.onFill, "#ffffff");
    assert.equal(dark.dark.onFill, "#000000");
    assert.equal(dark.light.onFill, "#000000");
    assertPalette("#ffffff");
    assertPalette("#000000");
});

test("all presets and edge colors satisfy every semantic role", () => {
    for (const seed of [
        ...PRESETS,
        "#000000",
        "#ffffff",
        "#666666",
        "#777777",
        "#ff0000",
        "#00ff00",
        "#0000ff",
        "#00ffff",
        "#ffff00",
        "#ff00ff",
        "#808080",
    ]) {
        assertPalette(seed);
    }
});

test("a deterministic coarse RGB cube satisfies every semantic role", () => {
    for (const red of [0, 51, 102, 153, 204, 255]) {
        for (const green of [0, 51, 102, 153, 204, 255]) {
            for (const blue of [0, 51, 102, 153, 204, 255]) {
                const seed = `#${[red, green, blue]
                    .map((channel) => channel.toString(16).padStart(2, "0"))
                    .join("")}`;
                assertPalette(seed);
            }
        }
    }
});

test("invalid or missing foreground preferences safely use automatic", () => {
    assert.equal(normalizeAccentForegroundPreference(undefined), "auto");
    assert.equal(normalizeAccentForegroundPreference("unknown"), "auto");
    assert.equal(normalizeAccentForegroundPreference("light"), "light");
    assert.equal(normalizeAccentForegroundPreference("dark"), "dark");
});

test("components cannot consume the obsolete universal accent tokens", () => {
    const forbidden =
        /--color-accent(?!-(?:seed|content|graphic|fill|subtle|selection|focus|native))|--color-on-accent(?!-(?:fill|subtle|selection))/;
    const extensions = new Set([".css", ".svelte", ".ts"]);
    const pending = [path.resolve("src")];
    const failures = [];

    while (pending.length > 0) {
        const current = pending.pop();
        for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
            const fullPath = path.join(current, entry.name);
            if (entry.isDirectory()) {
                pending.push(fullPath);
            } else if (
                extensions.has(path.extname(entry.name)) &&
                !fullPath.endsWith(path.join("utils", "theme.ts"))
            ) {
                if (forbidden.test(fs.readFileSync(fullPath, "utf8"))) {
                    failures.push(path.relative(process.cwd(), fullPath));
                }
            }
        }
    }

    assert.deepEqual(failures, []);
});

test("main's focused accent treatments use accessible semantic roles", () => {
    const extensions = new Set([".css", ".svelte"]);
    const pending = [path.resolve("src")];
    const accentTextPattern = /color:\s*var\(--color-accent-content\)\s*;/g;
    const accentTextMixPattern =
        /color:\s*color-mix\(\s*in srgb,\s*var\(--color-accent-seed\)/gs;
    let directUses = 0;
    let mixedUses = 0;

    while (pending.length > 0) {
        const current = pending.pop();
        for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
            const fullPath = path.join(current, entry.name);
            if (entry.isDirectory()) {
                pending.push(fullPath);
            } else if (extensions.has(path.extname(entry.name))) {
                const source = fs.readFileSync(fullPath, "utf8");
                directUses += [...source.matchAll(accentTextPattern)].length;
                mixedUses += [...source.matchAll(accentTextMixPattern)].length;
            }
        }
    }

    const focusedContentTreatments = [
        [
            path.resolve("src", "app.css"),
            /\.hero-label\s*\{[^}]*color:\s*var\(--color-accent-content\)/s,
        ],
        [
            path.resolve("src", "lib", "components", "CommandPalette.svelte"),
            /\.search-action\s*\{[^}]*color:\s*var\(--color-accent-content\)/s,
        ],
        [
            path.resolve("src", "lib", "components", "PlayerBar.svelte"),
            /\.track-info \.title:hover\s*\{[^}]*color:\s*var\(--color-accent-content\)/s,
        ],
        [
            path.resolve("src", "lib", "components", "QueuePanel.svelte"),
            /\.queue-row\.current \.row-title\s*\{[^}]*color:\s*var\(--color-accent-content\)/s,
        ],
        [
            path.resolve("src", "lib", "components", "ScrollIndex.svelte"),
            /\.scroll-index button\.active\s*\{[^}]*color:\s*var\(--color-accent-content\)/s,
        ],
        [
            path.resolve("src", "lib", "components", "Select.svelte"),
            /\.select-option\.selected\s*\{[^}]*color:\s*var\(--color-accent-content\)/s,
        ],
        [
            path.resolve("src", "lib", "components", "TrackRow.svelte"),
            /\.track-row\.current \.index-number\s*\{[^}]*color:\s*var\(--color-accent-content\)/s,
        ],
        [
            path.resolve("src", "lib", "components", "TrackRow.svelte"),
            /\.track-title\.current\s*\{[^}]*color:\s*var\(--color-accent-content\)/s,
        ],
        [
            path.resolve("src", "lib", "components", "TrackRow.svelte"),
            /\.track-snippet\s*\{[^}]*color:\s*var\(--color-accent-content\)/s,
        ],
        [
            path.resolve("src", "routes", "health", "+page.svelte"),
            /\.eyebrow\s*\{[^}]*color:\s*var\(--color-accent-content\)/s,
        ],
        [
            path.resolve("src", "routes", "settings", "+page.svelte"),
            /\.storage-test-url\s*\{[^}]*color:\s*var\(--color-accent-content\)/s,
        ],
        [
            path.resolve("src", "routes", "settings", "+page.svelte"),
            /\.rescan-note\s*\{[^}]*color:\s*var\(--color-accent-content\)/s,
        ],
        [
            path.resolve("src", "routes", "stats", "+page.svelte"),
            /\.insight-kicker\s*\{[^}]*color:\s*var\(--color-accent-content\)/s,
        ],
        [
            path.resolve("src", "routes", "stats", "+page.svelte"),
            /\.top-rank-peak\s*\{[^}]*color:\s*var\(--color-accent-content\)/s,
        ],
    ] as const;

    assert.equal(directUses, focusedContentTreatments.length);
    assert.equal(mixedUses, 1);
    for (const [file, pattern] of focusedContentTreatments) {
        assert.match(fs.readFileSync(file, "utf8"), pattern);
    }

    const player = fs.readFileSync(
        path.resolve("src", "lib", "components", "PlayerBar.svelte"),
        "utf8",
    );
    for (const pattern of [
        /\.control-btn:hover\s*\{[^}]*color:\s*var\(--color-accent-graphic\)/s,
        /\.mode-btn\.active\s*\{[^}]*color:\s*var\(--color-accent-graphic\)/s,
        /\.queue-toggle\.active\s*\{[^}]*color:\s*var\(--color-accent-graphic\)/s,
    ]) {
        assert.match(player, pattern);
    }

    const stats = fs.readFileSync(
        path.resolve("src", "routes", "stats", "+page.svelte"),
        "utf8",
    );
    assert.match(
        stats,
        /\.stat-card\.primary \.stat-value\s*\{[^}]*color:\s*color-mix\(\s*in srgb,\s*var\(--color-accent-seed\)/s,
    );
});

test("initial CSS palette matches the generated canonical default", () => {
    const css = fs.readFileSync(path.resolve("src", "app.css"), "utf8");
    const theme = createAccentTheme(DEFAULT_ACCENT_COLOR, "auto");
    const names = {
        content: "content",
        graphic: "graphic",
        fill: "fill",
        fillHover: "fill-hover",
        fillActive: "fill-active",
        fillDisabled: "fill-disabled",
        onFill: "on-fill",
        onFillDisabled: "on-fill-disabled",
        subtle: "subtle",
        onSubtle: "on-subtle",
        selection: "selection",
        onSelection: "on-selection",
        focus: "focus",
        native: "native",
    };

    assert.match(
        css,
        new RegExp(`--color-accent-seed:\\s*${DEFAULT_ACCENT_COLOR};`),
    );
    for (const mode of ["dark", "light"]) {
        for (const [property, cssName] of Object.entries(names)) {
            assert.match(
                css,
                new RegExp(
                    `--accent-${mode}-${cssName}:\\s*${theme[mode][property]};`,
                ),
                `${mode} ${property} default drifted from the generator`,
            );
        }
    }
});

test("applies and safely restores only validated cached accent settings", () => {
    const properties = new Map();
    const storage = new Map();
    globalThis.document = {
        documentElement: {
            style: {
                setProperty(name, value) {
                    properties.set(name, value);
                },
            },
        },
    };
    globalThis.localStorage = {
        getItem(key) {
            return storage.get(key) ?? null;
        },
        setItem(key, value) {
            storage.set(key, value);
        },
    };

    try {
        const theme = applyAccent("#eab308", "dark");
        assert.equal(properties.get("--color-accent-seed"), "#eab308");
        assert.equal(
            properties.get("--accent-dark-on-fill"),
            theme.dark.onFill,
        );

        cacheAccent("#3B82F6", "light");
        properties.clear();
        assert.equal(applyCachedAccent(), true);
        assert.equal(properties.get("--color-accent-seed"), "#3b82f6");

        storage.set(
            "sparkle.accent.v1",
            JSON.stringify({ seed: "not a color", preference: "dark" }),
        );
        properties.clear();
        assert.equal(applyCachedAccent(), false);
        assert.equal(properties.size, 0);
    } finally {
        delete globalThis.document;
        delete globalThis.localStorage;
    }
});

test("accent cache writes are best-effort when local storage is unavailable", () => {
    globalThis.localStorage = {
        setItem() {
            throw new Error("storage is blocked");
        },
    };

    try {
        assert.doesNotThrow(() => cacheAccent("#3b82f6", "light"));
    } finally {
        delete globalThis.localStorage;
    }
});
