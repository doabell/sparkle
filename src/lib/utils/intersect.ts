import type { Action } from "svelte/action";
import { tick } from "svelte";

export const intersect: Action<HTMLElement, () => void> = (node, callback) => {
    let disposed = false;
    let pending = false;
    const observer = new IntersectionObserver(
        async (entries) => {
            if (
                disposed ||
                pending ||
                !entries.some((entry) => entry.isIntersecting)
            )
                return;
            pending = true;
            observer.unobserve(node);
            callback();
            await tick();
            pending = false;
            if (!disposed) observer.observe(node);
        },
        { rootMargin: "600px" },
    );
    observer.observe(node);
    return {
        update(next) {
            callback = next;
        },
        destroy() {
            disposed = true;
            observer.disconnect();
        },
    };
};
