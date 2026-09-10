import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

/**
 * Record packages whose code is present in the shipped browser chunks.
 * @returns {import("vite").Plugin}
 */
export function frontendLicenses() {
    let root = "";
    let client = false;
    return {
        name: "sparkle-frontend-licenses",
        apply: "build",
        configResolved(config) {
            root = config.root;
            client = !config.build.ssr;
        },
        generateBundle(_options, bundle) {
            if (!client) return;
            const packages = new Map();
            for (const chunk of Object.values(bundle)) {
                if (chunk.type !== "chunk") continue;
                for (const module of Object.keys(chunk.modules)) {
                    const file = module.split("?")[0];
                    if (
                        !file
                            .replaceAll("\\", "/")
                            .includes("/node_modules/") ||
                        !existsSync(file)
                    )
                        continue;
                    for (let dir = dirname(file); ; dir = dirname(dir)) {
                        const manifest = join(dir, "package.json");
                        if (existsSync(manifest)) {
                            const pkg = JSON.parse(
                                readFileSync(manifest, "utf8"),
                            );
                            if (pkg.name && pkg.version) {
                                packages.set(pkg.name + "@" + pkg.version, {
                                    name: pkg.name,
                                    version: pkg.version,
                                    directory: dir,
                                });
                                break;
                            }
                        }
                        if (dirname(dir) === dir) break;
                    }
                }
            }
            const temporary = resolve(root, ".tmp");
            mkdirSync(temporary, { recursive: true });
            writeFileSync(
                join(temporary, "frontend-dependencies.json"),
                JSON.stringify(
                    [...packages.values()].sort((a, b) =>
                        a.name.localeCompare(b.name),
                    ),
                    null,
                    2,
                ) + "\n",
            );
        },
    };
}
