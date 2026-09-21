// @ts-nocheck
import { expect, test } from "bun:test";
import { resolve } from "node:path";
import { libraryTestExecutable } from "../scripts/cargo-artifacts.mjs";

const manifest = resolve("src-tauri/Cargo.toml");
const artifact = {
    reason: "compiler-artifact",
    manifest_path: manifest,
    target: { name: "sparkle_lib" },
    profile: { test: true },
    executable: resolve("src-tauri/target/release/deps/sparkle_lib-tests.exe"),
    fresh: false,
};
const output = (...messages) => messages.map(JSON.stringify).join("\r\n");

test.each([false, true])(
    "selects the current library test artifact (cached=%s)",
    (fresh) => {
        expect(
            libraryTestExecutable(
                "frontend output\n" +
                    output(
                        { ...artifact, profile: { test: false } },
                        {
                            ...artifact,
                            manifest_path: resolve("dependency/Cargo.toml"),
                        },
                        { ...artifact, target: { name: "sparkle" } },
                        { ...artifact, fresh },
                        { reason: "build-finished", success: true },
                    ),
                manifest,
            ),
        ).toBe(artifact.executable);
    },
);

test("deduplicates repeated messages for the same executable", () => {
    expect(libraryTestExecutable(output(artifact, artifact), manifest)).toBe(
        artifact.executable,
    );
});

test.each([
    "frontend only\n{}\nnull\n",
    output({ ...artifact, executable: null }),
    output(artifact, { ...artifact, executable: "different-tests.exe" }),
])(
    "fails closed when no unique library test executable is available",
    (messages) => {
        expect(() => libraryTestExecutable(messages, manifest)).toThrow(
            "Expected one sparkle_lib test executable",
        );
    },
);

test("packaging selects both workspace suites and rejects a missing core suite", () => {
    const coreManifest = resolve("src-tauri/core/Cargo.toml");
    const core = {
        ...artifact,
        manifest_path: coreManifest,
        target: { name: "sparkle_core" },
        executable: resolve(
            "src-tauri/target/release/deps/sparkle_core-tests.exe",
        ),
        fresh: true,
    };
    const messages = output(artifact, core);
    expect(libraryTestExecutable(messages, manifest)).toBe(artifact.executable);
    expect(libraryTestExecutable(messages, coreManifest, "sparkle_core")).toBe(
        core.executable,
    );
    expect(() =>
        libraryTestExecutable(output(artifact), coreManifest, "sparkle_core"),
    ).toThrow("Expected one sparkle_core test executable");
    expect(() =>
        libraryTestExecutable(
            output({ ...core, profile: { test: false } }),
            coreManifest,
            "sparkle_core",
        ),
    ).toThrow("Expected one sparkle_core test executable");
});
