<script lang="ts">
    import { onMount } from "svelte";
    import { listen } from "@tauri-apps/api/event";
    import {
        getOnlineSettings,
        setOnlineSettings,
        testArtworkStorage as testArtworkStorageApi,
        clearLyricsCache,
        clearArtistInfoCache,
        clearImagesCache,
        clearAllCaches,
        getCacheStats,
        getCacheDir,
        getStatus,
        getLoudnessStatus,
        scanLoudness,
        rescanLoudness,
        exportLibraryBackup,
        inspectLibraryBackup,
        importLibraryBackup,
        pickBackupToSave,
        pickBackupToImport,
        revealInExplorer,
        type AppStatus,
        type AccentForegroundPreference,
        type OnlineSettings,
        type CacheStat,
        type BackupManifest,
        type BackupSections,
        type LoudnessStatus,
    } from "$lib/api";
    import Loading from "$lib/components/Loading.svelte";
    import Select from "$lib/components/Select.svelte";
    import { addToast } from "$lib/stores/toast";
    import { songIndexLanguage } from "$lib/stores/songIndex";
    import {
        nowPlayingLayout,
        type NowPlayingLayout,
    } from "$lib/stores/uiPrefs";
    import { getFontStack } from "$lib/utils/fonts";
    import {
        DEFAULT_ACCENT_COLOR,
        applyAccent,
        cacheAccent,
        createAccentTheme,
        normalizeAccentForegroundPreference,
        normalizeHex,
        type AccentPalette,
    } from "$lib/utils/theme";
    import type { SongIndexLanguage } from "$lib/utils/songIndex";

    const DEFAULT_SPLIT_REGEX = ";";
    const DEFAULT_SPLIT_EXCEPTIONS = ["AC/DC", "Tyler, The Creator"];
    const DEFAULT_BACKUP_SECTIONS: BackupSections = {
        settings: true,
        playlists: true,
        custom_metadata: true,
        history: true,
    };

    const ARTWORK_STORE_OPTIONS = [
        { value: "disabled", label: "Disabled — no artwork upload" },
        { value: "catbox", label: "Catbox" },
        { value: "s3", label: "S3-compatible storage" },
    ];

    const SONG_INDEX_LANGUAGE_OPTIONS = [
        { value: "auto", label: "Automatic (system)" },
        { value: "en", label: "English / Latin" },
        { value: "ja", label: "日本語 (Japanese)" },
    ];

    const ACCENT_FOREGROUND_OPTIONS: {
        value: AccentForegroundPreference;
        label: string;
    }[] = [
        { value: "auto", label: "Automatic" },
        { value: "light", label: "Light (white)" },
        { value: "dark", label: "Dark (black)" },
    ];

    const NOW_PLAYING_LAYOUTS: {
        value: NowPlayingLayout;
        label: string;
        description: string;
    }[] = [
        {
            value: "album",
            label: "Album",
            description: "Large square cover with balanced lyrics",
        },
        {
            value: "artist",
            label: "Artist",
            description: "Circular portrait with the current album inset",
        },
        {
            value: "lyrics",
            label: "Lyrics",
            description: "Type-led reading with compact artwork",
        },
    ];

    const SHORTCUTS: { keys: string; action: string }[] = [
        { keys: "Space", action: "Play / pause" },
        { keys: "\u2190 / \u2192", action: "Seek \u00b15 s" },
        { keys: "Ctrl+\u2190 / Ctrl+\u2192", action: "Previous / next track" },
        { keys: "\u2191 / \u2193", action: "Volume \u00b15%" },
    ];

    const LICENSES = [
        { name: "Sparkle", license: "MIT", href: null },
        {
            name: "MusicBee-NeteaseLyrics",
            license: "Apache-2.0",
            href: "https://github.com/cqjjjzr/MusicBee-NeteaseLyrics",
        },
        {
            name: "MusicBee-QQLyrics",
            license: "Apache-2.0",
            href: "https://github.com/mslxl/MusicBee-QQLyrics",
        },
        {
            name: "ZonyLrcToolsX",
            license: "MIT",
            href: "https://github.com/real-zony/ZonyLrcToolsX",
        },
        {
            name: "KashiNaviLyricsPlugin",
            license: "MIT",
            href: "https://github.com/noriokun4649/mb_KashiNaviLyricsPlugin",
        },
        {
            name: "MusicBeePluginTemplate",
            license: "MIT",
            href: "https://github.com/htsign/MusicBeePluginTemplate",
        },
        {
            name: "DiscordBee",
            license: "Apache-2.0",
            href: "https://github.com/sll552/DiscordBee",
        },
    ] as const;

    const SETTINGS_CATEGORIES = [
        {
            id: "appearance",
            label: "Appearance",
            description: "Theme, type, and motion",
        },
        {
            id: "playback",
            label: "Playback",
            description: "Sound Check",
        },
        {
            id: "library",
            label: "Library",
            description: "Scanning and metadata",
        },
        {
            id: "sharing",
            label: "Sharing",
            description: "Discord Rich Presence",
        },
        {
            id: "data",
            label: "Data & storage",
            description: "Backups and cache",
        },
        {
            id: "advanced",
            label: "Advanced",
            description: "Shortcuts, logs, licenses",
        },
    ] as const;

    type SettingsCategory = (typeof SETTINGS_CATEGORIES)[number]["id"];

    interface SourceCategory {
        key:
            | "lyrics_sources"
            | "artist_info_sources"
            | "artist_image_sources"
            | "album_art_sources";
        label: string;
        hint: string;
        builtins: readonly string[];
        wikipedia: boolean;
    }

    const SOURCE_CATEGORIES: SourceCategory[] = [
        {
            key: "lyrics_sources",
            label: "Lyrics",
            hint: "Tried from top to bottom.",
            builtins: [
                "custom",
                "embedded",
                "lrc",
                "lrclib",
                "netease",
                "kashinavi",
                "qq",
            ],
            wikipedia: false,
        },
        {
            key: "artist_info_sources",
            label: "Artist info",
            hint: "Tried from top to bottom.",
            builtins: ["custom"],
            wikipedia: true,
        },
        {
            key: "artist_image_sources",
            label: "Artist images",
            hint: "Tried from top to bottom.",
            builtins: [
                "custom",
                "wikipedia:en",
                "shazam",
                "brave",
                "duckduckgo",
            ],
            wikipedia: true,
        },
        {
            key: "album_art_sources",
            label: "Album art",
            hint: "Tried from top to bottom.",
            builtins: ["custom", "embedded", "cover_art_archive"],
            wikipedia: false,
        },
    ];

    const PROVIDER_LABELS: Record<string, string> = {
        custom: "Custom (yours)",
        embedded: "Embedded tags",
        lrc: "Sidecar .lrc files",
        lrclib: "LRCLIB",
        netease: "NetEase",
        kashinavi: "KashiNavi",
        qq: "QQ Music",
        shazam: "Shazam / Apple Music",
        brave: "Brave Image Search",
        duckduckgo: "DuckDuckGo Images",
        cover_art_archive: "Cover Art Archive",
    };

    const ACCENT_PRESETS = [
        "#fa243c",
        "#fa5a24",
        "#fa24a8",
        "#a855f7",
        "#3b82f6",
        "#14b8a6",
        "#22c55e",
        "#eab308",
    ];

    function providerLabel(source: string): string {
        if (source.startsWith("wikipedia:")) {
            return `Wikipedia (${source.slice("wikipedia:".length)})`;
        }
        return PROVIDER_LABELS[source] ?? source;
    }

    const WIKIPEDIA_LANGUAGES = [
        "en",
        "zh",
        "ja",
        "ko",
        "de",
        "fr",
        "es",
        "it",
        "pt",
        "ru",
        "nl",
        "pl",
        "sv",
        "fi",
        "no",
        "da",
        "cs",
        "tr",
        "uk",
        "ar",
        "hi",
        "id",
        "th",
        "vi",
    ];

    let settings = $state<OnlineSettings | null>(null);
    let activeCategory = $state<SettingsCategory>("appearance");
    let activeCategoryInfo = $derived(
        SETTINGS_CATEGORIES.find(
            (category) => category.id === activeCategory,
        ) ?? SETTINGS_CATEGORIES[0],
    );
    let accentInput = $state(DEFAULT_ACCENT_COLOR);
    let accentInputInvalid = $derived(normalizeHex(accentInput) === null);
    let accentTheme = $derived(
        createAccentTheme(
            settings?.accent_color ?? DEFAULT_ACCENT_COLOR,
            normalizeAccentForegroundPreference(
                settings?.accent_foreground_preference,
            ),
        ),
    );
    let clearing = $state<string | null>(null);
    let cacheStats = $state<CacheStat[]>([]);
    let cacheDir = $state<string | null>(null);
    let status = $state<AppStatus | null>(null);
    let loudnessStatus = $state<LoudnessStatus | null>(null);
    let loudnessActionBusy = $state<"scan" | "rescan" | null>(null);
    let backupBusy = $state<"export" | "inspect" | "import" | null>(null);
    let exportSections = $state<BackupSections>({
        ...DEFAULT_BACKUP_SECTIONS,
    });
    let restoreSections = $state<BackupSections>({
        ...DEFAULT_BACKUP_SECTIONS,
    });
    let pendingBackup = $state<{
        path: string;
        name: string;
        manifest: BackupManifest;
    } | null>(null);
    let lastBackup = $state<BackupManifest | null>(null);

    let canExport = $derived(
        exportSections.settings ||
            exportSections.playlists ||
            exportSections.custom_metadata ||
            exportSections.history,
    );
    let canRestore = $derived(
        restoreSections.settings ||
            restoreSections.playlists ||
            restoreSections.custom_metadata ||
            restoreSections.history,
    );

    // --- Autosave ----------------------------------------------------------
    // Every mutation is persisted after a short debounce; no Save button.
    // Artist-rule changes only flag that a rescan is needed — scanning is a
    // manual action so settings stay responsive.
    let saveState = $state<"clean" | "dirty" | "saving" | "saved">("clean");
    let saveTimer: ReturnType<typeof setTimeout> | null = null;
    let lastSavedJson = "";
    let lastSplitKey = "";
    let rulesNeedRescan = $state(false);
    let artworkStorageTestBusy = $state(false);
    let artworkStorageTestUrl = $state<string | null>(null);

    $effect(() => {
        if (!settings) return;
        const json = JSON.stringify(settings);
        if (json === lastSavedJson) return;
        saveState = "dirty";
        if (saveTimer) clearTimeout(saveTimer);
        saveTimer = setTimeout(() => void autosave(json), 800);
    });

    async function autosave(json: string) {
        if (!settings) return;
        saveState = "saving";
        try {
            await setOnlineSettings(settings);
            cacheAccent(
                settings.accent_color,
                settings.accent_foreground_preference,
            );
            lastSavedJson = json;
            saveState = "saved";
            const splitKey = JSON.stringify([
                settings.artist_split_regex,
                settings.artist_split_exceptions,
            ]);
            if (lastSplitKey && splitKey !== lastSplitKey) {
                rulesNeedRescan = true;
            }
            lastSplitKey = splitKey;
        } catch (e) {
            saveState = "dirty";
            addToast(String(e), "error");
        }
    }

    async function runSoundCheckScan(action: "scan" | "rescan") {
        loudnessActionBusy = action;
        try {
            await (action === "scan" ? scanLoudness() : rescanLoudness());
            loudnessStatus = await getLoudnessStatus();
            addToast(
                action === "scan" ? "Scan started" : "Rescan started",
                "success",
            );
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            loudnessActionBusy = null;
        }
    }

    function chooseArtworkStore(value: string) {
        if (!settings) return;
        settings.discord_artwork_store = value;
        artworkStorageTestUrl = null;
    }

    async function testArtworkStorage() {
        if (!settings || settings.discord_artwork_store === "disabled") return;
        const store = settings.discord_artwork_store;
        artworkStorageTestBusy = true;
        artworkStorageTestUrl = null;
        try {
            const url = await testArtworkStorageApi();
            artworkStorageTestUrl = store === "s3" ? null : url;
            addToast(
                store === "s3" ? "S3 test passed" : "Catbox test passed",
                "success",
            );
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            artworkStorageTestBusy = false;
        }
    }

    async function backupLibrary() {
        const path = await pickBackupToSave();
        if (!path) return;
        backupBusy = "export";
        try {
            lastBackup = await exportLibraryBackup(path, exportSections);
            addToast("Backup saved", "success");
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            backupBusy = null;
        }
    }

    async function chooseBackup() {
        const path = await pickBackupToImport();
        if (!path) return;
        backupBusy = "inspect";
        try {
            const manifest = await inspectLibraryBackup(path);
            pendingBackup = {
                path,
                name: backupFileName(path),
                manifest,
            };
            restoreSections = {
                settings: manifest.settings,
                playlists: manifest.playlists > 0,
                custom_metadata:
                    manifest.lyrics + manifest.artist_bios + manifest.artwork >
                    0,
                history: manifest.history > 0,
            };
        } catch (e) {
            pendingBackup = null;
            addToast(String(e), "error");
        } finally {
            backupBusy = null;
        }
    }

    async function restoreLibrary() {
        if (!pendingBackup) return;
        backupBusy = "import";
        try {
            const summary = await importLibraryBackup(
                pendingBackup.path,
                restoreSections,
            );
            if (summary.settings) {
                const restoredSettings = await getOnlineSettings();
                normalizeLoadedAppearance(restoredSettings);
                settings = restoredSettings;
                lastSavedJson = JSON.stringify(restoredSettings);
            }
            const skipped =
                summary.unmatched_tracks + summary.unmatched_artwork;
            addToast(
                skipped > 0
                    ? `${skipped} item${skipped === 1 ? "" : "s"} skipped`
                    : "Restore complete",
                "success",
            );
            pendingBackup = null;
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            backupBusy = null;
        }
    }

    function backupFileName(path: string): string {
        return path.split(/[\\/]/).pop() || "Sparkle backup";
    }

    function formatBackupDate(timestamp: number): string {
        return new Intl.DateTimeFormat(undefined, {
            dateStyle: "medium",
            timeStyle: "short",
        }).format(new Date(timestamp * 1000));
    }

    function formatBackupSize(bytes: number): string {
        if (bytes < 1024) return `${bytes} B`;
        if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
        return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    }

    function normalizeLoadedAppearance(loaded: OnlineSettings) {
        loaded.accent_color =
            normalizeHex(loaded.accent_color ?? "") ?? DEFAULT_ACCENT_COLOR;
        loaded.accent_foreground_preference =
            normalizeAccentForegroundPreference(
                loaded.accent_foreground_preference,
            );
        accentInput = loaded.accent_color;
    }

    onMount(async () => {
        try {
            const loaded = await getOnlineSettings();
            if (!loaded.artist_split_regex) {
                loaded.artist_split_regex = DEFAULT_SPLIT_REGEX;
            }
            if (!loaded.artist_split_exceptions?.length) {
                loaded.artist_split_exceptions = [...DEFAULT_SPLIT_EXCEPTIONS];
            }
            if (!loaded.ui_font) {
                loaded.ui_font = "System";
            }
            if (!loaded.lyrics_font) {
                loaded.lyrics_font = "Monospace";
            }
            if (!loaded.brave_api_key) {
                loaded.brave_api_key = "";
            }
            normalizeLoadedAppearance(loaded);
            if (typeof loaded.discord_enabled !== "boolean") {
                loaded.discord_enabled = true;
            }
            if (typeof loaded.discord_app_id !== "string") {
                loaded.discord_app_id = "";
            }
            if (!loaded.discord_catbox_user_hash) {
                loaded.discord_catbox_user_hash = "";
            }
            if (
                !["disabled", "catbox", "s3"].includes(
                    loaded.discord_artwork_store,
                )
            ) {
                loaded.discord_artwork_store = "catbox";
            }
            if (typeof loaded.discord_artwork_s3_endpoint !== "string") {
                loaded.discord_artwork_s3_endpoint = "";
            }
            if (typeof loaded.discord_artwork_s3_bucket !== "string") {
                loaded.discord_artwork_s3_bucket = "";
            }
            if (typeof loaded.discord_artwork_s3_public_url !== "string") {
                loaded.discord_artwork_s3_public_url = "";
            }
            if (typeof loaded.discord_artwork_s3_access_key !== "string") {
                loaded.discord_artwork_s3_access_key = "";
            }
            if (typeof loaded.discord_artwork_s3_secret_key !== "string") {
                loaded.discord_artwork_s3_secret_key = "";
            }
            if (typeof loaded.discord_artwork_s3_session_token !== "string") {
                loaded.discord_artwork_s3_session_token = "";
            }
            if (typeof loaded.discord_artwork_s3_region !== "string") {
                loaded.discord_artwork_s3_region = "";
            }
            if (typeof loaded.discord_artwork_s3_prefix !== "string") {
                loaded.discord_artwork_s3_prefix = "";
            }
            if (typeof loaded.debug_logging_enabled !== "boolean") {
                loaded.debug_logging_enabled = false;
            }
            if (typeof loaded.sound_check_enabled !== "boolean") {
                loaded.sound_check_enabled = false;
            }
            settings = loaded;
            lastSavedJson = JSON.stringify(loaded);
            lastSplitKey = JSON.stringify([
                loaded.artist_split_regex,
                loaded.artist_split_exceptions,
            ]);
            displayOrders = Object.fromEntries(
                SOURCE_CATEGORIES.map((c) => {
                    const enabled = loaded[c.key] as string[];
                    return [
                        c.key,
                        [
                            ...enabled,
                            ...c.builtins.filter((b) => !enabled.includes(b)),
                        ],
                    ];
                }),
            );
            applyUiFont(settings.ui_font);
            applyMotion(settings.reduce_motion);
        } catch (e) {
            addToast(String(e), "error");
        }
        refreshCacheStats();
        getStatus()
            .then((s) => (status = s))
            .catch(() => (status = null));
        getCacheDir()
            .then((d) => (cacheDir = d))
            .catch(() => (cacheDir = null));
    });

    onMount(() => {
        getLoudnessStatus()
            .then((value) => (loudnessStatus = value))
            .catch(() => (loudnessStatus = null));
        const unlisten = listen<LoudnessStatus>(
            "loudness-status-changed",
            ({ payload }) => (loudnessStatus = payload),
        );
        return () => void unlisten.then((dispose) => dispose());
    });

    async function refreshCacheStats() {
        try {
            cacheStats = await getCacheStats();
        } catch {
            cacheStats = [];
        }
    }

    function formatBytes(bytes: number): string {
        if (bytes < 1024) return `${bytes} B`;
        if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
        return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    }

    function statFor(name: string): CacheStat | undefined {
        return cacheStats.find((s) => s.name === name);
    }

    $effect(() => {
        if (settings?.ui_font) {
            applyUiFont(settings.ui_font);
        }
    });

    $effect(() => {
        if (settings) {
            applyMotion(settings.reduce_motion);
        }
    });

    $effect(() => {
        if (settings?.accent_color) {
            applyAccent(
                settings.accent_color,
                normalizeAccentForegroundPreference(
                    settings.accent_foreground_preference,
                ),
            );
        }
    });

    function chooseAccent(color: string) {
        if (!settings) return;
        const normalized = normalizeHex(color) ?? DEFAULT_ACCENT_COLOR;
        settings.accent_color = normalized;
        accentInput = normalized;
    }

    function handleAccentInput(e: Event) {
        if (!settings) return;
        accentInput = (e.currentTarget as HTMLInputElement).value;
        const normalized = normalizeHex(accentInput);
        if (normalized) {
            settings.accent_color = normalized;
        }
    }

    function normalizeAccentInput() {
        const normalized = normalizeHex(accentInput);
        if (normalized) accentInput = normalized;
    }

    function chooseAccentForeground(value: string) {
        if (!settings) return;
        settings.accent_foreground_preference =
            normalizeAccentForegroundPreference(value);
    }

    function swatchForeground(color: string) {
        return createAccentTheme(color).dark.onFill;
    }

    function accentPreviewStyle(
        palette: AccentPalette,
        mode: "dark" | "light",
    ) {
        const background = mode === "dark" ? "#171719" : "#ffffff";
        const surface = mode === "dark" ? "#242426" : "#e9e9ed";
        const text = mode === "dark" ? "#ffffff" : "#111113";
        const muted = mode === "dark" ? "#b3b3b3" : "#5a5a5a";
        return [
            `--preview-background: ${background}`,
            `--preview-surface: ${surface}`,
            `--preview-text: ${text}`,
            `--preview-muted: ${muted}`,
            `--preview-fill: ${palette.fill}`,
            `--preview-fill-hover: ${palette.fillHover}`,
            `--preview-fill-disabled: ${palette.fillDisabled}`,
            `--preview-on-fill: ${palette.onFill}`,
            `--preview-on-fill-disabled: ${palette.onFillDisabled}`,
            `--preview-subtle: ${palette.subtle}`,
            `--preview-on-subtle: ${palette.onSubtle}`,
            `--preview-focus: ${palette.focus}`,
        ].join("; ");
    }

    function applyUiFont(fontName: string) {
        document.documentElement.style.setProperty(
            "--font-family",
            getFontStack(fontName),
        );
    }

    function applyMotion(reduce: boolean) {
        document.documentElement.dataset.motion = reduce ? "reduced" : "full";
    }

    async function clearCache(type: string, fn: () => Promise<void>) {
        clearing = type;
        try {
            await fn();
            addToast(`${type} cache cleared`, "success");
            await refreshCacheStats();
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            clearing = null;
        }
    }

    function addSource(key: keyof OnlineSettings, value: string) {
        if (!settings || !value.trim()) return;
        const list = settings[key] as string[];
        if (!list.includes(value.trim())) {
            (settings[key] as string[]) = [...list, value.trim()];
            if (
                displayOrders[key] &&
                !displayOrders[key].includes(value.trim())
            ) {
                displayOrders[key] = [...displayOrders[key], value.trim()];
            }
        }
    }

    function removeSource(key: keyof OnlineSettings, value: string) {
        if (!settings) return;
        (settings[key] as string[]) = (settings[key] as string[]).filter(
            (s) => s !== value,
        );
    }

    // Visual order of every provider row (enabled + disabled) per category.
    // Disabled rows stay in place, just dimmed — nothing jumps to the bottom.
    let displayOrders = $state<Record<string, string[]>>({});

    function toggleSource(
        category: SourceCategory,
        source: string,
        enable: boolean,
    ) {
        if (!settings) return;
        const key = category.key;
        const enabled = [...(settings[key] as string[])];
        if (enable) {
            // Re-insert where the row sits in the visual order.
            const order = displayOrders[key] ?? [];
            const idx = order.indexOf(source);
            let insertAt = enabled.length;
            for (let i = idx + 1; i < order.length; i++) {
                const j = enabled.indexOf(order[i]);
                if (j !== -1) {
                    insertAt = j;
                    break;
                }
            }
            enabled.splice(insertAt, 0, source);
        } else {
            const i = enabled.indexOf(source);
            if (i !== -1) enabled.splice(i, 1);
        }
        (settings[key] as string[]) = enabled;
    }

    function moveSource(
        category: SourceCategory,
        source: string,
        direction: -1 | 1,
    ) {
        if (!settings) return;
        const key = category.key;
        const enabled = settings[key] as string[];
        const order = [...(displayOrders[key] ?? [])];
        const i = order.indexOf(source);
        // Swap with the next enabled row in the visual order.
        let j = i + direction;
        while (j >= 0 && j < order.length && !enabled.includes(order[j])) {
            j += direction;
        }
        if (j < 0 || j >= order.length) return;
        [order[i], order[j]] = [order[j], order[i]];
        displayOrders[key] = order;
        (settings[key] as string[]) = order.filter((s) => enabled.includes(s));
    }

    let wikipediaLangChoice = $state<Record<string, string>>({});

    function addWikipediaSource(key: keyof OnlineSettings) {
        const lang = (wikipediaLangChoice[key] ?? "en").trim().toLowerCase();
        if (!lang) return;
        addSource(key, `wikipedia:${lang}`);
    }

    function handleSourceKeydown(e: KeyboardEvent, key: keyof OnlineSettings) {
        const input = e.currentTarget as HTMLInputElement;
        if (e.key === "Enter" || e.key === ",") {
            e.preventDefault();
            addSource(key, input.value);
            input.value = "";
        }
    }

    function handleSourceBlur(e: FocusEvent, key: keyof OnlineSettings) {
        const input = e.currentTarget as HTMLInputElement;
        if (input.value.trim()) {
            addSource(key, input.value);
            input.value = "";
        }
    }
