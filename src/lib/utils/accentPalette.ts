export const DEFAULT_ACCENT_COLOR = "#fa243c";

// Derive perceptually close role colors in OKLCH, gamut-map them into sRGB,
// then validate the final quantized sRGB values with WCAG relative luminance.
// The stored seed itself is never rewritten to a derived role color.

export type AccentForegroundPreference = "auto" | "light" | "dark";
export type Rgb = [number, number, number];

export interface AccentPalette {
    content: string;
    graphic: string;
    fill: string;
    fillHover: string;
    fillActive: string;
    fillDisabled: string;
    onFill: "#000000" | "#ffffff";
    onFillDisabled: "#000000" | "#ffffff";
    subtle: string;
    onSubtle: "#000000" | "#ffffff";
    selection: string;
    onSelection: "#000000" | "#ffffff";
    focus: string;
    native: string;
}

export interface AccentTheme {
    seed: string;
    preference: AccentForegroundPreference;
    dark: AccentPalette;
    light: AccentPalette;
}

interface Oklab {
    l: number;
    a: number;
    b: number;
}

interface Oklch {
    l: number;
    c: number;
    h: number;
}

const DARK_SURFACES = ["#0b0b0d", "#171719", "#242426", "#303034"];
const LIGHT_SURFACES = ["#f5f5f7", "#ffffff", "#e9e9ed", "#d8d8de"];
const TEXT_TARGET = 4.7;
const GRAPHIC_TARGET = 3.2;
// Auto favors light content when preserving it needs only a subtle perceptual
// adjustment. This keeps the canonical red white-on-red without forcing dark
// content onto every bright custom color.
const AUTO_LIGHT_ADJUSTMENT_LIMIT = 0.06;

