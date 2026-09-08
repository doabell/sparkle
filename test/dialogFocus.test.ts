// @ts-nocheck — Bun test types are not part of the app's TypeScript build.
import { test, expect } from "bun:test";
import { dialogFocus } from "../src/lib/utils/dialogFocus";

function fixture() {
    const previousDocument = globalThis.document;
    const previousElement = globalThis.HTMLElement;
    const listeners = new Map<string, (event: KeyboardEvent) => void>();
    const doc = {
        activeElement: null as Element | null,
        addEventListener: (
            type: string,
            listener: (event: KeyboardEvent) => void,
        ) => listeners.set(type, listener),
        removeEventListener: (type: string) => listeners.delete(type),
    };
    class Element {
        isConnected = true;
        visible = true;
        children: Element[] = [];
        focus() {
            doc.activeElement = this;
        }
        getClientRects() {
            return this.visible ? [{}] : [];
        }
        querySelectorAll() {
            return this.children;
        }
        contains(element: Element) {
            return element === this || this.children.includes(element);
        }
    }
    Object.assign(globalThis, { document: doc, HTMLElement: Element });
    const trigger = new Element();
    trigger.focus();
    const dialog = new Element();
    const first = new Element();
    const hidden = new Element();
    hidden.visible = false;
    const last = new Element();
    dialog.children = [first, last, hidden];
    let dismissed = 0;
    const action = dialogFocus(dialog as unknown as HTMLElement, () => {
        dismissed += 1;
    });
    const key = (value: string, shiftKey = false) => {
        let prevented = false;
        listeners.get("keydown")?.({
            key: value,
            shiftKey,
            preventDefault() {
                prevented = true;
            },
        } as KeyboardEvent);
        return prevented;
    };
    return {
        doc,
        trigger,
        dialog,
        first,
        last,
        action,
        key,
        listeners,
        cleanup() {
            action.destroy();
            Object.assign(globalThis, {
                document: previousDocument,
                HTMLElement: previousElement,
            });
        },
        dismissed: () => dismissed,
    };
}

test("modal keyboard navigation wraps visible controls and returns to its trigger", () => {
    const f = fixture();
    try {
        expect(f.doc.activeElement).toBe(f.dialog);
        expect(f.key("Tab")).toBe(true);
        expect(f.doc.activeElement).toBe(f.first);
        expect(f.key("Tab")).toBe(false);
        f.last.focus();
        expect(f.key("Tab")).toBe(true);
        expect(f.doc.activeElement).toBe(f.first);
        expect(f.key("Tab", true)).toBe(true);
        expect(f.doc.activeElement).toBe(f.last);
        expect(f.key("Tab", true)).toBe(false);
        expect(f.key("ArrowDown")).toBe(false);
        f.trigger.focus();
        f.key("Escape");
        expect(f.dismissed()).toBe(1);
        f.action.destroy();
        expect(f.doc.activeElement).toBe(f.trigger);
        expect(f.listeners.size).toBe(0);
    } finally {
        f.cleanup();
    }
});

test("modal keeps focus contained after switching content or disabling every control", () => {
    const f = fixture();
    try {
        f.trigger.focus();
        expect(f.key("Tab", true)).toBe(true);
        expect(f.doc.activeElement).toBe(f.last);
        f.trigger.focus();
        expect(f.key("Tab")).toBe(true);
        expect(f.doc.activeElement).toBe(f.first);
        f.dialog.focus();
        expect(f.key("Tab", true)).toBe(true);
        expect(f.doc.activeElement).toBe(f.last);
        f.dialog.children = [];
        expect(f.key("Tab")).toBe(true);
        expect(f.doc.activeElement).toBe(f.dialog);
        f.trigger.isConnected = false;
        f.action.destroy();
        expect(f.doc.activeElement).toBe(f.dialog);
    } finally {
        f.cleanup();
    }
});
