<script lang="ts">
    import { onMount } from "svelte";
    import {
        getOnlineSettings,
        setOnlineSettings,
        clearLyricsCache,
        clearArtistInfoCache,
        clearImagesCache,
        clearAllCaches,
        getCacheStats,
        getCacheDir,
        getStatus,
        exportLibraryBackup,
        inspectLibraryBackup,
        importLibraryBackup,
        pickBackupToSave,
        pickBackupToImport,
        revealInExplorer,
        type AppStatus,
        type OnlineSettings,
        type CacheStat,
        type BackupManifest,
        type BackupSections,
    } from "$lib/api";
    import Loading from "$lib/components/Loading.svelte";
    import Select from "$lib/components/Select.svelte";
    import { addToast } from "$lib/stores/toast";
    import { songIndexLanguage } from "$lib/stores/songIndex";
    import { getFontStack } from "$lib/utils/fonts";
    import { applyAccent, normalizeHex } from "$lib/utils/theme";
    import type { SongIndexLanguage } from "$lib/utils/songIndex";

    const DEFAULT_SPLIT_REGEX = ";";
    const DEFAULT_SPLIT_EXCEPTIONS = ["AC/DC", "Tyler, The Creator"];
    const DEFAULT_BACKUP_SECTIONS: BackupSections = {
        settings: true,
        playlists: true,
        custom_metadata: true,
        history: true,
    };

    const SONG_INDEX_LANGUAGE_OPTIONS = [
        { value: "auto", label: "Automatic (system)" },
        { value: "en", label: "English / Latin" },
        { value: "ja", label: "日本語 (Japanese)" },
    ];

    const SHORTCUTS: { keys: string; action: string }[] = [
        { keys: "Space", action: "Play / pause" },
        { keys: "\u2190 / \u2192", action: "Seek \u00b15 s" },
        { keys: "Ctrl+\u2190 / Ctrl+\u2192", action: "Previous / next track" },
        { keys: "\u2191 / \u2193", action: "Volume \u00b15%" },
    ];

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
            hint: "Enabled providers are tried in order. Custom uses lyrics files saved for each song; sidecar .lrc reads a matching file beside the audio.",
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
            hint: "Sources tried in order when fetching artist biographies. Custom uses the bio you write on the artist page.",
            builtins: ["custom"],
            wikipedia: true,
        },
        {
            key: "artist_image_sources",
            label: "Artist images",
            hint: "Sources tried in order when fetching artist images. Custom uses the image you pick on the artist page. Brave requires an API key below.",
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
            hint: "Sources tried in order when fetching album artwork. Custom uses the image you pick on the album page.",
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
    let clearing = $state<string | null>(null);
    let cacheStats = $state<CacheStat[]>([]);
    let cacheDir = $state<string | null>(null);
    let status = $state<AppStatus | null>(null);
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
                settings = restoredSettings;
                lastSavedJson = JSON.stringify(restoredSettings);
            }
            const skipped =
                summary.unmatched_tracks + summary.unmatched_artwork;
            addToast(
                skipped > 0
                    ? `Restore complete · ${skipped} unmatched item${skipped === 1 ? "" : "s"} skipped`
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
            if (typeof loaded.discord_enabled !== "boolean") {
                loaded.discord_enabled = true;
            }
            if (typeof loaded.discord_app_id !== "string") {
                loaded.discord_app_id = "";
            }
            if (!loaded.discord_catbox_user_hash) {
                loaded.discord_catbox_user_hash = "";
            }
            if (typeof loaded.debug_logging_enabled !== "boolean") {
                loaded.debug_logging_enabled = false;
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
            applyAccent(settings.accent_color);
        }
    });

    function chooseAccent(color: string) {
        if (!settings) return;
        settings.accent_color = color;
    }

    function handleAccentInput(e: Event) {
        if (!settings) return;
        const normalized = normalizeHex(
            (e.currentTarget as HTMLInputElement).value,
        );
        if (normalized) {
            settings.accent_color = normalized;
        }
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
    <h2 class="section-title">{title}</h2>
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
        <h1 class="page-title">Settings</h1>
        {#if settings}
            <span class="save-indicator" role="status">
                {#if saveState === "saving"}
                    Saving…
                {:else if saveState === "saved"}
                    Saved
                {:else if saveState === "dirty"}
                    Editing…
                {/if}
            </span>
        {/if}
    </div>

    {#if settings}
        <div class="form-card">
            {@render sectionTitle("Backup & restore")}
            <p class="hint">Music files, folders, and API keys stay local.</p>
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
                                    >Minutes and listening patterns</small
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
                                <span><strong>Listening history</strong></span>
                            </label>
                        </div>
                        <p class="backup-status">
                            Playlists with the same name are updated. Unmatched
                            songs are skipped.
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
                        <p class="backup-empty">
                            Choose a file to review its contents.
                        </p>
                    {/if}
                </section>
            </div>
        </div>

        <div class="form-card">
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
                {@render hint(
                    "Automatically rescan monitored folders when the app launches.",
                )}
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
                {@render hint(
                    "Regular expression used to split combined artist names into separate artists, e.g. “feat.” or “;”.",
                )}
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
                {@render hint(
                    "Artist names that should stay as one artist even if they match the separator rule.",
                )}
            </div>

            {#if rulesNeedRescan}
                <div class="field field-inline">
                    <span class="rescan-note">
                        Artist rules changed — run a Full rescan from the <a
                            class="rescan-link"
                            href="/folders">Folders page</a
                        > to apply them.
                    </span>
                </div>
            {/if}
        </div>

        <div class="form-card">
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
                            class="accent-swatch"
                            class:active={settings.accent_color?.toLowerCase() ===
                                color.toLowerCase()}
                            style:background-color={color}
                            onclick={() => chooseAccent(color)}
                            aria-label={`Theme color ${color}`}
                            aria-pressed={settings.accent_color?.toLowerCase() ===
                                color.toLowerCase()}
                        ></button>
                    {/each}
                    <input
                        type="text"
                        class="accent-hex"
                        value={settings.accent_color}
                        onchange={handleAccentInput}
                        spellcheck="false"
                        aria-label="Custom theme color (hex)"
                        placeholder="#fa243c"
                    />
                </div>
                {@render hint(
                    "Accent color used across the app. Pick a preset or enter any hex color.",
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
                {@render hint(
                    "Font used throughout the app interface. Type any installed font family.",
                )}
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
                {@render hint("Font used for lyrics in the now-playing view.")}
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
                {@render hint(
                    "Choose the language your library uses most. Japanese groups titles into あ, か, さ…; automatic follows the system locale.",
                )}
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
                {@render hint("Turn off page and card animations.")}
            </div>
        </div>

        <div class="form-card">
            {@render sectionTitle("Online sources")}
            <p class="hint">
                Providers are tried in order for each category. "Custom" is your
                own content — a bio or image you set on an artist page, artwork
                you set on an album page, or lyrics files you pick per song. Add
                Wikipedia editions per language below.
            </p>

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
                {@render hint(
                    "Used by the Brave artist image source, which searches the web for artist photos. Get a free key at brave.com/search/api. Leave empty to disable the Brave source.",
                )}
            </div>
        </div>

        <div class="form-card">
            {@render sectionTitle("Discord Rich Presence")}

            <div class="field field-inline">
                <label class="toggle">
                    <input
                        id="discord-enabled"
                        type="checkbox"
                        bind:checked={settings.discord_enabled}
                    />
                    <span class="toggle-slider" aria-hidden="true"></span>
                    <span>Show what I’m listening to on Discord</span>
                </label>
                {@render hint(
                    "Shows the current track and progress while it is playing, then clears it when paused or stopped.",
                )}
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
                    {@render hint(
                        "Enter the application ID registered for Sparkle in the Discord Developer Portal.",
                    )}
                </div>

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
                    {@render hint(
                        "Optional. Sparkle reuses its cached Catbox URLs; new artwork uploads only happen for cache misses.",
                    )}
                </div>
            {/if}
        </div>

        <div class="form-card">
            {@render sectionTitle("Cache")}
            <p class="hint">
                Clear cached online metadata to force a fresh fetch the next
                time you view lyrics, artist info, or artwork.
            </p>
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
                        class="btn-pill btn-primary"
                        disabled={clearing === "All"}
                        onclick={() => clearCache("All", clearAllCaches)}
                    >
                        {clearing === "All" ? "Clearing…" : "Clear all"}
                    </button>
                </div>
            </div>
        </div>

        <div class="form-card">
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

        <div class="form-card">
            {@render sectionTitle("Debug info")}
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
                        <span class="debug-value"
                            >2 MiB per file · 3 files kept</span
                        >
                    </div>
                    {#if settings}
                        <div class="debug-row debug-row-toggle">
                            <div class="debug-toggle-copy">
                                <span class="debug-label">Verbose logging</span>
                                <span class="debug-description">
                                    Include detailed Sparkle events in the log
                                    file.
                                </span>
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
                    <div class="debug-row debug-row-toggle">
                        <div class="debug-toggle-copy">
                            <span class="debug-label">Third-party licenses</span
                            >
                            <span class="debug-description">
                                Sparkle includes adapted open-source components.
                            </span>
                        </div>
                    </div>
                    <div class="third-party-notices">
                        <p class="hint">
                            Sparkle's own code is MIT-licensed. Adapted upstream
                            projects:
                        </p>
                        <ul>
                            <li>
                                <a
                                    href="https://github.com/cqjjjzr/MusicBee-NeteaseLyrics"
                                    target="_blank"
                                    rel="noreferrer">MusicBee-NeteaseLyrics</a
                                > — Apache-2.0
                            </li>
                            <li>
                                <a
                                    href="https://github.com/mslxl/MusicBee-QQLyrics"
                                    target="_blank"
                                    rel="noreferrer">MusicBee-QQLyrics</a
                                > — Apache-2.0
                            </li>
                            <li>
                                <a
                                    href="https://github.com/real-zony/ZonyLrcToolsX"
                                    target="_blank"
                                    rel="noreferrer">ZonyLrcToolsX</a
                                > — MIT
                            </li>
                            <li>
                                <a
                                    href="https://github.com/noriokun4649/mb_KashiNaviLyricsPlugin"
                                    target="_blank"
                                    rel="noreferrer">mb_KashiNaviLyricsPlugin</a
                                > — MIT
                            </li>
                            <li>
                                <a
                                    href="https://github.com/htsign/MusicBeePluginTemplate"
                                    target="_blank"
                                    rel="noreferrer">MusicBeePluginTemplate</a
                                > — MIT
                            </li>
                            <li>
                                <a
                                    href="https://github.com/sll552/DiscordBee"
                                    target="_blank"
                                    rel="noreferrer">DiscordBee</a
                                > — Apache-2.0
                            </li>
                        </ul>
                    </div>
                </div>
            {:else}
                <p class="hint">Debug info unavailable.</p>
            {/if}
        </div>
    {:else}
        <Loading />
    {/if}
</div>

<style>
    .settings-page {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xl);
        max-width: 720px;
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
        accent-color: var(--color-accent);
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

    .header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
    }

    .form-card {
        background-color: var(--color-surface);
        border-radius: var(--radius-lg);
        padding: var(--spacing-xl);
        display: flex;
        flex-direction: column;
        gap: var(--spacing-lg);
    }

    .form-card .section-title {
        font-size: var(--font-size-lg);
        margin-bottom: var(--spacing-xs);
    }

    .field {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
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
        border-color: var(--color-accent);
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
        font-size: var(--font-size-sm);
        color: var(--color-text-muted);
        min-height: 1.25rem;
    }

    .rescan-note {
        font-size: var(--font-size-sm);
        color: var(--color-accent);
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
        width: 1.75rem;
        height: 1.75rem;
        border-radius: var(--radius-full);
        border: 2px solid transparent;
        cursor: pointer;
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

    .accent-hex {
        width: 6.5rem;
        font-family: var(--font-family-monospace, monospace);
        font-size: var(--font-size-sm);
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
        gap: var(--spacing-sm);
    }

    .shortcut-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
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
        gap: var(--spacing-sm);
    }

    .debug-row {
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        gap: var(--spacing-md);
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

    .debug-description {
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
        line-height: var(--line-height);
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
        width: 2.5rem;
        height: 1.375rem;
        background-color: var(--color-surface-elevated);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-full);
        transition:
            background-color var(--transition-fast),
            border-color var(--transition-fast);
    }

    .toggle-slider::after {
        content: "";
        position: absolute;
        top: 0.125rem;
        left: 0.125rem;
        width: 1rem;
        height: 1rem;
        background-color: var(--color-text-muted);
        border-radius: 50%;
        transition:
            transform var(--transition-fast),
            background-color var(--transition-fast);
    }

    .toggle input:checked + .toggle-slider {
        background-color: var(--color-accent);
        border-color: var(--color-accent);
    }

    .toggle input:checked + .toggle-slider::after {
        transform: translateX(1.125rem);
        background-color: white;
    }

    .toggle input:focus + .toggle-slider {
        box-shadow: 0 0 0 2px var(--color-accent-focus, rgba(255, 90, 90, 0.4));
    }

    @media (max-width: 480px) {
        .backup-grid {
            grid-template-columns: 1fr;
        }

        .form-card {
            padding: var(--spacing-md);
        }
    }
</style>
