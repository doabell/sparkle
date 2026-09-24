// @ts-nocheck
import { afterEach, expect, spyOn, test } from "bun:test";
import { get } from "svelte/store";
import {
    LogicalSize,
    PhysicalPosition,
    PhysicalSize,
    type Monitor,
} from "@tauri-apps/api/window";
import { createMiniPlayer } from "../src/lib/stores/miniPlayer";

const normalSize = new PhysicalSize(1280, 720);
const normalPosition = new PhysicalPosition(120, 80);

function deferred() {
    let resolve!: () => void;
    const promise = new Promise<void>((done) => {
        resolve = done;
    });
    return { promise, resolve };
}

function fixture(getStorage = () => null, beforeShow = async () => {}) {
    const native = {
        size: normalSize,
        position: normalPosition,
        maximized: false,
        fullscreen: false,
        resizable: true,
        maximizable: true,
        alwaysOnTop: false,
        visible: false,
    };
    const monitor: Monitor = {
        name: "Secondary",
        position: new PhysicalPosition(-2560, 0),
        size: new PhysicalSize(2560, 1440),
        workArea: {
            position: new PhysicalPosition(-2560, 0),
            size: new PhysicalSize(2560, 1400),
        },
        scaleFactor: 1.5,
    };
    const window = {
        show: async () => {
            native.visible = true;
        },
        innerSize: async () => native.size,
        outerPosition: async () => native.position,
        isMaximized: async () => native.maximized,
        isFullscreen: async () => native.fullscreen,
        isResizable: async () => native.resizable,
        isMaximizable: async () => native.maximizable,
        isAlwaysOnTop: async () => native.alwaysOnTop,
        unmaximize: async () => {
            if (native.maximized) {
                native.size = normalSize;
                native.position = normalPosition;
            }
            native.maximized = false;
        },
        maximize: async () => {
            native.maximized = true;
        },
        setFullscreen: async (value: boolean) => {
            native.fullscreen = value;
        },
        setResizable: async (value: boolean) => {
            native.resizable = value;
        },
        setMaximizable: async (value: boolean) => {
            native.maximizable = value;
        },
        setAlwaysOnTop: async (value: boolean) => {
            native.alwaysOnTop = value;
        },
        setSize: async (value: LogicalSize | PhysicalSize) => {
            native.size =
                value instanceof LogicalSize
                    ? value.toPhysical(monitor.scaleFactor)
                    : value;
        },
        setPosition: async (value: PhysicalPosition) => {
            native.position = value;
        },
    };
    const errors: string[] = [];
    const player = createMiniPlayer(
        () => window,
        async () => monitor,
        (message) => errors.push(message),
        getStorage,
        beforeShow,
    );
    return { player, window, native, errors };
}

afterEach(() => {
    consoleError?.mockRestore();
    consoleError = undefined;
});
let consoleError: ReturnType<typeof spyOn<typeof console, "error">> | undefined;
function quietExpectedError() {
    consoleError = spyOn(console, "error").mockImplementation(() => {});
}

test("mini player fits the scaled work area and restores the original window", async () => {
    const { player, native } = fixture();
    await player.toggle();
    expect(get(player)).toEqual({ active: true, busy: false, pinned: true });
    expect(native.size).toEqual(new PhysicalSize(540, 231));
    expect(native.position).toEqual(new PhysicalPosition(-564, 1145));
    expect(native.alwaysOnTop).toBe(true);
    expect(native.resizable).toBe(false);
    expect(native.maximizable).toBe(false);

    native.position = new PhysicalPosition(-900, 500);
    await player.toggle();
    expect(get(player)).toEqual({ active: false, busy: false, pinned: true });
    expect(native.size).toEqual(normalSize);
    expect(native.position).toEqual(normalPosition);
    expect(native.alwaysOnTop).toBe(false);
    expect(native.resizable).toBe(true);
    expect(native.maximizable).toBe(true);

    await player.toggle();
    expect(native.position).toEqual(new PhysicalPosition(-900, 500));
});

