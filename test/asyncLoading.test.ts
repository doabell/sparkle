// @ts-nocheck
import { afterEach, expect, mock, test } from "bun:test";
import { createLatestRequest } from "../src/lib/utils/latestRequest";
import { createQueueStore } from "../src/lib/stores/queue";
import { onceVisible } from "../src/lib/utils/onceVisible";
import { intersect } from "../src/lib/utils/intersect";
import { invoke, listen } from "./support/platform";

const flush = async () => {
    for (let i = 0; i < 20; i++) await Promise.resolve();
};
function deferred() {
    let resolve, reject;
    const promise = new Promise((yes, no) => {
        resolve = yes;
        reject = no;
    });
    return { promise, resolve, reject };
}
const nativeQueue = (id) => ({
    tracks: [{ id }],
    revision: id,
    total: 1,
    offset: 0,
    current_pos: 0,
});
const cleanups = [];
const originalObserver = globalThis.IntersectionObserver;
afterEach(() => {
    for (const cleanup of cleanups.splice(0)) cleanup();
    invoke.mockReset();
    listen.mockReset();
    if (originalObserver) globalThis.IntersectionObserver = originalObserver;
    else delete globalThis.IntersectionObserver;
});

test("only the latest route load can publish after A to B to A navigation or unmount", async () => {
    const requests = createLatestRequest();
    const oldA = requests.start();
    const b = requests.start();
    const currentA = requests.start();
    expect(oldA()).toBe(false);
    expect(b()).toBe(false);
    expect(currentA()).toBe(true);
    requests.invalidate();
    expect(currentA()).toBe(false);
    expect(requests.start()()).toBe(true);
});

test("queue subscribes before reading and disposes late subscriptions without starting a read", async () => {
    const registrations = [deferred(), deferred()];
    const stops = [mock(() => {}), mock(() => {})];
    listen.mockImplementationOnce(() => registrations[0].promise);
    listen.mockImplementationOnce(() => registrations[1].promise);
    const values = [];
    const stop = createQueueStore().subscribe((value) => values.push(value));
    expect(invoke).not.toHaveBeenCalled();
    stop();
    registrations.forEach((registration, i) => registration.resolve(stops[i]));
    await flush();
    expect(stops.every((fn) => fn.mock.calls.length === 1)).toBe(true);
    expect(invoke).not.toHaveBeenCalled();
    expect(values.at(-1)).toEqual({
        queue: null,
        pages: new Map(),
        error: null,
    });
});

test("queue coalesces event bursts, rejects superseded reads, and ignores progress/volume changes", async () => {
    const handlers = new Map();
    const stops = [];
    listen.mockImplementation(async (event, handler) => {
        handlers.set(event, handler);
        const stop = mock(() => {});
        stops.push(stop);
        return stop;
    });
    const reads = [];
    invoke.mockImplementation(() => {
        const read = deferred();
        reads.push(read);
        return read.promise;
    });
    const store = createQueueStore();
    let value;
    const stop = store.subscribe((next) => (value = next));
    cleanups.push(stop);
    await flush();
    expect(reads.length).toBe(1);
    for (let i = 0; i < 20; i++) handlers.get("queue-changed")({});
    expect(reads.length).toBe(1);
    reads[0].resolve(nativeQueue(1));
    await flush();
    expect(value.queue).toBe(null);
    expect(reads.length).toBe(2);
    reads[1].resolve(nativeQueue(2));
    await flush();
    expect(value.queue).toEqual(nativeQueue(2));
    const stateChanged = handlers.get("playback-state-changed");
    stateChanged({ payload: { current_track: { id: 2 } } });
    await flush();
    reads[2].resolve(nativeQueue(2));
    await flush();
    for (let i = 0; i < 20; i++)
        stateChanged({ payload: { current_track: { id: 2 }, volume: i / 20 } });
    expect(reads.length).toBe(3);
    stateChanged({ payload: { current_track: null } });
    await flush();
    expect(reads.length).toBe(4);
    stop();
    const snapshot = value;
    reads[3].resolve(nativeQueue(3));
    await flush();
    handlers.get("queue-changed")({});
    await flush();
    expect(reads.length).toBe(4);
    expect(value).toBe(snapshot);
    expect(stops.every((fn) => fn.mock.calls.length === 1)).toBe(true);
});

test("queue reports read failures and recovers on the next change, including after reopening", async () => {
    const handlers = new Map();
    listen.mockImplementation(async (event, handler) => {
        handlers.set(event, handler);
        return () => {};
    });
    invoke.mockRejectedValueOnce(Error("read failed"));
    const store = createQueueStore();
    let value;
    const stop = store.subscribe((next) => (value = next));
    await flush();
    expect(value.error).toContain("read failed");
    invoke.mockResolvedValue(nativeQueue(9));
    handlers.get("queue-changed")({});
    await flush();
    expect(value.queue).toEqual(nativeQueue(9));
    expect(value.error).toBe(null);
    stop();
    const stopAgain = store.subscribe((next) => (value = next));
    cleanups.push(stopAgain);
    expect(value.queue).toBe(null);
    await flush();
    expect(value.queue).toEqual(nativeQueue(9));
});

