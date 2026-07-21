function hexToRgb(hex: string): [number, number, number] | null {
    const m = hex.trim().match(/^#?([0-9a-f]{6})$/i);
    if (!m) return null;
    const v = parseInt(m[1], 16);
    return [(v >> 16) & 255, (v >> 8) & 255, v & 255];
}

function lighten(
    [r, g, b]: [number, number, number],
    amount: number,
): [number, number, number] {
    const mix = (c: number) =>
        Math.min(255, Math.round(c + (255 - c) * amount));
    return [mix(r), mix(g), mix(b)];
}

// Applies the chosen accent color app-wide. Glows/shadows derive from
// --color-accent via color-mix, so only the two base vars need setting.
export function applyAccent(hex: string) {
    const rgb = hexToRgb(hex) ?? [250, 36, 60];
    const [r, g, b] = rgb;
    const [hr, hg, hb] = lighten(rgb, 0.18);
    const root = document.documentElement;
    root.style.setProperty("--color-accent", `rgb(${r}, ${g}, ${b})`);
    root.style.setProperty("--color-accent-hover", `rgb(${hr}, ${hg}, ${hb})`);
}

export function normalizeHex(value: string): string | null {
    const rgb = hexToRgb(value);
    if (!rgb) return null;
    const [r, g, b] = rgb;
    return `#${((1 << 24) | (r << 16) | (g << 8) | b).toString(16).slice(1)}`;
}
