import { spawnSync } from "node:child_process";
import {
    copyFileSync,
    existsSync,
    mkdirSync,
    mkdtempSync,
    readFileSync,
    readdirSync,
    writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const config = JSON.parse(
    readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"),
);
const tool = JSON.parse(
    readFileSync(join(root, ".config/dotnet-tools.json"), "utf8"),
).tools.vpk;
const cargo = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8");
const outputDir = join(root, ".tmp/releases");
if (process.platform !== "win32" || process.arch !== "x64") {
    throw new Error("Windows packaging requires Windows x64.");
}
if (!cargo.includes(`velopack = "=${tool.version}"`)) {
    throw new Error(
        "The Velopack Rust SDK and vpk tool must use the same pinned version.",
    );
}
if (existsSync(outputDir) && readdirSync(outputDir).length) {
    throw new Error(
        "Move or remove the previous .tmp/releases directory before packaging again.",
    );
}

function run(command, args) {
    const result = spawnSync(command, args, {
        cwd: root,
        stdio: "inherit",
        windowsHide: true,
    });
    if (result.error || result.status !== 0)
        throw new Error(
            result.error?.message || `${command} failed (${result.status}).`,
        );
}

run("bun", ["run", "version:check"]);
run("bun", ["run", "licenses:check"]);
run("bun", [
    "run",
    "tauri",
    "build",
    "--no-bundle",
    "--target",
    "x86_64-pc-windows-msvc",
    "--",
    "--locked",
]);
run("bun", ["run", "licenses:check", "--built"]);

const temporary = join(root, ".tmp");
mkdirSync(temporary, { recursive: true });
const stage = mkdtempSync(join(temporary, "velopack-stage-"));
const binaryDir = join(root, "src-tauri/target/x86_64-pc-windows-msvc/release");
copyFileSync(join(binaryDir, "sparkle.exe"), join(stage, "sparkle.exe"));
for (const name of readdirSync(binaryDir)) {
    if (name.toLowerCase().endsWith(".dll") && name !== "sparkle_lib.dll") {
        copyFileSync(join(binaryDir, name), join(stage, name));
    }
}
for (const name of ["LICENSE", "THIRD_PARTY_NOTICES.md"]) {
    copyFileSync(join(root, name), join(stage, name));
}
mkdirSync(join(stage, "licenses"));
for (const name of ["Apache-2.0.txt", "dependencies.json"]) {
    copyFileSync(join(root, "licenses", name), join(stage, "licenses", name));
}

const changelog = readFileSync(join(root, "CHANGELOG.md"), "utf8");
const releaseSection = changelog
    .split(/^## /m)
    .find((section) => section.startsWith(config.version + " "));
if (!releaseSection)
    throw new Error(`CHANGELOG.md needs a section for ${config.version}.`);
const notes = join(stage, "release-notes.md");
writeFileSync(
    notes,
    "## " +
        releaseSection.trim() +
        "\n\nWindows x64 setup. Creates a Start menu shortcut.\n\nUpgrading from an older MSI: close Sparkle, uninstall the MSI, then run this setup. Your library data stays in the same app-data directory. In-app updates require this setup version.\n",
);

const args = [
    "pack",
    "--packId",
    config.identifier,
    "--packVersion",
    config.version,
    "--packDir",
    stage,
    "--mainExe",
    "sparkle.exe",
    "--packTitle",
    config.productName,
    "--packAuthors",
    config.bundle.publisher,
    "--channel",
    "win-x64",
    "--runtime",
    "win-x64",
    "--framework",
    "webview2",
    "--shortcuts",
    "StartMenuRoot",
    "--aumid",
    config.identifier,
    "--noPortable",
    "true",
    "--delta",
    "None",
    "--icon",
    join(root, "src-tauri/icons/icon.ico"),
    "--releaseNotes",
    notes,
    "--outputDir",
    outputDir,
];
if (process.env.VPK) run(process.env.VPK, args);
else run("dotnet", ["tool", "run", "vpk", "--", ...args]);
run("powershell.exe", [
    "-NoProfile",
    "-File",
    "scripts/verify-windows-package.ps1",
]);
