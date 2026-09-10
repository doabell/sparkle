<script lang="ts">
    import { onMount } from "svelte";
    import { listen, type UnlistenFn } from "@tauri-apps/api/event";
    import {
        checkForUpdates,
        downloadUpdate,
        getUpdateStatus,
        installUpdate,
        type UpdateStatus,
    } from "$lib/api";

    let status = $state<UpdateStatus | null>(null);
    let requestPending = $state(false);
    let actionError = $state<string | null>(null);
    let mounted = false;
    let busy = $derived(
        requestPending ||
            status?.phase === "checking" ||
            status?.phase === "downloading" ||
            status?.phase === "installing",
    );

    function acceptStatus(next: UpdateStatus) {
        // The initial IPC response can arrive after a newer progress event.
        if (mounted && (!status || next.revision >= status.revision)) {
            status = next;
        }
    }

    onMount(() => {
        mounted = true;
        let unlisten: UnlistenFn | undefined;
        void (async () => {
            try {
                const stop = await listen<UpdateStatus>(
                    "update-status",
                    (event) => acceptStatus(event.payload),
                );
                if (!mounted) {
                    stop();
                    return;
                }
                unlisten = stop;
                acceptStatus(await getUpdateStatus());
            } catch (error) {
                if (mounted) actionError = String(error);
            }
        })();
        return () => {
            mounted = false;
            unlisten?.();
        };
    });

    async function run(action: () => Promise<UpdateStatus>) {
        if (busy) return;
        requestPending = true;
        actionError = null;
        try {
            acceptStatus(await action());
        } catch (error) {
            if (mounted) actionError = String(error);
        } finally {
            if (mounted) requestPending = false;
        }
    }
</script>

<div class="update-settings">
    <div class="update-heading">
        <div>
            <h3>Software updates</h3>
            {#if status}
                <p class="version">Sparkle {status.current_version}</p>
            {/if}
        </div>
        <a
            href="https://github.com/doabell/sparkle/releases"
            target="_blank"
            rel="noreferrer">GitHub Releases ↗</a
        >
    </div>
    <p class="hint">
        Check and download when you choose. Installing an update restarts
        Sparkle.
    </p>

    <div class="update-status" role="status" aria-live="polite">
        {#if !status}
            <p>
                {actionError
                    ? "Update information is unavailable."
                    : "Loading version…"}
            </p>
        {:else if status.phase === "unavailable"}
            <p>In-app updates are available in the Windows setup version.</p>
            <p class="hint">
                Development, portable, and older MSI builds can use GitHub
                Releases.
            </p>
        {:else if status.phase === "checking"}
            <p>Checking GitHub Releases…</p>
        {:else if status.phase === "up_to_date"}
            <p>You’re up to date.</p>
        {:else if status.phase === "available" && status.release}
            <p>Sparkle {status.release.version} is available.</p>
            <p class="hint">
                Up to {(status.release.size / (1024 * 1024)).toFixed(1)} MB to download
            </p>
        {:else if status.phase === "downloading"}
            <p>Downloading update… {status.download_percent ?? 0}%</p>
        {:else if status.phase === "ready" && status.release}
            <p>Sparkle {status.release.version} is ready to install.</p>
            <p class="hint">
                It will wait until you choose Restart to install.
            </p>
        {:else if status.phase === "installing"}
            <p>Closing Sparkle to install the update…</p>
        {:else}
            <p>No update check has been made this session.</p>
        {/if}
    </div>

    {#if status?.phase === "downloading"}
        <progress
            max="100"
            value={status.download_percent ?? 0}
            aria-label="Update download progress"
        ></progress>
    {/if}

    {#if actionError || (status?.error && status.phase !== "unavailable")}
        <p class="error" role="alert">{actionError || status?.error}</p>
    {/if}

    {#if !status && actionError}
        <div class="actions">
            <button disabled={busy} onclick={() => run(getUpdateStatus)}
                >Retry</button
            >
        </div>
    {:else if status && status.phase !== "unavailable"}
        <div class="actions">
            {#if status.phase === "ready" || status.phase === "installing"}
                <button
                    class="primary"
                    disabled={busy}
                    onclick={() => run(installUpdate)}
                    >Restart to install</button
                >
            {:else}
                <button disabled={busy} onclick={() => run(checkForUpdates)}
                    >{status.phase === "checking"
                        ? "Checking…"
                        : "Check for updates"}</button
                >
                {#if status.phase === "available" || status.phase === "downloading"}
                    <button
                        class="primary"
                        disabled={busy}
                        onclick={() => run(downloadUpdate)}
                        >{status.phase === "downloading"
                            ? "Downloading…"
                            : "Download update"}</button
                    >
                {/if}
            {/if}
        </div>
    {/if}

    {#if status?.release?.notes}
        <details>
            <summary>What’s new in {status.release.version}</summary>
            <pre>{status.release.notes}</pre>
        </details>
    {/if}
</div>

<style>
    .update-settings {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
        min-width: 0;
    }
    .update-heading {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        flex-wrap: wrap;
        gap: var(--spacing-md);
    }
    h3 {
        font-size: var(--font-size-base);
        font-weight: var(--font-weight-semibold);
    }
    p {
        margin: 0;
    }
    .version {
        margin-top: var(--spacing-xs);
        color: var(--color-text-secondary);
        font-size: var(--font-size-sm);
    }
    a,
    summary {
        font-size: var(--font-size-sm);
    }
    a {
        color: var(--color-accent-content);
    }
    a:hover {
        text-decoration: underline;
    }
    .hint {
        color: var(--color-text-muted);
        font-size: var(--font-size-sm);
        line-height: 1.6;
    }
    .update-status {
        display: grid;
        gap: var(--spacing-xs);
        font-size: var(--font-size-sm);
    }
    .actions {
        display: flex;
        flex-wrap: wrap;
        gap: var(--spacing-sm);
    }
    button {
        padding: var(--spacing-sm) var(--spacing-md);
        border: 1px solid var(--color-border);
        border-radius: var(--radius);
        font-size: var(--font-size-sm);
        transition: background-color var(--transition-feedback);
    }
    button:hover:not(:disabled) {
        background: var(--interactive-hover);
    }
    button.primary {
        background: var(--color-accent-fill);
        color: var(--color-on-accent-fill);
        border-color: transparent;
    }
    button.primary:hover:not(:disabled) {
        background: var(--color-accent-fill-hover);
    }
    button:disabled {
        opacity: 0.5;
        cursor: default;
    }
    .error {
        color: var(--color-error, #d55b5b);
        font-size: var(--font-size-sm);
        overflow-wrap: anywhere;
    }
    progress {
        width: 100%;
        height: 0.5rem;
        accent-color: var(--color-accent-native);
    }
    summary {
        cursor: pointer;
    }
    pre {
        margin: var(--spacing-md) 0 0;
        font: inherit;
        font-size: var(--font-size-sm);
        white-space: pre-wrap;
        overflow-wrap: anywhere;
        line-height: 1.65;
    }
</style>