test("preserves maximized state and normal restore-down bounds", async () => {
    const { player, native } = fixture();
    native.maximized = true;
    native.size = new PhysicalSize(2560, 1400);
    await player.toggle();
    expect(native.maximized).toBe(false);
    await player.toggle();
    expect(native.maximized).toBe(true);
    expect(native.size).toEqual(normalSize);
    expect(native.position).toEqual(normalPosition);
});

test("preserves fullscreen and existing window flags", async () => {
    const { player, native } = fixture();
    native.fullscreen = true;
    native.alwaysOnTop = true;
    native.resizable = false;
    native.maximizable = false;
    await player.toggle();
    expect(native.fullscreen).toBe(false);
    await player.toggle();
    expect(native.fullscreen).toBe(true);
    expect(native.alwaysOnTop).toBe(true);
    expect(native.resizable).toBe(false);
    expect(native.maximizable).toBe(false);
});

test("rolls back if pinning fails after resizing", async () => {
    quietExpectedError();
    const { player, window, native, errors } = fixture();
    window.setAlwaysOnTop = async (value) => {
        if (value) throw new Error("Pin failed");
        native.alwaysOnTop = value;
    };
    await player.toggle();
    expect(get(player)).toEqual({ active: false, busy: false, pinned: true });
    expect(native.size).toEqual(normalSize);
    expect(native.position).toEqual(normalPosition);
    expect(native.resizable).toBe(true);
    expect(native.maximizable).toBe(true);
    expect(native.alwaysOnTop).toBe(false);
    expect(errors).toHaveLength(1);
});

test("a failed restore still unpins and keeps the snapshot for retry", async () => {
    quietExpectedError();
    const { player, window, native, errors } = fixture();
    await player.toggle();
    const setSize = window.setSize;
    window.setSize = async () => {
        throw new Error("Resize failed");
    };
    await player.toggle();
    expect(native.alwaysOnTop).toBe(false);
    expect(get(player)).toEqual({ active: true, busy: false, pinned: true });
    expect(errors).toHaveLength(1);
    window.setSize = setSize;
    await player.toggle();
    expect(get(player)).toEqual({ active: false, busy: false, pinned: true });
    expect(native.size).toEqual(normalSize);
    expect(native.position).toEqual(normalPosition);
});

test("ignores repeated toggles while a native transition is pending", async () => {
    const { player, window, native } = fixture();
    const resizing = deferred();
    const finish = deferred();
    const setSize = window.setSize;
    window.setSize = async (value) => {
        resizing.resolve();
        await finish.promise;
        await setSize(value);
    };
    const opening = player.toggle();
    await resizing.promise;
    expect(get(player).busy).toBe(true);
    await player.toggle();
    finish.resolve();
    await opening;
    expect(get(player)).toEqual({ active: true, busy: false, pinned: true });
    expect(native.alwaysOnTop).toBe(true);
});

test("a placement read failure cannot prevent leaving mini player", async () => {
    const { player, window, native } = fixture();
    await player.toggle();
    window.outerPosition = async () => {
        throw new Error("Position unavailable");
    };
    await player.toggle();
    expect(get(player).active).toBe(false);
    expect(native.alwaysOnTop).toBe(false);
    expect(native.size).toEqual(normalSize);
});

test("pin can be switched off and on without leaving the mini player", async () => {
    const { player, native } = fixture();
    await player.toggle();
    await player.togglePinned();
    expect(get(player)).toEqual({ active: true, busy: false, pinned: false });
    expect(native.alwaysOnTop).toBe(false);
    expect(native.size).toEqual(new PhysicalSize(540, 231));
    await player.togglePinned();
    expect(get(player).pinned).toBe(true);
    expect(native.alwaysOnTop).toBe(true);
});

test("pin preference survives reopening without changing the full window's pin", async () => {
    const { player, native } = fixture();
    native.alwaysOnTop = true;
    await player.toggle();
    await player.togglePinned();
    await player.toggle();
    expect(native.alwaysOnTop).toBe(true);
    expect(get(player).pinned).toBe(false);
    await player.toggle();
    expect(native.alwaysOnTop).toBe(false);
    expect(get(player)).toEqual({ active: true, busy: false, pinned: false });
});

