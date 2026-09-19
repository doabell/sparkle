import { invoke } from "@tauri-apps/api/core";

export type LogLevel = "error" | "warn" | "info" | "debug" | "trace";

function messageFor(error: unknown): string | null {
    if (error === undefined) return null;
    const message =
        error instanceof Error
            ? error.message
            : typeof error === "string"
              ? error
              : "Unknown error";
    // Never serialize arbitrary objects: they can contain settings or credentials.
    return Array.from(message).slice(0, 2048).join("");
}

async function write(
    level: LogLevel,
    scope: string,
    event: string,
    error?: unknown,
): Promise<void> {
    try {
        await invoke("log_frontend", {
            level,
            scope,
            event,
            message: messageFor(error),
        });
    } catch {
        // No retries or recursive logging if the bridge itself is unavailable.
        console.warn(`[sparkle::frontend] ${scope}/${event} (log unavailable)`);
    }
}

export const logger = {
    error: (scope: string, event: string, error?: unknown) =>
        write("error", scope, event, error),
    warn: (scope: string, event: string, error?: unknown) =>
        write("warn", scope, event, error),
    info: (scope: string, event: string) => write("info", scope, event),
    debug: (scope: string, event: string, error?: unknown) =>
        write("debug", scope, event, error),
    trace: (scope: string, event: string) => write("trace", scope, event),
};

export function installErrorLogging(target: Window): () => void {
    const onError = (event: ErrorEvent) => {
        void logger.error(
            "runtime",
            "uncaught_error",
            event.error ?? event.message,
        );
    };
    const onRejection = (event: PromiseRejectionEvent) => {
        void logger.error("runtime", "unhandled_rejection", event.reason);
    };
    target.addEventListener("error", onError);
    target.addEventListener("unhandledrejection", onRejection);
    return () => {
        target.removeEventListener("error", onError);
        target.removeEventListener("unhandledrejection", onRejection);
    };
}
