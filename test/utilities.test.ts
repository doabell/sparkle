// @ts-nocheck
import { expect, test } from "bun:test";
import {
    bytesToBase64,
    bytesToDataUrl,
    imageDataToUrl,
    cachedImageToUrl,
} from "../src/lib/utils/base64";
import { FONT_OPTIONS, getFontStack } from "../src/lib/utils/fonts";
import { formatTime } from "../src/lib/utils/formatTime";
import { plural } from "../src/lib/utils/text";
import { intersect } from "../src/lib/utils/intersect";
import { smartGo } from "../src/lib/utils/nav";
import { goto, page } from "./support/platform";

test("base64 matches binary encoding across padding and chunk boundaries", () => {
    for (const length of [0, 1, 2, 3, 4, 255, 65534, 65535, 65536, 131073]) {
        const bytes = Array.from({ length }, (_, i) => i % 256);
        const expected = Buffer.from(bytes).toString("base64");
        expect(bytesToBase64(bytes)).toBe(expected);
        const native = globalThis.btoa;
        try {
            delete globalThis.btoa;
            expect(bytesToBase64(bytes)).toBe(expected);
        } finally {
            globalThis.btoa = native;
        }
    }
});

test("image URLs preserve MIME, reuse object cache, and handle missing images", () => {
    expect(bytesToDataUrl([1, 2, 3], "image/png")).toBe(
        "data:image/png;base64,AQID",
    );
    expect(imageDataToUrl({ source: "none" })).toBe("");
    expect(imageDataToUrl({ source: "none", data: [] }, "fallback")).toBe(
        "fallback",
    );
    const image = { source: "embedded", data: [1, 2, 3] };
    expect(imageDataToUrl(image)).toBe("data:image/jpeg;base64,AQID");
    expect(imageDataToUrl(image)).toBe("data:image/jpeg;base64,AQID");
    expect(imageDataToUrl({ ...image, mime_type: "image/png" })).toBe(
        "data:image/png;base64,AQID",
    );
    expect(cachedImageToUrl({})).toBe("");
    expect(cachedImageToUrl({}, "fallback")).toBe("fallback");
    expect(cachedImageToUrl({ file_path: "C:/art a.jpg" })).toBe(
        "asset://localhost/C%3A%2Fart%20a.jpg",
    );
});

test("font preferences preserve stacks and safely quote installed font names", () => {
    for (const option of FONT_OPTIONS)
        expect(typeof getFontStack(option.value ?? option)).toBe("string");
    expect(getFontStack("  ")).toBe(getFontStack("System"));
    expect(getFontStack("Custom, serif")).toBe("Custom, serif");
    expect(getFontStack(' My "Font" ')).toBe('"My Font", sans-serif');
    expect(getFontStack("Custom Code")).toBe('"Custom Code", monospace');
});

test("time and plural labels handle zero, rounding, and irregular nouns", () => {
    for (const [ms, label] of [
        [0, "0:00"],
        [499, "0:00"],
        [500, "0:01"],
        [59999, "1:00"],
        [3600000, "60:00"],
    ])
        expect(formatTime(ms)).toBe(label);
    expect(plural(0, "song")).toBe("0 songs");
    expect(plural(1, "song")).toBe("1 song");
    expect(plural(2, "person", "people")).toBe("2 people");
});

test("intersection action only triggers for visible entries and disconnects", () => {
    const original = globalThis.IntersectionObserver;
    let callback,
        options,
        observed,
        disconnected = false,
        calls = 0;
    globalThis.IntersectionObserver = class {
        constructor(cb, opts) {
            callback = cb;
            options = opts;
        }
        observe(node) {
            observed = node;
        }
        disconnect() {
            disconnected = true;
        }
    };
    try {
        const node = {};
        const action = intersect(node, () => calls++);
        expect(observed).toBe(node);
        expect(options).toEqual({ rootMargin: "600px" });
        callback([{ isIntersecting: false }]);
        expect(calls).toBe(0);
        callback([{ isIntersecting: false }, { isIntersecting: true }]);
        expect(calls).toBe(1);
        action.destroy();
        expect(disconnected).toBe(true);
    } finally {
        if (original === undefined) delete globalThis.IntersectionObserver;
        else globalThis.IntersectionObserver = original;
    }
});

test("same-route navigation replaces history while a new route pushes", async () => {
    goto.mockClear();
    page.set({ url: new URL("https://sparkle.test/albums") });
    await smartGo("/albums");
    expect(goto).toHaveBeenLastCalledWith("/albums", { replaceState: true });
    await smartGo("/artists");
    expect(goto).toHaveBeenLastCalledWith("/artists");
});