test("a rejected pin change leaves the previous state visible and allows retry", async () => {
    quietExpectedError();
    const { player, native, window, errors } = fixture();
    await player.toggle();
    const setAlwaysOnTop = window.setAlwaysOnTop;
    window.setAlwaysOnTop = async () => {
        throw new Error("Pin change failed");
    };
    await player.togglePinned();
    expect(get(player)).toEqual({ active: true, busy: false, pinned: true });
    expect(native.alwaysOnTop).toBe(true);
    expect(errors).toHaveLength(1);
    window.setAlwaysOnTop = setAlwaysOnTop;
    await player.togglePinned();
    expect(get(player).pinned).toBe(false);
    expect(native.alwaysOnTop).toBe(false);
});

test("a pin change cannot race a window transition or alter the full window", async () => {
    const { player, native, window } = fixture();
    await player.togglePinned();
    expect(native.alwaysOnTop).toBe(false);
    await player.toggle();
    let finish!: () => void;
    const setAlwaysOnTop = window.setAlwaysOnTop;
    window.setAlwaysOnTop = async (value) => {
        await new Promise<void>((resolve) => {
            finish = resolve;
        });
        await setAlwaysOnTop(value);
    };
    const pinning = player.togglePinned();
    await player.toggle();
    await player.togglePinned();
    expect(get(player)).toEqual({ active: true, busy: true, pinned: true });
    finish();
    await pinning;
    expect(get(player)).toEqual({ active: true, busy: false, pinned: false });
    expect(native.size).toEqual(new PhysicalSize(540, 231));
});

function preferences(initial: string | null = null) {
    let saved = initial;
    return {
        getItem: () => saved,
        setItem: (_key: string, value: string) => {
            saved = value;
        },
    };
}

test("first launch and invalid preferences keep the large player", async () => {
    for (const raw of [
        null,
        "not json",
        "null",
        '"mini"',
        '{"mode":"unknown"}',
        '{"mode":true}',
    ]) {
        const storage = preferences(raw);
        const { player, native, errors, window } = fixture(() => storage);
        window.setSize = async () => {
            throw new Error("Startup should not resize the default window");
        };
        await player.restoreMode();
        expect(get(player)).toEqual({
            active: false,
            busy: false,
            pinned: true,
        });
        expect(native.size).toEqual(normalSize);
        expect(native.visible).toBe(true);
        expect(errors).toHaveLength(0);
        expect(storage.getItem()).toBe(raw);
    }
});

test("relaunch restores mini mode and pin, and returning to large persists too", async () => {
    const storage = preferences();
    const first = fixture(() => storage);
    await first.player.restoreMode();
    await first.player.toggle();
    await first.player.togglePinned();
    expect(JSON.parse(storage.getItem()!)).toEqual({
        mode: "mini",
        pinned: false,
    });

    const second = fixture(() => storage);
    await second.player.restoreMode();
    expect(get(second.player)).toEqual({
        active: true,
        busy: false,
        pinned: false,
    });
    expect(second.native.size).toEqual(new PhysicalSize(540, 231));
    expect(second.native.alwaysOnTop).toBe(false);
    await second.player.toggle();
    expect(second.native.size).toEqual(normalSize);
    expect(JSON.parse(storage.getItem()!)).toEqual({
        mode: "large",
        pinned: false,
    });

    const third = fixture(() => storage);
    await third.player.restoreMode();
    expect(get(third.player)).toEqual({
        active: false,
        busy: false,
        pinned: false,
    });
    expect(third.native.size).toEqual(normalSize);
});

test("startup restoration runs once and cannot undo a user's selected mode", async () => {
    const storage = preferences('{"mode":"mini","pinned":true}');
    const { player, native } = fixture(() => storage);
    await Promise.all([player.restoreMode(), player.restoreMode()]);
    expect(get(player).active).toBe(true);
    await player.restoreMode();
    expect(get(player).active).toBe(true);
    await player.toggle();
    await player.restoreMode();
    expect(get(player).active).toBe(false);
    expect(native.size).toEqual(normalSize);

    const early = fixture(() => preferences());
    await early.player.toggle();
    await early.player.restoreMode();
    expect(get(early.player).active).toBe(true);
});

