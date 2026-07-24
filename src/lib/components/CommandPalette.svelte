<script lang="ts">
    import { goto } from "$app/navigation";
    import { moveCommandSelection } from "$lib/utils/commandPalette";
    import { onMount } from "svelte";

    interface Props {
        onClose: () => void;
    }
    let { onClose }: Props = $props();
    let query = $state("");
    let inputRef = $state<HTMLInputElement | null>(null);
    let paletteRef = $state<HTMLDivElement | null>(null);
    let backdropRef = $state<HTMLDivElement | null>(null);
    let selectedIndex = $state(0);
    const previouslyFocused =
        typeof document === "undefined"
            ? null
            : (document.activeElement as HTMLElement | null);
    const actions = [
        { label: "Home", hint: "Go to your library", href: "/" },
        {
            label: "Search",
            hint: "Search songs, artists, albums, lyrics",
            href: "/search",
        },
        { label: "Artists", hint: "Browse artists", href: "/artists" },
        { label: "Albums", hint: "Browse albums", href: "/albums" },
        { label: "Songs", hint: "Browse every song", href: "/songs" },
        { label: "Playlists", hint: "Manage playlists", href: "/playlists" },
        {
            label: "Listening stats",
            hint: "See your listening history",
            href: "/stats",
        },
        {
            label: "Library health",
            hint: "Find metadata gaps",
            href: "/health",
        },
        {
            label: "Folders",
            hint: "Scan and manage music folders",
            href: "/folders",
        },
        { label: "Settings", hint: "Customize Sparkle", href: "/settings" },
    ];
    let filteredActions = $derived(
        actions.filter((action) =>
            `${action.label} ${action.hint}`
                .toLowerCase()
                .includes(query.trim().toLowerCase()),
        ),
    );
    let hasSearchAction = $derived(query.trim().length > 0);
    let selectableCount = $derived(
        filteredActions.length + (hasSearchAction ? 1 : 0),
    );
    let activeOptionId = $derived(
        selectableCount > 0 ? `palette-option-${selectedIndex}` : undefined,
    );

    $effect(() => {
        query;
        selectedIndex = 0;
        queueMicrotask(() => inputRef?.focus());
    });

    $effect(() => {
        const optionId = activeOptionId;
        if (!optionId) return;
        queueMicrotask(() => {
            paletteRef
                ?.querySelector(`#${optionId}`)
                ?.scrollIntoView({ block: "nearest" });
        });
    });

    function activate(href: string) {
        onClose();
        goto(href);
    }

    function moveSelection(delta: number) {
        selectedIndex = moveCommandSelection(
            selectedIndex,
            delta,
            selectableCount,
        );
    }

    function handleKeydown(event: KeyboardEvent) {
        if (event.key === "ArrowDown") {
            event.preventDefault();
            moveSelection(1);
        }
        if (event.key === "ArrowUp") {
            event.preventDefault();
            moveSelection(-1);
        }
        if (event.key === "Enter") {
            event.preventDefault();
            if (hasSearchAction && selectedIndex === 0) {
                activate(`/search?q=${encodeURIComponent(query.trim())}`);
            } else {
                const actionIndex = selectedIndex - (hasSearchAction ? 1 : 0);
                const action = filteredActions[actionIndex];
                if (action) activate(action.href);
            }
        }
    }

    function focusableElements(): HTMLElement[] {
        if (!paletteRef) return [];
        return Array.from(
            paletteRef.querySelectorAll<HTMLElement>(
                "button:not([disabled]):not([tabindex='-1']), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex='-1'])",
            ),
        );
    }

    function handleModalKeydown(event: KeyboardEvent) {
        if (event.key === "Escape") {
            event.preventDefault();
            event.stopImmediatePropagation();
            onClose();
            return;
        }

        if (event.key !== "Tab") return;

        const focusable = focusableElements();
        if (focusable.length === 0) {
            event.preventDefault();
            paletteRef?.focus();
            return;
        }

        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        const current = document.activeElement;
        if (
            !paletteRef?.contains(current) ||
            (!event.shiftKey && current === last) ||
            (event.shiftKey && current === first)
        ) {
            event.preventDefault();
            (event.shiftKey ? last : first).focus();
        }
    }

    onMount(() => {
        const inertStates = new Map<HTMLElement, boolean>();
        const parent = backdropRef?.parentElement;
        if (parent && backdropRef) {
            for (const sibling of Array.from(parent.children)) {
                if (
                    sibling === backdropRef ||
                    !(sibling instanceof HTMLElement)
                )
                    continue;
                inertStates.set(sibling, sibling.inert);
                sibling.inert = true;
            }
        }

        window.addEventListener("keydown", handleModalKeydown, true);
        queueMicrotask(() => inputRef?.focus());

        return () => {
            window.removeEventListener("keydown", handleModalKeydown, true);
            for (const [element, wasInert] of inertStates) {
                element.inert = wasInert;
            }
            queueMicrotask(() => {
                if (previouslyFocused?.isConnected) previouslyFocused.focus();
            });
        };
    });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
    class="palette-backdrop"
    role="presentation"
    bind:this={backdropRef}
    onclick={onClose}
