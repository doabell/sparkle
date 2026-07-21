export interface ScrollContainer {
    scrollTop: number;
    scrollHeight: number;
    clientHeight: number;
}

export interface ScrollRestoreScheduler {
    requestFrame(callback: () => void): number;
    cancelFrame(frame: number): void;
}

export interface ContentScrollSnapshot {
    top: number;
    maxScrollTop: number;
}

export type ObserveScrollMutations = (callback: () => void) => () => void;

/**
 * Reapply a snapshot after navigation while a route's content is mounting.
 * A route can finish mounting asynchronously, so mutation notifications
 * trigger another attempt instead of relying on a fixed number of frames.
 */
export function createContentScrollRestorer(
    getContainer: () => ScrollContainer | null,
    scheduler: ScrollRestoreScheduler,
    observeMutations?: ObserveScrollMutations,
) {
    let frame: number | null = null;
    let stopObserving: (() => void) | null = null;
    let target: ContentScrollSnapshot | null = null;

    function stop() {
        if (frame !== null) {
            scheduler.cancelFrame(frame);
            frame = null;
        }
        stopObserving?.();
        stopObserving = null;
        target = null;
    }

    function schedule() {
        if (target === null || frame !== null) return;
        frame = scheduler.requestFrame(apply);
    }

    function apply() {
        frame = null;
        if (target === null) return;

        const container = getContainer();
        if (!container) return;

        const maxScrollTop = Math.max(
            0,
            container.scrollHeight - container.clientHeight,
        );

        const targetTop = Math.max(0, target.top);
        const ratio =
            target.maxScrollTop > 0
                ? Math.min(1, targetTop / target.maxScrollTop)
                : 0;

        // A progressively rendered list must first grow far enough to reach
        // the old absolute position. Staying at its current end keeps the
        // lazy sentinel active; once it is reachable, use the old percentage
        // against the new content height.
        if (maxScrollTop < targetTop) {
            container.scrollTop = maxScrollTop;
            return;
        }

        container.scrollTop = ratio * maxScrollTop;

        stop();
    }

    function restore(snapshot: ContentScrollSnapshot | number) {
        stop();
        target =
            typeof snapshot === "number"
                ? { top: snapshot, maxScrollTop: snapshot }
                : snapshot;
        stopObserving = observeMutations?.(schedule) ?? null;
        schedule();
    }

    return { restore, stop };
}
