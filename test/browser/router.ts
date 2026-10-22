import { writable } from "svelte/store";

export const page = writable({
    params: { id: "1" },
    url: new URL("https://sparkle.test/playlists/1"),
});
export const navigations: string[] = [];
export const goto = async (url: string) => {
    navigations.push(url);
};

export function navigate(id: number) {
    page.set({
        params: { id: String(id) },
        url: new URL(`https://sparkle.test/playlists/${id}`),
    });
}