test("queue handles listener failures and never publishes a read rejected after disposal", async () => {
    listen.mockRejectedValue(Error("events unavailable"));
    const read = deferred();
    invoke.mockImplementation(() => read.promise);
    let value;
    const stop = createQueueStore().subscribe((next) => (value = next));
    await flush();
    expect(value.error).toContain("events unavailable");
    stop();
    const snapshot = value;
    read.reject(Error("late failure"));
    await flush();
    expect(value).toBe(snapshot);
});

test("queue pages and retained metadata stay bounded while traversing a large queue", async () => {
    listen.mockResolvedValue(() => {});
    invoke.mockImplementation(async (_, { offset = 82_300, limit }) => ({
        revision: 1,
        total: 100_000,
        offset,
        current_pos: 82_342,
        tracks: Array.from({ length: limit }, (_, i) => ({ id: offset + i })),
    }));
    const store = createQueueStore();
    let value;
    const stop = store.subscribe((next) => (value = next));
    cleanups.push(stop);
    await flush();
    expect(value.queue.current_pos).toBe(82_342);
    expect([...value.pages.keys()]).toEqual([82_300]);
    for (let i = 0; i < 30; i++) {
        store.setRange(i * 100, i * 100 + 25);
        await flush();
        expect(value.pages.size).toBeLessThanOrEqual(6);
        expect(value.pages.get(i * 100)[0].id).toBe(i * 100);
    }
    const reads = invoke.mock.calls.length;
    store.setRange(2905, 2915);
    await flush();
    expect(invoke.mock.calls.length).toBe(reads);
    expect(invoke.mock.calls.every(([, args]) => args.limit === 100)).toBe(
        true,
    );
    stop();
    store.setRange(700, 725);
    store.retry();
    await flush();
    expect(invoke.mock.calls.length).toBe(reads);
});

test("queue invalidates other cached pages on revision changes and retries failed ranges", async () => {
    listen.mockResolvedValue(() => {});
    let revision = 1;
    invoke.mockImplementation(async (_, { offset = 0 }) => ({
        revision,
        total: 1000,
        offset,
        current_pos: 0,
        tracks: Array.from({ length: 100 }, (_, i) => ({
            id: revision * 1000 + offset + i,
        })),
    }));
    const store = createQueueStore();
    let value;
    cleanups.push(store.subscribe((next) => (value = next)));
    await flush();
    store.setRange(100, 120);
    await flush();
    expect(value.pages.size).toBe(2);
    revision = 2;
    store.setRange(200, 220);
    await flush();
    expect([...value.pages.keys()]).toEqual([200]);
    expect(value.pages.get(200)[0].id).toBe(2200);
    invoke.mockRejectedValueOnce(Error("page unavailable"));
    store.setRange(300, 320);
    await flush();
    expect(value.error).toContain("page unavailable");
    const reads = invoke.mock.calls.length;
    store.setRange(300, 320);
    await flush();
    expect(invoke.mock.calls.length).toBe(reads);
    store.retry();
    await flush();
    expect(value.error).toBe(null);
    expect(value.pages.get(300)[0].id).toBe(2300);
});

test("queue drops invalidated page failures and reads the latest viewport after rapid scrolling", async () => {
    const handlers = new Map();
    listen.mockImplementation(async (event, handler) => {
        handlers.set(event, handler);
        return () => {};
    });
    const page = (offset = 0) => ({
        revision: 2,
        total: 2000,
        offset,
        current_pos: 0,
        tracks: Array.from({ length: 100 }, (_, i) => ({ id: offset + i })),
    });
    invoke.mockResolvedValue(page());
    const store = createQueueStore();
    let value;
    cleanups.push(store.subscribe((next) => (value = next)));
    await flush();
    const stale = deferred();
    invoke.mockImplementationOnce(() => stale.promise);
    store.setRange(100, 150);
    await flush();
    store.setRange(1500, 1525);
    handlers.get("queue-changed")({});
    stale.reject(Error("stale page failed"));
    invoke.mockImplementation(async (_, { offset }) => page(offset));
    await flush();
    expect(value.error).toBe(null);
    expect(value.pages.has(100)).toBe(false);
    expect(value.pages.get(1500)[0].id).toBe(1500);
    expect(invoke.mock.calls.at(-1)[1]).toEqual({ offset: 1500, limit: 100 });
});