test("failed startup restoration preserves the saved choice and allows manual retry", async () => {
    quietExpectedError();
    const original = '{"mode":"mini","pinned":true}';
    const storage = preferences(original);
    const { player, native, window } = fixture(() => storage);
    const setAlwaysOnTop = window.setAlwaysOnTop;
    window.setAlwaysOnTop = async (value) => {
        if (value) throw new Error("Pin unavailable");
        await setAlwaysOnTop(value);
    };
    await player.restoreMode();
    expect(get(player).active).toBe(false);
    expect(native.size).toEqual(normalSize);
    expect(native.visible).toBe(true);
    expect(storage.getItem()).toBe(original);
    window.setAlwaysOnTop = setAlwaysOnTop;
    await player.toggle();
    expect(get(player).active).toBe(true);
});

test("failed mode and pin changes do not overwrite saved preferences", async () => {
    quietExpectedError();
    const storage = preferences();
    const { player, window } = fixture(() => storage);
    await player.toggle();
    const saved = storage.getItem();
    window.setSize = async () => {
        throw new Error("Resize unavailable");
    };
    await player.toggle();
    expect(storage.getItem()).toBe(saved);
    window.setAlwaysOnTop = async () => {
        throw new Error("Pin unavailable");
    };
    await player.togglePinned();
    expect(storage.getItem()).toBe(saved);
});

test("unavailable or full storage never blocks player controls", async () => {
    for (const getStorage of [
        () => {
            throw new Error("Storage blocked");
        },
        () => ({
            getItem: () => {
                throw new Error("Read blocked");
            },
            setItem: () => {
                throw new Error("Storage full");
            },
        }),
    ]) {
        const { player, native, errors } = fixture(getStorage);
        await player.restoreMode();
        expect(get(player).active).toBe(false);
        await player.toggle();
        await player.togglePinned();
        expect(get(player)).toEqual({
            active: true,
            busy: false,
            pinned: false,
        });
        await player.toggle();
        expect(native.size).toEqual(normalSize);
        expect(get(player).active).toBe(false);
        expect(errors).toHaveLength(0);
    }
});

test("startup stays hidden until mini geometry and layout are ready, then shows once", async () => {
    const storage = preferences('{"mode":"mini","pinned":false}');
    const rendering = deferred();
    const finishLayout = deferred();
    const { player, window, native } = fixture(
        () => storage,
        () => {
            rendering.resolve();
            return finishLayout.promise;
        },
    );
    const resizing = deferred();
    const finishResize = deferred();
    const setSize = window.setSize;
    window.setSize = async (value) => {
        resizing.resolve();
        await finishResize.promise;
        await setSize(value);
    };
    const visibleStates: unknown[] = [];
    const show = window.show;
    window.show = async () => {
        visibleStates.push({
            size: native.size,
            position: native.position,
            ...get(player),
        });
        await show();
    };

    const opening = player.restoreMode();
    await resizing.promise;
    expect(native.visible).toBe(false);
    expect(native.size).toEqual(normalSize);
    await player.restoreMode();
    finishResize.resolve();
    await rendering.promise;
    expect(native.visible).toBe(false);
    expect(native.size).toEqual(new PhysicalSize(540, 231));
    expect(get(player).active).toBe(true);
    finishLayout.resolve();
    await opening;
    await player.restoreMode();

    expect(native.visible).toBe(true);
    expect(visibleStates).toEqual([
        {
            size: new PhysicalSize(540, 231),
            position: new PhysicalPosition(-564, 1145),
            active: true,
            busy: false,
            pinned: false,
        },
    ]);
});

test("a startup layout error still reveals the window", async () => {
    quietExpectedError();
    const { player, native } = fixture(
        () => null,
        async () => {
            throw new Error("Render failed");
        },
    );
    await player.restoreMode();
    expect(native.visible).toBe(true);
    expect(native.size).toEqual(normalSize);
    expect(consoleError).toHaveBeenCalledTimes(1);
});
