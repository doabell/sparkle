/** @typedef {{path:string, lines:number, linesHit:number, functions:number, functionsHit:number}} FileCoverage */

/**
 * Parse LCOV without averaging per-file percentages (which favors tiny files).
 * @param {string} text
 * @returns {FileCoverage[]}
 */
export function parseLcov(text) {
    return text.split("end_of_record").flatMap((record) => {
        const path = record
            .match(/^SF:(.+)$/m)?.[1]
            ?.trim()
            .replaceAll("\\", "/");
        if (!path) return [];
        const lines = new Map();
        for (const match of record.matchAll(/^DA:(\d+),(\d+)/gm)) {
            const line = Number(match[1]);
            lines.set(line, (lines.get(line) ?? 0) + Number(match[2]));
        }
        const functions = Number(record.match(/^FNF:(\d+)/m)?.[1] ?? 0);
        const functionsHit = Number(record.match(/^FNH:(\d+)/m)?.[1] ?? 0);
        if (functionsHit > functions)
            throw new Error(`Invalid function totals: ${path}`);
        return [
            {
                path,
                lines: lines.size,
                linesHit: [...lines.values()].filter((count) => count > 0)
                    .length,
                functions,
                functionsHit,
            },
        ];
    });
}

/** @param {FileCoverage[]} files */
export function summarize(files) {
    const totals = files.reduce(
        (sum, file) => ({
            lines: sum.lines + file.lines,
            linesHit: sum.linesHit + file.linesHit,
            functions: sum.functions + file.functions,
            functionsHit: sum.functionsHit + file.functionsHit,
        }),
        { lines: 0, linesHit: 0, functions: 0, functionsHit: 0 },
    );
    return {
        ...totals,
        linePercent: totals.lines ? (100 * totals.linesHit) / totals.lines : 0,
        functionPercent: totals.functions
            ? (100 * totals.functionsHit) / totals.functions
            : 0,
    };
}

/**
 * @param {FileCoverage[]} files
 * @param {{lines:number, functions?:number, perFileLines?:number}} thresholds
 */
export function checkThresholds(
    files,
    { lines, functions = 0, perFileLines = 0 },
) {
    const result = summarize(files);
    if (!files.length || !result.lines)
        throw new Error("Coverage report is empty");
    if (result.linePercent < lines || result.functionPercent < functions) {
        throw new Error(
            `Coverage below threshold: lines ${result.linePercent.toFixed(2)}% (need ${lines}%), functions ${result.functionPercent.toFixed(2)}% (need ${functions}%)`,
        );
    }
    for (const file of files) {
        if (file.lines && (100 * file.linesHit) / file.lines < perFileLines) {
            throw new Error(
                `${file.path}: line coverage below ${perFileLines}%`,
            );
        }
    }
    return result;
}

/** @param {{path:string}[]} files @param {string[]} expectedPaths */
export function checkInventory(files, expectedPaths) {
    const reported = new Set(files.map((file) => file.path));
    const missing = expectedPaths.filter((path) => !reported.has(path));
    if (missing.length)
        throw new Error(
            `Production files missing from coverage:\n${missing.join("\n")}`,
        );
}
