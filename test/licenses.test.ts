import { test } from "node:test";
import assert from "node:assert/strict";
import report from "../licenses/dependencies.json";

test("offline notices retain upstream copyright holders omitted by SPDX fallbacks", () => {
    for (const [name, notice] of [
        ["velopack", "Copyright © 2024 Velopack Ltd."],
        ["webview2-com", "Copyright (c) 2021 Bill Avery"],
        ["tauri", "Tauri Apps Contributors"],
        ["windows", "Microsoft Corporation"],
    ]) {
        const texts = report.licenses
            .filter((entry) =>
                entry.components.some((pkg) => pkg.name === name),
            )
            .map((entry) => entry.text)
            .join("\n");
        assert.ok(
            texts.includes(notice),
            `Missing upstream attribution for ${name}`,
        );
    }
    for (const entry of report.licenses.filter(
        (entry) => entry.license === "MIT",
    )) {
        assert.ok(
            !entry.text.includes("<copyright holders>"),
            "Generic SPDX MIT text omits the real copyright notice",
        );
    }
});
