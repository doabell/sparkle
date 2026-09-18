// @ts-nocheck
import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import {
    mkdtempSync,
    mkdirSync,
    readFileSync,
    renameSync,
    rmSync,
    writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import {
    changedPaths,
    classifyChanges,
    comparisonBase,
} from "../scripts/ci-changes.mjs";

const full = { frontend: true, rust: true, package: true };

test.each([
    [[], { frontend: false, rust: false, package: false }],
    [
        [
            "docs/testing.md",
            "README.md",
            "SECURITY.md",
            ".vscode/settings.json",
        ],
        { frontend: false, rust: false, package: false },
    ],
    [
        ["src/routes/+page.svelte", "static/theme-init.js", "vite.config.js"],
        { frontend: true, rust: false, package: true },
    ],
    [
        ["test/api.test.ts", "test/support/platform.ts"],
        { frontend: true, rust: false, package: false },
    ],
    [
        ["test/fixtures/lrc.json"],
        { frontend: true, rust: true, package: false },
    ],
    [
        ["src-tauri/src/audio.rs", "docs/testing.md"],
        { frontend: false, rust: true, package: true },
    ],
    [
        ["src-tauri/patches/tauri-plugin-media/src/lib.rs"],
        { frontend: false, rust: true, package: true },
    ],
    [
        [
            "CHANGELOG.md",
            "LICENSE",
            "THIRD_PARTY_NOTICES.md",
            "licenses/about.toml",
        ],
        { frontend: false, rust: false, package: true },
    ],
    [["src/lib/api.ts", "src-tauri/Cargo.toml"], full],
    [["package.json"], full],
    [["bun.lock"], full],
    [[".cargo/config.toml"], full],
    [["rust-toolchain.toml"], full],
    [[".github/workflows/ci.yml"], full],
    [["scripts/ci-changes.mjs"], full],
    [["new-build-config.json"], full],
    [null, full],
])("select checks conservatively for %j", (paths, expected) => {
    expect(classifyChanges(paths)).toEqual(expected);
});

test("missing or unavailable comparison history runs every check", () => {
    for (const base of [
        null,
        "0".repeat(40),
        "--invalid-option",
        "f".repeat(40),
    ]) {
        const paths = changedPaths({ base, sha: "a".repeat(40) });
        expect(paths).toBeNull();
        expect(classifyChanges(paths)).toEqual(full);
    }
});

const repository = "doabell/sparkle";
const successfulRun = (sha: string, workflow = "ci.yml") => ({
    path: `.github/workflows/${workflow}`,
    event: "push",
    head_branch: "main",
    head_sha: sha,
    status: "completed",
    conclusion: "success",
    repository: { full_name: repository },
    head_repository: { full_name: repository },
});

test("each main workflow uses its own successful baseline; PRs need no API", async () => {
    const base = "a".repeat(40);
    for (const workflow of ["ci.yml", "coverage.yml"]) {
        expect(
            await comparisonBase({
                eventName: "push",
                event: { before: "b".repeat(40) },
                repository,
                workflow,
                api: async (endpoint: string) => {
                    expect(endpoint).toContain(`/workflows/${workflow}/runs?`);
                    const run = successfulRun(base, workflow);
                    return {
                        workflow_runs: [
                            {
                                ...run,
                                head_sha: "b".repeat(40),
                                conclusion: "cancelled",
                            },
                            {
                                ...run,
                                head_sha: "c".repeat(40),
                                conclusion: "failure",
                            },
                            run,
                        ],
                    };
                },
            }),
        ).toBe(base);
    }
    expect(
        await comparisonBase({
            eventName: "pull_request",
            event: { pull_request: { base: { sha: base } } },
            repository,
            workflow: "ci.yml",
            api: async () => {
                throw new Error("PRs must not need API access");
            },
        }),
    ).toBe(base);
});

test.each([
    { path: ".github/workflows/coverage.yml" },
    { event: "pull_request" },
    { head_branch: "feature" },
    { status: "in_progress" },
    { conclusion: "failure" },
    { head_repository: { full_name: "someone/fork" } },
    { repository: { full_name: "someone/fork" } },
])("invalid baseline metadata runs all checks: %j", async (override) => {
    const base = await comparisonBase({
        eventName: "push",
        event: {},
        repository,
        workflow: "ci.yml",
        api: async () => ({
            workflow_runs: [{ ...successfulRun("a".repeat(40)), ...override }],
        }),
    });
    expect(base).toBeNull();
    expect(
        classifyChanges(changedPaths({ base, sha: "b".repeat(40) })),
    ).toEqual(full);
});

test("empty baseline history runs all checks", async () => {
    expect(
        await comparisonBase({
            eventName: "push",
            event: {},
            repository,
            workflow: "ci.yml",
            api: async () => ({ workflow_runs: [] }),
        }),
    ).toBeNull();
});

// Real Git subprocesses need more time on busy Windows runners.
test("diffs preserve renames, deleted paths and native work before a later docs push", async () => {
    const cwd = mkdtempSync(join(tmpdir(), "sparkle-ci-paths-"));
    const git = (...args: string[]) => {
        const result = spawnSync(
            "git",
            [
                "-c",
                "user.name=Sparkle Tests",
                "-c",
                "user.email=tests@example.invalid",
                "-c",
                "commit.gpgSign=false",
                ...args,
            ],
            { cwd, encoding: "utf8", windowsHide: true },
        );
        if (result.status !== 0) throw new Error(result.stderr);
        return result.stdout.trim();
    };
    try {
        git("init", "-q");
        mkdirSync(join(cwd, "src-tauri"));
        mkdirSync(join(cwd, "docs"));
        writeFileSync(join(cwd, "src-tauri/example.rs"), "fn example() {}\n");
        writeFileSync(join(cwd, "src-tauri/deleted.rs"), "fn deleted() {}\n");
        git("add", ".");
        git("commit", "-qm", "base");
        const base = git("rev-parse", "HEAD");
        renameSync(
            join(cwd, "src-tauri/example.rs"),
            join(cwd, "docs/example.md"),
        );
        rmSync(join(cwd, "src-tauri/deleted.rs"));
        writeFileSync(join(cwd, "docs/file with spaces.md"), "docs\n");
        git("add", ".");
        git("commit", "-qm", "changed");
        const sha = git("rev-parse", "HEAD");
        const paths = changedPaths({ base, sha, cwd });
        expect(paths?.sort()).toEqual([
            "docs/example.md",
            "docs/file with spaces.md",
            "src-tauri/deleted.rs",
            "src-tauri/example.rs",
        ]);
        expect(classifyChanges(paths)).toEqual({
            frontend: false,
            rust: true,
            package: true,
        });
        expect(changedPaths({ base: sha, sha: base, cwd })).toBeNull();

        writeFileSync(join(cwd, "docs/later.md"), "later docs-only push\n");
        git("add", ".");
        git("commit", "-qm", "docs only");
        const docsSha = git("rev-parse", "HEAD");
        const successfulBase = await comparisonBase({
            eventName: "push",
            event: { before: sha },
            repository,
            workflow: "ci.yml",
            api: async () => ({ workflow_runs: [successfulRun(base)] }),
        });
        expect(
            classifyChanges(
                changedPaths({ base: successfulBase, sha: docsSha, cwd }),
            ).rust,
        ).toBe(true);
        // Once the native commit has passed, a subsequent docs-only change is cheap.
        expect(
            classifyChanges(changedPaths({ base: sha, sha: docsSha, cwd })),
        ).toEqual({
            frontend: false,
            rust: false,
            package: false,
        });

        const eventPath = join(cwd, "event.json");
        const outputPath = join(cwd, "outputs.txt");
        writeFileSync(
            eventPath,
            JSON.stringify({ pull_request: { base: { sha } } }),
        );
        const cli = spawnSync(
            process.execPath,
            [
                fileURLToPath(
                    new URL("../scripts/ci-changes.mjs", import.meta.url),
                ),
            ],
            {
                cwd,
                encoding: "utf8",
                windowsHide: true,
                env: {
                    ...process.env,
                    GITHUB_EVENT_NAME: "pull_request",
                    GITHUB_EVENT_PATH: eventPath,
                    GITHUB_SHA: docsSha,
                    GITHUB_OUTPUT: outputPath,
                },
            },
        );
        expect(cli.status).toBe(0);
        expect(readFileSync(outputPath, "utf8")).toBe(
            "frontend=false\nrust=false\npackage=false\n",
        );
    } finally {
        rmSync(cwd, { recursive: true, force: true });
    }
}, 30_000);
