<script lang="ts">
    import { onDestroy, tick } from "svelte";

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

    const componentId = $props.id();
    const triggerId = `${componentId}-trigger`;
    const listboxId = `${componentId}-listbox`;

    let open = $state(false);
    let activeIndex = $state(-1);
    let rootRef = $state<HTMLDivElement | null>(null);
    let triggerRef = $state<HTMLButtonElement | null>(null);
    let listboxRef = $state<HTMLDivElement | null>(null);
    let typeahead = "";
    let typeaheadTimer: ReturnType<typeof setTimeout> | undefined;

    let selectedIndex = $derived(
        options.findIndex((option) => option.value === value),
    );
    let currentLabel = $derived(options[selectedIndex]?.label ?? value);
    let activeOptionId = $derived(
        activeIndex >= 0 ? `${componentId}-option-${activeIndex}` : undefined,
    );

    $effect(() => {
        if (!open || activeIndex < 0) return;
        activeIndex;
        void tick().then(() => {
            listboxRef
                ?.querySelector<HTMLElement>(`#${activeOptionId}`)
                ?.scrollIntoView({ block: "nearest" });
        });
    });

    function resetTypeahead() {
        typeahead = "";
        if (typeaheadTimer) {
            clearTimeout(typeaheadTimer);
            typeaheadTimer = undefined;
        }
    }

    function scheduleTypeaheadReset() {
        if (typeaheadTimer) clearTimeout(typeaheadTimer);
        typeaheadTimer = setTimeout(() => {
            typeahead = "";
            typeaheadTimer = undefined;
        }, 500);
    }

    function normalizedLabel(label: string) {
        return label.trim().toLocaleLowerCase();
    }

    function findTypeaheadMatch(query: string, fromIndex: number) {
        const normalizedQuery = normalizedLabel(query);
        if (!normalizedQuery || options.length === 0) return -1;

        for (let offset = 1; offset <= options.length; offset += 1) {
            const index =
                (fromIndex + offset + options.length) % options.length;
            if (
                normalizedLabel(options[index].label).startsWith(
                    normalizedQuery,
                )
            ) {
                return index;
            }
        }

        return -1;
    }

    function typeaheadMatch(key: string, fromIndex: number) {
        const nextQuery = `${typeahead}${key}`;
        let match = findTypeaheadMatch(nextQuery, fromIndex);

        if (match >= 0) {
            typeahead = nextQuery;
        } else {
            typeahead = key;
            match = findTypeaheadMatch(typeahead, fromIndex);
        }

        scheduleTypeaheadReset();
        return match;
    }

    function openList(preferredIndex = selectedIndex) {
        if (options.length === 0) return;
        activeIndex =
            preferredIndex >= 0 && preferredIndex < options.length
                ? preferredIndex
                : 0;
        open = true;
        void tick().then(() => listboxRef?.focus());
    }

    function closeList(restoreFocus: boolean) {
        open = false;
        activeIndex = -1;
        resetTypeahead();
        if (restoreFocus) {
            void tick().then(() => triggerRef?.focus());
        }
    }

    function choose(index: number) {
        const option = options[index];
        if (!option) return;
        onchange(option.value);
        closeList(true);
    }

    function toggle() {
        if (open) {
            closeList(true);
        } else {
            openList();
        }
    }

    function moveActive(delta: number) {
        if (options.length === 0) return;
        const current =
            activeIndex >= 0
                ? activeIndex
                : selectedIndex >= 0
                  ? selectedIndex
                  : 0;
        activeIndex = Math.max(
            0,
            Math.min(options.length - 1, current + delta),
        );
    }

    function hasTypingModifiers(event: KeyboardEvent) {
        return event.altKey || event.ctrlKey || event.metaKey;
    }

    function handleTriggerKeydown(event: KeyboardEvent) {
        if (options.length === 0) return;

        switch (event.key) {
            case "ArrowDown":
            case "ArrowUp":
                event.preventDefault();
                openList();
                return;
            case "Home":
                event.preventDefault();
                openList(0);
                return;
            case "End":
                event.preventDefault();
                openList(options.length - 1);
                return;
            case "Enter":
            case " ":
                event.preventDefault();
                openList();
                return;
        }

        if (event.key.length === 1 && !hasTypingModifiers(event)) {
            event.preventDefault();
            const match = typeaheadMatch(event.key, selectedIndex);
            if (match >= 0) onchange(options[match].value);
        }
    }

    function handleListboxKeydown(event: KeyboardEvent) {
        switch (event.key) {
            case "ArrowDown":
                event.preventDefault();
                moveActive(1);
                return;
            case "ArrowUp":
                event.preventDefault();
                moveActive(-1);
                return;
            case "Home":
                event.preventDefault();
                activeIndex = 0;
                return;
            case "End":
                event.preventDefault();
                activeIndex = options.length - 1;
                return;
            case "Enter":
            case " ":
                event.preventDefault();
                choose(activeIndex);
                return;
            case "Escape":
                event.preventDefault();
                event.stopPropagation();
                closeList(true);
                return;
        }

        if (event.key.length === 1 && !hasTypingModifiers(event)) {
            event.preventDefault();
            const match = typeaheadMatch(event.key, activeIndex);
            if (match >= 0) activeIndex = match;
        }
    }

    function handleWindowPointerdown(event: PointerEvent) {
        if (
            open &&
            event.target instanceof Node &&
            !rootRef?.contains(event.target)
        ) {
            closeList(false);
        }
    }

    function handleWindowKeydown(event: KeyboardEvent) {
        if (open && event.key === "Escape") {
            event.preventDefault();
            closeList(true);
        }
    }

    function handleFocusout(event: FocusEvent) {
        if (
            open &&
            event.relatedTarget instanceof Node &&
            !rootRef?.contains(event.relatedTarget)
        ) {
            closeList(false);
        }
    }

    onDestroy(resetTypeahead);
