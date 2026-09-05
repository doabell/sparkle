<script lang="ts">
    import { toasts } from "$lib/stores/toast";
    import { cubicOut } from "svelte/easing";
    import { fly } from "svelte/transition";

    function toastFly(node: Element) {
        const reduced =
            window.matchMedia("(prefers-reduced-motion: reduce)").matches ||
            document.documentElement.dataset.motion !== "full";
        return fly(node, {
            y: -12,
            duration: reduced ? 0 : 220,
            easing: cubicOut,
        });
    }
</script>

<div
    class="toaster"
    role="region"
    aria-live="polite"
    aria-label="Notifications"
>
    {#each $toasts as toast (toast.id)}
        <div class="toast {toast.type}" transition:toastFly>
            <span class="message">{toast.message}</span>
            <button
                aria-label="Dismiss"
                onclick={() => toasts.removeToast(toast.id)}>×</button
            >
        </div>
    {/each}
</div>

<style>
    .toaster {
        position: fixed;
        top: calc(var(--window-chrome-height) + var(--spacing-sm));
        right: var(--spacing-lg);
        z-index: 1000;
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
        align-items: flex-end;
        pointer-events: none;
    }

    .toast {
        pointer-events: auto;
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
        padding: var(--spacing-sm) var(--spacing-md);
        border-radius: var(--radius-lg);
        background-color: var(--color-surface-elevated);
        color: var(--color-text);
        border: 1px solid var(--color-border);
        box-shadow: var(--shadow-md);
        min-width: 240px;
        max-width: 360px;
        font-size: var(--font-size-sm);
    }

    .toast.success {
        background-color: var(--color-success);
        color: var(--color-background);
        border-color: transparent;
    }

    .toast.error {
        background-color: var(--color-error);
        color: var(--color-text);
        border-color: transparent;
    }

    .message {
        flex: 1;
        line-height: var(--line-height);
    }

    button {
        background: transparent;
        border: none;
        color: inherit;
        font-size: var(--font-size-lg);
        line-height: 1;
        cursor: pointer;
        padding: 0;
        margin: -0.25rem -0.5rem -0.25rem 0;
        opacity: 0.8;
        transition: opacity var(--transition-fast);
    }

    button:hover {
        opacity: 1;
    }
</style>
