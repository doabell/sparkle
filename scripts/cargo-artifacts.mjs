import { resolve } from "node:path";

/**
 * Select a workspace library's tests from this Cargo invocation, including cache hits.
 * @param {string} output
 * @param {string} manifestPath
 * @param {string} [targetName]
 */
export function libraryTestExecutable(
    output,
    manifestPath,
    targetName = "sparkle_lib",
) {
    const executables = new Set();
    const manifest = resolve(manifestPath).toLowerCase();
    for (const line of output.split(/\r?\n/)) {
        let artifact;
        try {
            artifact = JSON.parse(line);
        } catch {
            continue;
        }
        if (
            artifact?.reason === "compiler-artifact" &&
            typeof artifact.manifest_path === "string" &&
            resolve(artifact.manifest_path).toLowerCase() === manifest &&
            artifact.target?.name === targetName &&
            artifact.profile?.test === true &&
            typeof artifact.executable === "string"
        ) {
            executables.add(artifact.executable);
        }
    }
    if (executables.size !== 1) {
        throw new Error(
            `Expected one ${targetName} test executable from Cargo; found ${executables.size}.`,
        );
    }
    return [...executables][0];
}
