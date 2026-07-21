import type { ContentScrollSnapshot } from "./scrollRestore";

export interface ScrollbackRegistration<T = unknown> {
    key: string;
    capture: () => T;
    restore: (value: T) => void;
}

export interface ScrollbackSnapshot {
    route: string;
    scroll: ContentScrollSnapshot;
    page: {
        key: string;
        value: unknown;
    } | null;
}

export interface ScrollbackRestoreResult {
    scroll: ContentScrollSnapshot | number | null;
    pageRestored: boolean;
}

export const SCROLLBACK_HISTORY_STATE_KEY = "sparkle:scrollback";

export interface HistoryStateAdapter {
    state: Record<string, unknown> | null;
    replaceState(
        state: Record<string, unknown>,
        title: string,
        url?: string | null,
    ): void;
}

export function saveScrollbackToHistory(
    history: HistoryStateAdapter,
    snapshot: ScrollbackSnapshot,
    url?: string | null,
) {
    history.replaceState(
        {
            ...(history.state ?? {}),
            [SCROLLBACK_HISTORY_STATE_KEY]: snapshot,
        },
        "",
        url,
    );
}

export function readScrollbackFromHistory(
    history: HistoryStateAdapter,
    route: string,
): ScrollbackSnapshot | null {
    const snapshot = history.state?.[SCROLLBACK_HISTORY_STATE_KEY];
    if (!snapshot || typeof snapshot !== "object") return null;

    const candidate = snapshot as Partial<ScrollbackSnapshot>;
    return candidate.route === route && candidate.scroll
        ? (candidate as ScrollbackSnapshot)
        : null;
}

/**
 * Keeps route-specific state behind the layout's one history snapshot. Pages
 * only register the small piece of state that cannot be reconstructed from
 * the DOM, such as a progressively rendered item count.
 */
export function createScrollbackRegistry() {
    let active: ScrollbackRegistration | null = null;
    let pendingRestore: {
        key: string;
        value: unknown;
    } | null = null;

    function register<T>(registration: ScrollbackRegistration<T>) {
        const normalized: ScrollbackRegistration = {
            key: registration.key,
            capture: () => registration.capture(),
            restore: (value) => registration.restore(value as T),
        };
        active = normalized;
        if (pendingRestore?.key === normalized.key) {
            normalized.restore(pendingRestore.value);
            pendingRestore = null;
        }
        return () => {
            if (active === normalized) active = null;
        };
    }

    function capture(route: string, scroll: ContentScrollSnapshot) {
        return {
            route,
            scroll,
            page: active
                ? {
                      key: active.key,
                      value: active.capture(),
                  }
                : null,
        } satisfies ScrollbackSnapshot;
    }

    function restore(
        route: string,
        snapshot: ScrollbackSnapshot | ContentScrollSnapshot | number | null,
    ): ScrollbackRestoreResult {
        if (!snapshot) {
            return { scroll: null, pageRestored: false };
        }

        if (typeof snapshot === "number" || "top" in snapshot) {
            pendingRestore = null;
            return { scroll: snapshot, pageRestored: false };
        }

        pendingRestore = null;
        const pageMatches = snapshot.route === route && snapshot.page !== null;

        if (pageMatches && active?.key === snapshot.page!.key) {
            active!.restore(snapshot.page!.value);
        } else if (pageMatches) {
            pendingRestore = {
                key: snapshot.page!.key,
                value: snapshot.page!.value,
            };
        }

        return {
            scroll: snapshot.scroll,
            pageRestored: pageMatches && active?.key === snapshot.page!.key,
        };
    }

    return { register, capture, restore };
}

export const scrollbackRegistry = createScrollbackRegistry();
