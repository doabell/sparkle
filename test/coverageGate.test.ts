// @ts-nocheck
import { expect, test } from "bun:test";
import {
    parseLcov,
    summarize,
    checkThresholds,
    checkInventory,
} from "../scripts/lib/coverage.mjs";

test("coverage weights executable lines, not file percentages, and normalizes Windows paths", () => {
    const files = parseLcov(
        "TN:\nSF:src\\a.ts\nDA:1,1\nDA:1,2\nFNF:1\nFNH:1\nend_of_record\nSF:src/b.ts\nDA:1,0\nDA:2,0\nDA:3,0\nFNF:3\nFNH:0\nend_of_record",
    );
    expect(files[0].path).toBe("src/a.ts");
    expect(summarize(files)).toEqual({
        lines: 4,
        linesHit: 1,
        functions: 4,
        functionsHit: 1,
        linePercent: 25,
        functionPercent: 25,
    });
    expect(() =>
        checkThresholds(files, { lines: 25, functions: 25 }),
    ).not.toThrow();
    expect(() => checkThresholds(files, { lines: 25.01 })).toThrow(
        "below threshold",
    );
    expect(() => checkThresholds(files, { lines: 0, functions: 26 })).toThrow(
        "below threshold",
    );
    expect(() => checkThresholds(files, { lines: 0, perFileLines: 1 })).toThrow(
        "src/b.ts",
    );
});

test("coverage gates fail closed for empty reports, missing modules and invalid totals", () => {
    expect(() => checkThresholds([], { lines: 0 })).toThrow("empty");
    expect(() =>
        checkThresholds(parseLcov("SF:empty.ts\nend_of_record"), { lines: 0 }),
    ).toThrow("empty");
    expect(() =>
        checkInventory([{ path: "src/a.ts" }], ["src/a.ts", "src/b.ts"]),
    ).toThrow("src/b.ts");
    expect(() =>
        checkInventory([{ path: "src/a.ts" }], ["src/a.ts"]),
    ).not.toThrow();
    expect(() => parseLcov("SF:a.ts\nFNF:1\nFNH:2\nend_of_record")).toThrow(
        "Invalid",
    );
});
