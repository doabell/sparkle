import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
    existsSync,
    mkdtempSync,
    readFileSync,
    readdirSync,
    rmSync,
    rmdirSync,
    writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = join(root, "licenses/dependencies.json");
const normalize = (text) => text.replace(/\r\n?/g, "\n").trim() + "\n";
const read = (path) => normalize(readFileSync(path, "utf8"));
const hash = (text) => createHash("sha256").update(text).digest("hex");
const inputs = [
    "bun.lock",
    "package.json",
    "src-tauri/Cargo.lock",
    "src-tauri/Cargo.toml",
    "src-tauri/patches/tauri-plugin-media/Cargo.toml",
    "src-tauri/patches/tauri-plugin-media/LICENSE-MIT",
    "licenses/about.toml",
    "scripts/generate-licenses.mjs",
    "scripts/lib/frontend-licenses.mjs",
    "vite.config.js",
];
const fingerprint = hash(
    inputs.map((path) => path + "\n" + read(join(root, path))).join("\n"),
);

if (process.argv.includes("--check")) {
    if (
        !existsSync(output) ||
        JSON.parse(read(output)).fingerprint !== fingerprint
    ) {
        throw new Error(
            "Dependency licenses are stale. Run bun run licenses:generate and commit licenses/dependencies.json.",
        );
    }
    if (process.argv.includes("--built")) {
        const shipped = JSON.parse(
            read(join(root, ".tmp/frontend-dependencies.json")),
        );
        const covered = new Set(
            JSON.parse(read(output)).licenses.flatMap((group) =>
                group.components
                    .filter((pkg) => pkg.ecosystem === "JavaScript")
                    .map((pkg) => pkg.name + "@" + pkg.version),
            ),
        );
        for (const pkg of shipped) {
            if (!covered.has(pkg.name + "@" + pkg.version)) {
                throw new Error(
                    "Missing bundled dependency license: " +
                        pkg.name +
                        "@" +
                        pkg.version,
                );
            }
        }
    }
    console.log("Dependency license report matches the locked dependencies.");
    process.exit(0);
}

const temporary = mkdtempSync(join(tmpdir(), "sparkle-licenses-"));
const rawReport = join(temporary, "rust.json");
const about = spawnSync(
    process.env.CARGO_ABOUT || "cargo-about",
    [
        "generate",
        "--manifest-path",
        "src-tauri/Cargo.toml",
        "--config",
        "licenses/about.toml",
        "--locked",
        "--fail",
        "--format",
        "json",
        "--output-file",
        rawReport,
    ],
    {
        cwd: root,
        encoding: "utf8",
        maxBuffer: 32 * 1024 * 1024,
        windowsHide: true,
    },
);
if (about.error || about.status !== 0) {
    throw new Error(
        about.error?.message || about.stderr || "cargo-about failed",
    );
}
const rust = JSON.parse(read(rawReport));
rmSync(rawReport);
rmdirSync(temporary);
const groups = new Map();
const rustLicenses = new Map();

function add(license, text, component) {
    if (!text?.trim()) throw new Error("Empty license for " + component.name);
    text = normalize(text);
    const key = hash(license + "\n" + text);
    if (!groups.has(key))
        groups.set(key, { key, license, text, components: [] });
    const list = groups.get(key).components;
    if (
        !list.some(
            (item) =>
                item.name === component.name &&
                item.version === component.version &&
                item.ecosystem === component.ecosystem,
        )
    ) {
        list.push(component);
    }
}

for (const license of rust.licenses) {
    for (const { crate: pkg } of license.used_by) {
        if (pkg.name === "sparkle") continue;
        const component = {
            name: pkg.name,
            version: pkg.version,
            ecosystem: "Rust",
            source:
                pkg.name === "tauri-plugin-media"
                    ? "https://github.com/doabell/sparkle/tree/main/src-tauri/patches/tauri-plugin-media"
                    : "https://crates.io/crates/" +
                      pkg.name +
                      "/" +
                      pkg.version,
        };
        add(license.id, license.text, component);
        const key = pkg.name;
        if (!rustLicenses.has(key)) rustLicenses.set(key, []);
        rustLicenses.get(key).push({ license: license.id, text: license.text });
    }
}