>
    <div
        class="palette"
        bind:this={paletteRef}
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        tabindex="-1"
        onclick={(event) => event.stopPropagation()}
    >
        <div class="palette-search">
            <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                aria-hidden="true"
                ><circle cx="11" cy="11" r="7" /><path d="m20 20-4-4" /></svg
            >
            <input
                bind:this={inputRef}
                bind:value={query}
                placeholder="Search or jump to…"
                aria-label="Search commands"
                role="combobox"
                aria-autocomplete="list"
                aria-controls="palette-options"
                aria-expanded="true"
                aria-activedescendant={activeOptionId}
                onkeydown={handleKeydown}
            />
            <kbd>Esc</kbd>
        </div>
        <div class="palette-list" id="palette-options" role="listbox">
            {#if query.trim()}<button
                    id="palette-option-0"
                    class="palette-item search-action"
                    class:selected={selectedIndex === 0}
                    role="option"
                    aria-selected={selectedIndex === 0}
                    tabindex="-1"
                    onpointermove={() => (selectedIndex = 0)}
                    onclick={() =>
                        activate(
                            `/search?q=${encodeURIComponent(query.trim())}`,
                        )}
                    ><span>Search for “{query.trim()}”</span><span
                        class="palette-hint">Enter</span
                    ></button
                >{/if}
            {#each filteredActions as action, index (action.href)}<button
                    id={`palette-option-${index + (hasSearchAction ? 1 : 0)}`}
                    class="palette-item"
                    class:selected={selectedIndex ===
                        index + (hasSearchAction ? 1 : 0)}
                    role="option"
                    aria-selected={selectedIndex ===
                        index + (hasSearchAction ? 1 : 0)}
                    tabindex="-1"
                    onpointermove={() =>
                        (selectedIndex = index + (hasSearchAction ? 1 : 0))}
                    onclick={() => activate(action.href)}
                    ><span>{action.label}</span><span class="palette-hint"
                        >{action.hint}</span
                    ></button
                >{/each}
            {#if filteredActions.length === 0 && !query.trim()}<div
                    class="palette-empty"
                >
                    Nothing to show.
                </div>{/if}
        </div>
        <div class="palette-footer">
            <span><kbd>↑↓</kbd> Navigate</span><span><kbd>Enter</kbd> Open</span
            ><span><kbd>Esc</kbd> Close</span>
        </div>
    </div>
</div>

<style>
    .palette-backdrop {
        position: fixed;
        inset: 0;
        z-index: 200;
        display: flex;
        justify-content: center;
        align-items: flex-start;
        padding-top: 12vh;
        background: rgba(0, 0, 0, 0.55);
        backdrop-filter: blur(8px);
    }
    .palette {
        width: min(560px, calc(100vw - 2rem));
        overflow: hidden;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-xl);
        background: var(--color-surface);
        box-shadow: var(--shadow-lg);
    }
    .palette-search {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        padding: var(--spacing-md);
        border-bottom: 1px solid var(--color-border);
    }
    .palette-search svg {
        width: 1.25rem;
        color: var(--color-text-muted);
        flex-shrink: 0;
    }
    .palette-search input {
        min-width: 0;
        flex: 1;
        border: 0;
        outline: 0;
        background: transparent;
        font-size: var(--font-size-lg);
    }
    kbd {
        padding: 0.15rem 0.4rem;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-sm);
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
    }
    .palette-list {
        max-height: 52vh;
        overflow-y: auto;
        padding: var(--spacing-sm);
    }
    .palette-item {
        display: flex;
        align-items: center;
        justify-content: space-between;
        width: 100%;
        gap: var(--spacing-md);
        padding: var(--spacing-sm) var(--spacing-md);
        border-radius: var(--radius);
        text-align: left;
        color: var(--color-text);
    }
    .palette-item:hover,
    .palette-item:focus-visible,
    .palette-item.selected {
        outline: 0;
        background: var(--color-surface-elevated);
    }
    .search-action {
        color: var(--color-accent-content);
    }
    .palette-hint,
    .palette-empty {
        color: var(--color-text-muted);
        font-size: var(--font-size-sm);
    }
    .palette-footer {
        display: flex;
        gap: var(--spacing-md);
        padding: var(--spacing-sm) var(--spacing-md);
        border-top: 1px solid var(--color-border);
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
    }
    .palette-footer span {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-xs);
    }
</style>
