<script lang="ts">
    interface Props {
        message: string | null;
        issues?: string[];
        failed?: boolean;
    }
    let { message, issues = [], failed = false }: Props = $props();
</script>

<div class="search-feedback" class:failed>
    <p role="status" aria-live="polite">{message ?? ""}</p>
    {#if issues.length > 0}
        <details>
            <summary>Provider details ({issues.length})</summary>
            <ul>
                {#each issues as issue}<li>{issue}</li>{/each}
            </ul>
        </details>
    {/if}
</div>

<style>
    .search-feedback {
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
        line-height: var(--line-height);
        overflow-wrap: anywhere;
    }
    p {
        margin: 0;
    }
    p:empty {
        display: none;
    }
    .failed p {
        color: var(--color-error);
    }
    details {
        margin-top: var(--spacing-xs);
    }
    summary {
        cursor: pointer;
        width: fit-content;
    }
    summary:hover {
        color: var(--color-text);
    }
    summary:focus-visible {
        outline: 2px solid var(--color-accent-focus);
        outline-offset: 2px;
    }
    ul {
        margin-top: var(--spacing-xs);
        display: grid;
        gap: var(--spacing-xs);
    }
</style>
