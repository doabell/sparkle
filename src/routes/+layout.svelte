<script lang="ts">
    import "../app.css";
    import { page } from "$app/stores";
    import { afterNavigate, beforeNavigate } from "$app/navigation";
    import Sidebar from "$lib/components/Sidebar.svelte";
    import PlayerBar from "$lib/components/PlayerBar.svelte";
    import Toaster from "$lib/components/Toaster.svelte";
    import MediaSession from "$lib/components/MediaSession.svelte";
    import CommandPalette from "$lib/components/CommandPalette.svelte";
    import { onMount } from "svelte";
    import { listen } from "@tauri-apps/api/event";
    import { getCurrentWindow } from "@tauri-apps/api/window";
    import { getOnlineSettings } from "$lib/api";
    import { getFontStack } from "$lib/utils/fonts";
    import { applyAccent } from "$lib/utils/theme";
    import { createContentScrollRestorer } from "$lib/utils/scrollRestore";
    import { windowPageTitle } from "$lib/stores/windowPageTitle";
    import { getRouteTitle, getWindowTitle } from "$lib/utils/windowTitle";
    import {
        readScrollbackFromHistory,
        saveScrollbackToHistory,
        scrollbackRegistry,
        type ScrollbackSnapshot,
    } from "$lib/utils/scrollback";
    import {
        playback,
        play,
        pause,
        nextTrack,
        previousTrack,
        setVolume,
        seek,
    } from "$lib/stores/playback";

    const SEEK_STEP_MS = 5000;
    const VOLUME_STEP = 0.05;

    // Back only makes sense on detail pages drilled into from a list — never
    // on Home or top-level pages, where it would crowd the page title.
    const BACK_ROUTES = [
        "/artists/",
        "/albums/",
        "/genres/",
        "/playlists/",
        "/now-playing",
    ];
    // Hero pages reach the very top of the content; the floating back button
    // overlays their blurred backdrop, so they need no clearance. Other detail
    // pages get extra top padding so the button never touches the title.
    const HERO_ROUTES = ["/artists/", "/albums/", "/now-playing"];
    const SCROLLBACK_ROUTES = new Set(["/albums", "/artists"]);

    let canGoBack = $state(false);
    let backClearance = $state(false);
    let contentElement = $state<HTMLElement | null>(null);
    let paletteOpen = $state(false);
    const scrollRestorer = createContentScrollRestorer(
        () => contentElement,
        {
            requestFrame: (callback) => requestAnimationFrame(callback),
            cancelFrame: (frame) => cancelAnimationFrame(frame),
        },
        (callback) => {
            if (!contentElement) return () => {};
            const observer = new MutationObserver(callback);
            observer.observe(contentElement, {
                childList: true,
                subtree: true,
                attributes: true,
                attributeFilter: ["class", "src", "style"],
            });
            return () => observer.disconnect();
        },
    );

    function stopScrollRestore() {
        scrollRestorer.stop();
    }

    function captureContentScroll() {
        const top = contentElement?.scrollTop ?? 0;
        const maxScrollTop = Math.max(
            0,
            (contentElement?.scrollHeight ?? 0) -
                (contentElement?.clientHeight ?? 0),
        );
        return { top, maxScrollTop };
    }

    function routeKey(url: URL): string {
        return `${url.pathname}${url.search}${url.hash}`;
    }

    function supportsScrollback(route: string): boolean {
        return SCROLLBACK_ROUTES.has(route.split(/[?#]/, 1)[0]);
    }

    function restoreContentScroll(
        snapshot:
            | ScrollbackSnapshot
            | { top: number; maxScrollTop: number }
            | number
            | null,
        route = routeKey($page.url),
    ) {
        if (!snapshot || !supportsScrollback(route)) return;

        const restored = scrollbackRegistry.restore(route, snapshot);
        if (restored.scroll !== null) {
            scrollRestorer.restore(restored.scroll);
        }
    }

    function captureScrollback(route: string) {
        return scrollbackRegistry.capture(route, captureContentScroll());
    }

    function saveCurrentScrollback(route = routeKey($page.url)) {
        if (!supportsScrollback(route)) return;

        saveScrollbackToHistory(
            window.history,
            captureScrollback(route),
            window.location.href,
        );
    }

    // SvelteKit snapshots are tied to individual history entries, which keeps
    // separate list positions when navigating through several detail pages.
    export const snapshot = {
        capture: () => {
            const route = routeKey($page.url);
            return supportsScrollback(route) ? captureScrollback(route) : null;
        },
        restore: restoreContentScroll,
    };

    beforeNavigate(({ from }) => {
        if (!from) return;
        saveCurrentScrollback(routeKey(from.url));
    });

    afterNavigate(({ type, to }) => {
        if (type === "popstate" && to && supportsScrollback(routeKey(to.url))) {
            const saved = readScrollbackFromHistory(
                window.history,
                routeKey(to.url),
            );
            if (saved) restoreContentScroll(saved, routeKey(to.url));
        }

        if (type !== "popstate") {
            stopScrollRestore();
            contentElement?.scrollTo({ top: 0, left: 0, behavior: "auto" });
        }
    });

    $effect(() => {
        const path = $page.url.pathname;
        canGoBack =
            window.history.length > 1 &&
            BACK_ROUTES.some((r) => path.startsWith(r));
        backClearance =
            canGoBack && !HERO_ROUTES.some((r) => path.startsWith(r));
    });

    $effect(() => {
        windowPageTitle.set(getRouteTitle($page.url.pathname));
    });

    // The current route provides the stable context. While audio is actively
    // playing, the track takes priority so the window remains identifiable
    // even when navigation happens in the background. Paused/stopped playback
    // returns the title to the page, while the next track updates reactively.
    $effect(() => {
        if (typeof document === "undefined") return;
        const title = getWindowTitle(
            $page.url.pathname,
            $playback.current_track,
            $playback.is_playing,
            $windowPageTitle,
        );
        document.title = title;
        try {
            getCurrentWindow()
                .setTitle(title)
                .catch(() => {
                    // The web preview has no native Tauri window; document.title
                    // above remains the complete fallback for that environment.
                });
        } catch {
            // getCurrentWindow can throw before returning a promise outside
            // Tauri, where document.title is the available title surface.
        }
    });

    function goBack() {
        window.history.back();
    }

    let { children } = $props();

    function isInteractiveElement(target: EventTarget | null): boolean {
        if (!(target instanceof Element)) return false;
        return Boolean(
            target.closest(
                "input, textarea, select, button, a[href], label, [contenteditable='true'], [role='button'], [role='combobox'], [role='link'], [role='listbox'], [role='menuitem'], [role='option'], [role='slider'], [role='textbox']",
            ),
        );
    }

    function togglePlayPause() {
        $playback.is_playing ? pause() : play();
    }

    async function loadUiFont() {
        try {
            const settings = await getOnlineSettings();
            document.documentElement.style.setProperty(
                "--font-family",
                getFontStack(settings.ui_font || "System"),
            );
            document.documentElement.dataset.motion = settings.reduce_motion
                ? "reduced"
                : "full";
            applyAccent(settings.accent_color || "#fa243c");
        } catch (err) {
            console.error("Failed to load UI font setting:", err);
        }
    }

    function handleKeydown(event: KeyboardEvent) {
        if (
            (event.ctrlKey || event.metaKey) &&
            event.key.toLowerCase() === "k"
        ) {
            event.preventDefault();
            paletteOpen = !paletteOpen;
            return;
        }
        if (event.defaultPrevented || isInteractiveElement(event.target))
            return;

        const key = event.key;

        switch (key) {
            case " ":
                event.preventDefault();
                togglePlayPause();
                break;
            case "ArrowLeft":
                event.preventDefault();
                if (event.ctrlKey) {
                    previousTrack();
                } else {
                    seek(Math.max(0, $playback.position_ms - SEEK_STEP_MS));
                }
                break;
            case "ArrowRight":
                event.preventDefault();
                if (event.ctrlKey) {
                    nextTrack();
                } else {
                    seek(
                        Math.min(
                            $playback.duration_ms,
                            $playback.position_ms + SEEK_STEP_MS,
                        ),
                    );
                }
                break;
            case "ArrowUp":
                event.preventDefault();
                setVolume(Math.min(1, $playback.volume + VOLUME_STEP));
                break;
            case "ArrowDown":
                event.preventDefault();
                setVolume(Math.max(0, $playback.volume - VOLUME_STEP));
                break;
            case "n":
            case "N":
                event.preventDefault();
                nextTrack();
                break;
            case "p":
            case "P":
                event.preventDefault();
                previousTrack();
                break;
        }
    }

    onMount(() => {
        loadUiFont();

        const unlisteners: (() => void)[] = [];

        async function initMediaKeyListeners() {
            try {
                unlisteners.push(
                    await listen("media-key-play-pause", () => {
                        togglePlayPause();
                    }),
                );
            } catch (err) {
                console.error(
                    "Failed to listen for media key play/pause:",
                    err,
                );
            }

            try {
                unlisteners.push(await listen("media-key-next", nextTrack));
            } catch (err) {
                console.error("Failed to listen for media key next:", err);
            }

            try {
                unlisteners.push(
                    await listen("media-key-previous", previousTrack),
                );
            } catch (err) {
                console.error("Failed to listen for media key previous:", err);
            }
        }

        initMediaKeyListeners();

        return () => {
            unlisteners.forEach((unlisten) => unlisten());
        };
    });
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="app">
    <Sidebar />
    <main
        bind:this={contentElement}
        class="content"
        class:back-clearance={backClearance}
    >
        {#if canGoBack}
            <button
                class="back-fab"
                onclick={goBack}
                aria-label="Go back"
                title="Back"
            >
                <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2.75"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    <path d="m15 18-6-6 6-6" />
                </svg>
            </button>
        {/if}
        {@render children()}
    </main>
    <div class="player-wrapper">
        <PlayerBar />
    </div>
    <Toaster />
    <MediaSession />
    {#if paletteOpen}
        <CommandPalette onClose={() => (paletteOpen = false)} />
    {/if}
</div>

<style>
    .app {
        display: grid;
        grid-template-columns: var(--sidebar-width) 1fr;
        grid-template-areas: "sidebar content";
        height: 100vh;
        width: 100vw;
        color: var(--color-text);
        background-color: var(--color-background);
    }

    .content {
        grid-area: content;
        overflow-y: auto;
        padding: var(--spacing-xl) var(--spacing-2xl)
            calc(var(--spacing-2xl) + var(--player-height));
    }

    /* Detail pages without a hero reserve room for the floating back button. */
    .content.back-clearance {
        padding-top: calc(var(--spacing-xl) + 2.75rem);
    }

    .player-wrapper {
        position: fixed;
        left: var(--sidebar-width);
        right: 0;
        bottom: 0;
        z-index: 50;
    }

    .player-wrapper :global(.player-bar) {
        position: relative;
        width: 100%;
    }

    .back-fab {
        position: fixed;
        top: var(--spacing-md);
        left: calc(var(--sidebar-width) + var(--spacing-md));
        z-index: 40;
        display: flex;
        align-items: center;
        justify-content: center;
        width: 2.25rem;
        height: 2.25rem;
        border-radius: var(--radius-full);
        /* Same language as every secondary button: subtle fill + border, text
       glyph; backdrop blur only so it stays legible over hero artwork. */
        background: rgba(var(--color-surface-rgb), 0.65);
        backdrop-filter: blur(20px) saturate(1.8);
        -webkit-backdrop-filter: blur(20px) saturate(1.8);
        border: 1px solid var(--color-border);
        color: var(--color-text-secondary);
        box-shadow: var(--shadow-sm);
        transition:
            color var(--transition-fast),
            transform var(--transition-fast),
            background-color var(--transition-fast),
            border-color var(--transition-fast);
    }

    .back-fab:hover {
        background-color: rgba(255, 255, 255, 0.12);
        border-color: rgba(255, 255, 255, 0.18);
        color: var(--color-text);
        transform: scale(1.04);
    }

    .back-fab svg {
        width: 1.25rem;
        height: 1.25rem;
        /* Optical balance: a lone chevron reads better nudged left of center. */
        margin-right: 2px;
    }

    @media (max-width: 767px) {
        .app {
            grid-template-columns: 1fr;
            grid-template-areas: "content";
        }

        .player-wrapper {
            left: 0;
        }

        .back-fab {
            left: var(--spacing-md);
        }

        .content {
            padding: var(--spacing-2xl) var(--spacing-md)
                calc(var(--spacing-md) + var(--player-height));
        }
    }
</style>
