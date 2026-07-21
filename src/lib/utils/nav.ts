import { goto } from "$app/navigation";
import { get } from "svelte/store";
import { page } from "$app/stores";

// Navigating to the page you're already on replaces instead of pushing, so
// Back never walks through duplicate copies of the same page (e.g. album
// cover → now playing, then lyrics → now playing = one Back to return).
export function smartGo(url: string) {
    if (get(page).url.pathname === url) {
        goto(url, { replaceState: true });
    } else {
        goto(url);
    }
}
