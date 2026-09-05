<script lang="ts">
    import { page } from "$app/stores";

    const items = [
        {
            label: "Home",
            href: "/",
            icon: `<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"/>
        <path d="M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>
      </svg>`,
        },
        {
            label: "Search",
            href: "/search",
            icon: `<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="11" cy="11" r="8"/>
        <path d="m21 21-4.3-4.3"/>
      </svg>`,
        },
        {
            label: "Artists",
            href: "/artists",
            icon: `<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/>
        <circle cx="12" cy="7" r="4"/>
      </svg>`,
        },
        {
            label: "Albums",
            href: "/albums",
            icon: `<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="10"/>
        <circle cx="12" cy="12" r="3"/>
      </svg>`,
        },
        {
            label: "Songs",
            href: "/songs",
            icon: `<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M9 18V5l12-2v13"/>
        <circle cx="6" cy="18" r="3"/>
        <circle cx="18" cy="16" r="3"/>
      </svg>`,
        },
        {
            label: "Genres",
            href: "/genres",
            icon: `<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82z"/>
        <line x1="7" y1="7" x2="7.01" y2="7"/>
      </svg>`,
        },
        {
            label: "Playlists",
            href: "/playlists",
            icon: `<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <line x1="8" y1="6" x2="21" y2="6"/>
        <line x1="8" y1="12" x2="21" y2="12"/>
        <line x1="8" y1="18" x2="21" y2="18"/>
        <line x1="3" y1="6" x2="3.01" y2="6"/>
        <line x1="3" y1="12" x2="3.01" y2="12"/>
        <line x1="3" y1="18" x2="3.01" y2="18"/>
      </svg>`,
        },
        {
            label: "Stats",
            href: "/stats",
            icon: `<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M3 3v18h18"/>
        <path d="m7 15 4-6 4 3 5-8"/>
      </svg>`,
        },
        {
            label: "Library Health",
            href: "/health",
            icon: `<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M3 12h4l2-7 4 14 2-7h6"/>
      </svg>`,
        },
        {
            label: "Folders",
            href: "/folders",
            icon: `<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
      </svg>`,
        },
        {
            label: "Settings",
            href: "/settings",
            icon: `<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="3"/>
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51H15a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
      </svg>`,
        },
    ];

    let open = $state(false);
    let pathname = $derived($page.url.pathname);

    function isActive(href: string) {
        return href === "/" ? pathname === "/" : pathname.startsWith(href);
    }

    function close() {
        open = false;
    }
</script>

<button
    class="menu-toggle"
    type="button"
    aria-label="Toggle navigation"
    aria-expanded={open}
    onclick={() => (open = !open)}
>
    <svg
        xmlns="http://www.w3.org/2000/svg"
        width="20"
        height="20"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
    >
        <line x1="3" y1="6" x2="21" y2="6" />
        <line x1="3" y1="12" x2="21" y2="12" />
        <line x1="3" y1="18" x2="21" y2="18" />
    </svg>
</button>

<nav class="sidebar" class:open aria-label="Main navigation">
    <div class="logo">
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <path
                d="M12 3l2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5L12 3z"
            />
        </svg>
        Sparkle
    </div>
    <ul class="nav-items">
        {#each items as item}
            {@const active = isActive(item.href)}
            <li class:active>
                <a
                    href={item.href}
                    onclick={close}
                    aria-current={active ? "page" : undefined}
                >
                    <span class="icon" class:active>{@html item.icon}</span>
                    <span class="label">{item.label}</span>
                </a>
            </li>
        {/each}
    </ul>
</nav>

{#if open}
    <button
        class="scrim"
        type="button"
        aria-label="Close navigation"
        onclick={close}
    ></button>
{/if}

<style>
    .sidebar {
        grid-area: sidebar;
        position: relative;
        z-index: 20;
        width: var(--sidebar-width);
        height: 100vh;
        background: linear-gradient(
            180deg,
            var(--color-surface) 0%,
            rgba(var(--color-surface-rgb), 0.98) 100%
        );
        padding: 0 var(--spacing-md) var(--spacing-lg);
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
        overflow-y: auto;
        border-right: 1px solid var(--color-border);
    }

    .logo {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 0.625rem;
        min-height: 4rem;
        flex-shrink: 0;
        font-size: var(--font-size-2xl);
        font-weight: var(--font-weight-bold);
        line-height: var(--line-height-tight);
        /* Optical centering: the wordmark needs a little more room above/right. */
        padding: 0.75rem 0.75rem 0 0;
        color: var(--color-text);
        letter-spacing: -0.02em;
        user-select: none;
    }

    .logo svg {
        width: 1.75rem;
        height: 1.75rem;
        flex-shrink: 0;
        color: var(--color-accent-graphic);
    }

    .nav-items {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    li {
        position: relative;
        border-radius: var(--radius);
    }

    li.active a {
        background-color: var(--color-surface-elevated);
        color: var(--color-text);
        font-weight: var(--font-weight-semibold);
    }

    li.active .icon {
        color: var(--color-accent-graphic);
    }

    li.active::before {
        content: "";
        position: absolute;
        left: 0;
        top: 50%;
        transform: translateY(-50%);
        width: 3px;
        height: 1.25rem;
        background-color: var(--color-accent-graphic);
        border-radius: var(--radius-full);
    }

    a {
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
        padding: var(--spacing-sm) var(--spacing-md);
        border-radius: var(--radius);
        color: var(--color-text-secondary);
        transition:
            background-color var(--transition-fast),
            color var(--transition-fast);
    }

    a:hover {
        background-color: var(--color-surface-elevated);
        color: var(--color-text);
    }

    .icon {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 1.25rem;
        height: 1.25rem;
        flex-shrink: 0;
    }

    .icon :global(svg) {
        width: 100%;
        height: 100%;
    }

    .label {
        font-size: var(--font-size-sm);
        font-weight: var(--font-weight-medium);
    }

    .menu-toggle,
    .scrim {
        display: none;
    }

    @media (max-width: 767px) {
        .sidebar {
            position: fixed;
            left: 0;
            top: 0;
            z-index: 100;
            width: var(--sidebar-mobile-width);
            transform: translateX(-100%);
            transition: transform var(--transition-base);
            box-shadow: var(--shadow-md);
            background: var(--color-surface);
        }

        .sidebar.open {
            transform: translateX(0);
        }

        .menu-toggle {
            display: flex;
            align-items: center;
            justify-content: center;
            position: fixed;
            top: 0;
            left: var(--spacing-md);
            z-index: 200;
            width: 2.5rem;
            height: var(--window-chrome-height);
            border-radius: var(--radius);
            color: var(--color-text);
            background-color: transparent;
            border: none;
            transition:
                background-color var(--transition-fast),
                color var(--transition-fast);
        }

        .menu-toggle:hover {
            color: var(--color-accent-graphic);
            background-color: var(--color-surface-raised);
        }

        .scrim {
            display: block;
            position: fixed;
            inset: 0;
            z-index: 99;
            background-color: var(--color-overlay);
            border: none;
        }
    }
</style>
