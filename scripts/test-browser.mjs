// Run real Svelte components in an installed Chromium browser. Native IPC is
// mocked by the fixture; a disposable browser profile keeps user data isolated.
// Usage: bun scripts/test-browser.mjs
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, extname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { compile } from "svelte/compiler";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixture = join(root, "test/browser");
const browser =
    process.env.SPARKLE_TEST_BROWSER ||
    [
        ...[
            process.env["ProgramFiles(x86)"],
            process.env.ProgramFiles,
            process.env.LOCALAPPDATA,
        ]
            .filter(Boolean)
            .flatMap((base) => [
                join(base, "Microsoft/Edge/Application/msedge.exe"),
                join(base, "Google/Chrome/Application/chrome.exe"),
            ]),
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ...[
            "chromium",
            "chromium-browser",
            "google-chrome",
            "microsoft-edge",
        ].map((name) => Bun.which(name)),
    ].find((path) => path && existsSync(path));
if (!browser)
    throw Error(
        "Install Chromium, Chrome or Edge, or set SPARKLE_TEST_BROWSER to its executable.",
    );

const temporaryRoot = resolve(tmpdir());
const directory = resolve(
    await mkdtemp(join(temporaryRoot, "sparkle-browser-tests-")),
);
if (
    dirname(directory) !== temporaryRoot ||
    !basename(directory).startsWith("sparkle-browser-tests-")
)
    throw Error("Unexpected browser test directory");
try {
    const build = await Bun.build({
        entrypoints: [join(fixture, "regressions.ts")],
        outdir: directory,
        target: "browser",
        conditions: ["browser"],
        plugins: [
            {
                name: "svelte-browser-tests",
                setup(build) {
                    build.onResolve(
                        { filter: /^\$app\/(stores|navigation)$/ },
                        () => ({ path: join(fixture, "router.ts") }),
                    );
                    build.onResolve({ filter: /^\$lib\// }, ({ path }) => ({
                        path: join(
                            root,
                            "src/lib",
                            path.slice(5) + (extname(path) ? "" : ".ts"),
                        ),
                    }));
                    build.onLoad({ filter: /\.svelte$/ }, async ({ path }) => ({
                        contents: compile(await readFile(path, "utf8"), {
                            filename: path,
                            generate: "client",
                            css: "injected",
                            dev: true,
                        }).js.code,
                        loader: "js",
                        resolveDir: dirname(path),
                    }));
                },
            },
        ],
    });
    if (!build.success)
        throw new AggregateError(build.logs, "Browser fixture build failed");
    const html = join(directory, "index.html");
    await writeFile(
        html,
        '<!doctype html><html><head><meta charset="utf-8"></head><body><div id="app"></div><script type="module" src="./regressions.js"></script></body></html>',
    );
    const output = await new Promise((accept, reject) => {
        const child = spawn(
            browser,
            [
                "--headless",
                "--disable-gpu",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-extensions",
                "--disable-background-networking",
                "--allow-file-access-from-files",
                `--user-data-dir=${join(directory, "profile")}`,
                "--virtual-time-budget=10000",
                "--dump-dom",
                pathToFileURL(html).href,
            ],
            { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] },
        );
        let stdout = "",
            stderr = "";
        child.stdout.on("data", (chunk) => {
            stdout += chunk;
        });
        child.stderr.on("data", (chunk) => {
            stderr = (stderr + chunk).slice(-4000);
        });
        const timer = setTimeout(() => {
            child.kill();
            reject(Error("Browser regressions timed out"));
        }, 30000);
        child.once("error", (error) => {
            clearTimeout(timer);
            reject(error);
        });
        child.once("close", (code) => {
            clearTimeout(timer);
            if (code === 0) accept(stdout);
            else reject(Error(`Browser exited with ${code}: ${stderr}`));
        });
    });
    const encoded = /data-test-result="([^"]+)"/.exec(output)?.[1];
    if (!encoded)
        throw Error("Browser fixtures did not finish or could not start");
    const result = JSON.parse(decodeURIComponent(encoded));
    for (const failure of result.failures) console.error(failure);
    console.log(
        `${result.passed} browser checks passed, ${result.failures.length} failed`,
    );
    if (result.failures.length) process.exitCode = 1;
} finally {
    // This is the unique directory returned by mkdtemp above, never a user path.
    await rm(directory, {
        recursive: true,
        force: true,
        maxRetries: 5,
        retryDelay: 200,
    });
}
