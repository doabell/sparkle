import { resolve } from "node:path";

/**
 * Select Sparkle's library tests from the current Cargo invocation, including cache hits.
 * @param {string} output
 * @param {string} manifestPath
 */
export function libraryTestExecutable(output, manifestPath) {
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
            artifact.target?.name === "sparkle_lib" &&
            artifact.profile?.test === true &&
            typeof artifact.executable === "string"
        ) {
            executables.add(artifact.executable);
        }
    }
    if (executables.size !== 1) {
        throw new Error(
            `Expected one Sparkle library test executable from Cargo; found ${executables.size}.`,
        );
    }
    return [...executables][0];
}
