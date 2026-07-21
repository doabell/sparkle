// @ts-nocheck
import { strict as assert } from "node:assert";
import { test } from "node:test";
import {
    createContentScrollRestorer,
    type ScrollContainer,
} from "../src/lib/utils/scrollRestore.ts";

class FakeScheduler {
    private nextFrame = 1;
    private callbacks = new Map<number, () => void>();

    requestFrame(callback: () => void): number {
        const frame = this.nextFrame++;
        this.callbacks.set(frame, callback);
        return frame;
    }

    cancelFrame(frame: number): void {
        this.callbacks.delete(frame);
    }

    flushOne(): void {
        const next = this.callbacks.entries().next();
        if (next.done) return;
        this.callbacks.delete(next.value[0]);
        next.value[1]();
    }
}

class FakeMutationObserver {
    private callback: (() => void) | null = null;

    observe(callback: () => void): () => void {
        this.callback = callback;
        return () => (this.callback = null);
    }

    notify(): void {
        this.callback?.();
    }
}

function containerWithMax(maxScrollTop: number): ScrollContainer {
    let top = 0;
    return {
        get scrollTop() {
            return top;
        },
        set scrollTop(value: number) {
            top = Math.min(value, maxScrollTop);
        },
        scrollHeight: maxScrollTop + 600,
        clientHeight: 600,
    };
}

test("preserves the scroll percentage as a lazy list grows", () => {
    const scheduler = new FakeScheduler();
    const mutations = new FakeMutationObserver();
    let container = containerWithMax(8_000);
    const restorer = createContentScrollRestorer(
        () => container,
        scheduler,
        (callback) => mutations.observe(callback),
    );

    restorer.restore({ top: 9_000, maxScrollTop: 10_000 });
    scheduler.flushOne();

    container = containerWithMax(16_000);
    mutations.notify();
    scheduler.flushOne();

    assert.equal(container.scrollTop, 14_400);
});