export function hexToRgb(hex: string): Rgb | null {
    const match = hex.trim().match(/^#?([0-9a-f]{6})$/i);
    if (!match) return null;
    const value = Number.parseInt(match[1], 16);
    return [(value >> 16) & 255, (value >> 8) & 255, value & 255];
}

export function normalizeHex(value: string): string | null {
    const rgb = hexToRgb(value);
    return rgb ? rgbToHex(rgb) : null;
}

export function normalizeAccentForegroundPreference(
    value: unknown,
): AccentForegroundPreference {
    return value === "light" || value === "dark" ? value : "auto";
}

function srgbToLinear(channel: number): number {
    const value = channel / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
}

function linearToSrgb(channel: number): number {
    return channel <= 0.0031308
        ? channel * 12.92
        : 1.055 * channel ** (1 / 2.4) - 0.055;
}

function relativeLuminance([r, g, b]: Rgb): number {
    return (
        0.2126 * srgbToLinear(r) +
        0.7152 * srgbToLinear(g) +
        0.0722 * srgbToLinear(b)
    );
}

export function contrastRatio(first: Rgb, second: Rgb): number {
    const firstLuminance = relativeLuminance(first);
    const secondLuminance = relativeLuminance(second);
    const lighter = Math.max(firstLuminance, secondLuminance);
    const darker = Math.min(firstLuminance, secondLuminance);
    return (lighter + 0.05) / (darker + 0.05);
}

export function foregroundFor(background: Rgb): "#000000" | "#ffffff" {
    return contrastRatio(background, [0, 0, 0]) >=
        contrastRatio(background, [255, 255, 255])
        ? "#000000"
        : "#ffffff";
}

function rgbToHex([r, g, b]: Rgb): string {
    return `#${[r, g, b]
        .map((channel) => Math.round(channel).toString(16).padStart(2, "0"))
        .join("")}`;
}

function rgbToOklab([red, green, blue]: Rgb): Oklab {
    const r = srgbToLinear(red);
    const g = srgbToLinear(green);
    const b = srgbToLinear(blue);

    const l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    const m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    const s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
    const lRoot = Math.cbrt(l);
    const mRoot = Math.cbrt(m);
    const sRoot = Math.cbrt(s);

    return {
        l: 0.2104542553 * lRoot + 0.793617785 * mRoot - 0.0040720468 * sRoot,
        a: 1.9779984951 * lRoot - 2.428592205 * mRoot + 0.4505937099 * sRoot,
        b: 0.0259040371 * lRoot + 0.7827717662 * mRoot - 0.808675766 * sRoot,
    };
}

function oklabToOklch({ l, a, b }: Oklab): Oklch {
    const chroma = Math.hypot(a, b);
    let hue = (Math.atan2(b, a) * 180) / Math.PI;
    if (hue < 0) hue += 360;
    return { l, c: chroma, h: chroma < 1e-7 ? 0 : hue };
}

function rgbToOklch(rgb: Rgb): Oklch {
    return oklabToOklch(rgbToOklab(rgb));
}

function rawOklchToSrgb({ l, c, h }: Oklch): [number, number, number] {
    const radians = (h * Math.PI) / 180;
    const a = c * Math.cos(radians);
    const b = c * Math.sin(radians);
    const lRoot = l + 0.3963377774 * a + 0.2158037573 * b;
    const mRoot = l - 0.1055613458 * a - 0.0638541728 * b;
    const sRoot = l - 0.0894841775 * a - 1.291485548 * b;
    const lLinear = lRoot ** 3;
    const mLinear = mRoot ** 3;
    const sLinear = sRoot ** 3;

    return [
        linearToSrgb(
            4.0767416621 * lLinear -
                3.3077115913 * mLinear +
                0.2309699292 * sLinear,
        ),
        linearToSrgb(
            -1.2684380046 * lLinear +
                2.6097574011 * mLinear -
                0.3413193965 * sLinear,
        ),
        linearToSrgb(
            -0.0041960863 * lLinear -
                0.7034186147 * mLinear +
                1.707614701 * sLinear,
        ),
    ];
}

function inSrgbGamut(channels: [number, number, number]): boolean {
    return channels.every(
        (channel) =>
            Number.isFinite(channel) &&
            channel >= -1e-7 &&
            channel <= 1.0000001,
    );
}

function oklchToRgb(color: Oklch): Rgb {
    const bounded = {
        l: Math.min(1, Math.max(0, color.l)),
        c: Math.max(0, color.c),
        h: color.h,
    };
    let channels = rawOklchToSrgb(bounded);

    if (!inSrgbGamut(channels)) {
        let low = 0;
        let high = bounded.c;
        for (let index = 0; index < 28; index += 1) {
            const chroma = (low + high) / 2;
            const candidate = rawOklchToSrgb({
                ...bounded,
                c: chroma,
            });
            if (inSrgbGamut(candidate)) {
                low = chroma;
                channels = candidate;
            } else {
                high = chroma;
            }
        }
    }

    return channels.map((channel) =>
        Math.min(255, Math.max(0, Math.round(channel * 255))),
    ) as Rgb;
}

function meetsContrast(
    foreground: Rgb,
    backgrounds: Rgb[],
    target: number,
): boolean {
    return backgrounds.every(
        (background) => contrastRatio(foreground, background) >= target,
    );
}

function searchLightness(
    seed: Rgb,
    backgrounds: Rgb[],
    target: number,
    direction: "lighter" | "darker",
): Rgb {
    if (meetsContrast(seed, backgrounds, target)) return seed;

    const oklch = rgbToOklch(seed);
    const endpoint = oklchToRgb({
        ...oklch,
        l: direction === "lighter" ? 1 : 0,
    });
    if (!meetsContrast(endpoint, backgrounds, target)) {
        return endpoint;
    }

    let passing = direction === "lighter" ? 1 : 0;
    let failing = oklch.l;
    for (let index = 0; index < 32; index += 1) {
        const lightness = (passing + failing) / 2;
        const candidate = oklchToRgb({ ...oklch, l: lightness });
        if (meetsContrast(candidate, backgrounds, target)) {
            passing = lightness;
        } else {
            failing = lightness;
        }
    }
    return oklchToRgb({ ...oklch, l: passing });
}

function oklabDistance(first: Rgb, second: Rgb): number {
    const a = rgbToOklab(first);
    const b = rgbToOklab(second);
    return Math.hypot(a.l - b.l, a.a - b.a, a.b - b.b);
}

function fillCandidate(
    seed: Rgb,
    foreground: "#000000" | "#ffffff",
    target = TEXT_TARGET,
): Rgb {
    const foregroundRgb: Rgb =
        foreground === "#ffffff" ? [255, 255, 255] : [0, 0, 0];
    return searchLightness(
        seed,
        [foregroundRgb],
        target,
        foreground === "#ffffff" ? "darker" : "lighter",
    );
}

function chooseAccessibleFill(
    seed: Rgb,
    preference: AccentForegroundPreference,
    target = TEXT_TARGET,
): { fill: Rgb; foreground: "#000000" | "#ffffff" } {
    const lightFill = fillCandidate(seed, "#ffffff", target);
    const darkFill = fillCandidate(seed, "#000000", target);

    if (preference === "light") {
        return { fill: lightFill, foreground: "#ffffff" };
    }
    if (preference === "dark") {
        return { fill: darkFill, foreground: "#000000" };
    }

    const lightDistance = oklabDistance(seed, lightFill);
    const darkDistance = oklabDistance(seed, darkFill);
    if (
        lightDistance <= AUTO_LIGHT_ADJUSTMENT_LIMIT ||
        lightDistance < darkDistance
    ) {
        return { fill: lightFill, foreground: "#ffffff" };
    }
    return { fill: darkFill, foreground: "#000000" };
}

function moveFillState(
    fill: Rgb,
    foreground: "#000000" | "#ffffff",
    amount: number,
): Rgb {
    const oklch = rgbToOklch(fill);
    const preferredDirection = foreground === "#ffffff" ? -1 : 1;
    const minimumChannelDelta = Math.max(6, Math.round(amount * 180));
    const isVisiblyDifferent = (candidate: Rgb) =>
        Math.max(
            ...candidate.map((channel, index) =>
                Math.abs(channel - fill[index]),
            ),
        ) >= minimumChannelDelta;
    const foregroundRgb: Rgb =
        foreground === "#ffffff" ? [255, 255, 255] : [0, 0, 0];
    const preferredRoom = preferredDirection > 0 ? 1 - oklch.l : oklch.l;
    // At a lightness endpoint, moving farther away from the foreground clips
    // or collapses chroma. Reverse the whole state path at that endpoint.
    const firstDirection =
        preferredRoom >= amount ? preferredDirection : -preferredDirection;

    for (const direction of [firstDirection, -firstDirection]) {
        for (let step = 0; step <= 24; step += 1) {
            const candidate = oklchToRgb({
                ...oklch,
                l: oklch.l + direction * (amount + step * 0.01),
            });
            if (
                isVisiblyDifferent(candidate) &&
                contrastRatio(foregroundRgb, candidate) >= TEXT_TARGET
            ) {
                return candidate;
            }
        }
    }
    return fill;
}

function deriveDisabledFill(fill: Rgb, foreground: "#000000" | "#ffffff"): Rgb {
    const oklch = rgbToOklch(fill);
    const muted = oklchToRgb({ ...oklch, c: oklch.c * 0.35 });
    return fillCandidate(muted, foreground);
}

function blendRgb(foreground: Rgb, background: Rgb, alpha: number): Rgb {
    return foreground.map((channel, index) =>
        Math.round(channel * alpha + background[index] * (1 - alpha)),
    ) as Rgb;
}

function derivePalette(
    seed: Rgb,
    mode: "dark" | "light",
    preference: AccentForegroundPreference,
): AccentPalette {
    const surfaces = (mode === "dark" ? DARK_SURFACES : LIGHT_SURFACES).map(
        (color) => hexToRgb(color) as Rgb,
    );
    const direction = mode === "dark" ? "lighter" : "darker";
    const subtleBase = surfaces[2];
    const subtle = blendRgb(seed, subtleBase, mode === "dark" ? 0.16 : 0.12);
    const textBackgrounds = [...surfaces, subtle];
    const content = searchLightness(
        seed,
        textBackgrounds,
        TEXT_TARGET,
        direction,
    );
    const graphic = searchLightness(seed, surfaces, GRAPHIC_TARGET, direction);
    const { fill, foreground } = chooseAccessibleFill(seed, preference);
    const fillHover = moveFillState(fill, foreground, 0.035);
    const fillActive = moveFillState(fill, foreground, 0.065);
    const fillDisabled = deriveDisabledFill(fill, foreground);

    const onSubtle = mode === "dark" ? "#ffffff" : "#000000";

    const selectionSeed = blendRgb(
        seed,
        surfaces[0],
        mode === "dark" ? 0.42 : 0.3,
    );
    const selectionPair = chooseAccessibleFill(selectionSeed, "auto");

    return {
        content: rgbToHex(content),
        graphic: rgbToHex(graphic),
        fill: rgbToHex(fill),
        fillHover: rgbToHex(fillHover),
        fillActive: rgbToHex(fillActive),
        fillDisabled: rgbToHex(fillDisabled),
        onFill: foreground,
        onFillDisabled: foreground,
        subtle: rgbToHex(subtle),
        onSubtle,
        selection: rgbToHex(selectionPair.fill),
        onSelection: selectionPair.foreground,
        focus: rgbToHex(graphic),
        native: rgbToHex(fill),
    };
}

export function createAccentTheme(
    value: string,
    requestedPreference: AccentForegroundPreference = "auto",
): AccentTheme {
    const seed = normalizeHex(value) ?? DEFAULT_ACCENT_COLOR;
    const rgb = hexToRgb(seed) as Rgb;
    const preference = normalizeAccentForegroundPreference(requestedPreference);
    return {
        seed,
        preference,
        dark: derivePalette(rgb, "dark", preference),
        light: derivePalette(rgb, "light", preference),
    };
}
