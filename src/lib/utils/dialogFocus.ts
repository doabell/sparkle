/** Keep keyboard navigation inside a modal and return focus to its trigger. */
export function dialogFocus(node: HTMLElement, onClose: () => void) {
    const previous = document.activeElement;
    node.focus();
    function handleKeydown(event: KeyboardEvent) {
        if (event.key === "Escape" && !event.defaultPrevented) {
            onClose();
            return;
        }
        if (event.key !== "Tab") return;
        const controls = Array.from(
            node.querySelectorAll<HTMLElement>(
                'button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), a[href], summary, [tabindex="0"]',
            ),
        ).filter((element) => element.getClientRects().length > 0);
        const first = controls[0];
        const last = controls.at(-1);
        if (!first || !last) {
            event.preventDefault();
            node.focus();
        } else if (
            event.shiftKey &&
            (document.activeElement === first ||
                document.activeElement === node ||
                !node.contains(document.activeElement))
        ) {
            event.preventDefault();
            last.focus();
        } else if (
            !event.shiftKey &&
            (document.activeElement === last ||
                document.activeElement === node ||
                !node.contains(document.activeElement))
        ) {
            event.preventDefault();
            first.focus();
        }
    }
    document.addEventListener("keydown", handleKeydown);
    return {
        destroy() {
            document.removeEventListener("keydown", handleKeydown);
            if (previous instanceof HTMLElement && previous.isConnected)
                previous.focus();
        },
    };
}
