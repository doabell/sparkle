<script lang="ts">
    interface Props {
        ascending: boolean;
        ontoggle: () => void;
        ariaLabel?: string;
    }

    let {
        ascending,
        ontoggle,
        ariaLabel = "Toggle sort direction",
    }: Props = $props();
</script>

<button
    type="button"
    class="sort-dir"
    onclick={ontoggle}
    aria-label={ariaLabel}
    title={ascending ? "Ascending" : "Descending"}
>
    <svg
        class="sort-dir-icon"
        class:desc={!ascending}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
    >
        <path d="M12 5v14" />
        <path d="m19 12-7 7-7-7" />
    </svg>
</button>

<style>
    .sort-dir {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.875rem;
        height: 1.875rem;
        border-radius: var(--radius-full);
        border: 1px solid var(--color-border);
        background-color: var(--color-surface-elevated);
        color: var(--color-text-secondary);
        cursor: pointer;
        transition:
            background-color var(--transition-feedback),
            border-color var(--transition-feedback),
            color var(--transition-feedback);
    }

    .sort-dir:hover {
        background-color: var(--interactive-active);
        color: var(--color-text);
    }

    .sort-dir:focus-visible {
        outline: 2px solid var(--color-accent-focus);
        outline-offset: 1px;
    }

    .sort-dir-icon {
        width: 0.9375rem;
        height: 0.9375rem;
        transition: transform var(--transition-transform);
        transform: rotate(180deg);
    }

    .sort-dir-icon.desc {
        transform: rotate(0deg);
    }
</style>
