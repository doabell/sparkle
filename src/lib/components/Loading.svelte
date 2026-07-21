<script lang="ts">
    interface Props {
        variant?: "full" | "inline";
    }

    let { variant = "full" }: Props = $props();
</script>

<div
    class="loading"
    class:full={variant === "full"}
    class:inline={variant === "inline"}
    aria-busy="true"
>
    <svg
        class="spinner"
        viewBox="0 0 24 24"
        xmlns="http://www.w3.org/2000/svg"
        aria-hidden="true"
    >
        <circle
            class="track"
            cx="12"
            cy="12"
            r="10"
            fill="none"
            stroke="currentColor"
            stroke-width="4"
            opacity="0.2"
        />
        <path
            class="arc"
            fill="none"
            stroke="currentColor"
            stroke-width="4"
            d="M12 2 a10 10 0 0 1 10 10"
            stroke-linecap="round"
        />
    </svg>
    {#if variant === "full"}
        <span class="label">Loading…</span>
    {/if}
</div>

<style>
    .loading {
        display: flex;
        align-items: center;
        justify-content: center;
        color: var(--color-text-muted);
    }

    .full {
        flex-direction: column;
        gap: 0.75rem;
        min-height: 50vh;
    }

    .inline {
        display: inline-flex;
        gap: 0.5rem;
    }

    .spinner {
        width: 40px;
        height: 40px;
        animation: spin 1s linear infinite;
    }

    .inline .spinner {
        width: 1em;
        height: 1em;
    }

    .label {
        font-size: var(--font-size-sm);
        color: var(--color-text-muted);
    }

    @keyframes spin {
        to {
            transform: rotate(360deg);
        }
    }
</style>
