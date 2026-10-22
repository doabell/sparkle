import type { Action } from "svelte/action";

// Share one observer across a grid. Delay provider/IPC work, not just image
// decoding, until a mounted placeholder is near the viewport.
const pending = new Map<Element, () => void>();
let observer: IntersectionObserver | null = null;

function release(node: Element) {
    pending.delete(node);
    observer?.unobserve(node);
    if (pending.size === 0) {
        observer?.disconnect();
        observer = null;
    }
}

export const onceVisible: Action<HTMLElement, () => void> = (
    node,
    callback,
) => {
    if (typeof IntersectionObserver === "undefined") {
        callback();
        return;
    }
    observer ??= new IntersectionObserver(
        (entries) => {
            for (const entry of entries) {
                if (!entry.isIntersecting) continue;
                const show = pending.get(entry.target);
                release(entry.target);
                show?.();
            }
        },
        { rootMargin: "300px" },
    );
    pending.set(node, callback);
    observer.observe(node);
    return {
        update(next) {
            if (pending.has(node)) pending.set(node, next);
        },
        destroy() {
            release(node);
        },
    };
};
