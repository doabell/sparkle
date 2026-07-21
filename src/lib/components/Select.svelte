<script lang="ts">
    interface Option {
        value: string;
        label: string;
    }

    interface Props {
        options: Option[];
        value: string;
        onchange: (value: string) => void;
        ariaLabel?: string;
    }

    let { options, value, onchange, ariaLabel }: Props = $props();

    let open = $state(false);
    let rootRef = $state<HTMLDivElement | undefined>();

    let currentLabel = $derived(
        options.find((o) => o.value === value)?.label ?? value,
    );

    function toggle(e: MouseEvent) {
        e.stopPropagation();
        open = !open;
    }

    function choose(v: string) {
        onchange(v);
        open = false;
    }

    function handleWindowClick(e: MouseEvent) {
        if (!open) return;
        if (rootRef?.contains(e.target as Node)) return;
        open = false;
    }

    function handleKeydown(e: KeyboardEvent) {
        if (e.key === "Escape" && open) {
            e.stopPropagation();
            open = false;
        }
    }
</script>

<svelte:window onclick={handleWindowClick} onkeydown={handleKeydown} />

<div class="select" bind:this={rootRef}>
    <button
        type="button"
        class="select-trigger"
        class:open
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        onclick={toggle}
    >
        <span class="select-value ellipsis">{currentLabel}</span>
        <svg
            class="select-chevron"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
        >
            <path d="m6 9 6 6 6-6" />
        </svg>
    </button>

    {#if open}
        <div class="select-menu" role="listbox" aria-label={ariaLabel}>
            {#each options as option (option.value)}
                <button
                    type="button"
                    class="select-option"
                    class:selected={option.value === value}
                    role="option"
                    aria-selected={option.value === value}
                    onclick={() => choose(option.value)}
                >
                    <span class="ellipsis">{option.label}</span>
                    {#if option.value === value}
                        <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2.5"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            aria-hidden="true"
                        >
                            <path d="M20 6 9 17l-5-5" />
                        </svg>
                    {/if}
                </button>
            {/each}
        </div>
    {/if}
</div>

<style>
    .select {
        position: relative;
        display: inline-block;
    }

    .select-trigger {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-sm);
        padding: var(--spacing-xs) var(--spacing-sm) var(--spacing-xs)
            var(--spacing-md);
        border-radius: var(--radius-full);
        border: 1px solid var(--color-border);
        background-color: var(--color-surface-elevated);
        color: var(--color-text);
        font-size: var(--font-size-sm);
        cursor: pointer;
        transition:
            background-color var(--transition-fast),
            border-color var(--transition-fast);
    }

    .select-trigger:hover,
    .select-trigger.open {
        background-color: var(--color-surface-raised);
    }

    .select-trigger:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: 1px;
    }

    .select-value {
        max-width: 10rem;
    }

    .select-chevron {
        width: 0.875rem;
        height: 0.875rem;
        color: var(--color-text-muted);
        transition: transform var(--transition-fast);
    }

    .select-trigger.open .select-chevron {
        transform: rotate(180deg);
    }

    .select-menu {
        position: absolute;
        top: calc(100% + 0.375rem);
        right: 0;
        z-index: 60;
        width: max-content;
        min-width: 100%;
        max-width: 20rem;
        max-height: 16rem;
        overflow-y: auto;
        padding: var(--spacing-xs);
        background: rgba(var(--color-surface-rgb), 0.95);
        backdrop-filter: blur(24px) saturate(1.8);
        -webkit-backdrop-filter: blur(24px) saturate(1.8);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-lg);
        box-shadow: var(--shadow-lg);
        animation: select-in 120ms ease-out;
    }

    @keyframes select-in {
        from {
            opacity: 0;
            transform: translateY(-4px) scale(0.98);
        }
        to {
            opacity: 1;
            transform: translateY(0) scale(1);
        }
    }

    .select-option {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
        width: 100%;
        padding: var(--spacing-xs) var(--spacing-sm);
        border-radius: var(--radius-sm);
        color: var(--color-text);
        font-size: var(--font-size-sm);
        text-align: left;
        cursor: pointer;
        transition:
            background-color var(--transition-fast),
            color var(--transition-fast);
        white-space: nowrap;
    }

    .select-option:hover {
        background-color: rgba(255, 255, 255, 0.08);
    }

    .select-option.selected {
        color: var(--color-accent);
        font-weight: var(--font-weight-semibold);
    }

    .select-option svg {
        width: 0.875rem;
        height: 0.875rem;
        flex-shrink: 0;
    }
</style>