// Preserve separate upstream NOTICE files as well as the license text.
for (const { package: pkg } of rust.crates) {
    if (pkg.name === "sparkle") continue;
    const dir = dirname(pkg.manifest_path);
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
        if (entry.isFile() && /^NOTICE(?:[._-]|$)/i.test(entry.name)) {
            add("Notice", read(join(dir, entry.name)), {
                name: pkg.name,
                version: pkg.version,
                ecosystem: "Rust",
                source:
                    "https://crates.io/crates/" + pkg.name + "/" + pkg.version,
            });
        }
    }
}

// A temporary empty report bootstraps a clean checkout. The completed report
// replaces it before the release build; normal builds use the committed report.
if (!existsSync(output))
    writeFileSync(output, JSON.stringify({ fingerprint: "", licenses: [] }));
const build = spawnSync("bun", ["run", "build"], {
    cwd: root,
    stdio: "inherit",
    windowsHide: true,
});
if (build.error || build.status !== 0)
    throw new Error(
        "Build the frontend before collecting dependency licenses.",
    );
const queue = JSON.parse(read(join(root, ".tmp/frontend-dependencies.json")));
const seen = new Set();
const nativeLicenseForNpm = {
    "@tauri-apps/plugin-dialog": "tauri-plugin-dialog",
    "@tauri-apps/plugin-opener": "tauri-plugin-opener",
    "tauri-plugin-media-api": "tauri-plugin-media",
};

while (queue.length) {
    const { directory: dir } = queue.shift();
    if (seen.has(dir)) continue;
    seen.add(dir);
    const pkg = JSON.parse(read(join(dir, "package.json")));
    const component = {
        name: pkg.name,
        version: pkg.version,
        ecosystem: "JavaScript",
        source:
            "https://www.npmjs.com/package/" + pkg.name + "/v/" + pkg.version,
    };
    const files = readdirSync(dir, { withFileTypes: true })
        .filter(
            (entry) =>
                entry.isFile() &&
                /^(?:LICENSE|LICENCE|COPYING|NOTICE)(?:[._-]|$)/i.test(
                    entry.name,
                ),
        )
        .map((entry) => entry.name)
        .sort();
    const fullLicenses = files.filter(
        (name) =>
            !name.toLowerCase().endsWith(".spdx") && !/^NOTICE/i.test(name),
    );
    for (const name of files) {
        add(
            /^NOTICE/i.test(name) || name.endsWith(".spdx")
                ? "Notice"
                : pkg.license,
            read(join(dir, name)),
            component,
        );
    }
    if (!fullLicenses.length) {
        // These npm distributions omit their full license files. Their matching
        // native packages are from the same project and carry the full texts.
        const native = nativeLicenseForNpm[pkg.name];
        const licenses = rustLicenses.get(native);
        if (!licenses?.length)
            throw new Error(
                "No complete license text for " + pkg.name + " " + pkg.version,
            );
        for (const license of licenses)
            add(license.license, license.text, component);
    }
}

const compare = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
const licenses = [...groups.values()];
for (const group of licenses) {
    group.components.sort(
        (a, b) =>
            compare(a.name, b.name) ||
            compare(a.version, b.version) ||
            compare(a.ecosystem, b.ecosystem),
    );
}
licenses.sort(
    (a, b) =>
        compare(a.license, b.license) ||
        compare(a.components[0].name, b.components[0].name) ||
        compare(a.key, b.key),
);
writeFileSync(
    output,
    JSON.stringify({ fingerprint, licenses }, null, 2) + "\n",
);
console.log(
    "Wrote " +
        licenses.length +
        " license/notice groups to licenses/dependencies.json.",
);
