export const FONT_OPTIONS = [
    { label: "System", value: "System" },
    { label: "Inter", value: "Inter" },
    { label: "SF Pro", value: "SF Pro" },
    { label: "Roboto", value: "Roboto" },
    { label: "Monospace", value: "Monospace" },
    { label: "Georgia", value: "Georgia" },
];

export const FONT_STACKS: Record<string, string> = {
    System: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif',
    Inter: '"Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
    "SF Pro":
        '-apple-system, BlinkMacSystemFont, "SF Pro Display", "SF Pro Text", sans-serif',
    Roboto: '"Roboto", -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
    Monospace:
        '"SF Mono", "Menlo", "Monaco", "Consolas", "Liberation Mono", "Courier New", monospace',
    Georgia: 'Georgia, "Times New Roman", Times, serif',
};

export function getFontStack(fontName: string): string {
    const known = FONT_STACKS[fontName];
    if (known) return known;
    const custom = fontName.trim();
    if (!custom) return FONT_STACKS.System;
    // Already a full font-family list — use verbatim.
    if (custom.includes(",")) return custom;
    const fallback = /mono|console|courier|code/i.test(custom)
        ? "monospace"
        : "sans-serif";
    return `"${custom.replace(/"/g, "")}", ${fallback}`;
}
