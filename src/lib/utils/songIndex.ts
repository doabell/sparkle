export type SongIndexLanguage = "auto" | "en" | "ja";
export type ScrollIndexMode = "text" | "year";

const KANJI_BUCKET = "漢";

const JAPANESE_KANA_BUCKETS: readonly [string, string][] = [
    ["あ", "あいうえおゔ"],
    ["か", "かきくけこがぎぐげご"],
    ["さ", "さしすせそざじずぜぞ"],
    ["た", "たちつてとだぢづでど"],
    ["な", "なにぬねの"],
    ["は", "はひふへほばびぶべぼぱぴぷぺぽ"],
    ["ま", "まみむめも"],
    ["や", "やゆよ"],
    ["ら", "らりるれろ"],
    ["わ", "わをん"],
];

export function resolveSongIndexLanguage(
    language: SongIndexLanguage,
): Exclude<SongIndexLanguage, "auto"> {
    if (language !== "auto") return language;
    const locale =
        typeof navigator === "undefined"
            ? "en"
            : navigator.language.toLowerCase();
    return locale.startsWith("ja") ? "ja" : "en";
}

export function getSongCollator(language: SongIndexLanguage): Intl.Collator {
    const resolved = resolveSongIndexLanguage(language);
    return new Intl.Collator(resolved, {
        numeric: true,
        sensitivity: "base",
    });
}

function normalizeKana(character: string): string {
    const normalized = character.normalize("NFKC");
    const codePoint = normalized.codePointAt(0) ?? 0;
    // Katakana and hiragana have matching code points offset by 0x60.
    if (codePoint >= 0x30a1 && codePoint <= 0x30f6) {
        return String.fromCodePoint(codePoint - 0x60);
    }
    return normalized[0] ?? "";
}

function japaneseBucket(character: string): string | null {
    const kana = normalizeKana(character);
    for (const [label, characters] of JAPANESE_KANA_BUCKETS) {
        if (characters.includes(kana)) return label;
    }
    return null;
}

/** Return the visible bucket used by the song scroll index for a value. */
export function getSongIndexBucket(
    value: string | null | undefined,
    language: SongIndexLanguage,
): string {
    const first = [...(value ?? "").trim()][0];
    if (!first) return "#";

    if (resolveSongIndexLanguage(language) === "ja") {
        const bucket = japaneseBucket(first);
        if (bucket) return bucket;
    }

    // Browsers can identify Han characters, but they do not expose a native
    // kanji-to-reading transliterator. Keep Han in a meaningful native bucket
    // instead of hiding it under '#'; Intl.Collator still handles its order.
    if (/\p{Script=Han}/u.test(first)) return KANJI_BUCKET;

    const latin = first.normalize("NFD").replace(/[\u0300-\u036f]/g, "");
    if (/^[a-z]$/i.test(latin)) return latin.toUpperCase();
    return "#";
}

export function getSongIndexBucketTitle(label: string): string {
    if (label === KANJI_BUCKET) return "Kanji / Han characters";
    if (label === "#") return "Other";
    return label;
}

export function getYearScrollIndexLabel(label: string): string {
    if (label === "?" || label.startsWith("Unknown")) return "?";
    return /^\d{4}$/.test(label) ? label.slice(-2) : label;
}

/** Resolve whether a list index should display years or text buckets. */
export function resolveScrollIndexMode(
    groupBy: string,
    sortBy: string,
): ScrollIndexMode {
    return groupBy === "year" || (groupBy === "none" && sortBy === "year")
        ? "year"
        : "text";
}

/** Apply the single display policy shared by every list index. */
export function getScrollIndexLabel(
    value: string | null | undefined,
    mode: ScrollIndexMode,
    language: SongIndexLanguage,
): string {
    return mode === "year"
        ? getYearScrollIndexLabel(value ?? "?")
        : getSongIndexBucket(value, language);
}

export interface ScrollIndexGroup {
    key: string;
    offset: number;
}

/** Build the compact index for an already grouped list. */
export function createGroupScrollIndexEntries(
    groups: readonly ScrollIndexGroup[],
    mode: ScrollIndexMode,
    language: SongIndexLanguage,
): Array<{
    key: string;
    label: string;
    title: string;
    index: number;
    kind: "group" | "year";
}> {
    const seen = new Set<string>();
    const entries: Array<{
        key: string;
        label: string;
        title: string;
        index: number;
        kind: "group" | "year";
    }> = [];

    for (const group of groups) {
        if (!group.key) continue;
        const label = getScrollIndexLabel(group.key, mode, language);
        if (seen.has(label)) continue;
        seen.add(label);
        entries.push({
            key: `group-${group.offset}`,
            label,
            title: `Jump to ${group.key}`,
            index: group.offset,
            kind: mode === "year" ? "year" : "group",
        });
    }

    return entries;
}

export function createScrollIndexEntries<T>(
    items: readonly T[],
    valueOf: (item: T) => string | null | undefined,
    language: SongIndexLanguage,
    mode: ScrollIndexMode = "text",
): Array<{
    key: string;
    label: string;
    title: string;
    index: number;
    kind: "bucket";
}> {
    const seen = new Set<string>();
    const entries: Array<{
        key: string;
        label: string;
        title: string;
        index: number;
        kind: "bucket";
    }> = [];

    for (const [index, item] of items.entries()) {
        const label = getScrollIndexLabel(valueOf(item), mode, language);
        if (seen.has(label)) continue;
        seen.add(label);
        entries.push({
            key: `${label}-${index}`,
            label,
            title: `Jump to ${getSongIndexBucketTitle(label)}`,
            index,
            kind: "bucket",
        });
    }
    return entries;
}
