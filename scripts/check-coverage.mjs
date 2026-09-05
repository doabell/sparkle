import { readFileSync, readdirSync } from "node:fs";
import { resolve, relative } from "node:path";
import {
    parseLcov,
    summarize,
    checkThresholds,
    checkInventory,
} from "./lib/coverage.mjs";

const language = process.argv[2];
if (!["typescript", "rust"].includes(language))
    throw new Error("Usage: bun scripts/check-coverage.mjs typescript|rust");
const root = language === "typescript" ? "src" : "src-tauri/src";
const report =
    language === "typescript"
        ? "coverage/typescript/lcov.info"
        : "coverage/rust/lcov.info";
const extension = language === "typescript" ? ".ts" : ".rs";
const normalize = (path) => path.replaceAll("\\", "/");
const expected = readdirSync(root, { recursive: true })
    .map((path) => normalize(`${root}/${path}`))
    .filter(
        (path) =>
            path.endsWith(extension) &&
            !path.endsWith(".d.ts") &&
            !path.includes("/tests/") &&
            path !== "src-tauri/src/main.rs",
    );
const files = parseLcov(readFileSync(report, "utf8"))
    .map((file) => ({
        ...file,
        path: normalize(relative(resolve("."), resolve(file.path))),
    }))
    .filter((file) => expected.includes(file.path));
checkInventory(files, expected);
if (language === "rust") {
    for (const path of expected) {
        if (/^\s*mod\s+tests\s*\{/m.test(readFileSync(path, "utf8"))) {
            throw new Error(
                `${path}: move inline unit tests into a tests/ module so they do not inflate production coverage`,
            );
        }
    }
}
function reportGroup(label, group, thresholds) {
    const summary = summarize(group);
    console.log(
        `${label}: ${summary.linePercent.toFixed(2)}% lines (${summary.linesHit}/${summary.lines}), ${summary.functionPercent.toFixed(2)}% functions (${summary.functionsHit}/${summary.functions})`,
    );
    checkThresholds(group, thresholds);
}
if (language === "typescript") {
    reportGroup("TypeScript", files, {
        lines: 95,
        functions: 95,
        perFileLines: 80,
    });
} else {
    reportGroup("Rust overall", files, { lines: 45, functions: 35 });
    // Stable, offline library behavior has a higher gate than device/network
    // orchestration. These complete modules also remain in the overall gate.
    const core = files.filter(
        (file) =>
            /^src-tauri\/src\/(analytics|artwork_store|backup|cache|db|db_writer|models|normalizer|settings)\.rs$/.test(
                file.path,
            ) ||
            /^src-tauri\/src\/providers\/lyrics\/(mod|embedded|lrc)\.rs$/.test(
                file.path,
            ),
    );
    reportGroup("Rust core library", core, { lines: 85, functions: 65 });
}
