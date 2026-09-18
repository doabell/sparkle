import { expect, test } from "bun:test";
import {
    existsSync,
    mkdtempSync,
    readFileSync,
    rmSync,
    writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
    findWindowsPackage,
    restoreWindowsPackage,
} from "../scripts/reuse-windows-package.mjs";

const repository = "doabell/sparkle";
const sha = "a".repeat(40);
const run = {
    id: 123,
    path: ".github/workflows/ci.yml",
    event: "push",
    head_branch: "main",
    head_sha: sha,
    status: "completed",
    conclusion: "success",
    repository: { full_name: repository },
    head_repository: { full_name: repository },
};
const artifact = {
    id: 456,
    name: "sparkle-windows-x64",
    expired: false,
    expires_at: "2099-01-01T00:00:00Z",
    workflow_run: { id: run.id, head_branch: "main", head_sha: sha },
};

function fakeApi(runs = [run], artifacts = [artifact]) {
    return async (endpoint: string) => {
        if (endpoint.includes("/workflows/ci.yml/runs?")) {
            const query = new URLSearchParams(endpoint.split("?")[1]);
            expect(query.get("head_sha")).toBe(sha);
            expect(query.get("branch")).toBe("main");
            expect(query.get("event")).toBe("push");
            expect(query.get("status")).toBe("success");
            return { workflow_runs: runs };
        }
        expect(endpoint).toBe(
            `repos/${repository}/actions/runs/${run.id}/artifacts?per_page=100`,
        );
        return { artifacts };
    };
}

test("reuse requires a successful main CI package from the exact commit", async () => {
    expect(
        await findWindowsPackage({ repository, sha, api: fakeApi() }),
    ).toEqual({ run, artifact });
});

test.each([
    { head_sha: "b".repeat(40) },
    { head_branch: "feature" },
    { event: "pull_request" },
    { status: "in_progress" },
    { conclusion: "failure" },
    { conclusion: "cancelled" },
    { path: ".github/workflows/other.yml" },
    { repository: { full_name: "someone/fork" } },
    { head_repository: { full_name: "someone/fork" } },
    { id: 0 },
])("reject untrusted or unsuccessful runs: %j", async (override) => {
    expect(
        await findWindowsPackage({
            repository,
            sha,
            api: fakeApi([{ ...run, ...override }]),
        }),
    ).toBeNull();
});

test.each([
    { expired: true },
    { expires_at: "2000-01-01T00:00:00Z" },
    { expires_at: "invalid" },
    { name: "coverage" },
    { workflow_run: { ...artifact.workflow_run, id: 999 } },
    { workflow_run: { ...artifact.workflow_run, head_branch: "feature" } },
    { workflow_run: { ...artifact.workflow_run, head_sha: "b".repeat(40) } },
])("reject expired or mismatched artifacts: %j", async (override) => {
    expect(
        await findWindowsPackage({
            repository,
            sha,
            api: fakeApi([run], [{ ...artifact, ...override }]),
        }),
    ).toBeNull();
});

test("absent main runs or deleted artifacts request a source build", async () => {
    for (const api of [fakeApi([], []), fakeApi([run], [])]) {
        expect(await findWindowsPackage({ repository, sha, api })).toBeNull();
    }
});

test("restore promotes a completed download and isolates a failed partial download", async () => {
    const root = mkdtempSync(join(tmpdir(), "sparkle-ci-artifact-"));
    const directory = join(root, "releases");
    try {
        const failed = await restoreWindowsPackage({
            repository,
            sha,
            api: fakeApi(),
            directory,
            download: async (_match, staging: string) => {
                writeFileSync(join(staging, "partial.zip"), "incomplete");
                throw new Error("Artifact expired during download");
            },
        });
        expect(failed).toBeNull();
        expect(existsSync(directory)).toBe(false);

        const restored = await restoreWindowsPackage({
            repository,
            sha,
            api: fakeApi(),
            directory,
            download: async (match, staging: string) => {
                expect(match).toEqual({ run, artifact });
                writeFileSync(join(staging, "Setup.exe"), "test package");
            },
        });
        expect(restored).toEqual({ run, artifact });
        expect(readFileSync(join(directory, "Setup.exe"), "utf8")).toBe(
            "test package",
        );
        expect(existsSync(join(directory, "partial.zip"))).toBe(false);
    } finally {
        rmSync(root, { recursive: true, force: true });
    }
});

test("lookup errors leave a clean destination for the fallback build", async () => {
    const root = mkdtempSync(join(tmpdir(), "sparkle-ci-unavailable-"));
    const directory = join(root, "releases");
    try {
        const restored = await restoreWindowsPackage({
            repository,
            sha,
            directory,
            api: async () => {
                throw new Error("GitHub API unavailable");
            },
            download: async () => {
                throw new Error("must not download");
            },
        });
        expect(restored).toBeNull();
        expect(existsSync(directory)).toBe(false);
    } finally {
        rmSync(root, { recursive: true, force: true });
    }
});
