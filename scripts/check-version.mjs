import { readFile } from "node:fs/promises";

const packageJson = JSON.parse(await readFile("package.json", "utf8"));
const tauriConfig = JSON.parse(
    await readFile("src-tauri/tauri.conf.json", "utf8"),
);
const cargoToml = await readFile("src-tauri/Cargo.toml", "utf8");
const cargoVersion = cargoToml.match(
    /^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m,
)?.[1];

const versions = {
    "package.json": packageJson.version,
    "src-tauri/Cargo.toml": cargoVersion,
    "src-tauri/tauri.conf.json": tauriConfig.version,
};
const uniqueVersions = new Set(Object.values(versions));

if (!cargoVersion || uniqueVersions.size !== 1) {
    console.error("Version mismatch:", versions);
    process.exit(1);
}

const version = packageJson.version;
if (
    process.env.GITHUB_REF_TYPE === "tag" &&
    process.env.GITHUB_REF_NAME !== `v${version}`
) {
    console.error(
        `Release tag must be v${version}; received ${process.env.GITHUB_REF_NAME}.`,
    );
    process.exit(1);
}

console.log(`Sparkle version ${version} is consistent.`);
