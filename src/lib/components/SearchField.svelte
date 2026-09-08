<script lang="ts">
    import Loading from "$lib/components/Loading.svelte";

    interface Props {
        value: string;
        label: string;
        placeholder?: string;
        busy?: boolean;
        disabled?: boolean;
        onsearch: () => void;
    }

    let {
        value = $bindable(),
        label,
        placeholder = "Search…",
        busy = false,
        disabled = false,
        onsearch,
    }: Props = $props();
    const id = $props.id();
</script>

<form
    class="search-field"
    role="search"
    aria-label={label}
    onsubmit={(event) => {
        event.preventDefault();
        if (!busy && !disabled && value.trim()) onsearch();
    }}
>
    <label class="search-field-label" for={id}>{label}</label>
    <div class="search-input-group">
        <input
            {id}
            type="search"
            bind:value
            {placeholder}
            spellcheck="false"
            autocomplete="off"
            disabled={busy || disabled}
            onkeydown={(event) => {
                if (event.key === "Enter" && event.isComposing)
                    event.preventDefault();
            }}
        />
        <button
            class="btn-pill btn-secondary"
            type="submit"
            disabled={busy || disabled || !value.trim()}
        >
            {#if busy}<Loading variant="inline" />{/if}
            {busy ? "Searching…" : "Search"}
        </button>
    </div>
</form>

<style>
    .search-field {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
    }
    .search-field-label {
        font-size: var(--font-size-sm);
        font-weight: var(--font-weight-semibold);
    }
</style>
