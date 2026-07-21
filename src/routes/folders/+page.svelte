<script lang="ts">
    import {
        listFolders,
        pickFolder,
        addFolder,
        removeFolder,
        scanLibrary,
        setFolderEnabled,
        type Folder,
        type ScanResult,
        type ScanProgress,
    } from "$lib/api";
    import Loading from "$lib/components/Loading.svelte";
    import { addToast } from "$lib/stores/toast";
    import { onMount } from "svelte";
    import { listen } from "@tauri-apps/api/event";

    let folders = $state<Folder[]>([]);
    let loading = $state(true);
    let scanning = $state(false);
    let scanResult = $state<ScanResult | null>(null);
    let scanProgress = $state<ScanProgress | null>(null);

    async function load() {
        loading = true;
        try {
            folders = await listFolders();
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            loading = false;
        }
    }

    onMount(() => {
        load();
        let unlisten: (() => void) | undefined;
        void listen<ScanProgress>("scan-progress", (event) => {
            scanProgress = event.payload;
        }).then((cleanup) => (unlisten = cleanup));
        return () => unlisten?.();
    });

    async function handleAdd() {
        const path = await pickFolder();
        if (!path) return;
        const firstFolder = folders.length === 0;
        try {
            await addFolder(path);
            addToast("Folder added", "success");
            scanResult = null;
            await load();
            if (firstFolder) {
                await handleScan(false);
            }
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    async function handleRemove(id: number) {
        try {
            await removeFolder(id);
            addToast("Folder removed", "success");
            scanResult = null;
            await load();
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    async function handleScan(force = false) {
        scanning = true;
        scanResult = null;
        scanProgress = null;
        try {
            const result = await scanLibrary(force);
            scanResult = result;
            if (result.errors > 0) {
                addToast(
                    `Scan complete with ${result.errors} error${result.errors === 1 ? "" : "s"}`,
                    "error",
                );
            } else {
                addToast("Scan complete", "success");
            }
            await load();
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            scanning = false;
            scanProgress = null;
        }
    }

    async function handleToggleEnabled(folder: Folder) {
        try {
            await setFolderEnabled(folder.id, !folder.enabled);
            await load();
        } catch (e) {
            addToast(String(e), "error");
        }
    }
</script>

<div class="folders-page page-enter">
    <div class="header">
        <h1 class="page-title">Folders</h1>
        <div class="actions">
            <button
                class="btn-pill btn-secondary"
                onclick={handleAdd}
                disabled={loading || scanning}
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
                    <path d="M5 12h14" />
                    <path d="M12 5v14" />
                </svg>
                Add folder
            </button>
            <button
                class="btn-pill btn-primary"
                onclick={() => handleScan(false)}
                disabled={loading || scanning}
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
                    <path
                        d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8"
                    />
                    <path d="M21 3v5h-5" />
                </svg>
                {scanning ? "Scanning..." : "Scan library"}
            </button>
            <button
                class="btn-pill btn-secondary"
                onclick={() => handleScan(true)}
                disabled={loading || scanning}
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
                    <path
                        d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8"
                    />
                    <path d="M21 3v5h-5" />
                </svg>
                Full rescan
            </button>
        </div>
    </div>

    {#if scanning}
        <div class="scan-progress" aria-live="polite">
            <div class="scan-progress-head">
                <div class="scan-progress-title">
                    <Loading variant="inline" />
                    <span
                        >{scanProgress?.phase === "cleaning"
                            ? "Finishing library scan..."
                            : "Scanning library..."}</span
                    >
                </div>
                {#if scanProgress && scanProgress.total > 0}
                    <span class="scan-progress-count"
                        >{scanProgress.scanned} / {scanProgress.total}</span
                    >
                {/if}
            </div>
            <div
                class="scan-progress-track"
                role="progressbar"
                aria-valuemin="0"
                aria-valuemax={scanProgress?.total ?? 0}
                aria-valuenow={scanProgress?.scanned ?? 0}
            >
                <div
                    class="scan-progress-fill"
                    style={`width: ${scanProgress?.total ? Math.min(100, (scanProgress.scanned / scanProgress.total) * 100) : 0}%`}
                ></div>
            </div>
            {#if scanProgress}
                <div class="scan-progress-meta">
                    <span
                        >{scanProgress.added} added · {scanProgress.updated} updated
                        · {scanProgress.errors} errors</span
                    >
                    {#if scanProgress.current_path}
                        <span
                            class="scan-current-path"
                            title={scanProgress.current_path}
                            >{scanProgress.current_path}</span
                        >
                    {/if}
                </div>
            {/if}
        </div>
    {/if}

    {#if scanResult}
        <div class="scan-summary" class:has-errors={scanResult.errors > 0}>
            <div class="scan-title">Scan results</div>
            <div class="scan-stats">
                <span class="stat">
                    <span class="stat-value">{scanResult.scanned}</span>
                    <span class="stat-label">Scanned</span>
                </span>
                <span class="stat">
                    <span class="stat-value">{scanResult.added}</span>
                    <span class="stat-label">Added</span>
                </span>
                <span class="stat">
                    <span class="stat-value">{scanResult.updated}</span>
                    <span class="stat-label">Updated</span>
                </span>
                <span class="stat">
                    <span class="stat-value">{scanResult.removed}</span>
                    <span class="stat-label">Removed</span>
                </span>
                <span class="stat errors">
                    <span class="stat-value">{scanResult.errors}</span>
                    <span class="stat-label">Errors</span>
                </span>
            </div>
            {#if scanResult.errors > 0}
                <p class="scan-warning">
                    Some files could not be scanned. Check the folder contents
                    and try again.
                </p>
            {/if}
        </div>
    {/if}

    {#if loading}
        <Loading />
    {:else if folders.length === 0}
        <div class="empty-state">
            <div class="empty-icon">
                <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.5"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    <path
                        d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"
                    />
                </svg>
            </div>
            <p class="empty-title">No folders monitored</p>
            <p class="empty-text">
                Add a folder to start building your library.
            </p>
            <button class="btn-pill btn-primary" onclick={handleAdd}
                >Add your first folder</button
            >
        </div>
    {:else}
        <ul class="folder-list">
            {#each folders as folder (folder.id)}
                <li class="folder-item">
                    <div class="folder-icon">
                        <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="1.5"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            aria-hidden="true"
                        >
                            <path
                                d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"
                            />
                        </svg>
                    </div>
                    <div class="folder-info">
                        <div class="folder-path ellipsis">{folder.path}</div>
                        <div class="folder-meta">
                            {#if folder.scanned_at}
                                <span
                                    >Scanned {new Date(
                                        folder.scanned_at * 1000,
                                    ).toLocaleString()}</span
                                >
                            {:else}
                                <span>Not scanned yet</span>
                            {/if}
                        </div>
                    </div>
                    <button
                        class="switch"
                        class:on={folder.enabled}
                        role="switch"
                        aria-checked={folder.enabled}
                        aria-label={folder.enabled
                            ? "Disable folder"
                            : "Enable folder"}
                        title={folder.enabled
                            ? "Enabled — included in scans"
                            : "Disabled — skipped by scans"}
                        onclick={() => handleToggleEnabled(folder)}
                    >
                        <span class="switch-thumb" aria-hidden="true"></span>
                    </button>
                    <button
                        class="remove-btn"
                        aria-label="Remove folder"
                        onclick={() => handleRemove(folder.id)}
                        disabled={scanning}
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
                            <path d="M3 6h18" />
                            <path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6" />
                            <path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" />
                            <line x1="10" x2="10" y1="11" y2="17" />
                            <line x1="14" x2="14" y1="11" y2="17" />
                        </svg>
                    </button>
                </li>
            {/each}
        </ul>
    {/if}
</div>

<style>
    .folders-page {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xl);
    }

    .header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
        flex-wrap: wrap;
    }

    .actions {
        display: flex;
        gap: var(--spacing-md);
    }

    .actions .btn-pill svg {
        width: 1rem;
        height: 1rem;
    }

    .scan-progress {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
        padding: var(--spacing-md) var(--spacing-lg);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-lg);
        background: var(--color-surface);
    }

    .scan-progress-head,
    .scan-progress-title,
    .scan-progress-meta {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
    }

    .scan-progress-head,
    .scan-progress-meta {
        justify-content: space-between;
    }

    .scan-progress-title {
        font-weight: var(--font-weight-semibold);
    }

    .scan-progress-count,
    .scan-progress-meta {
        color: var(--color-text-muted);
        font-size: var(--font-size-sm);
        font-variant-numeric: tabular-nums;
    }

    .scan-progress-track {
        height: 0.4rem;
        overflow: hidden;
        border-radius: var(--radius-full);
        background: var(--color-surface-raised);
    }

    .scan-progress-fill {
        height: 100%;
        border-radius: inherit;
        background: var(--color-accent);
        transition: width 180ms ease;
    }

    .scan-current-path {
        max-width: 55%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .folder-icon {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 2.5rem;
        height: 2.5rem;
        flex-shrink: 0;
        border-radius: var(--radius);
        background-color: var(--color-surface-elevated);
        color: var(--color-text-muted);
    }

    .folder-icon svg {
        width: 1.25rem;
        height: 1.25rem;
    }

    .scan-progress {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        color: var(--color-text-muted);
        font-size: var(--font-size-sm);
    }

    .scan-summary {
        background-color: var(--color-surface);
        padding: var(--spacing-md) var(--spacing-lg);
        border-radius: var(--radius-lg);
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
    }

    .scan-summary.has-errors {
        border: 1px solid var(--color-error);
    }

    .scan-title {
        font-weight: var(--font-weight-semibold);
        color: var(--color-text);
    }

    .scan-stats {
        display: flex;
        gap: var(--spacing-xl);
        flex-wrap: wrap;
    }

    .stat {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .stat-value {
        font-size: var(--font-size-2xl);
        font-weight: var(--font-weight-bold);
        line-height: var(--line-height-tight);
        color: var(--color-text);
    }

    .stat-label {
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
        text-transform: uppercase;
        letter-spacing: 0.05em;
    }

    .stat.errors .stat-value {
        color: var(--color-error);
    }

    .scan-warning {
        margin: 0;
        color: var(--color-error);
        font-size: var(--font-size-sm);
    }

    .folder-list {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
    }

    .folder-item {
        background-color: var(--color-surface);
        padding: var(--spacing-md) var(--spacing-lg);
        border-radius: var(--radius-lg);
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
        flex-wrap: wrap;
        transition: background-color var(--transition-fast);
    }

    .folder-item:hover {
        background-color: var(--color-surface-elevated);
    }

    .folder-info {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        min-width: 0;
        flex: 1;
    }

    .folder-path {
        font-weight: var(--font-weight-medium);
        word-break: break-all;
        color: var(--color-text);
    }

    .folder-meta {
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
        flex-wrap: wrap;
    }

    .remove-btn {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 2.25rem;
        height: 2.25rem;
        flex-shrink: 0;
        border-radius: var(--radius-full);
        background-color: transparent;
        color: var(--color-text-muted);
        border: 1px solid var(--color-border);
        transition:
            background-color var(--transition-fast),
            color var(--transition-fast),
            border-color var(--transition-fast);
    }

    .remove-btn:hover:not(:disabled) {
        color: var(--color-error);
        border-color: var(--color-error);
        background-color: rgba(226, 33, 52, 0.08);
    }

    .remove-btn:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }

    .remove-btn svg {
        width: 1rem;
        height: 1rem;
    }

    @media (max-width: 640px) {
        .header {
            flex-direction: column;
            align-items: flex-start;
        }
    }
</style>