</script>

<svelte:window
    onpointerdown={handleWindowPointerdown}
    onkeydown={handleWindowKeydown}
/>

<div class="select" bind:this={rootRef} onfocusout={handleFocusout}>
    <button
        bind:this={triggerRef}
        id={triggerId}
        type="button"
        class="select-trigger"
        class:open
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listboxId : undefined}
        aria-label={ariaLabel ? `${ariaLabel}: ${currentLabel}` : undefined}
        disabled={options.length === 0}
        onclick={toggle}
        onkeydown={handleTriggerKeydown}
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
        <div
            bind:this={listboxRef}
            id={listboxId}
            class="select-menu"
            role="listbox"
            tabindex="-1"
            aria-label={ariaLabel}
            aria-labelledby={ariaLabel ? undefined : triggerId}
            aria-activedescendant={activeOptionId}
            onkeydown={handleListboxKeydown}
        >
            {#each options as option, index (option.value)}
                <button
                    id={`${componentId}-option-${index}`}
                    type="button"
                    class="select-option"
                    class:active={index === activeIndex}
                    class:selected={option.value === value}
                    role="option"
                    aria-selected={option.value === value}
                    tabindex="-1"
                    onpointermove={() => (activeIndex = index)}
                    onclick={() => choose(index)}
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
        max-width: 100%;
    }

    .select-trigger {
        display: inline-flex;
        align-items: center;
        max-width: 100%;
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
        outline: 2px solid var(--color-accent-focus);
        outline-offset: 1px;
    }

    .select-trigger:disabled {
        cursor: default;
        opacity: 0.55;
    }

    .select-value {
        max-width: 10rem;
    }

    .select-chevron {
        width: 0.875rem;
        height: 0.875rem;
        flex-shrink: 0;
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
        max-width: min(20rem, calc(100vw - 2rem));
        max-height: min(16rem, calc(100vh - 2rem));
        overflow-y: auto;
        padding: var(--spacing-xs);
        background: rgba(var(--color-surface-rgb), 0.95);
        backdrop-filter: blur(24px) saturate(1.8);
        -webkit-backdrop-filter: blur(24px) saturate(1.8);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-lg);
        box-shadow: var(--shadow-lg);
        outline: none;
        animation: select-in var(--motion-duration-fast)
            var(--motion-ease-enter);
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

    .select-option:hover,
    .select-option.active {
        background-color: var(--interactive-hover);
    }

    .select-option.selected {
        color: var(--color-accent-content);
        font-weight: var(--font-weight-semibold);
    }

    .select-option svg {
        width: 0.875rem;
        height: 0.875rem;
        flex-shrink: 0;
    }

    @media (prefers-reduced-motion: reduce) {
        .select-menu {
            animation: none;
        }

        .select-chevron {
            transition: none;
        }
    }
</style>
