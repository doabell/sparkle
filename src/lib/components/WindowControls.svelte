<script lang="ts">
    import { onMount } from "svelte";
    import {
        getCurrentWindow,
        type Window as TauriWindow,
    } from "@tauri-apps/api/window";

    let appWindow: TauriWindow | null = null;
    let maximized = $state(false);

    async function withWindow(action: (window: TauriWindow) => Promise<void>) {
        try {
            appWindow ??= getCurrentWindow();
            await action(appWindow);
        } catch {
            // Browser previews have no native window. Keep the controls inert.
        }
    }

    async function syncMaximized() {
        try {
            appWindow ??= getCurrentWindow();
            maximized = await appWindow.isMaximized();
        } catch {
            maximized = false;
        }
    }

    async function toggleMaximize() {
        await withWindow((window) => window.toggleMaximize());
        await syncMaximized();
    }

    onMount(() => {
        let disposed = false;
        let unlistenResize: (() => void) | undefined;

        try {
            appWindow = getCurrentWindow();
            void syncMaximized();
            void appWindow
                .onResized(() => void syncMaximized())
                .then((unlisten) => {
                    if (disposed) unlisten();
                    else unlistenResize = unlisten;
                })
                .catch(() => {
                    // Browser previews cannot subscribe to native resize events.
                });
        } catch {
            // The regular web preview intentionally has no native window.
        }

        return () => {
            disposed = true;
            unlistenResize?.();
        };
    });
</script>

<div class="window-drag-region" data-tauri-drag-region></div>
<div class="window-controls" aria-label="Window controls">
    <button
        type="button"
        class="window-control"
        aria-label="Minimize"
        title="Minimize"
        onclick={() => void withWindow((window) => window.minimize())}
    >
        <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="M4 8h8" />
        </svg>
    </button>
    <button
        type="button"
        class="window-control"
        aria-label={maximized ? "Restore" : "Maximize"}
        title={maximized ? "Restore" : "Maximize"}
        onclick={() => void toggleMaximize()}
    >
        <svg viewBox="0 0 16 16" aria-hidden="true">
            {#if maximized}
                <path d="M6 5V3h7v7h-2" />
                <rect x="3" y="5" width="8" height="8" rx="1.5" />
            {:else}
                <rect x="4" y="4" width="8" height="8" rx="1.5" />
            {/if}
        </svg>
    </button>
    <button
        type="button"
        class="window-control close"
        aria-label="Close"
        title="Close"
        onclick={() => void withWindow((window) => window.close())}
    >
        <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="m4.5 4.5 7 7m0-7-7 7" />
        </svg>
    </button>
</div>

<style>
    .window-drag-region {
        position: fixed;
        inset: 0 0 auto;
        z-index: 190;
        height: var(--window-chrome-height);
    }

    .window-controls {
        position: fixed;
        top: 0;
        right: 0;
        z-index: 200;
        display: flex;
        color: var(--color-text-secondary);
    }

    .window-control {
        position: relative;
        display: flex;
        align-items: center;
        justify-content: center;
        width: var(--window-chrome-height);
        height: var(--window-chrome-height);
        border-radius: 0;
        background: transparent;
        color: inherit;
        transition: color var(--transition-fast);
    }

    /* The visible tile is inset; the invisible rectangular target still
       reaches every window edge, including the exact Close corner. */
    .window-control::before {
        content: "";
        position: absolute;
        inset: 0.375rem;
        border-radius: var(--radius);
        pointer-events: none;
        transition: background-color var(--transition-fast);
    }

    .window-control:hover {
        color: var(--color-text);
    }

    .window-control:hover::before {
        background-color: var(--interactive-hover);
    }

    .window-control:active::before {
        background-color: var(--interactive-active);
    }

    /* Keep the edge hit targets fixed while the glyph gives press feedback. */
    .window-control:hover svg {
        transform: scale(var(--motion-hover-scale));
    }

    .window-control:active svg {
        transform: scale(var(--motion-press-scale));
    }

    .window-control:focus-visible {
        outline-offset: -3px;
    }

    .window-control.close:hover {
        color: var(--color-error);
    }

    .window-control.close:hover::before {
        background-color: color-mix(
            in srgb,
            var(--color-error) 12%,
            transparent
        );
    }

    .window-control.close:active::before {
        background-color: color-mix(
            in srgb,
            var(--color-error) 18%,
            transparent
        );
    }

    .window-control svg {
        position: relative;
        width: 1rem;
        height: 1rem;
        fill: none;
        stroke: currentColor;
        stroke-width: 1.5;
        stroke-linecap: round;
        stroke-linejoin: round;
        pointer-events: none;
        transition: transform var(--transition-fast);
    }

    @media (prefers-reduced-motion: reduce) {
        .window-control,
        .window-control::before,
        .window-control svg {
            transition: none;
        }
    }
</style>
