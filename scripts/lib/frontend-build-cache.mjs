import { createHash } from "node:crypto";
import {
    existsSync,
    lstatSync,
    mkdirSync,
    readdirSync,
    readFileSync,
    renameSync,
    unlinkSync,
    writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";

const schema = 1;
const inputDirectories = ["src", "static", "scripts", "licenses"];
const dependencyCaches = new Set([".cache", ".vite", ".vite-temp"]);

/**
 * Hash names and bytes, including additions/deletions and empty directories.
 * Links are deliberately not followed: an unfamiliar installation gets a full build.
 * @param {import("node:crypto").Hash} hash
 * @param {string} path
 * @param {string} name
 * @param {boolean} [dependencies]
 */
function addTree(hash, path, name, dependencies = false) {
    hash.update(JSON.stringify(name));
    if (!existsSync(path)) {
        hash.update("missing");
        return;
    }
    const stat = lstatSync(path);
    if (stat.isSymbolicLink()) throw new Error("Linked frontend input");
    if (stat.isFile()) {
        hash.update(`file:${stat.size}:`);
        hash.update(readFileSync(path));
    } else if (stat.isDirectory()) {
        hash.update("directory");
        for (const entry of readdirSync(path).sort()) {
            if (
                dependencies &&
                (dependencyCaches.has(entry) || entry.startsWith(".old-"))
            )
                continue;
            addTree(hash, join(path, entry), `${name}/${entry}`, dependencies);
        }
    } else {
        throw new Error("Unsupported frontend input");
    }
}

/** @param {string} root */
function dependenciesDirectory(root) {
    for (let directory = root; ; directory = dirname(directory)) {
        const candidate = join(directory, "node_modules");
        if (existsSync(join(candidate, "vite/package.json"))) return candidate;
        if (dirname(directory) === directory)
            throw new Error("Frontend dependencies are missing");
    }
}

/**
 * Never persist environment values or source contents; only their digest.
 * @param {string} root
 * @param {NodeJS.ProcessEnv} environment
 * @param {string} runtime
 */
function inputFingerprint(root, environment, runtime) {
    const hash = createHash("sha256");
    hash.update(JSON.stringify({ schema, root, runtime }));
    hash.update(
        JSON.stringify(
            Object.entries(environment)
                .filter(([, value]) => value !== undefined)
                .sort(([a], [b]) => a.localeCompare(b)),
        ),
    );
    // Root files cover config, lockfiles, .env variants, and embedded notices.
    for (const entry of readdirSync(root, { withFileTypes: true }).sort(
        (a, b) => a.name.localeCompare(b.name),
    )) {
        if (
            (entry.isFile() || entry.isSymbolicLink()) &&
            entry.name !== ".prettiercache" &&
            !entry.name.endsWith(".tsbuildinfo")
        )
            addTree(hash, join(root, entry.name), entry.name);
    }
    for (const directory of inputDirectories)
        addTree(hash, join(root, directory), directory);
    // Include installed code too: a lockfile alone misses edited or replaced packages.
    addTree(hash, dependenciesDirectory(root), "node_modules", true);
    return hash.digest("hex");
}

/** @param {string} root */
function outputFingerprint(root) {
    if (
        !existsSync(join(root, "build/index.html")) ||
        !existsSync(join(root, ".tmp/frontend-dependencies.json")) ||
        !lstatSync(join(root, "build/index.html")).isFile() ||
        !lstatSync(join(root, ".tmp/frontend-dependencies.json")).isFile()
    )
        throw new Error("Generated frontend or dependency report is missing");
    const hash = createHash("sha256");
    addTree(hash, join(root, "build"), "build");
    addTree(
        hash,
        join(root, ".tmp/frontend-dependencies.json"),
        "frontend-dependencies.json",
    );
    return hash.digest("hex");
}

/**
 * @param {{root: string, build: () => void, force?: boolean,
 * environment?: NodeJS.ProcessEnv, runtime?: string}} options
 * @returns {{reused: boolean}}
 */
export function buildFrontend({
    root,
    build,
    force = false,
    environment = process.env,
    runtime = JSON.stringify({
        versions: process.versions,
        platform: process.platform,
        arch: process.arch,
    }),
}) {
    root = resolve(root);
    const cache = join(root, ".tmp/frontend-build-cache.json");
    let input;
    try {
        input = inputFingerprint(root, environment, runtime);
        if (!force) {
            const saved = JSON.parse(readFileSync(cache, "utf8"));
            if (
                saved.schema === schema &&
                saved.input === input &&
                saved.output === outputFingerprint(root)
            )
                return { reused: true };
        }
    } catch {
        // Missing/corrupt cache data or unsupported inputs must never skip Vite.
    }
    if (existsSync(cache)) unlinkSync(cache);
    build();
    // A successful process must also leave the assets required by packaging.
    const output = outputFingerprint(root);
    if (input) {
        try {
            if (input === inputFingerprint(root, environment, runtime)) {
                mkdirSync(dirname(cache), { recursive: true });
                const temporary = `${cache}.${process.pid}.tmp`;
                writeFileSync(
                    temporary,
                    JSON.stringify({ schema, input, output }) + "\n",
                );
                renameSync(temporary, cache);
            }
        } catch {
            // The build is usable even if caching is unavailable or inputs changed.
        }
    }
    return { reused: false };
}
