// @ts-nocheck
import { afterEach, expect, test } from "bun:test";
import { get } from "svelte/store";
import { createLibraryScanStore } from "../src/lib/stores/libraryScan";
import { toasts } from "../src/lib/stores/toast";
import { invoke, listen } from "./support/platform";

afterEach(() => {
    invoke.mockReset();
    listen.mockReset();
    toasts.clearToasts();
});

const idle = {
    revision: 0,
    running: false,
    progress: null,
    result: null,
    error: null,
};
const result = { scanned: 10, added: 2, updated: 0, removed: 0, errors: 0 };
const progress = {
    phase: "scanning",
    scanned: 4,
    total: 10,
    added: 2,
    updated: 0,
    removed: 0,
    errors: 0,
};
const flush = async () => {
    for (let i = 0; i < 15; i++) await Promise.resolve();
};
function deferred() {
    let resolve, reject;
    const promise = new Promise((yes, no) => {
        resolve = yes;
        reject = no;
    });
    return { promise, resolve, reject };
}

test("navigation keeps the same scan, progress and result without a second invocation", async () => {
    const native = deferred();
    let handler;
    let cleanupCalls = 0;
    let snapshot = idle;
    listen.mockImplementation(async (_, callback) => {
        handler = callback;
        return () => cleanupCalls++;
    });
    invoke.mockImplementation(async (command) =>
        command === "scan_library" ? native.promise : snapshot,
    );
    const scan = createLibraryScanStore();
    await scan.start();
    expect(invoke).not.toHaveBeenCalled();
    const disconnect = scan.connect();
    await flush();
    const leavePage = scan.subscribe(() => {});
    const request = scan.start();
    expect(get(scan).starting).toBe(true);
    handler({ payload: { ...idle, revision: 1, running: true } });
    handler({ payload: { ...idle, revision: 2, running: true, progress } });
    leavePage();
    const returnToPage = scan.subscribe(() => {});
    expect(get(scan).progress.scanned).toBe(4);
    expect(get(scan).running).toBe(true);
    expect(scan.start(true)).toBe(request);
    expect(
        invoke.mock.calls.filter(([command]) => command === "scan_library"),
    ).toHaveLength(1);
    expect(listen).toHaveBeenCalledTimes(1);
    expect(cleanupCalls).toBe(0);
    snapshot = { ...idle, revision: 3, result };
    handler({ payload: snapshot });
    native.resolve(result);
    await request;
    expect(get(scan).starting).toBe(false);
    expect(get(scan).running).toBe(false);
    expect(get(scan).result).toEqual(result);
    expect(get(toasts).map((toast) => toast.message)).toEqual([
        "Scan complete",
    ]);
    returnToPage();
    expect(get(scan).result).toEqual(result);
    disconnect();
    expect(cleanupCalls).toBe(1);
});

test("startup scans survive delayed snapshots and out-of-order events", async () => {
    const initial = deferred();
    let handler;
    listen.mockImplementation(async (_, callback) => {
        handler = callback;
        return () => {};
    });
    invoke.mockReturnValue(initial.promise);
    const scan = createLibraryScanStore();
    const disconnect = scan.connect();
    await flush();
    handler({ payload: { ...idle, revision: 5, running: true, progress } });
    initial.resolve(idle);
    await flush();
    handler({
        payload: {
            ...idle,
            revision: 4,
            running: true,
            progress: { ...progress, scanned: 1 },
        },
    });
    await scan.start(true);
    expect(get(scan).progress.scanned).toBe(4);
    expect(invoke).toHaveBeenCalledTimes(1);
    handler({
        payload: { ...idle, revision: 6, result: { ...result, errors: 2 } },
    });
    expect(get(toasts).at(-1).message).toBe("Scan complete with 2 errors");
    handler({
        payload: { ...idle, revision: 7, result: { ...result, errors: 1 } },
    });
    expect(get(toasts).at(-1).message).toBe("Scan complete with 1 error");
    disconnect();
});

test("a native failure clears pending state and allows an explicit retry", async () => {
    let handler;
    let snapshot = idle;
    const native = deferred();
    listen.mockImplementation(async (_, callback) => {
        handler = callback;
        return () => {};
    });
    invoke.mockImplementation(async (command) =>
        command === "scan_library" ? native.promise : snapshot,
    );
    const scan = createLibraryScanStore();
    const disconnect = scan.connect();
    await flush();
    const request = scan.start();
    snapshot = { ...idle, revision: 2, error: "folder unavailable" };
    handler({ payload: snapshot });
    native.reject("folder unavailable");
    await request;
    expect(get(scan).running).toBe(false);
    expect(get(scan).starting).toBe(false);
    expect(get(toasts)).toHaveLength(1);
    invoke.mockImplementation(async (command) => {
        if (command === "scan_library") {
            snapshot = { ...idle, revision: 4, result };
            return result;
        }
        return snapshot;
    });
    await scan.start(true);
    expect(invoke).toHaveBeenCalledWith("scan_library", { force: true });
    expect(get(scan).result).toEqual(result);
    expect(get(scan).error).toBeNull();
    disconnect();
});

test("rejected duplicate requests recover the active native scan instead of resetting it", async () => {
    let snapshot = idle;
    listen.mockResolvedValue(() => {});
    invoke.mockImplementation(async (command) => {
        if (command === "scan_library") {
            snapshot = { ...idle, revision: 3, running: true, progress };
            throw "A library scan is already running";
        }
        return snapshot;
    });
    const scan = createLibraryScanStore();
    const disconnect = scan.connect();
    await flush();
    await scan.start();
    expect(get(scan).running).toBe(true);
    expect(get(scan).progress).toEqual(progress);
    expect(get(toasts).at(-1).message).toBe(
        "A library scan is already running",
    );
    disconnect();
});

test("disposing during listener registration cleans up and ignores late events", async () => {
    const registration = deferred();
    let handler;
    let cleanups = 0;
    listen.mockImplementation((_, callback) => {
        handler = callback;
        return registration.promise;
    });
    const scan = createLibraryScanStore();
    const disconnect = scan.connect();
    disconnect();
    registration.resolve(() => cleanups++);
    await flush();
    handler({ payload: { ...idle, revision: 1, running: true } });
    expect(cleanups).toBe(1);
    expect(invoke).not.toHaveBeenCalled();
    expect(get(scan).ready).toBe(false);
});

test("disposing during the initial snapshot ignores both late success and failure", async () => {
    for (const fails of [false, true]) {
        const initial = deferred();
        listen.mockResolvedValue(() => {});
        invoke.mockReturnValue(initial.promise);
        const scan = createLibraryScanStore();
        const disconnect = scan.connect();
        await flush();
        disconnect();
        if (fails) initial.reject("disconnected");
        else initial.resolve({ ...idle, revision: 5, running: true });
        await flush();
        expect(get(scan).ready).toBe(false);
        expect(get(scan).error).toBeNull();
    }
});

test("connection and command failures are visible and never leave a pending scan", async () => {
    listen.mockRejectedValue("listener unavailable");
    const scan = createLibraryScanStore();
    const failedConnection = scan.connect();
    await flush();
    expect(get(scan).error).toBe("listener unavailable");
    failedConnection();
    listen.mockResolvedValue(() => {});
    invoke.mockResolvedValue(idle);
    const disconnect = scan.connect();
    await flush();
    expect(get(scan).ready).toBe(true);
    invoke.mockRejectedValue("native unavailable");
    await scan.start();
    expect(get(scan).starting).toBe(false);
    expect(get(scan).error).toBe("native unavailable");
    expect(get(toasts).at(-1).message).toBe("native unavailable");
    disconnect();
});