test("successful queue pages do not hide failed pages that still need a retry", async () => {
    listen.mockResolvedValue(() => {});
    const page = (offset = 0) => ({
        revision: 1,
        total: 1000,
        offset,
        current_pos: 0,
        tracks: Array.from({ length: 100 }, (_, i) => ({ id: offset + i })),
    });
    invoke.mockImplementation(async (_, { offset }) => page(offset));
    const store = createQueueStore();
    let value;
    cleanups.push(store.subscribe((next) => (value = next)));
    await flush();
    invoke.mockRejectedValueOnce(Error("page unavailable"));
    // The viewport straddles two pages. The first fails and the second succeeds.
    store.setRange(150, 220);
    await flush();
    expect(value.pages.has(100)).toBe(false);
    expect(value.pages.has(200)).toBe(true);
    expect(value.error).toContain("page unavailable");
    store.setRange(300, 320);
    await flush();
    store.setRange(100, 120);
    await flush();
    expect(value.error).toContain("page unavailable");
    store.retry();
    await flush();
    expect(value.error).toBe(null);
    expect(value.pages.get(100)[0].id).toBe(100);
});

test("failed queue pages do not accumulate without bound across a long browsing session", async () => {
    listen.mockResolvedValue(() => {});
    invoke.mockResolvedValueOnce({
        revision: 1,
        total: 10000,
        offset: 0,
        current_pos: 0,
        tracks: Array.from({ length: 100 }, (_, id) => ({ id })),
    });
    invoke.mockRejectedValue(Error("offline"));
    const store = createQueueStore();
    cleanups.push(store.subscribe(() => {}));
    await flush();
    for (let i = 1; i <= 20; i++) {
        store.setRange(i * 100, i * 100 + 20);
        await flush();
    }
    const calls = invoke.mock.calls.length;
    // The oldest failure is no longer retained, so revisiting can retry it.
    store.setRange(100, 120);
    await flush();
    expect(invoke.mock.calls.length).toBe(calls + 1);
});

function observeFixture() {
    const observers = [];
    globalThis.IntersectionObserver = class {
        observe = mock(() => {});
        unobserve = mock(() => {});
        disconnect = mock(() => {});
        constructor(callback, options) {
            this.callback = callback;
            this.options = options;
            observers.push(this);
        }
    };
    return observers;
}

test("artwork shares an observer, loads only visible nodes, and releases unmounted placeholders", () => {
    const observers = observeFixture();
    const a = {},
        b = {},
        c = {};
    const loadA = mock(() => {}),
        loadB = mock(() => {}),
        loadC = mock(() => {});
    const first = onceVisible(a, loadA),
        second = onceVisible(b, () => {});
    second.update(loadB);
    expect(observers.length).toBe(1);
    const observer = observers[0];
    expect(observer.options).toEqual({ rootMargin: "300px" });
    observer.callback([{ target: a, isIntersecting: false }]);
    expect(loadA).not.toHaveBeenCalled();
    first.destroy();
    observer.callback([
        { target: a, isIntersecting: true },
        { target: b, isIntersecting: true },
    ]);
    expect(loadA).not.toHaveBeenCalled();
    expect(loadB).toHaveBeenCalledTimes(1);
    expect(observer.disconnect).toHaveBeenCalledTimes(1);
    second.update(() => {
        throw Error("already shown");
    });
    const third = onceVisible(c, loadC);
    expect(observers.length).toBe(2);
    third.destroy();
    second.destroy();
    expect(loadC).not.toHaveBeenCalled();
});

test("artwork falls back to immediate loading without an observer", () => {
    delete globalThis.IntersectionObserver;
    const load = mock(() => {});
    onceVisible({}, load);
    expect(load).toHaveBeenCalledTimes(1);
});

test("lazy sentinel re-arms after rendering and cannot revive after destruction", async () => {
    const observers = observeFixture();
    const callback = mock(() => {}),
        next = mock(() => {}),
        node = {};
    const action = intersect(node, callback),
        observer = observers[0];
    await observer.callback([{ isIntersecting: false }]);
    expect(callback).not.toHaveBeenCalled();
    const frame = observer.callback([{ isIntersecting: true }]);
    observer.callback([{ isIntersecting: true }]);
    expect(callback).toHaveBeenCalledTimes(1);
    await frame;
    expect(observer.observe).toHaveBeenCalledTimes(2);
    action.update(next);
    const last = observer.callback([{ isIntersecting: true }]);
    action.destroy();
    await last;
    await observer.callback([{ isIntersecting: true }]);
    expect(next).toHaveBeenCalledTimes(1);
    expect(observer.observe).toHaveBeenCalledTimes(2);
    expect(observer.disconnect).toHaveBeenCalledTimes(1);
});
