import { spawnSync } from "node:child_process";
import {
    appendFileSync,
    existsSync,
    mkdirSync,
    mkdtempSync,
    renameSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";

const artifactName = "sparkle-windows-x64";

export async function findWindowsPackage({
    repository,
    sha,
    api,
    now = Date.now(),
}) {
    if (!/^[\w.-]+\/[\w.-]+$/.test(repository) || !/^[a-f0-9]{40}$/.test(sha)) {
        throw new Error("A repository and exact commit SHA are required.");
    }
    const sameRepository = (repo) =>
        repo?.full_name?.toLowerCase() === repository.toLowerCase();
    const query = new URLSearchParams({
        branch: "main",
        event: "push",
        status: "success",
        head_sha: sha,
        per_page: "100",
    });
    const { workflow_runs: runs } = await api(
        `repos/${repository}/actions/workflows/ci.yml/runs?${query}`,
    );
    for (const run of runs) {
        // Do not promote PR, fork, other-workflow or merely same-version outputs.
        if (
            !Number.isSafeInteger(run.id) ||
            run.id <= 0 ||
            run.path !== ".github/workflows/ci.yml" ||
            run.event !== "push" ||
            run.head_branch !== "main" ||
            run.head_sha !== sha ||
            run.status !== "completed" ||
            run.conclusion !== "success" ||
            !sameRepository(run.repository) ||
            !sameRepository(run.head_repository)
        ) {
            continue;
        }
        const { artifacts } = await api(
            `repos/${repository}/actions/runs/${run.id}/artifacts?per_page=100`,
        );
        const artifact = artifacts.find(
            (item) =>
                item.name === artifactName &&
                item.expired === false &&
                Date.parse(item.expires_at) > now &&
                item.workflow_run?.id === run.id &&
                item.workflow_run?.head_branch === "main" &&
                item.workflow_run?.head_sha === sha,
        );
        if (artifact) return { run, artifact };
    }
    return null;
}

export async function restoreWindowsPackage({
    repository,
    sha,
    api,
    download,
    directory,
}) {
    if (existsSync(directory)) {
        throw new Error("Package output directory must not already exist.");
    }
    let match;
    let temporary;
    try {
        match = await findWindowsPackage({ repository, sha, api });
        if (!match) return null;
        mkdirSync(dirname(directory), { recursive: true });
        temporary = mkdtempSync(join(dirname(directory), "release-artifact-"));
        await download(match, temporary);
    } catch {
        // Expiry can race the download. Leave partial files in an isolated staging
        // directory so the fallback build always gets a fresh .tmp/releases.
        console.log(
            "CI package lookup/download unavailable; building from source.",
        );
        return null;
    }
    renameSync(temporary, directory);
    return match;
}

function gh(args) {
    const result = spawnSync("gh", args, {
        encoding: "utf8",
        windowsHide: true,
        timeout: 120_000,
        maxBuffer: 10 * 1024 * 1024,
    });
    if (result.error || result.status !== 0) {
        throw new Error("GitHub artifact request failed.");
    }
    return result.stdout;
}

if (import.meta.main) {
    const repository = process.env.GITHUB_REPOSITORY;
    const match = await restoreWindowsPackage({
        repository,
        sha: process.env.GITHUB_SHA,
        api: async (endpoint) => JSON.parse(gh(["api", endpoint])),
        download: async ({ run }, directory) =>
            gh([
                "run",
                "download",
                String(run.id),
                "--repo",
                repository,
                "--name",
                artifactName,
                "--dir",
                directory,
            ]),
        directory: resolve(".tmp/releases"),
    });
    const message = match
        ? `Reusing Windows package from successful main CI run ${match.run.id} (${process.env.GITHUB_SHA}).`
        : "No reusable Windows package; building this release from source.";
    console.log(message);
    appendFileSync(process.env.GITHUB_OUTPUT, `reused=${Boolean(match)}\n`);
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${message}\n`);
}
