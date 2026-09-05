import { mkdirSync } from "node:fs";
import { spawnSync } from "node:child_process";

mkdirSync("coverage/rust", { recursive: true });
for (const [command, args] of [
    [
        "cargo",
        [
            "llvm-cov",
            "--locked",
            "--manifest-path",
            "src-tauri/Cargo.toml",
            "--lib",
            "--ignore-filename-regex",
            String.raw`[/\\]tests[/\\]`,
            "--lcov",
            "--output-path",
            "coverage/rust/lcov.info",
        ],
    ],
    [process.execPath, ["scripts/check-coverage.mjs", "rust"]],
]) {
    const result = spawnSync(command, args, { stdio: "inherit" });
    if (result.error) {
        console.error(result.error.message);
        process.exit(1);
    }
    if (result.status !== 0) process.exit(result.status ?? 1);
}
