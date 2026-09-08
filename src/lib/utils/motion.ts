/** Read both preferences at the time of an action, including changes after mount. */
export function prefersReducedMotion(): boolean {
    if (typeof window === "undefined" || typeof document === "undefined")
        return true;
    return (
        window.matchMedia("(prefers-reduced-motion: reduce)").matches ||
        document.documentElement.dataset.motion !== "full"
    );
}

export function motionScrollBehavior(): ScrollBehavior {
    return prefersReducedMotion() ? "instant" : "smooth";
}
