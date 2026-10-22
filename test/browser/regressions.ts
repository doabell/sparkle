// @ts-nocheck — this fixture runs in the browser, outside the Bun test runner.
import { mount, unmount, tick } from "svelte";
import Lists from "./Lists.svelte";
import Playlist from "../../src/routes/playlists/[id]/+page.svelte";
import { navigate, navigations } from "./router";

const target = document.querySelector("#app");
const result = { passed: 0, failures: [] };
function check(name, actual, expected) {
    if (actual === expected) result.passed++;
    else
        result.failures.push(
            `${name}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
        );
}
const settle = async () => {
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 50));
    await tick();
};
const activeRow = () =>
    Number(
        document.activeElement?.closest("[data-virtual-index]")?.dataset
            .virtualIndex,
    );
async function key(value) {
    document.activeElement.dispatchEvent(
        new KeyboardEvent("keydown", { key: value, bubbles: true }),
    );
    await settle();
}
function deferred() {
    let resolve, reject;
    const promise = new Promise((yes, no) => {
        resolve = yes;
        reject = no;
    });
    return { promise, resolve, reject };
}
const playlist = (id, count = 3) => ({
    id,
    name: `Playlist ${id}`,
    description: null,
    folder_path: null,
    live_mix: null,
    tracks: Array.from({ length: count }, (_, i) => ({
        id: i + 1,
        title: `Track ${i + 1}`,
        album_id: null,
        album_title: null,
        artist_names: [],
        artist_ids: [],
        duration_ms: 1000,
    })),
});
const reads = [];
let removal;
let deletion;
window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
        if (command === "get_playlist") {
            const read = deferred();
            reads.push({ args, ...read });
            return read.promise;
        }
        if (command === "remove_track_from_playlist") return removal.promise;
        if (command === "delete_playlist") return deletion.promise;
        if (command === "get_playlists") return [];
        if (command === "get_album_art") return { source: "none" };
        if (command === "log_frontend") return;
        throw Error(`Unexpected IPC ${command}`);
    },
    convertFileSrc: () => "",
};
window.addEventListener("error", (event) =>
    result.failures.push(event.message),
);
window.addEventListener("unhandledrejection", (event) =>
    result.failures.push(String(event.reason)),
);

async function lists() {
    let app = mount(Lists, { target });
    try {
        await settle();
        target.querySelector('[data-virtual-index="6"] button').focus();
        await key("Home");
        check("Home skips the first group heading", activeRow(), 1);
        await key("ArrowUp");
        check("ArrowUp stays on the first track", activeRow(), 1);
        await key("End");
        check("End skips the trailing group heading", activeRow(), 11);
        await key("ArrowDown");
        check("ArrowDown stays on the last track", activeRow(), 11);
        target.querySelector('[data-virtual-index="5"] button').focus();
        await key("ArrowUp");
        check("ArrowUp skips a group heading", activeRow(), 3);
        await key("ArrowDown");
        check("ArrowDown skips a group heading", activeRow(), 5);
    } finally {
        await unmount(app);
    }
    app = mount(Lists, { target, props: { large: true } });
    try {
        await settle();
        target.querySelector('[data-virtual-index="0"] button').focus();
        await key("End");
        check("End reaches an unloaded queue page", activeRow(), 99999);
        check(
            "Large queues keep DOM rows bounded",
            target.querySelectorAll("[data-virtual-index]").length < 40,
            true,
        );
        await key("Home");
        check("Home returns to the first queue row", activeRow(), 0);
    } finally {
        await unmount(app);
    }
}

async function overlappingPlaylistRead(failRefresh) {
    reads.length = 0;
    removal = deferred();
    navigate(1);
    const app = mount(Playlist, { target });
    try {
        await settle();
        reads[0].resolve(playlist(1));
        await settle();
        target.querySelector("button.remove-track").click();
        await settle();
        navigate(2);
        await settle();
        reads[1].resolve(playlist(2));
        await settle();
        navigate(1);
        await settle();
        removal.resolve();
        await settle();
        check("Removal refresh starts after returning to A", reads.length, 4);
        if (failRefresh) reads[3].reject(Error("Refresh failed"));
        else reads[3].resolve(playlist(1, 2));
        await settle();
        // The older route read completes last and must not overwrite the refresh.
        reads[2].resolve(playlist(1));
        await settle();
        if (failRefresh) {
            check(
                "A failed refresh settles the pending route load",
                target
                    .querySelector(".error")
                    ?.textContent.includes("Refresh failed"),
                true,
            );
        } else {
            check(
                "A refresh settles the pending route load",
                target.querySelector(".playlist-title")?.textContent,
                "Playlist 1",
            );
            check(
                "Stale route results cannot restore removed tracks",
                target.querySelectorAll("button.remove-track").length,
                2,
            );
        }
    } finally {
        await unmount(app);
    }
}

async function playlistTeardown() {
    for (const deleting of [false, true]) {
        reads.length = 0;
        navigations.length = 0;
        removal = deferred();
        deletion = deferred();
        navigate(1);
        const app = mount(Playlist, { target });
        await settle();
        reads[0].resolve(playlist(1));
        await settle();
        target
            .querySelector(
                deleting ? ".delete-playlist-btn" : "button.remove-track",
            )
            .click();
        await settle();
        await unmount(app);
        (deleting ? deletion : removal).resolve();
        await settle();
        check("A destroyed playlist cannot start a refresh", reads.length, 1);
        check(
            "A destroyed playlist cannot redirect the new page",
            navigations.length,
            0,
        );
        // Settle any unexpected read so a failure cannot affect the next case.
        if (reads.length > 1) reads[1].resolve(playlist(1));
    }
}

try {
    await lists();
    await overlappingPlaylistRead(false);
    await overlappingPlaylistRead(true);
    await playlistTeardown();
} catch (error) {
    result.failures.push(String(error.stack ?? error));
} finally {
    document.body.dataset.testResult = encodeURIComponent(
        JSON.stringify(result),
    );
}
