<script lang="ts">
    interface Props {
        value: "grid" | "row";
        onchange: (value: "grid" | "row") => void;
        label?: string;
    }

    let { value, onchange, label = "View" }: Props = $props();
</script>

<div class="segmented-control icon-only" role="group" aria-label={label}>
    <button
        type="button"
        class:active={value === "grid"}
        aria-label="Grid view"
        aria-pressed={value === "grid"}
        title="Grid view"
        onclick={() => onchange("grid")}
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
            <rect x="3" y="3" width="7" height="7" rx="1" />
            <rect x="14" y="3" width="7" height="7" rx="1" />
            <rect x="3" y="14" width="7" height="7" rx="1" />
            <rect x="14" y="14" width="7" height="7" rx="1" />
        </svg>
    </button>
    <button
        type="button"
        class:active={value === "row"}
        aria-label="List view"
        aria-pressed={value === "row"}
        title="List view"
        onclick={() => onchange("row")}
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
            <line x1="4" y1="6" x2="20" y2="6" />
            <line x1="4" y1="12" x2="20" y2="12" />
            <line x1="4" y1="18" x2="20" y2="18" />
        </svg>
    </button>
</div>
