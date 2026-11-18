// @ts-nocheck
import { expect, test } from "bun:test";
import { fitGridRow } from "../src/lib/utils/fitGridRow.ts";

test("a grid row follows CSS columns on mount and resize and cleans up its observer", () => {
    const originalObserver = globalThis.ResizeObserver;
    const originalStyle = globalThis.getComputedStyle;
    let resize;
    let observed;
    let disconnected = false;
    let tracks = "180px 180px 180px";
    globalThis.ResizeObserver = class {
        constructor(callback) {
            resize = callback;
        }
        observe(node) {
            observed = node;
        }
        disconnect() {
            disconnected = true;
        }
    };
    globalThis.getComputedStyle = () => ({ gridTemplateColumns: tracks });
    try {
        const node = {};
        const counts = [];
        const action = fitGridRow(node, (count) => counts.push(count));
        expect(observed).toBe(node);
        expect(counts).toEqual([3]);
        resize();
        expect(counts).toEqual([3]);

        tracks = Array(8).fill("175px").join(" ");
        resize();
        tracks = " 190px   190px ";
        resize();
        expect(counts).toEqual([3, 8, 2]);

        const nextCounts = [];
        action.update((count) => nextCounts.push(count));
        expect(nextCounts).toEqual([2]);
        tracks = "none";
        resize();
        expect(nextCounts).toEqual([2, 1]);
        expect(counts).toEqual([3, 8, 2]);
        action.destroy();
        expect(disconnected).toBe(true);
    } finally {
        if (originalObserver === undefined) delete globalThis.ResizeObserver;
        else globalThis.ResizeObserver = originalObserver;
        if (originalStyle === undefined) delete globalThis.getComputedStyle;
        else globalThis.getComputedStyle = originalStyle;
    }
});