</script>

{#snippet sectionTitle(title: string)}
    <div class="section-heading">
        <h3 class="section-title">{title}</h3>
    </div>
{/snippet}

{#snippet fieldLabel(label: string, htmlFor: string)}
    <label for={htmlFor}>{label}</label>
{/snippet}

{#snippet hint(text: string)}
    <p class="hint">{text}</p>
{/snippet}

{#snippet pillList(key: keyof OnlineSettings, placeholder: string)}
    {#if settings}
        {@const sources = settings[key] as string[]}
        <div
            class="pill-list"
            tabindex={0}
            role="button"
            aria-label="Focus source input"
            onclick={(e) => {
                const input = (e.currentTarget as HTMLElement).querySelector(
                    "input",
                );
                input?.focus();
            }}
            onkeydown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    const input = (
                        e.currentTarget as HTMLElement
                    ).querySelector("input");
                    input?.focus();
                }
            }}
        >
            {#each sources as source (source)}
                <span class="pill" role="listitem">
                    <span class="pill-text">{source}</span>
                    <button
                        class="pill-remove"
                        aria-label={`Remove ${source}`}
                        onclick={() => removeSource(key, source)}
                    >
                        <svg
                            viewBox="0 0 24 24"
                            fill="currentColor"
                            aria-hidden="true"
                        >
                            <path
                                d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"
                            />
                        </svg>
                    </button>
                </span>
            {/each}
            <input
                type="text"
                class="pill-input"
                {placeholder}
                onkeydown={(e) => handleSourceKeydown(e, key)}
                onblur={(e) => handleSourceBlur(e, key)}
            />
        </div>
    {/if}
{/snippet}

{#snippet sortableSourceList(category: SourceCategory)}
    {#if settings}
        {@const key = category.key}
        {@const enabled = settings[key] as string[]}
        {@const order = displayOrders[key] ?? enabled}
        <div class="field">
            <span class="field-label">{category.label}</span>
            <div class="source-list" role="list" aria-label={category.label}>
                {#each order as source, index (source)}
                    {@const isEnabled = enabled.includes(source)}
                    <div
                        class="source-row"
                        class:disabled={!isEnabled}
                        role="listitem"
                    >
                        <span class="source-name">{providerLabel(source)}</span>
                        <div class="source-actions">
                            {#if isEnabled}
                                <button
                                    class="source-action"
                                    aria-label={`Move ${providerLabel(source)} up`}
                                    onclick={() =>
                                        moveSource(category, source, -1)}
                                >
                                    <svg
                                        viewBox="0 0 24 24"
                                        fill="currentColor"
                                        aria-hidden="true"
                                    >
                                        <path
                                            d="M7.41 15.41L12 10.83l4.59 4.58L18 14l-6-6-6 6z"
                                        />
                                    </svg>
                                </button>
                                <button
                                    class="source-action"
                                    aria-label={`Move ${providerLabel(source)} down`}
                                    onclick={() =>
                                        moveSource(category, source, 1)}
                                >
                                    <svg
                                        viewBox="0 0 24 24"
                                        fill="currentColor"
                                        aria-hidden="true"
                                    >
                                        <path
                                            d="M7.41 8.59L12 13.17l4.59-4.58L18 10l-6 6-6-6z"
                                        />
                                    </svg>
                                </button>
                            {/if}
                            <button
                                class="switch"
                                class:on={isEnabled}
                                role="switch"
                                aria-checked={isEnabled}
                                aria-label={isEnabled
                                    ? `Disable ${providerLabel(source)}`
                                    : `Enable ${providerLabel(source)}`}
                                onclick={() =>
                                    toggleSource(category, source, !isEnabled)}
                            >
                                <span class="switch-thumb" aria-hidden="true"
                                ></span>
                            </button>
                        </div>
                    </div>
                {/each}
                {#if category.wikipedia}
                    <div class="source-row add-row" role="listitem">
                        <span class="source-name add-label"
                            >Add Wikipedia language</span
                        >
                        <div class="source-actions">
                            <Select
                                options={WIKIPEDIA_LANGUAGES.map((l) => ({
                                    value: l,
                                    label: l,
                                }))}
                                value={wikipediaLangChoice[key] ?? "en"}
                                onchange={(v) =>
                                    (wikipediaLangChoice = {
                                        ...wikipediaLangChoice,
                                        [key]: v,
                                    })}
                                ariaLabel={`Wikipedia language for ${category.label}`}
                            />
                            <button
                                class="btn-pill btn-secondary add-btn"
                                onclick={() => addWikipediaSource(key)}
                            >
                                Add
                            </button>
                        </div>
                    </div>
                {/if}
            </div>
            {@render hint(category.hint)}
        </div>
    {/if}
{/snippet}

<div class="settings-page">
    <div class="header">
        <div class="page-heading">
            <h1 class="page-title">Settings</h1>
        </div>
        {#if settings}
            <span class="save-indicator" role="status">
                {#if saveState === "saving"}
                    Saving…
                {:else if saveState === "saved"}
                    Saved
                {:else if saveState === "dirty"}
                    Editing…
                {:else}
                    Saves automatically
                {/if}
            </span>
        {/if}
    </div>

    {#if settings}
        <nav class="settings-index" aria-label="Settings categories">
            {#each SETTINGS_CATEGORIES as category (category.id)}
                <button
                    type="button"
                    class:active={activeCategory === category.id}
                    aria-current={activeCategory === category.id
                        ? "page"
                        : undefined}
                    aria-controls="settings-category-content"
                    onclick={() => (activeCategory = category.id)}
                >
                    <span>{category.label}</span>
                    <small>{category.description}</small>
                </button>
            {/each}
        </nav>

        <div
            class="category-heading"
            id="settings-category-content"
            tabindex="-1"
        >
            <h2>{activeCategoryInfo.label}</h2>
        </div>

        <div class="form-card" hidden={activeCategory !== "data"}>
            {@render sectionTitle("Backup & restore")}
            <p class="privacy-note">Files and keys stay local.</p>
            <div class="backup-grid">
                <section
                    class="backup-panel"
                    aria-labelledby="create-backup-title"
                >
                    <div class="backup-panel-heading">
                        <h3 id="create-backup-title">Create backup</h3>
                        <span>Compressed</span>
                    </div>
                    <div class="backup-options">
                        <label class="backup-option">
                            <input
                                type="checkbox"
                                bind:checked={exportSections.settings}
                            />
                            <span
                                ><strong>Settings</strong><small
                                    >Appearance and providers</small
                                ></span
                            >
                        </label>
                        <label class="backup-option">
                            <input
                                type="checkbox"
                                bind:checked={exportSections.playlists}
                            />
                            <span
                                ><strong>Playlists</strong><small
                                    >Order and descriptions</small
                                ></span
                            >
                        </label>
                        <label class="backup-option">
                            <input
                                type="checkbox"
                                bind:checked={exportSections.custom_metadata}
                            />
                            <span
                                ><strong>Custom metadata</strong><small
                                    >Lyrics, bios, and artwork</small
                                ></span
                            >
                        </label>
                        <label class="backup-option">
                            <input
                                type="checkbox"
                                bind:checked={exportSections.history}
                            />
                            <span
                                ><strong>Listening history</strong><small
                                    >Playback history</small
                                ></span
                            >
                        </label>
                    </div>
                    <button
                        class="btn-pill btn-primary backup-button"
                        disabled={backupBusy !== null || !canExport}
                        onclick={backupLibrary}
                    >
                        {backupBusy === "export"
                            ? "Creating…"
                            : "Create backup"}
                    </button>
                    {#if lastBackup}
                        <p class="backup-status">
                            Saved {formatBackupSize(lastBackup.file_size_bytes)} ·
                            {lastBackup.tracks.toLocaleString()} songs
                        </p>
                    {/if}
                </section>

                <section
                    class="backup-panel"
                    aria-labelledby="restore-backup-title"
                >
                    <div class="backup-panel-heading">
                        <h3 id="restore-backup-title">Restore backup</h3>
                        <span>Preview first</span>
                    </div>
                    <button
                        class="btn-pill btn-secondary backup-button"
                        disabled={backupBusy !== null}
                        onclick={chooseBackup}
                    >
                        {backupBusy === "inspect"
                            ? "Checking…"
                            : pendingBackup
                              ? "Choose another file"
                              : "Choose backup"}
                    </button>

                    {#if pendingBackup}
                        <div class="backup-preview">
                            <strong>{pendingBackup.name}</strong>
                            <small>
                                {formatBackupDate(
                                    pendingBackup.manifest.created_at,
                                )} · {formatBackupSize(
                                    pendingBackup.manifest.file_size_bytes,
                                )}
                            </small>
                            <dl>
                                <div>
                                    <dt>Songs</dt>
                                    <dd>
                                        {pendingBackup.manifest.tracks.toLocaleString()}
                                    </dd>
                                </div>
                                <div>
                                    <dt>Playlists</dt>
                                    <dd>
                                        {pendingBackup.manifest.playlists.toLocaleString()}
                                    </dd>
                                </div>
                                <div>
                                    <dt>Custom items</dt>
                                    <dd>
                                        {(
                                            pendingBackup.manifest.lyrics +
                                            pendingBackup.manifest.artist_bios +
                                            pendingBackup.manifest.artwork
                                        ).toLocaleString()}
                                    </dd>
                                </div>
                                <div>
                                    <dt>Listens</dt>
                                    <dd>
                                        {pendingBackup.manifest.history.toLocaleString()}
                                    </dd>
                                </div>
                            </dl>
                        </div>
                        <div class="backup-options restore-options">
                            <label
                                class:unavailable={!pendingBackup.manifest
                                    .settings}
                                class="backup-option"
                            >
                                <input
                                    type="checkbox"
                                    bind:checked={restoreSections.settings}
                                    disabled={!pendingBackup.manifest.settings}
                                />
                                <span><strong>Settings</strong></span>
                            </label>
                            <label
                                class:unavailable={pendingBackup.manifest
                                    .playlists === 0}
                                class="backup-option"
                            >
                                <input
                                    type="checkbox"
                                    bind:checked={restoreSections.playlists}
                                    disabled={pendingBackup.manifest
                                        .playlists === 0}
                                />
                                <span><strong>Playlists</strong></span>
                            </label>
                            <label
                                class:unavailable={pendingBackup.manifest
                                    .lyrics +
                                    pendingBackup.manifest.artist_bios +
                                    pendingBackup.manifest.artwork ===
                                    0}
                                class="backup-option"
                            >
                                <input
                                    type="checkbox"
                                    bind:checked={
                                        restoreSections.custom_metadata
                                    }
                                    disabled={pendingBackup.manifest.lyrics +
                                        pendingBackup.manifest.artist_bios +
                                        pendingBackup.manifest.artwork ===
                                        0}
                                />
                                <span><strong>Custom metadata</strong></span>
                            </label>
                            <label
                                class:unavailable={pendingBackup.manifest
                                    .history === 0}
                                class="backup-option"
                            >
                                <input
                                    type="checkbox"
                                    bind:checked={restoreSections.history}
                                    disabled={pendingBackup.manifest.history ===
                                        0}
                                />
                                <span
                                    ><strong>Listening history</strong><small
                                        >Includes the playback trace when
                                        available</small
                                    ></span
                                >
                            </label>
                        </div>
                        <p class="backup-status">
                            Updates matches; skips missing songs.
                        </p>
                        <button
                            class="btn-pill btn-primary backup-button"
                            disabled={backupBusy !== null || !canRestore}
                            onclick={restoreLibrary}
                        >
                            {backupBusy === "import"
                                ? "Restoring…"
                                : "Restore selected"}
                        </button>
                    {:else}
                        <p class="backup-empty">Choose a backup file.</p>
                    {/if}
                </section>
            </div>
        </div>

        <div class="form-card" hidden={activeCategory !== "library"}>
            {@render sectionTitle("Library")}

            <div class="field field-inline">
                <label class="toggle">
                    <input
                        id="scan-on-startup"
                        type="checkbox"
                        bind:checked={settings.scan_on_startup}
                    />
                    <span class="toggle-slider" aria-hidden="true"></span>
                    <span>Scan library on startup</span>
                </label>
                {@render hint("Rescans folders when Sparkle opens.")}
            </div>

            <div class="field">
                {@render fieldLabel(
                    "Artist name separators",
                    "artist-split-regex",
                )}
                <input
                    id="artist-split-regex"
                    type="text"
                    bind:value={settings.artist_split_regex}
                />
                {@render hint("Splits combined artist names.")}
            </div>

            <div class="field">
                {@render fieldLabel(
                    "Never split these artists",
                    "artist-split-exceptions",
                )}
                {@render pillList(
                    "artist_split_exceptions",
                    "Add an artist name…",
                )}
                {@render hint("Keeps these names together.")}
            </div>

            {#if rulesNeedRescan}
                <div class="field field-inline">
                    <span class="rescan-note">
                        Rescan changed rules in <a
                            class="rescan-link"
                            href="/folders">Folders</a
                        >.
                    </span>
                </div>
            {/if}
        </div>

        <div class="form-card" hidden={activeCategory !== "playback"}>
            {@render sectionTitle("Sound Check")}

            <div class="field field-inline">
                <label class="toggle">
                    <input
                        id="sound-check-enabled"
                        type="checkbox"
                        bind:checked={settings.sound_check_enabled}
                    />
                    <span class="toggle-slider" aria-hidden="true"></span>
                    <span>Normalize song loudness</span>
                </label>
            </div>

            <p class="hint">Lowers louder songs automatically.</p>

            {#if loudnessStatus}
                <div class="sound-check-status" aria-live="polite">
                    <div class="sound-check-summary">
                        <strong>
                            {loudnessStatus.analyzed} of {loudnessStatus.total}
                            songs analyzed
                        </strong>
                        <span>
                            {#if settings.sound_check_enabled && loudnessStatus.running}
                                Scanning {loudnessStatus.prioritized_pending > 0
                                    ? "next-up songs"
                                    : "library"}…
                            {:else if !settings.sound_check_enabled && loudnessStatus.pending > 0}
                                Paused · {loudnessStatus.pending} pending
                            {:else if loudnessStatus.pending > 0}
                                {loudnessStatus.pending} pending
                            {:else if loudnessStatus.failed > 0}
                                Finished with skipped songs
                            {:else}
                                Up to date
                            {/if}
                        </span>
                    </div>
                    <progress
                        class="sound-check-progress"
                        max={Math.max(loudnessStatus.total, 1)}
                        value={loudnessStatus.analyzed + loudnessStatus.failed}
                    ></progress>
                    {#if loudnessStatus.failed > 0}
                        <p class="hint">
                            {loudnessStatus.failed} scan{loudnessStatus.failed ===
                            1
                                ? ""
                                : "s"} failed.
                        </p>
                    {/if}
                </div>
            {/if}

            <div class="field field-inline sound-check-actions">
                <button
                    type="button"
                    class="btn-pill btn-primary"
                    onclick={() => runSoundCheckScan("scan")}
                    disabled={loudnessActionBusy !== null ||
                        !settings.sound_check_enabled ||
                        !loudnessStatus?.total ||
                        !loudnessStatus.pending}
                >
                    {loudnessActionBusy === "scan" ? "Starting…" : "Scan"}
                </button>
                <button
                    type="button"
                    class="btn-pill btn-secondary"
                    onclick={() => runSoundCheckScan("rescan")}
                    disabled={loudnessActionBusy !== null ||
                        !settings.sound_check_enabled ||
                        !loudnessStatus?.total}
                >
                    {loudnessActionBusy === "rescan" ? "Starting…" : "Rescan"}
                </button>
            </div>
        </div>

        <div class="form-card" hidden={activeCategory !== "appearance"}>
            {@render sectionTitle("Appearance")}

            <div class="field">
                <span class="field-label" id="accent-label">Theme color</span>
                <div
                    class="accent-row"
                    role="group"
                    aria-labelledby="accent-label"
                >
                    {#each ACCENT_PRESETS as color (color)}
                        <button
                            type="button"
                            class="accent-swatch"
                            class:active={settings.accent_color?.toLowerCase() ===
                                color.toLowerCase()}
                            style:background-color={color}
                            style:color={swatchForeground(color)}
                            onclick={() => chooseAccent(color)}
                            aria-label={`Theme color ${color}`}
                            aria-pressed={settings.accent_color?.toLowerCase() ===
                                color.toLowerCase()}
                        >
                            {#if settings.accent_color?.toLowerCase() === color.toLowerCase()}
                                <span aria-hidden="true">✓</span>
                            {/if}
                        </button>
                    {/each}
                    <label class="accent-picker">
                        <input
                            type="color"
                            value={settings.accent_color}
                            oninput={(event) =>
                                chooseAccent(
                                    (event.currentTarget as HTMLInputElement)
                                        .value,
                                )}
                            aria-label="Choose a custom theme color"
                        />
                        <span>Custom</span>
                    </label>
                    <input
                        type="text"
                        class="accent-hex"
                        value={accentInput}
                        oninput={handleAccentInput}
                        onblur={normalizeAccentInput}
                        spellcheck="false"
                        aria-label="Custom theme color (hex)"
                        aria-invalid={accentInputInvalid}
                        aria-describedby="accent-help accent-error"
                        placeholder={DEFAULT_ACCENT_COLOR}
                    />
                    <button
                        type="button"
                        class="btn-pill btn-secondary accent-reset"
                        onclick={() => chooseAccent(DEFAULT_ACCENT_COLOR)}
                        disabled={!accentInputInvalid &&
                            accentInput === DEFAULT_ACCENT_COLOR &&
                            settings.accent_color === DEFAULT_ACCENT_COLOR}
                    >
                        Reset
                    </button>
                </div>
                <p class="hint" id="accent-help">Accessible in both themes.</p>
                <p
                    class="accent-error"
                    id="accent-error"
                    role={accentInputInvalid ? "alert" : undefined}
                >
                    {accentInputInvalid ? "Use six-digit hex." : ""}
                </p>
            </div>

            <div class="field">
                <span class="field-label" id="accent-foreground-label"
                    >Filled-control text</span
                >
                <div
                    class="select-field"
                    role="group"
                    aria-labelledby="accent-foreground-label"
                >
                    <Select
                        options={ACCENT_FOREGROUND_OPTIONS}
                        value={settings.accent_foreground_preference}
                        onchange={chooseAccentForeground}
                        ariaLabel="Filled-control text preference"
                    />
                </div>
                {@render hint("Adjusts filled-control contrast.")}
            </div>

            <div class="field">
                <span class="field-label">Theme preview</span>
                <div class="accent-preview-grid">
                    <div
                        class="accent-preview"
                        style={accentPreviewStyle(accentTheme.dark, "dark")}
                        role="img"
                        aria-label="Dark theme accent preview"
                    >
                        <span class="accent-preview-mode">Dark</span>
                        <strong>Custom color</strong>
                        <span class="accent-preview-link">Accent text</span>
                        <div class="accent-preview-controls">
                            <span class="accent-preview-button">Play</span>
                            <span class="accent-preview-button hover-sample"
                                >Hover</span
                            >
                            <span class="accent-preview-chip">Selected</span>
                            <span class="accent-preview-focus">Focus</span>
                            <span class="accent-preview-button disabled"
                                >Disabled</span
                            >
                        </div>
                    </div>
                    <div
                        class="accent-preview"
                        style={accentPreviewStyle(accentTheme.light, "light")}
                        role="img"
                        aria-label="Light theme accent preview"
                    >
                        <span class="accent-preview-mode">Light</span>
                        <strong>Custom color</strong>
                        <span class="accent-preview-link">Accent text</span>
                        <div class="accent-preview-controls">
                            <span class="accent-preview-button">Play</span>
                            <span class="accent-preview-button hover-sample"
                                >Hover</span
                            >
                            <span class="accent-preview-chip">Selected</span>
                            <span class="accent-preview-focus">Focus</span>
                            <span class="accent-preview-button disabled"
                                >Disabled</span
                            >
                        </div>
                    </div>
                </div>
            </div>

            <div class="field">
                <span class="field-label" id="now-playing-layout-label"
                    >Now playing layout</span
                >
                <div
                    class="now-playing-layouts"
                    role="radiogroup"
                    aria-labelledby="now-playing-layout-label"
                >
                    {#each NOW_PLAYING_LAYOUTS as layout (layout.value)}
                        <button
                            type="button"
                            class="now-playing-layout"
                            class:active={$nowPlayingLayout === layout.value}
                            role="radio"
                            aria-checked={$nowPlayingLayout === layout.value}
                            onclick={() => nowPlayingLayout.set(layout.value)}
                        >
                            <span
                                class="now-playing-preview"
                                data-layout={layout.value}
                                aria-hidden="true"
                            >
                                <span class="preview-artist"></span>
                                <span class="preview-album"></span>
                                <span class="preview-lyrics"></span>
                            </span>
                            <span class="layout-copy">
                                <strong>{layout.label}</strong>
                                <small>{layout.description}</small>
                            </span>
                            <span class="layout-check" aria-hidden="true"
                                >✓</span
                            >
                        </button>
                    {/each}
                </div>
                {@render hint(
                    "Changes the page opened from the player thumbnail.",
                )}
            </div>

            <div class="field">
                {@render fieldLabel("UI font", "ui-font")}
                <input
                    id="ui-font"
                    type="text"
                    bind:value={settings.ui_font}
                    placeholder="Font family, e.g. Inter"
                    spellcheck="false"
                />
                {@render hint("Uses an installed font.")}
            </div>

            <div class="field">
                {@render fieldLabel("Lyrics font", "lyrics-font")}
                <input
                    id="lyrics-font"
                    type="text"
                    bind:value={settings.lyrics_font}
                    placeholder="Font family, e.g. Monospace"
                    spellcheck="false"
                />
                {@render hint("Changes now-playing lyrics.")}
            </div>

            <div class="field">
                <span class="field-label" id="song-index-language-label"
                    >Songs scroll index</span
                >
                <div
                    class="select-field"
                    role="group"
                    aria-labelledby="song-index-language-label"
                >
                    <Select
                        options={SONG_INDEX_LANGUAGE_OPTIONS}
                        value={$songIndexLanguage}
                        onchange={(v) =>
                            ($songIndexLanguage = v as SongIndexLanguage)}
                        ariaLabel="Songs scroll index language"
                    />
                </div>
                {@render hint("Sets alphabet grouping.")}
            </div>

            <div class="field field-inline">
                <label class="toggle">
                    <input
                        id="reduce-motion"
                        type="checkbox"
                        bind:checked={settings.reduce_motion}
                    />
                    <span class="toggle-slider" aria-hidden="true"></span>
                    <span>Reduce motion</span>
                </label>
                {@render hint("Disables interface animations.")}
            </div>
        </div>

        <div class="form-card" hidden={activeCategory !== "library"}>
            {@render sectionTitle("Online sources")}

            {#each SOURCE_CATEGORIES as category (category.key)}
                {@render sortableSourceList(category)}
            {/each}

            <div class="field">
                {@render fieldLabel("Brave Search API key", "brave-api-key")}
                <input
                    id="brave-api-key"
                    type="password"
                    bind:value={settings.brave_api_key}
                    placeholder="Paste your API key"
                    spellcheck="false"
                    autocomplete="off"
                />
                {@render hint("Enables Brave image search.")}
            </div>
        </div>

        <div class="form-card" hidden={activeCategory !== "sharing"}>
            {@render sectionTitle("Discord")}

            <div class="field field-inline">
                <label class="toggle">
                    <input
                        id="discord-enabled"
                        type="checkbox"
                        bind:checked={settings.discord_enabled}
                    />
                    <span class="toggle-slider" aria-hidden="true"></span>
                    <span>Share playback on Discord</span>
                </label>
            </div>

            {#if settings.discord_enabled}
                <div class="field">
                    {@render fieldLabel(
                        "Discord application ID",
                        "discord-app-id",
                    )}
                    <input
                        id="discord-app-id"
                        type="text"
                        bind:value={settings.discord_app_id}
                        inputmode="numeric"
                        spellcheck="false"
                        autocomplete="off"
                    />
                </div>

                <div class="field">
                    {@render fieldLabel(
                        "Artwork storage",
                        "discord-artwork-store",
                    )}
                    <div
                        class="select-field"
                        role="group"
                        aria-labelledby="discord-artwork-store"
                    >
                        <Select
                            options={ARTWORK_STORE_OPTIONS}
                            value={settings.discord_artwork_store}
                            onchange={chooseArtworkStore}
                            ariaLabel="Discord artwork storage"
                        />
                    </div>
                    {@render hint("Hosts Discord artwork.")}
                </div>

                {#if settings.discord_artwork_store === "catbox"}
                    <div class="field">
                        {@render fieldLabel(
                            "Catbox user hash",
                            "discord-catbox-user-hash",
                        )}
                        <input
                            id="discord-catbox-user-hash"
                            type="password"
                            bind:value={settings.discord_catbox_user_hash}
                            spellcheck="false"
                            autocomplete="off"
                        />
                        {@render hint("Optional Catbox account hash.")}
                    </div>
                {/if}

                {#if settings.discord_artwork_store === "s3"}
                    <div class="s3-section">
                        <div>
                            <h3>S3 artwork</h3>
                        </div>

                        <div class="field">
                            {@render fieldLabel(
                                "S3 endpoint",
                                "discord-artwork-s3-endpoint",
                            )}
                            <input
                                id="discord-artwork-s3-endpoint"
                                type="url"
                                bind:value={
                                    settings.discord_artwork_s3_endpoint
                                }
                                placeholder="https://s3.example.com"
                                spellcheck="false"
                                autocomplete="off"
                            />
                            {@render hint("S3-compatible API URL.")}
                        </div>

                        <div class="field">
                            {@render fieldLabel(
                                "S3 bucket",
                                "discord-artwork-s3-bucket",
                            )}
                            <input
                                id="discord-artwork-s3-bucket"
                                type="text"
                                bind:value={settings.discord_artwork_s3_bucket}
                                placeholder="sparkle-artwork"
                                spellcheck="false"
                                autocomplete="off"
                            />
                        </div>

                        <div class="field">
                            {@render fieldLabel(
                                "Public artwork URL",
                                "discord-artwork-s3-public-url",
                            )}
                            <input
                                id="discord-artwork-s3-public-url"
                                type="url"
                                bind:value={
                                    settings.discord_artwork_s3_public_url
                                }
                                placeholder="https://cdn.example.com/sparkle"
                                spellcheck="false"
                                autocomplete="off"
                            />
                            {@render hint("Public Discord artwork URL.")}
                        </div>

                        <div class="field">
                            {@render fieldLabel(
                                "S3 access key",
                                "discord-artwork-s3-access-key",
                            )}
                            <input
                                id="discord-artwork-s3-access-key"
                                type="text"
                                bind:value={
                                    settings.discord_artwork_s3_access_key
                                }
                                spellcheck="false"
                                autocomplete="off"
                            />
                        </div>

                        <div class="field">
                            {@render fieldLabel(
                                "S3 secret key",
                                "discord-artwork-s3-secret-key",
                            )}
                            <input
                                id="discord-artwork-s3-secret-key"
                                type="password"
                                bind:value={
                                    settings.discord_artwork_s3_secret_key
                                }
                                spellcheck="false"
                                autocomplete="new-password"
                            />
                        </div>

                        <div class="field">
                            {@render fieldLabel(
                                "S3 session token",
                                "discord-artwork-s3-session-token",
                            )}
                            <input
                                id="discord-artwork-s3-session-token"
                                type="password"
                                bind:value={
                                    settings.discord_artwork_s3_session_token
                                }
                                spellcheck="false"
                                autocomplete="new-password"
                            />
                            {@render hint("Optional temporary credential.")}
                        </div>

                        <div class="field">
                            {@render fieldLabel(
                                "S3 region",
                                "discord-artwork-s3-region",
                            )}
                            <input
                                id="discord-artwork-s3-region"
                                type="text"
                                bind:value={settings.discord_artwork_s3_region}
                                placeholder="us-east-1"
                                spellcheck="false"
                                autocomplete="off"
                            />
                        </div>

                        <div class="field">
                            {@render fieldLabel(
                                "S3 object prefix",
                                "discord-artwork-s3-prefix",
                            )}
                            <input
                                id="discord-artwork-s3-prefix"
                                type="text"
                                bind:value={settings.discord_artwork_s3_prefix}
                                placeholder="sparkle/"
                                spellcheck="false"
                                autocomplete="off"
                            />
                            {@render hint("Defaults to sparkle/.")}
                        </div>

                        <p class="hint">Credentials stay local.</p>
                    </div>
                {/if}

                {#if settings.discord_artwork_store !== "disabled"}
                    <div class="storage-test">
                        <button
                            class="btn-pill btn-secondary"
                            disabled={artworkStorageTestBusy ||
                                saveState === "dirty" ||
                                saveState === "saving"}
                            onclick={testArtworkStorage}
                        >
                            {artworkStorageTestBusy
                                ? "Testing…"
                                : settings.discord_artwork_store === "s3"
                                  ? "Test S3"
                                  : "Test Catbox"}
                        </button>
                        {@render hint(
                            settings.discord_artwork_store === "s3"
                                ? "Uploads, verifies, then deletes."
                                : "Uploads a test image.",
                        )}
                        {#if artworkStorageTestUrl}
                            <a
                                class="storage-test-url"
                                href={artworkStorageTestUrl}
                                target="_blank"
                                rel="noreferrer">Open test upload</a
                            >
                        {/if}
                    </div>
                {/if}
            {/if}
        </div>

        <div class="form-card" hidden={activeCategory !== "data"}>
            {@render sectionTitle("Cache")}
            <div class="cache-list">
                {#each [{ name: "Lyrics", clear: clearLyricsCache }, { name: "Artist info", clear: clearArtistInfoCache }, { name: "Images", clear: clearImagesCache }] as entry (entry.name)}
                    {@const stat = statFor(entry.name)}
                    <div class="cache-row">
                        <div class="cache-info">
                            <span class="cache-name">{entry.name}</span>
                            <span class="cache-meta">
                                {#if stat}
                                    {stat.items} item{stat.items === 1
                                        ? ""
                                        : "s"} · {formatBytes(stat.bytes)}
                                {:else}
                                    —
                                {/if}
                            </span>
                        </div>
                        <button
                            class="btn-pill btn-secondary"
                            disabled={clearing === entry.name}
                            onclick={() => clearCache(entry.name, entry.clear)}
                        >
                            {clearing === entry.name ? "Clearing…" : "Clear"}
                        </button>
                    </div>
                {/each}
                <div class="cache-row total">
                    <div class="cache-info">
                        <span class="cache-name">All caches</span>
                        <span class="cache-meta">
                            {formatBytes(
                                cacheStats.reduce((sum, s) => sum + s.bytes, 0),
                            )} total
                        </span>
                    </div>
                    <button
                        class="btn-pill btn-secondary"
                        disabled={clearing === "All"}
                        onclick={() => clearCache("All", clearAllCaches)}
                    >
                        {clearing === "All" ? "Clearing…" : "Clear all"}
                    </button>
                </div>
            </div>
        </div>

        <div class="form-card" hidden={activeCategory !== "advanced"}>
            {@render sectionTitle("Keyboard shortcuts")}
            <ul class="shortcut-list">
                {#each SHORTCUTS as shortcut (shortcut.keys)}
                    <li class="shortcut-row">
                        <kbd>{shortcut.keys}</kbd>
                        <span class="shortcut-action">{shortcut.action}</span>
                    </li>
                {/each}
            </ul>
        </div>

        <div class="form-card" hidden={activeCategory !== "advanced"}>
            {@render sectionTitle("Diagnostics")}
            {#if status}
                <div class="debug-list">
                    <div class="debug-row">
                        <span class="debug-label">Library path</span>
                        <span class="debug-value">{status.db_path}</span>
                        <button
                            class="debug-open"
                            onclick={() => revealInExplorer(status!.db_path)}
                            aria-label="Show in Explorer"
                            title="Show in Explorer"
                        >
                            <svg
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                aria-hidden="true"
                            >
                                <path d="M15 3h6v6" />
                                <path d="M10 14 21 3" />
                                <path
                                    d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"
                                />
                            </svg>
                        </button>
                    </div>
                    {#if cacheDir}
                        <div class="debug-row">
                            <span class="debug-label">Cache path</span>
                            <span class="debug-value">{cacheDir}</span>
                            <button
                                class="debug-open"
                                onclick={() => revealInExplorer(cacheDir!)}
                                aria-label="Show in Explorer"
                                title="Show in Explorer"
                            >
                                <svg
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="2"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                    aria-hidden="true"
                                >
                                    <path d="M15 3h6v6" />
                                    <path d="M10 14 21 3" />
                                    <path
                                        d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"
                                    />
                                </svg>
                            </button>
                        </div>
                    {/if}
                    <div class="debug-row">
                        <span class="debug-label">Schema version</span>
                        <span class="debug-value">{status.schema_version}</span>
                    </div>
                    <div class="debug-row">
                        <span class="debug-label">Audio output</span>
                        <span class="debug-value">
                            {status.audio_backend}
                            {status.audio_output_mode} ·
                            {status.audio_precision_bits}-bit float processing
                        </span>
                    </div>
                    <div class="debug-row">
                        <span class="debug-label">Log file</span>
                        <span class="debug-value">{status.log_path}</span>
                        <button
                            class="debug-open"
                            onclick={() => revealInExplorer(status!.log_path)}
                            aria-label="Show log file in Explorer"
                            title="Show log file in Explorer"
                        >
                            <svg
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                aria-hidden="true"
                            >
                                <path d="M15 3h6v6" />
                                <path d="M10 14 21 3" />
                                <path
                                    d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"
                                />
                            </svg>
                        </button>
                    </div>
                    <div class="debug-row">
                        <span class="debug-label">Log rotation</span>
                        <span class="debug-value">2 MiB · 3 files</span>
                    </div>
                    {#if settings}
                        <div class="debug-row debug-row-toggle">
                            <div class="debug-toggle-copy">
                                <span class="debug-label">Verbose logging</span>
                            </div>
                            <label class="toggle">
                                <input
                                    type="checkbox"
                                    bind:checked={
                                        settings.debug_logging_enabled
                                    }
                                    aria-label="Enable verbose logging"
                                />
                                <span class="toggle-slider" aria-hidden="true"
                                ></span>
                                <span
                                    >{settings.debug_logging_enabled
                                        ? "On"
                                        : "Off"}</span
                                >
                            </label>
                        </div>
                    {/if}
                </div>
            {:else}
                <p class="hint">Unavailable</p>
            {/if}
        </div>

        <div class="form-card" hidden={activeCategory !== "advanced"}>
            {@render sectionTitle("Licenses")}
            <ul class="license-list">
                {#each LICENSES as item (item.name)}
                    <li class="license-row">
                        {#if item.href}
                            <a
                                href={item.href}
                                target="_blank"
                                rel="noreferrer"
                            >
                                {item.name}
                            </a>
                        {:else}
                            <span>{item.name}</span>
                        {/if}
                        <span class="license-type">{item.license}</span>
                    </li>
                {/each}
            </ul>
        </div>
    {:else}
        <div class="settings-loading">
            <Loading />
        </div>
    {/if}
</div>

<style>
    .settings-page {
        display: grid;
        grid-template-columns: 13rem minmax(0, 1fr);
        align-items: start;
        gap: var(--spacing-lg) var(--spacing-xl);
        width: 100%;
        max-width: 1120px;
        margin: 0 auto;
    }

    .header {
        grid-column: 1 / -1;
        display: flex;
        align-items: flex-end;
        justify-content: space-between;
        gap: var(--spacing-lg);
    }

    .page-heading {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .settings-index {
        grid-column: 1;
        grid-row: 2 / span 20;
        position: sticky;
        top: 0;
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        min-width: 0;
        padding: var(--spacing-xs);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-lg);
        background: color-mix(in srgb, var(--color-surface) 82%, transparent);
        backdrop-filter: blur(18px) saturate(1.4);
        -webkit-backdrop-filter: blur(18px) saturate(1.4);
    }

    .settings-index button {
        position: relative;
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        gap: 0.1rem;
        width: 100%;
        min-width: 0;
        padding: 0.65rem var(--spacing-md);
        border-radius: var(--radius);
        color: var(--color-text-secondary);
        text-align: left;
        transition:
            background-color var(--transition-fast),
            color var(--transition-fast);
    }

    .settings-index button::before {
        content: "";
        position: absolute;
        top: 50%;
        left: 0.25rem;
        width: 3px;
        height: 1.25rem;
        border-radius: var(--radius-full);
        background: transparent;
        transform: translateY(-50%);
    }

    .settings-index button:hover {
        background: color-mix(in srgb, var(--color-text) 7%, transparent);
        color: var(--color-text);
    }

    .settings-index button.active {
        background: var(--color-surface-elevated);
        color: var(--color-text);
    }

    .settings-index button.active::before {
        background: var(--color-accent-graphic);
    }

    .settings-index button span {
        font-size: var(--font-size-sm);
        font-weight: var(--font-weight-semibold);
    }

    .settings-index button small {
        max-width: 100%;
        overflow: hidden;
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .category-heading {
        grid-column: 2;
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        padding: var(--spacing-xs) var(--spacing-xs) var(--spacing-sm);
    }

    .category-heading h2 {
        font-size: var(--font-size-2xl);
        line-height: var(--line-height-tight);
        letter-spacing: -0.02em;
    }

    .settings-loading {
        grid-column: 1 / -1;
    }

    .backup-grid {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: var(--spacing-md);
    }

    .backup-panel {
        display: flex;
        flex-direction: column;
        align-items: stretch;
        gap: var(--spacing-md);
        min-width: 0;
        padding: var(--spacing-lg);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-lg);
        background: var(--color-surface-elevated);
    }

    .backup-panel-heading {
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        gap: var(--spacing-sm);
    }

    .backup-panel-heading h3 {
        margin: 0;
        font-size: var(--font-size-md);
    }

    .backup-panel-heading span,
    .backup-status,
    .backup-empty,
    .backup-preview small {
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
    }

    .backup-options {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .backup-option {
        display: flex;
        align-items: flex-start;
        gap: var(--spacing-sm);
        padding: var(--spacing-xs) 0;
        cursor: pointer;
    }

    .backup-option input {
        margin-top: 0.18rem;
        accent-color: var(--color-accent-native);
    }

    .backup-option span {
        display: flex;
        min-width: 0;
        flex-direction: column;
        gap: 0.1rem;
    }

    .backup-option strong {
        color: var(--color-text);
        font-size: var(--font-size-sm);
        font-weight: var(--font-weight-semibold);
    }

    .backup-option small {
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-normal);
    }

    .backup-option.unavailable {
        opacity: 0.42;
        cursor: default;
    }

    .backup-button {
        align-self: flex-start;
    }

    .backup-status,
    .backup-empty {
        margin: 0;
        line-height: 1.45;
    }

    .backup-preview {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        min-width: 0;
        padding: var(--spacing-md);
        border-radius: var(--radius);
        background: var(--color-surface-raised);
    }

    .backup-preview > strong {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: var(--font-size-sm);
    }

    .backup-preview dl {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: var(--spacing-sm);
        margin: var(--spacing-sm) 0 0;
    }

    .backup-preview dl div {
        display: flex;
        flex-direction: column;
        gap: 0.1rem;
    }

    .backup-preview dt {
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
    }

    .backup-preview dd {
        margin: 0;
        color: var(--color-text);
        font-size: var(--font-size-sm);
        font-weight: var(--font-weight-semibold);
    }

    .restore-options {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .form-card {
        grid-column: 2;
        display: flex;
        flex-direction: column;
        gap: var(--spacing-lg);
        min-width: 0;
        padding: var(--spacing-xl);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-xl);
        background: color-mix(in srgb, var(--color-surface) 92%, transparent);
        box-shadow: var(--shadow-sm);
    }

    .form-card[hidden] {
        display: none;
    }

    .section-heading {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        padding-bottom: var(--spacing-md);
        border-bottom: 1px solid var(--color-border);
    }

    .section-title {
        font-size: var(--font-size-lg);
        letter-spacing: -0.01em;
    }

    .privacy-note {
        margin: 0;
        padding: var(--spacing-sm) var(--spacing-md);
        border-radius: var(--radius);
        background: color-mix(
            in srgb,
            var(--color-accent-subtle) 52%,
            transparent
        );
        color: var(--color-text-secondary);
        font-size: var(--font-size-sm);
        line-height: var(--line-height);
    }

    .field {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
    }

    .s3-section {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-lg);
        padding: var(--spacing-lg);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-lg);
        background: var(--color-surface-elevated);
    }

    .s3-section h3 {
        margin: 0 0 var(--spacing-xs);
        color: var(--color-text);
        font-size: var(--font-size-md);
    }

    .storage-test {
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        gap: var(--spacing-sm);
        padding-top: var(--spacing-md);
        border-top: 1px solid var(--color-border);
    }

    .storage-test-url {
        max-width: 100%;
        overflow: hidden;
        color: var(--color-accent-content);
        font-size: var(--font-size-sm);
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    label {
        font-weight: var(--font-weight-semibold);
        font-size: var(--font-size-sm);
        color: var(--color-text);
    }

    .pill-list {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: var(--spacing-sm);
        padding: var(--spacing-sm);
        border: 1px solid var(--color-border);
        border-radius: var(--radius);
        background: var(--color-surface-elevated);
        min-height: 2.5rem;
        cursor: text;
    }

    .pill-list:focus-within {
        border-color: var(--color-accent-focus);
    }

    .pill {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-xs);
        padding: var(--spacing-xs) var(--spacing-sm);
        background-color: var(--color-surface-raised);
        border-radius: var(--radius-full);
        font-size: var(--font-size-sm);
        color: var(--color-text);
    }

    .pill-text {
        max-width: 16rem;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .pill-remove {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 1rem;
        height: 1rem;
        color: var(--color-text-muted);
        border-radius: var(--radius-full);
        transition:
            color var(--transition-fast),
            background-color var(--transition-fast);
    }

    .pill-remove:hover {
        color: var(--color-text);
        background-color: rgba(255, 255, 255, 0.1);
    }

    .pill-remove svg {
        width: 0.875rem;
        height: 0.875rem;
    }

    .pill-input {
        flex: 1;
        min-width: 6rem;
        border: none;
        background: transparent;
        padding: var(--spacing-xs);
        font-size: var(--font-size-sm);
    }

    .pill-input:focus {
        outline: none;
        border: none;
    }

    .source-list {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .source-row {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        padding: var(--spacing-sm) var(--spacing-md);
        border-radius: var(--radius);
        background: var(--color-surface-elevated);
        border: 1px solid var(--color-border);
        transition:
            background-color var(--transition-fast),
            opacity var(--transition-fast);
    }

    .source-row:hover {
        background: var(--color-surface-raised);
    }

    .source-row.disabled {
        opacity: 0.55;
    }

    .source-name {
        flex: 1;
        font-size: var(--font-size-sm);
        color: var(--color-text);
    }

    .source-actions {
        display: flex;
        align-items: center;
        gap: var(--spacing-xs);
    }

    .source-action {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 1.5rem;
        height: 1.5rem;
        color: var(--color-text-muted);
        border-radius: var(--radius);
        transition:
            color var(--transition-fast),
            background-color var(--transition-fast);
    }

    .source-action:hover:not(:disabled) {
        color: var(--color-text);
        background-color: rgba(255, 255, 255, 0.1);
    }

    .source-action:disabled {
        opacity: 0.3;
        cursor: not-allowed;
    }

    .source-action svg {
        width: 1rem;
        height: 1rem;
    }

    .add-row {
        border-style: dashed;
    }

    .add-label {
        color: var(--color-text-muted);
    }

    .add-btn {
        padding: var(--spacing-xs) var(--spacing-md);
        font-size: var(--font-size-xs);
    }

    .save-indicator {
        min-height: 1.75rem;
        padding: var(--spacing-xs) var(--spacing-sm);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-full);
        background: var(--color-surface);
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-medium);
        white-space: nowrap;
    }

    .rescan-note {
        font-size: var(--font-size-sm);
        color: var(--color-accent-content);
    }

    .rescan-link {
        color: inherit;
        text-decoration: underline;
    }

    .accent-row {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        flex-wrap: wrap;
    }

    .accent-swatch {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.75rem;
        height: 1.75rem;
        border-radius: var(--radius-full);
        border: 2px solid transparent;
        cursor: pointer;
        font-size: 0.75rem;
        font-weight: var(--font-weight-bold);
        text-shadow: 0 1px 3px rgba(0, 0, 0, 0.45);
        transition:
            transform var(--transition-fast),
            border-color var(--transition-fast);
    }

    .accent-swatch:hover {
        transform: scale(1.1);
    }

    .accent-swatch.active {
        border-color: var(--color-text);
    }

    .accent-picker {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-xs);
        min-height: 2rem;
        padding: 0.2rem var(--spacing-sm) 0.2rem 0.2rem;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-full);
        background: var(--color-surface-elevated);
        cursor: pointer;
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-semibold);
    }

    .accent-picker:hover {
        background: var(--color-surface-raised);
    }

    .accent-picker input[type="color"] {
        width: 1.5rem;
        height: 1.5rem;
        min-width: 1.5rem;
        padding: 0;
        border: 0;
        border-radius: var(--radius-full);
        background: transparent;
        cursor: pointer;
        overflow: hidden;
    }

    .accent-picker input[type="color"]::-webkit-color-swatch-wrapper {
        padding: 0;
    }

    .accent-picker input[type="color"]::-webkit-color-swatch {
        border: 0;
        border-radius: var(--radius-full);
    }

    .accent-hex {
        width: 7rem;
        font-family: var(--font-family-monospace, monospace);
        font-size: var(--font-size-sm);
    }

    .accent-hex[aria-invalid="true"] {
        border-color: var(--color-error);
    }

    .accent-reset {
        min-height: 2rem;
        padding: var(--spacing-xs) var(--spacing-md);
        font-size: var(--font-size-xs);
    }

    .accent-error {
        min-height: 1.1rem;
        margin: 0;
        color: var(--color-error);
        font-size: var(--font-size-xs);
    }

    .accent-preview-grid {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: var(--spacing-md);
    }

    .accent-preview {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
        min-width: 0;
        padding: var(--spacing-md);
        border: 1px solid
            color-mix(in srgb, var(--preview-text) 14%, transparent);
        border-radius: var(--radius-lg);
        background: var(--preview-background);
        color: var(--preview-text);
    }

    .accent-preview-mode {
        color: var(--preview-muted);
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-semibold);
        letter-spacing: 0.06em;
        text-transform: uppercase;
    }

    .accent-preview-link {
        color: var(--preview-text);
        font-size: var(--font-size-sm);
        font-weight: var(--font-weight-semibold);
    }

    .accent-preview-controls {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: var(--spacing-sm);
        padding-top: var(--spacing-xs);
    }

    .accent-preview-button,
    .accent-preview-chip,
    .accent-preview-focus {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-height: 1.75rem;
        padding: var(--spacing-xs) var(--spacing-sm);
        border-radius: var(--radius-full);
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-semibold);
    }

    .accent-preview-button {
        background: var(--preview-fill);
        color: var(--preview-on-fill);
    }

    .accent-preview-button.hover-sample {
        background: var(--preview-fill-hover);
    }

    .accent-preview-button.disabled {
        background: var(--preview-fill-disabled);
        color: var(--preview-on-fill-disabled);
    }

    .accent-preview-chip {
        background: var(--preview-subtle);
        color: var(--preview-on-subtle);
    }

    .accent-preview-focus {
        color: var(--preview-text);
        background: var(--preview-surface);
        outline: 2px solid var(--preview-focus);
        outline-offset: 1px;
    }

    .now-playing-layouts {
        display: grid;
        grid-template-columns: repeat(3, minmax(0, 1fr));
        gap: var(--spacing-md);
    }

    .now-playing-layout {
        position: relative;
        display: flex;
        flex-direction: column;
        align-items: stretch;
        gap: var(--spacing-sm);
        min-width: 0;
        padding: var(--spacing-sm);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-lg);
        background: var(--color-surface-elevated);
        text-align: left;
        transition:
            transform var(--transition-fast),
            border-color var(--transition-fast),
            background-color var(--transition-fast),
            box-shadow var(--transition-fast);
    }

    .now-playing-layout:hover {
        transform: translateY(-2px);
        border-color: color-mix(in srgb, var(--color-text) 22%, transparent);
        background: var(--color-surface-raised);
        box-shadow: var(--shadow-sm);
    }

    .now-playing-layout.active {
        border-color: var(--color-accent-graphic);
        background: color-mix(
            in srgb,
            var(--color-accent-subtle) 42%,
            var(--color-surface-elevated)
        );
        box-shadow:
            0 0 0 1px
                color-mix(in srgb, var(--color-accent-graphic) 30%, transparent),
            var(--shadow-sm);
    }

    .now-playing-preview {
        position: relative;
        display: block;
        width: 100%;
        aspect-ratio: 1.55;
        overflow: hidden;
        border-radius: var(--radius);
        background:
            radial-gradient(
                circle at 22% 28%,
                color-mix(in srgb, var(--color-accent-seed) 24%, transparent),
                transparent 44%
            ),
            color-mix(in srgb, var(--color-background) 92%, #24243a);
        box-shadow: inset 0 0 0 1px
            color-mix(in srgb, var(--color-text) 8%, transparent);
    }

    .preview-artist,
    .preview-album,
    .preview-lyrics {
        position: absolute;
        display: block;
    }

    .preview-artist {
        border-radius: 50%;
        background: linear-gradient(145deg, #9f9aa8, #393745 72%);
        box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.16);
    }

    .preview-album {
        border-radius: 4px;
        background:
            linear-gradient(135deg, rgba(255, 255, 255, 0.16), transparent),
            linear-gradient(145deg, #e35d71, #6446a7 75%);
        box-shadow: 0 5px 12px rgba(0, 0, 0, 0.38);
    }

    .preview-lyrics {
        background: repeating-linear-gradient(
            to bottom,
            color-mix(in srgb, var(--color-text) 72%, transparent) 0 2px,
            transparent 2px 10px
        );
        opacity: 0.82;
    }

    .now-playing-preview[data-layout="album"] .preview-artist {
        bottom: 7%;
        left: 8%;
        width: 11%;
        aspect-ratio: 1;
    }

    .now-playing-preview[data-layout="album"] .preview-album {
        top: 12%;
        bottom: 24%;
        left: 8%;
        aspect-ratio: 1;
    }

    .now-playing-preview[data-layout="album"] .preview-lyrics {
        top: 22%;
        right: 9%;
        bottom: 22%;
        width: 38%;
    }

    .now-playing-preview[data-layout="artist"] .preview-artist {
        top: 12%;
        bottom: 12%;
        left: 8%;
        aspect-ratio: 1;
    }

    .now-playing-preview[data-layout="artist"] .preview-album {
        left: 38%;
        bottom: 10%;
        width: 25%;
        aspect-ratio: 1;
    }

    .now-playing-preview[data-layout="artist"] .preview-lyrics {
        top: 22%;
        right: 8%;
        bottom: 22%;
        width: 28%;
    }

    .now-playing-preview[data-layout="lyrics"] .preview-album {
        top: 16%;
        left: 8%;
        width: 26%;
        aspect-ratio: 1;
    }

    .now-playing-preview[data-layout="lyrics"] .preview-artist {
        top: 68%;
        left: 8%;
        width: 12%;
        aspect-ratio: 1;
        box-shadow: var(--shadow-sm);
    }

    .now-playing-preview[data-layout="lyrics"] .preview-lyrics {
        top: 16%;
        right: 8%;
        bottom: 16%;
        width: 49%;
        background: repeating-linear-gradient(
            to bottom,
            color-mix(in srgb, var(--color-text) 78%, transparent) 0 3px,
            transparent 3px 13px
        );
    }

    .layout-copy {
        display: flex;
        flex-direction: column;
        gap: 2px;
        padding: 0 var(--spacing-xs) var(--spacing-xs);
    }

    .layout-copy strong {
        color: var(--color-text);
        font-size: var(--font-size-sm);
    }

    .layout-copy small {
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-normal);
        line-height: 1.35;
    }

    .layout-check {
        position: absolute;
        top: var(--spacing-md);
        right: var(--spacing-md);
        display: none;
        align-items: center;
        justify-content: center;
        width: 1.4rem;
        height: 1.4rem;
        border-radius: 50%;
        background: var(--color-accent-fill);
        color: var(--color-on-accent-fill);
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-bold);
        box-shadow: var(--shadow-sm);
    }

    .now-playing-layout.active .layout-check {
        display: flex;
    }

    .field-label {
        font-weight: var(--font-weight-semibold);
        font-size: var(--font-size-sm);
        color: var(--color-text);
    }

    .select-field {
        display: flex;
    }

    .hint {
        margin: 0;
        font-size: var(--font-size-sm);
        color: var(--color-text-muted);
        line-height: var(--line-height);
    }

    .sound-check-status {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
        padding: var(--spacing-md);
        border: 1px solid
            color-mix(
                in srgb,
                var(--color-accent-graphic) 24%,
                var(--color-border)
            );
        border-radius: var(--radius);
        background: color-mix(
            in srgb,
            var(--color-accent-subtle) 42%,
            var(--color-surface-elevated)
        );
    }

    .sound-check-summary {
        display: flex;
        justify-content: space-between;
        gap: var(--spacing-md);
        font-size: var(--font-size-sm);
    }

    .sound-check-summary span {
        color: var(--color-text-muted);
    }

    .sound-check-progress {
        width: 100%;
        height: 0.45rem;
        accent-color: var(--color-accent-native);
    }

    .cache-list {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .cache-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
        padding: var(--spacing-sm) var(--spacing-md);
        border-radius: var(--radius);
        background: var(--color-surface-elevated);
        border: 1px solid var(--color-border);
    }

    .cache-row.total {
        border-color: rgba(255, 255, 255, 0.16);
    }

    .cache-info {
        display: flex;
        flex-direction: column;
        gap: 1px;
        min-width: 0;
    }

    .cache-name {
        font-size: var(--font-size-sm);
        font-weight: var(--font-weight-medium);
        color: var(--color-text);
    }

    .cache-meta {
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
        font-variant-numeric: tabular-nums;
    }

    .shortcut-list {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .shortcut-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
        padding: var(--spacing-sm) var(--spacing-md);
        border: 1px solid var(--color-border);
        border-radius: var(--radius);
        background: var(--color-surface-elevated);
    }

    .shortcut-action {
        font-size: var(--font-size-sm);
        color: var(--color-text-secondary);
    }

    kbd {
        font-family: inherit;
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-semibold);
        color: var(--color-text);
        background-color: var(--color-surface-elevated);
        border: 1px solid var(--color-border);
        border-bottom-width: 2px;
        border-radius: var(--radius-sm);
        padding: var(--spacing-xs) var(--spacing-sm);
        white-space: nowrap;
    }

    .debug-list {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .debug-row {
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        gap: var(--spacing-md);
        min-width: 0;
        padding: var(--spacing-sm) var(--spacing-md);
        border: 1px solid var(--color-border);
        border-radius: var(--radius);
        background: var(--color-surface-elevated);
        font-size: var(--font-size-sm);
    }

    .debug-label {
        color: var(--color-text-muted);
        flex-shrink: 0;
    }

    .debug-value {
        color: var(--color-text-secondary);
        word-break: break-all;
        text-align: right;
        flex: 1;
    }

    .debug-row-toggle {
        align-items: center;
    }

    .debug-toggle-copy {
        display: flex;
        flex: 1;
        flex-direction: column;
        gap: 0.125rem;
        min-width: 0;
    }

    .debug-open {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 1.5rem;
        height: 1.5rem;
        flex-shrink: 0;
        border-radius: var(--radius-sm);
        color: var(--color-text-muted);
        transition:
            color var(--transition-fast),
            background-color var(--transition-fast);
    }

    .debug-open:hover {
        color: var(--color-text);
        background-color: rgba(255, 255, 255, 0.08);
    }

    .debug-open svg {
        width: 0.875rem;
        height: 0.875rem;
    }

    .license-list {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .license-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
        min-width: 0;
        padding: var(--spacing-sm) var(--spacing-md);
        border: 1px solid var(--color-border);
        border-radius: var(--radius);
        background: var(--color-surface-elevated);
        font-size: var(--font-size-sm);
    }

    .license-row > :first-child {
        overflow: hidden;
        color: var(--color-text);
        font-weight: var(--font-weight-medium);
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .license-row a:hover {
        text-decoration: underline;
    }

    .license-type {
        flex-shrink: 0;
        padding: 0.15rem var(--spacing-sm);
        border-radius: var(--radius-full);
        background: var(--color-surface-raised);
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-semibold);
    }

    .field-inline {
        flex-direction: row;
        align-items: center;
        gap: var(--spacing-md);
        flex-wrap: wrap;
    }

    .toggle {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-sm);
        cursor: pointer;
        user-select: none;
        font-weight: var(--font-weight-semibold);
        font-size: var(--font-size-sm);
        color: var(--color-text);
    }

    .toggle input {
        position: absolute;
        opacity: 0;
        width: 0;
        height: 0;
    }

    .toggle-slider {
        position: relative;
        width: 2.25rem;
        height: 1.25rem;
        flex-shrink: 0;
        background-color: var(--color-surface-raised);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-full);
        transition:
            background-color var(--transition-fast),
            border-color var(--transition-fast);
    }

    .toggle-slider::after {
        content: "";
        position: absolute;
        top: 50%;
        left: 0.125rem;
        width: 0.875rem;
        height: 0.875rem;
        background-color: var(--color-text-muted);
        border-radius: 50%;
        transform: translateY(-50%);
        transition:
            transform var(--transition-fast),
            background-color var(--transition-fast);
    }

    .toggle input:checked + .toggle-slider {
        background-color: var(--color-accent-fill);
        border-color: var(--color-accent-graphic);
    }

    .toggle input:checked + .toggle-slider::after {
        transform: translate(1rem, -50%);
        background-color: var(--color-on-accent-fill);
    }

    .toggle input:focus-visible + .toggle-slider {
        outline: 2px solid var(--color-accent-focus);
        outline-offset: 2px;
    }

    @media (max-width: 900px) {
        .settings-page {
            grid-template-columns: minmax(0, 1fr);
        }

        .settings-index {
            grid-column: 1;
            grid-row: auto;
            position: static;
            display: grid;
            grid-template-columns: repeat(3, minmax(0, 1fr));
        }

        .category-heading,
        .form-card {
            grid-column: 1;
        }
    }

    @media (max-width: 640px) {
        .header {
            align-items: flex-start;
        }

        .settings-index {
            display: flex;
            flex-direction: row;
            overflow-x: auto;
            scrollbar-width: none;
        }

        .settings-index::-webkit-scrollbar {
            display: none;
        }

        .settings-index button {
            width: auto;
            min-width: max-content;
            padding: var(--spacing-sm) var(--spacing-md);
        }

        .settings-index button::before {
            top: auto;
            right: var(--spacing-sm);
            bottom: 0.2rem;
            left: var(--spacing-sm);
            width: auto;
            height: 2px;
            transform: none;
        }

        .settings-index button small {
            display: none;
        }

        .sound-check-summary,
        .cache-row,
        .debug-row {
            align-items: flex-start;
            flex-direction: column;
        }

        .debug-value {
            text-align: left;
        }

        .now-playing-layouts {
            grid-template-columns: 1fr;
        }

        .now-playing-layout {
            display: grid;
            grid-template-columns: minmax(8rem, 0.8fr) minmax(0, 1fr);
            align-items: center;
        }
    }

    @media (max-width: 480px) {
        .accent-preview-grid {
            grid-template-columns: 1fr;
        }

        .backup-grid {
            grid-template-columns: 1fr;
        }

        .form-card {
            padding: var(--spacing-md);
        }

        .backup-panel,
        .s3-section {
            padding: var(--spacing-md);
        }
    }
</style>
