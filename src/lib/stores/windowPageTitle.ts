import { writable } from "svelte/store";

// Detail pages replace the generic route label after their entity has loaded.
// The root layout owns the fallback and combines this with playback state.
export const windowPageTitle = writable<string | null>(null);
