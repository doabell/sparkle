import { spawnSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";

const allChecks = () => ({ frontend: true, rust: true, package: true });
const frontendConfig = new Set([
    "svelte.config.js",
    "vite.config.js",
    "tsconfig.json",
]);

/**
 * Only known inputs may skip work. New configuration/build paths run everything.
 * @param {string[] | null} paths
 */
export function classifyChanges(paths) {
    if (paths === null) return allChecks();
    const checks = { frontend: false, rust: false, package: false };
    for (const path of paths) {
        if (
            path.startsWith("docs/") ||
            path.startsWith(".vscode/") ||
            ["README.md", "SECURITY.md"].includes(path)
        ) {
            continue;
        }
        if (
            ["CHANGELOG.md", "LICENSE", "THIRD_PARTY_NOTICES.md"].includes(
                path,
            ) ||
            path.startsWith("licenses/")
        ) {
            checks.package = true;
        } else if (path.startsWith("test/fixtures/")) {
            // Rust includes the LRC contract fixture from the frontend test tree.
            checks.frontend = checks.rust = true;
        } else if (path.startsWith("test/")) {
            checks.frontend = true;
        } else if (
            path.startsWith("src/") ||
            path.startsWith("static/") ||
            frontendConfig.has(path)
        ) {
            checks.frontend = checks.package = true;
        } else if (path.startsWith("src-tauri/")) {
            checks.rust = checks.package = true;
        } else {
            return allChecks();
        }
    }
    return checks;
}

/**
 * @typedef {{path?: string, event?: string, head_branch?: string, head_sha?: string,
 * status?: string, conclusion?: string, repository?: {full_name?: string},
 * head_repository?: {full_name?: string}}} WorkflowRun
 * @param {{eventName?: string, event: {pull_request?: {base?: {sha?: string}}},
 * repository?: string, workflow?: string,
 * api: (endpoint: string) => Promise<{workflow_runs: WorkflowRun[]}>}} options
 */
export async function comparisonBase({
    eventName,
    event,
    repository,
    workflow,
    api,
}) {
    if (eventName === "pull_request")
        return event.pull_request?.base?.sha ?? null;
    if (
        eventName !== "push" ||
        !repository ||
        !workflow ||
        !/^[\w.-]+\/[\w.-]+$/.test(repository) ||
        !["ci.yml", "coverage.yml"].includes(workflow)
    ) {
        return null;
    }
    // A previous push may have failed or been cancelled. Comparing only event.before
    // would let a later docs-only push skip that unfinished work and turn CI green.
    const { workflow_runs: runs } = await api(
        `repos/${repository}/actions/workflows/${workflow}/runs?branch=main&event=push&status=success&per_page=100`,
    );
    const baseline = runs.find(
        (run) =>
            run.path === `.github/workflows/${workflow}` &&
            run.event === "push" &&
            run.head_branch === "main" &&
            run.status === "completed" &&
            run.conclusion === "success" &&
            run.repository?.full_name?.toLowerCase() ===
                repository.toLowerCase() &&
            run.head_repository?.full_name?.toLowerCase() ===
                repository.toLowerCase(),
    );
    return baseline?.head_sha ?? null;
}

/** @param {{base?: string | null, sha?: string, cwd?: string}} options */
export function changedPaths({ base, sha, cwd = process.cwd() }) {
    // New branches, missing history and unknown baselines must never skip checks.
    if (
        typeof base !== "string" ||
        typeof sha !== "string" ||
        ![base, sha].every(
            (value) => /^[a-f0-9]{40}$/i.test(value) && !/^0+$/.test(value),
        )
    ) {
        return null;
    }
    const ancestor = spawnSync(
        "git",
        ["merge-base", "--is-ancestor", base, sha],
        {
            cwd,
            windowsHide: true,
        },
    );
    // Force-pushed history or a rerun older than the baseline needs full checks.
    if (ancestor.error || ancestor.status !== 0) return null;
    const result = spawnSync(
        "git",
        ["diff", "--no-renames", "--name-only", "-z", base, sha, "--"],
        { cwd, encoding: "utf8", windowsHide: true },
    );
    if (result.error || result.status !== 0) return null;
    // NUL delimiters preserve spaces/newlines; disabling renames includes both paths.
    return result.stdout.split("\0").filter(Boolean);
}

if (import.meta.main) {
    const output = process.env.GITHUB_OUTPUT;
    if (!output) throw new Error("GITHUB_OUTPUT is required.");
    let paths = null;
    try {
        const eventPath = process.env.GITHUB_EVENT_PATH;
        if (!eventPath) throw new Error("GITHUB_EVENT_PATH is required.");
        const base = await comparisonBase({
            eventName: process.env.GITHUB_EVENT_NAME,
            event: JSON.parse(readFileSync(eventPath, "utf8")),
            repository: process.env.GITHUB_REPOSITORY,
            workflow: process.env.CI_WORKFLOW_FILE,
            api: async (endpoint) => {
                const result = spawnSync("gh", ["api", endpoint], {
                    encoding: "utf8",
                    windowsHide: true,
                    timeout: 30_000,
                    maxBuffer: 10 * 1024 * 1024,
                });
                if (result.error || result.status !== 0)
                    throw new Error("CI baseline unavailable.");
                return JSON.parse(result.stdout);
            },
        });
        paths = changedPaths({ base, sha: process.env.GITHUB_SHA });
    } catch {
        console.log("Change detection unavailable; running all checks.");
    }
    const checks = classifyChanges(paths);
    console.log("Checks for this change:", checks);
    appendFileSync(
        output,
        Object.entries(checks)
            .map(([name, enabled]) => `${name}=${enabled}\n`)
            .join(""),
    );
}
