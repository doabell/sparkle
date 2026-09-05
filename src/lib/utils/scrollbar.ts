export function scrollbarGeometry(
    viewportHeight: number,
    contentHeight: number,
    scrollTop: number,
    trackHeight: number,
) {
    const maxScroll = Math.max(0, contentHeight - viewportHeight);
    const track = Math.max(0, trackHeight);
    const thumbHeight = Math.min(
        track,
        Math.max(28, (track * viewportHeight) / Math.max(1, contentHeight)),
    );
    const travel = Math.max(0, track - thumbHeight);
    const position = Math.max(0, Math.min(maxScroll, scrollTop));
    return {
        maxScroll,
        position,
        thumbHeight,
        travel,
        thumbTop: maxScroll > 0 ? (travel * position) / maxScroll : 0,
    };
}

export function scrollTopFromThumb(
    thumbTop: number,
    travel: number,
    maxScroll: number,
) {
    return travel > 0
        ? Math.max(0, Math.min(1, thumbTop / travel)) * maxScroll
        : 0;
}

export function scrollbarKeyTarget(
    key: string,
    position: number,
    viewport: number,
    maxScroll: number,
    shift = false,
): number | null {
    const page = viewport * 0.9;
    const delta: Record<string, number> = {
        ArrowUp: -40,
        ArrowDown: 40,
        PageUp: -page,
        PageDown: page,
        " ": shift ? -page : page,
        Home: -position,
        End: maxScroll - position,
    };
    if (!Object.hasOwn(delta, key)) return null;
    return Math.max(0, Math.min(maxScroll, position + delta[key]));
}
