export interface LrcLine {
    timeMs: number;
    text: string;
}

export const LYRIC_TRANSITION_DURATION_MS = 200;

export function parseLrc(text: string): LrcLine[] {
    const lines: LrcLine[] = [];
    for (const raw of text.split(/\r?\n/)) {
        const trimmed = raw.trim();
        if (!trimmed) continue;
        const tagRegex = /\[(\d+):(\d+(?:\.\d+)?)\]/g;
        const times: number[] = [];
        let match: RegExpExecArray | null;
        while ((match = tagRegex.exec(trimmed)) !== null) {
            const minutes = parseInt(match[1], 10);
            const seconds = parseFloat(match[2]);
            times.push(Math.round((minutes * 60 + seconds) * 1000));
        }
        const textOnly = trimmed.replace(tagRegex, "").trim();
        if (times.length === 0 || textOnly.length === 0) continue;
        for (const timeMs of times) {
            lines.push({ timeMs, text: textOnly });
        }
    }
    return lines.sort((a, b) => a.timeMs - b.timeMs);
}

export function activeLineIndex(lines: LrcLine[], timeMs: number): number {
    let index = -1;
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
