<script lang="ts">
    import "../app.css";
    import { browser } from "$app/environment";
    import { page } from "$app/stores";
    import { afterNavigate, beforeNavigate } from "$app/navigation";
    import Sidebar from "$lib/components/Sidebar.svelte";
    import PlayerBar from "$lib/components/PlayerBar.svelte";
    import Toaster from "$lib/components/Toaster.svelte";
    import MediaSession from "$lib/components/MediaSession.svelte";
    import CommandPalette from "$lib/components/CommandPalette.svelte";
    import WindowControls from "$lib/components/WindowControls.svelte";
    import PageScrollbar from "$lib/components/PageScrollbar.svelte";
    import { onMount } from "svelte";
    import { listen } from "@tauri-apps/api/event";
    import { getCurrentWindow } from "@tauri-apps/api/window";
    import { getOnlineSettings } from "$lib/api";
    import { getFontStack } from "$lib/utils/fonts";
    import {
        applyCachedThemeMode,
        applyThemeMode,
        cacheThemeMode,
    } from "$lib/utils/themeMode";
    import {
        DEFAULT_ACCENT_COLOR,
        applyAccent,
        applyCachedAccent,
        cacheAccent,
        normalizeAccentForegroundPreference,
    } from "$lib/utils/theme";
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
    const overlayScrollbarSupported =
        browser && CSS.supports("selector(::-webkit-scrollbar)");
    if (browser) {
        applyCachedAccent();
        applyCachedThemeMode();
    }
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
    // Preserve the originating grid position when returning from a detail page.
    const SCROLLBACK_ROUTES = new Set(["/albums", "/artists"]);

    let canGoBack = $state(false);
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
                "input, textarea, select, button, a[href], label, [contenteditable='true'], [role='button'], [role='combobox'], [role='link'], [role='listbox'], [role='menuitem'], [role='option'], [role='slider'], [role='scrollbar'], [role='textbox']",
            ),
        );
    }

    function togglePlayPause(source: "keyboard" | "system_media") {
        $playback.is_playing ? pause(source) : play(source);
    }

    async function loadUiSettings() {
        try {
            const settings = await getOnlineSettings();
            applyThemeMode(settings.theme_mode);
            cacheThemeMode(settings.theme_mode);
            document.documentElement.style.setProperty(
                "--font-family",
                getFontStack(settings.ui_font || "System"),
            );
            document.documentElement.dataset.motion = settings.reduce_motion
                ? "reduced"
                : "full";
            const accentColor = settings.accent_color || DEFAULT_ACCENT_COLOR;
            const accentPreference = normalizeAccentForegroundPreference(
                settings.accent_foreground_preference,
            );
            applyAccent(accentColor, accentPreference);
            cacheAccent(accentColor, accentPreference);
        } catch (err) {
            console.error("Failed to load UI settings:", err);
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
                togglePlayPause("keyboard");
                break;
            case "ArrowLeft":
                event.preventDefault();
                if (event.ctrlKey) {
                    previousTrack("keyboard");
                } else {
                    seek(
                        Math.max(0, $playback.position_ms - SEEK_STEP_MS),
                        "keyboard",
                    );
                }
                break;
            case "ArrowRight":
                event.preventDefault();
                if (event.ctrlKey) {
                    nextTrack("keyboard");
                } else {
                    seek(
                        Math.min(
                            $playback.duration_ms,
                            $playback.position_ms + SEEK_STEP_MS,
                        ),
                        "keyboard",
                    );
                }
                break;
            case "ArrowUp":
                event.preventDefault();
                setVolume(
                    Math.min(1, $playback.volume + VOLUME_STEP),
                    "keyboard",
                );
                break;
            case "ArrowDown":
                event.preventDefault();
                setVolume(
                    Math.max(0, $playback.volume - VOLUME_STEP),
                    "keyboard",
                );
                break;
        }
    }

    onMount(() => {
        loadUiSettings();

        const unlisteners: (() => void)[] = [];

        async function initMediaKeyListeners() {
            try {
                unlisteners.push(
                    await listen("media-key-play-pause", () => {
                        togglePlayPause("system_media");
                    }),
                );
            } catch (err) {
                console.error(
                    "Failed to listen for media key play/pause:",
                    err,
                );
            }

            try {
                unlisteners.push(
                    await listen("media-key-next", () =>
                        nextTrack("system_media"),
                    ),
                );
            } catch (err) {
                console.error("Failed to listen for media key next:", err);
            }

            try {
                unlisteners.push(
                    await listen("media-key-previous", () =>
                        previousTrack("system_media"),
                    ),
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

<WindowControls />
<div class="app">
    <Sidebar />
    <main
        id="page-content"
        bind:this={contentElement}
        class="content"
        class:now-playing-content={$page.url.pathname === "/now-playing"}
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
                    stroke-width="2"
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
    <PageScrollbar
        target={contentElement}
        enabled={overlayScrollbarSupported &&
            $page.url.pathname !== "/now-playing"}
    />
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
        grid-template-rows: minmax(0, 1fr) auto;
        grid-template-areas:
            "sidebar content"
            "sidebar player";
        height: 100vh;
        width: 100vw;
        color: var(--color-text);
        background-color: var(--color-background);
    }

    .content {
        --content-padding-top: calc(
            var(--window-chrome-height) + var(--spacing-sm)
        );
        --content-padding-inline: var(--spacing-2xl);
        grid-area: content;
        min-width: 0;
        min-height: 0;
        overflow-y: auto;
        padding: var(--content-padding-top) var(--content-padding-inline)
            var(--spacing-2xl);
    }

    /* WebView2 lets us replace only the vertical gutter. Native horizontal
       scrolling remains visible; unsupported browsers keep their native bars. */
    .content::-webkit-scrollbar:vertical {
        display: none;
    }

    .content.now-playing-content {
        display: flex;
        flex-direction: column;
        overflow: hidden;
        padding-bottom: var(--spacing-md);
    }

    .player-wrapper {
        grid-area: player;
        position: relative;
        min-width: 0;
        z-index: 50;
    }

    .player-wrapper :global(.player-bar) {
        position: relative;
        width: 100%;
    }

    .back-fab {
        position: fixed;
        top: var(--spacing-sm);
        left: calc(var(--sidebar-width) + var(--spacing-md));
        z-index: 200;
        display: flex;
        align-items: center;
        justify-content: center;
        width: 2.5rem;
        height: var(--window-chrome-height);
        border-radius: var(--radius);
        background: transparent;
        border: none;
        color: var(--color-text-secondary);
        transition:
            color var(--transition-fast),
            background-color var(--transition-fast);
    }

    .back-fab:hover {
        background-color: var(--interactive-hover);
        color: var(--color-text);
    }

    .back-fab:hover svg {
        transform: scale(var(--motion-hover-scale));
    }

    .back-fab:active svg {
        transform: scale(var(--motion-press-scale));
    }

    .back-fab:focus-visible {
        outline-offset: -3px;
    }

    .back-fab svg {
        width: 1.25rem;
        height: 1.25rem;
        /* Optical balance: a lone chevron reads better nudged left of center. */
        margin-right: 2px;
        transition: transform var(--transition-fast);
    }

    @media (max-width: 767px) {
        .app {
            grid-template-columns: 1fr;
            grid-template-areas:
                "content"
                "player";
        }

        .back-fab {
            left: calc(var(--spacing-md) + 2.5rem);
        }

        .content {
            --content-padding-inline: var(--spacing-md);
            padding-bottom: var(--spacing-md);
        }
    }
</style>
