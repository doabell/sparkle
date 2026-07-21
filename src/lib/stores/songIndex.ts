import { uiPref } from "$lib/stores/uiPrefs";
import type { SongIndexLanguage } from "$lib/utils/songIndex";

export const songIndexLanguage = uiPref<SongIndexLanguage>(
    "songs.indexLanguage",
    "auto",
);
