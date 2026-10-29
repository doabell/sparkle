import { uiPref } from "#lib/stores/uiPrefs.ts";
import type { SongIndexLanguage } from "#lib/utils/songIndex.ts";

export const songIndexLanguage = uiPref<SongIndexLanguage>(
    "songs.indexLanguage",
    "auto",
);
