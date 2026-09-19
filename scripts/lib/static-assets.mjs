import {
    existsSync,
    lstatSync,
    mkdirSync,
    readdirSync,
    readFileSync,
    rmdirSync,
    unlinkSync,
    writeFileSync,
} from "node:fs";
import { join, resolve, sep } from "node:path";

/** @param {string} root */
function readTree(root) {
    /** @type {Map<string, Buffer>} */
    const files = new Map();
    /** @type {string[]} */
    const directories = [];
    if (!existsSync(root)) return { files, directories };
    if (lstatSync(root).isSymbolicLink())
        throw new Error(`Asset directory must not be a symbolic link: ${root}`);
    /** @param {string} relative */
    function visit(relative) {
        for (const entry of readdirSync(join(root, relative), {
            withFileTypes: true,
        })) {
            const path = join(relative, entry.name);
            if (entry.isDirectory()) {
                directories.push(path);
                visit(path);
            } else if (entry.isFile()) {
                files.set(path, readFileSync(join(root, path)));
            } else {
                throw new Error(`Unsupported asset type: ${join(root, path)}`);
            }
        }
    }
    visit("");
    return { files, directories };
}

/**
 * Publish a complete generated asset tree, retaining unchanged files and mtimes.
 * Cargo tracks the embedded frontend files, so rewriting identical bytes would
 * needlessly recompile and link the native app. Every build still runs Vite.
 * @param {string} source
 * @param {string} destination
 */
export function publishStaticAssets(source, destination) {
    source = resolve(source);
    destination = resolve(destination);
    // These are separate generated trees, never parent/child directories.
    const from = source.toLowerCase();
    const to = destination.toLowerCase();
    if (from === to || from.startsWith(to + sep) || to.startsWith(from + sep)) {
        throw new Error("Asset source and destination must not overlap.");
    }
    if (!existsSync(source)) throw new Error("Generated assets are missing.");
    const next = readTree(source);
    const previous = readTree(destination);
    let written = 0;
    let removed = 0;
    let reused = 0;

    // Remove stale outputs first, including file/directory replacements. Walking
    // validated trees avoids recursive deletion and never follows symlinks.
    for (const path of previous.files.keys()) {
        if (!next.files.has(path)) {
            unlinkSync(join(destination, path));
            removed++;
        }
    }
    const nextDirectories = new Set(next.directories);
    for (const path of previous.directories.reverse()) {
        if (!nextDirectories.has(path)) rmdirSync(join(destination, path));
    }
    mkdirSync(destination, { recursive: true });
    for (const path of next.directories)
        mkdirSync(join(destination, path), { recursive: true });
    for (const [path, bytes] of next.files) {
        if (previous.files.get(path)?.equals(bytes)) {
            reused++;
        } else {
            writeFileSync(join(destination, path), bytes);
            written++;
        }
    }
    return { written, removed, reused };
}
