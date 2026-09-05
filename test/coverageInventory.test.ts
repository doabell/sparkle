// @ts-nocheck
import { test, expect } from "bun:test";
import { readdirSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

// Bun instruments loaded files only. Import EVERY production TS module so
// adding an untested module lowers coverage instead of hiding it from reports.
test("coverage includes every production TypeScript module", async () => {
    const files = readdirSync("src", { recursive: true }).filter(
        (file) => file.endsWith(".ts") && !file.endsWith(".d.ts"),
    );
    expect(files.length).toBeGreaterThan(0);
    for (const file of files) {
        await import(pathToFileURL(resolve("src", file)).href);
    }
});
