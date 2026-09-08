export interface LrcLine {
    timeMs: number;
    text: string;
}

export const LYRIC_TRANSITION_DURATION_MS = 200;

export function normalizeLyricSpacing(text: string): string {
    return text.replace(/[ \u3000]+/g, " ").trim();
}

const breakBefore = new Set(" \u3000\"'“‘＂＇([{（［｛〈《「『【〔〖〘〚｢");
const breakAfter = new Set(".。．｡?？!！/／");
const closingPunctuation = /^[\p{Pe}\p{Pf}.,。，、．｡?？!！/／]/u;
const cjk =
    /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u;
const wordSegmenter =
    typeof Intl.Segmenter === "function"
        ? new Intl.Segmenter(undefined, { granularity: "word" })
        : null;

/** Preferred wrap points; the saved lyric text and timestamps stay untouched. */
export function lyricWrapSegments(text: string): string[] {
    text = normalizeLyricSpacing(text);
    const boundaries = new Set<number>([0, text.length]);
    let index = 0;
    for (const char of text) {
        if (breakBefore.has(char)) boundaries.add(index);
        index += char.length;
        if (breakAfter.has(char)) boundaries.add(index);
    }
    if (wordSegmenter && cjk.test(text)) {
        for (const word of wordSegmenter.segment(text)) {
            const end = word.index + word.segment.length;
            if (
                word.isWordLike &&
                cjk.test(word.segment) &&
                !closingPunctuation.test(text.slice(end))
            ) {
                boundaries.add(end);
            }
        }
    }
    const points = [...boundaries].sort((a, b) => a - b);
    return points.slice(1).map((end, i) => text.slice(points[i], end));
}

const timestampPattern = /\[(\d+):(\d+(?:\.\d+)?)\]/g;
const wordTimestampPattern = /<(\d+):(\d+(?:\.\d+)?)>/g;
const offsetPattern = /^\s*\[offset:\s*([+-]?\d+)\s*\]\s*$/gim;
const metadataPattern =
    /^\[(?:ar|al|ti|au|by|offset|re|ve|length|id):[^\]]*\]$/i;

function timestampMs(minutes: string, seconds: string): number | null {
    const time = Math.round((Number(minutes) * 60 + Number(seconds)) * 1000);
    return Number.isSafeInteger(time) && time >= 0 ? time : null;
}

export function lrcFileOffsetMs(text: string): number {
    let offset = 0;
    for (const match of text.matchAll(offsetPattern)) {
        const value = Number(match[1]);
        if (Number.isSafeInteger(value)) offset = value;
    }
    return offset;
}

/** This contract also runs against Rust's parser in providers/lyrics/document.rs. */
export function parseLrc(text: string): LrcLine[] {
    const lines: LrcLine[] = [];
    const offset = lrcFileOffsetMs(text);
    for (const raw of text.split(/\r\n?|\n/)) {
        const trimmed = raw.trim();
        if (!trimmed.startsWith("[") || metadataPattern.test(trimmed)) continue;
        const textOnly = trimmed
            .replace(timestampPattern, "")
            .replace(wordTimestampPattern, "")
            .trim();
        // Blank timed cues intentionally keep the previous sentence visible.
        if (!textOnly) continue;
        for (const match of trimmed.matchAll(timestampPattern)) {
            const time = timestampMs(match[1], match[2]);
            if (time !== null)
                lines.push({
                    timeMs: Math.max(
                        0,
                        Math.min(Number.MAX_SAFE_INTEGER, time - offset),
                    ),
                    text: textOnly,
                });
        }
    }
    return lines.sort((a, b) => a.timeMs - b.timeMs);
}

export function lrcPlainText(text: string): string {
    const lines = parseLrc(text);
    if (lines.length) return lines.map((line) => line.text).join("\n");
    return text
        .split(/\r\n?|\n/)
        .map((line) => line.trim())
        .filter((line) => !metadataPattern.test(line))
        .map((line) =>
            line
                .replace(timestampPattern, "")
                .replace(wordTimestampPattern, "")
                .trim(),
        )
        .filter(Boolean)
        .join("\n");
}

function formatTimestamp(time: number): string {
    return `${String(Math.floor(time / 60000)).padStart(2, "0")}:${String(Math.floor(time / 1000) % 60).padStart(2, "0")}.${String(time % 1000).padStart(3, "0")}`;
}

export function applyLrcOffset(text: string, delayMs: number): string {
    const offset = lrcFileOffsetMs(text);
    return text
        .replace(/^\uFEFF/, "")
        .split(/\r\n?|\n/)
        .map((line) => {
            if (/^\s*\[offset:\s*[+-]?\d+\s*\]\s*$/i.test(line))
                return "[offset:0]";
            const shifted = (
                full: string,
                minutes: string,
                seconds: string,
            ): string => {
                const time = timestampMs(minutes, seconds);
                if (time === null) return full;
                const normalized = Math.max(
                    0,
                    Math.min(Number.MAX_SAFE_INTEGER, time - offset),
                );
                const value = formatTimestamp(
                    Math.max(
                        0,
                        Math.min(
                            Number.MAX_SAFE_INTEGER,
                            normalized + Math.round(delayMs),
                        ),
                    ),
                );
                return full.startsWith("[") ? `[${value}]` : `<${value}>`;
            };
            return line
                .replace(timestampPattern, shifted)
                .replace(wordTimestampPattern, shifted);
        })
        .join("\n");
}

export function activeLineIndex(lines: LrcLine[], timeMs: number): number {
    // The first sentence is visible during the intro, even before its cue.
    let index = lines.length ? 0 : -1;
    for (let i = 0; i < lines.length; i++) {
        if (lines[i].timeMs <= timeMs) {
            index = i;
        } else {
            break;
        }
    }
    return index;
}

export function anticipatedLineIndex(
    lines: LrcLine[],
    timeMs: number,
    leadMs = LYRIC_TRANSITION_DURATION_MS,
): number {
    return activeLineIndex(lines, timeMs + Math.max(0, leadMs));
}
