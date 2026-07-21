import type { Action } from "svelte/action";

// Svelte action that fires when the node scrolls near the viewport. Used for
// progressive (lazy) rendering of long lists: put it on a sentinel div and
// grow the visible window in the callback.
export const intersect: Action<HTMLElement, () => void> = (node, callback) => {
    const observer = new IntersectionObserver(
        (entries) => {
            if (entries.some((entry) => entry.isIntersecting)) {
                callback();
            }
        },
        { rootMargin: "600px" },
    );
    observer.observe(node);
    return {
        destroy() {
            observer.disconnect();
        },
    };
};
