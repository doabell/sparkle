// @ts-nocheck
import { afterEach, expect, spyOn, test } from "bun:test";
import { installErrorLogging, logger } from "../src/lib/logger";
import { logFrontend } from "./support/platform";

afterEach(() => logFrontend.mockReset());

test("frontend levels and failures reach the native logging command", async () => {
    for (const level of ["error", "warn", "info", "debug", "trace"]) {
        await logger[level]("playback", "ready");
        expect(logFrontend).toHaveBeenLastCalledWith({
            level,
            scope: "playback",
            event: "ready",
            message: null,
        });
    }
    await logger.error("playback", "load_failed", new Error("decoder failed"));
    expect(logFrontend.mock.calls.at(-1)[0].message).toBe("decoder failed");
    await logger.warn("media", "unavailable", "closed");
    expect(logFrontend.mock.calls.at(-1)[0].message).toBe("closed");
    await logger.debug("provider", "failed", { token: "secret" });
    expect(logFrontend.mock.calls.at(-1)[0].message).toBe("Unknown error");
    await logger.error("runtime", "failed", "🎵".repeat(3000));
    expect(logFrontend.mock.calls.at(-1)[0].message).toBe("🎵".repeat(2048));
});

test("an unavailable bridge does not reject, retry, or expose the error payload", async () => {
    logFrontend.mockRejectedValue(new Error("bridge unavailable"));
    const warn = spyOn(console, "warn").mockImplementation(() => {});
    try {
        await logger.error("runtime", "failed", "private detail");
        expect(logFrontend).toHaveBeenCalledTimes(1);
        expect(warn).toHaveBeenCalledTimes(1);
        expect(warn.mock.calls[0][0]).not.toContain("private detail");
    } finally {
        warn.mockRestore();
    }
});

test("runtime error listeners capture failures and clean up on unmount", () => {
    const target = new EventTarget();
    const cleanup = installErrorLogging(target);
    const send = (type, properties) =>
        target.dispatchEvent(Object.assign(new Event(type), properties));
    send("error", { error: new Error("uncaught") });
    expect(logFrontend.mock.calls.at(-1)[0]).toMatchObject({
        event: "uncaught_error",
        message: "uncaught",
    });
    send("error", { message: "script failed" });
    expect(logFrontend.mock.calls.at(-1)[0].message).toBe("script failed");
    send("unhandledrejection", { reason: "rejected" });
    expect(logFrontend.mock.calls.at(-1)[0]).toMatchObject({
        event: "unhandled_rejection",
        message: "rejected",
    });
    cleanup();
    send("error", { message: "after cleanup" });
    send("unhandledrejection", { reason: "after cleanup" });
    expect(logFrontend).toHaveBeenCalledTimes(3);
});
