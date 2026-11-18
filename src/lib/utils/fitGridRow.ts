import type { Action } from "svelte/action";

// Use the grid's resolved columns so card sizing, gaps and media queries
// remain defined in CSS. Only render the items that fit in one row.
export const fitGridRow: Action<HTMLElement, (columns: number) => void> = (
    node,
    onColumns,
) => {
    let columns = 0;
    const measure = () => {
        const tracks = getComputedStyle(node).gridTemplateColumns;
        const next =
            tracks && tracks !== "none" ? tracks.trim().split(/\s+/).length : 1;
        if (next !== columns) {
            columns = next;
            onColumns(columns);
        }
    };
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    measure();

    return {
        update(callback) {
            onColumns = callback;
            onColumns(columns);
        },
        destroy() {
            observer.disconnect();
        },
    };
};
