// @ts-nocheck
import { expect, test } from "bun:test";
import {
    existsSync,
    mkdirSync,
    mkdtempSync,
    readFileSync,
    rmSync,
    statSync,
    utimesSync,
    writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { publishStaticAssets } from "../scripts/lib/static-assets.mjs";

function fixture() {
    const root = mkdtempSync(join(tmpdir(), "sparkle-static-assets-"));
    const source = join(root, "staged");
    const destination = join(root, "build");
    mkdirSync(source);
    mkdirSync(destination);
    return { root, source, destination };
}

test("identical assets retain timestamps while changed and new bytes are published", () => {
    const { root, source, destination } = fixture();
    try {
        writeFileSync(join(source, "unchanged.js"), "same bytes");
        writeFileSync(join(destination, "unchanged.js"), "same bytes");
        writeFileSync(join(source, "changed.js"), "new bytes");
        writeFileSync(join(destination, "changed.js"), "old bytes");
        writeFileSync(join(source, "new.js"), "new file");
        const old = new Date("2000-01-01T00:00:00Z");
        utimesSync(join(destination, "unchanged.js"), old, old);
        utimesSync(join(destination, "changed.js"), old, old);

        expect(publishStaticAssets(source, destination)).toEqual({
            written: 2,
            removed: 0,
            reused: 1,
        });
        expect(statSync(join(destination, "unchanged.js")).mtimeMs).toBe(
            old.getTime(),
        );
        expect(
            statSync(join(destination, "changed.js")).mtimeMs,
        ).toBeGreaterThan(old.getTime());
        expect(readFileSync(join(destination, "changed.js"), "utf8")).toBe(
            "new bytes",
        );
        expect(readFileSync(join(destination, "new.js"), "utf8")).toBe(
            "new file",
        );
        const timestamp = statSync(join(destination, "changed.js")).mtimeMs;
        const directoryTimestamp = statSync(destination).mtimeMs;
        expect(publishStaticAssets(source, destination)).toEqual({
            written: 0,
            removed: 0,
            reused: 3,
        });
        expect(statSync(join(destination, "changed.js")).mtimeMs).toBe(
            timestamp,
        );
        expect(statSync(destination).mtimeMs).toBe(directoryTimestamp);
    } finally {
        rmSync(root, { recursive: true, force: true });
    }
});

test("a clean build publishes nested assets into a new destination", () => {
    const { root, source } = fixture();
    const destination = join(root, "fresh");
    try {
        mkdirSync(join(source, "_app/chunks"), { recursive: true });
        writeFileSync(join(source, "index.html"), "entry point");
        writeFileSync(join(source, "_app/chunks/main.js"), "client code");
        expect(publishStaticAssets(source, destination)).toEqual({
            written: 2,
            removed: 0,
            reused: 0,
        });
        expect(
            readFileSync(join(destination, "_app/chunks/main.js"), "utf8"),
        ).toBe("client code");
    } finally {
        rmSync(root, { recursive: true, force: true });
    }
});

test("removed files and replaced directories cannot leave stale packaged assets", () => {
    const { root, source, destination } = fixture();
    try {
        mkdirSync(join(destination, "old/nested"), { recursive: true });
        writeFileSync(join(destination, "old/nested/chunk.js"), "obsolete");
        writeFileSync(join(destination, "becomes-directory"), "obsolete file");
        mkdirSync(join(source, "becomes-directory"));
        writeFileSync(
            join(source, "becomes-directory/chunk.js"),
            "new nested asset",
        );
        writeFileSync(join(source, "old"), "directory becomes a file");
        writeFileSync(join(root, "outside.txt"), "untouched");
        expect(publishStaticAssets(source, destination)).toEqual({
            written: 2,
            removed: 2,
            reused: 0,
        });
        expect(readFileSync(join(destination, "old"), "utf8")).toBe(
            "directory becomes a file",
        );
        expect(
            readFileSync(
                join(destination, "becomes-directory/chunk.js"),
                "utf8",
            ),
        ).toBe("new nested asset");
        expect(readFileSync(join(root, "outside.txt"), "utf8")).toBe(
            "untouched",
        );
    } finally {
        rmSync(root, { recursive: true, force: true });
    }
});

test("missing output or overlapping paths fail before modifying existing assets", () => {
    const { root, source, destination } = fixture();
    try {
        writeFileSync(join(destination, "index.html"), "existing");
        expect(() =>
            publishStaticAssets(join(root, "missing"), destination),
        ).toThrow("missing");
        expect(() => publishStaticAssets(source, source)).toThrow("overlap");
        expect(() => publishStaticAssets(root, source)).toThrow("overlap");
        expect(() => publishStaticAssets(source, root)).toThrow("overlap");
        expect(readFileSync(join(destination, "index.html"), "utf8")).toBe(
            "existing",
        );
        expect(existsSync(join(root, "missing"))).toBe(false);
    } finally {
        rmSync(root, { recursive: true, force: true });
    }
});
