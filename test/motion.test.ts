// @ts-nocheck
import { expect, test } from "bun:test";
import {
    motionScrollBehavior,
    prefersReducedMotion,
} from "../src/lib/utils/motion";

test("either motion preference prevents smooth navigation, including changes after mount", () => {
    const previousWindow = globalThis.window;
    const previousDocument = globalThis.document;
    let osReduced = false;
    const dataset = { motion: "full" };
    globalThis.window = {
        matchMedia(query) {
            expect(query).toBe("(prefers-reduced-motion: reduce)");
            return { matches: osReduced };
        },
    };
    globalThis.document = { documentElement: { dataset } };
    try {
        expect(prefersReducedMotion()).toBe(false);
        expect(motionScrollBehavior()).toBe("smooth");
        osReduced = true;
        expect(motionScrollBehavior()).toBe("instant");
        osReduced = false;
        dataset.motion = "reduced";
        expect(motionScrollBehavior()).toBe("instant");
        dataset.motion = "full";
        expect(motionScrollBehavior()).toBe("smooth");
        delete dataset.motion;
        expect(motionScrollBehavior()).toBe("instant");
    } finally {
        if (previousWindow === undefined) delete globalThis.window;
        else globalThis.window = previousWindow;
        if (previousDocument === undefined) delete globalThis.document;
        else globalThis.document = previousDocument;
    }
});

test("motion stays disabled before the client is available", () => {
    const previousWindow = globalThis.window;
    const previousDocument = globalThis.document;
    try {
        delete globalThis.window;
        delete globalThis.document;
        expect(prefersReducedMotion()).toBe(true);
        expect(motionScrollBehavior()).toBe("instant");
        globalThis.window = {};
        expect(prefersReducedMotion()).toBe(true);
    } finally {
        if (previousWindow === undefined) delete globalThis.window;
        else globalThis.window = previousWindow;
        if (previousDocument === undefined) delete globalThis.document;
        else globalThis.document = previousDocument;
    }
});
