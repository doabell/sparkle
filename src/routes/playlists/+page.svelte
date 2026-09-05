<script lang="ts">
    import { goto } from "$app/navigation";
    import {
        getPlaylists,
        createPlaylist,
        deletePlaylist,
        pickFolder,
        getPlaylist,
        getPlaylistCollageAlbumIds,
        refreshLiveMixes,
        type Playlist,
    } from "$lib/api";
    import { loadQueue } from "$lib/stores/playback";
    import { uiPref } from "$lib/stores/uiPrefs";
    import Loading from "$lib/components/Loading.svelte";
    import CoverCollage from "$lib/components/CoverCollage.svelte";
    import Select from "$lib/components/Select.svelte";
    import SortDirButton from "$lib/components/SortDirButton.svelte";
    import { addToast } from "$lib/stores/toast";
    import { onMount } from "svelte";

    const SORT_OPTIONS = [
        { value: "name", label: "Name" },
        { value: "tracks", label: "Tracks" },
    ];

    let playlists = $state<Playlist[]>([]);
    let loading = $state(true);
    let dialogOpen = $state(false);
    let saving = $state(false);
    let deleting = $state<number | null>(null);
    let mode = $state<"manual" | "folder">("manual");
    let name = $state("");
    let description = $state("");
    let folderPath = $state<string | null>(null);
    let collageIds = $state<Record<number, number[]>>({});
    const collageLoading = new Set<number>();
    const sortBy = uiPref<string>("playlists.sortBy", "name");
    const sortAsc = uiPref("playlists.sortAsc", true);

    function chooseSort(v: string) {
        // Direction is the user's choice — switching fields never touches it.
        $sortBy = v;
    }

    interface PlaylistGroup {
        key: string;
        items: Playlist[];
    }

    let playlistGroups = $derived.by<PlaylistGroup[]>(() => {
        const dir = $sortAsc ? 1 : -1;
        const sortItems = (list: Playlist[]) =>
            [...list].sort((a, b) => {
                if ($sortBy === "tracks") {
                    return (
                        dir * (a.track_count - b.track_count) ||
                        a.name.localeCompare(b.name)
                    );
                }
                return dir * a.name.localeCompare(b.name);
            });
        const sorted = sortItems(playlists);
        return [
            { key: "My Playlists", items: sorted.filter((p) => !p.live_mix) },
            { key: "Mixes", items: sorted.filter((p) => p.live_mix) },
        ].filter((group) => group.items.length > 0);
    });

    onMount(load);

    async function load() {
        loading = true;
        try {
            playlists = await getPlaylists();
            collageIds = {};
            collageLoading.clear();
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            loading = false;
        }
    }

    async function loadCollage(playlistId: number) {
        if (playlistId in collageIds || collageLoading.has(playlistId)) return;
        collageLoading.add(playlistId);
        try {
            const ids = await getPlaylistCollageAlbumIds(playlistId);
            collageIds = { ...collageIds, [playlistId]: ids };
        } catch {
            // A missing collage should not prevent the playlist from opening.
        } finally {
            collageLoading.delete(playlistId);
        }
    }

    function lazyCollage(node: HTMLElement, playlistId: number) {
        if (typeof IntersectionObserver === "undefined") {
            void loadCollage(playlistId);
            return;
        }
        const observer = new IntersectionObserver(
            (entries) => {
                if (!entries.some((entry) => entry.isIntersecting)) return;
                observer.disconnect();
                void loadCollage(playlistId);
            },
            { rootMargin: "300px" },
        );
        observer.observe(node);
        return {
            destroy() {
                observer.disconnect();
            },
        };
    }

    function resetForm() {
        name = "";
        description = "";
        folderPath = null;
        mode = "manual";
    }

    function openDialog() {
        resetForm();
        dialogOpen = true;
    }

    async function handlePickFolder() {
        const path = await pickFolder();
        if (path) folderPath = path;
    }

    async function handleCreate() {
        if (!name.trim()) return;
        if (mode === "folder" && !folderPath) {
            addToast("Select a folder for this playlist", "error");
            return;
        }
        saving = true;
        try {
            await createPlaylist(
                name.trim(),
                description.trim() || undefined,
                mode === "folder" ? folderPath! : undefined,
            );
            addToast("Playlist created", "success");
            dialogOpen = false;
            await load();
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            saving = false;
        }
    }

    async function handleDelete(id: number, event: MouseEvent) {
        event.preventDefault();
        event.stopPropagation();
        deleting = id;
        try {
            await deletePlaylist(id);
            addToast("Playlist deleted", "success");
            await load();
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            deleting = null;
        }
    }

    async function handlePlay(event: MouseEvent, playlist: Playlist) {
        event.stopPropagation();
        event.preventDefault();
        if (playlist.track_count === 0) return;
        try {
            const detail = await getPlaylist(playlist.id);
            // Card play is an explicit "play this playlist" — in playlist order.
            await loadQueue(
                detail.tracks.map((t) => t.id),
                0,
                false,
                { kind: "playlist", id: String(playlist.id) },
            );
            goto(`/playlists/${playlist.id}`);
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    async function refreshMixes() {
        try {
            await refreshLiveMixes();
            await load();
            addToast("Live mixes refreshed", "success");
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    function folderDisplayName(path: string) {
        return path.split(/[\\/]/).pop() || path;
    }
</script>

<div class="playlists-page page-enter">
    <div class="header">
        <h1 class="page-title">Playlists</h1>
        <div class="controls">
            <button class="btn-pill btn-secondary" onclick={refreshMixes}>
                Refresh mixes
            </button>
            <div class="sort-field">
                <span class="sort-label">Sort</span>
                <Select
                    options={SORT_OPTIONS}
                    value={$sortBy}
                    onchange={chooseSort}
                    ariaLabel="Sort playlists"
                />
                <SortDirButton
                    ascending={$sortAsc}
                    ontoggle={() => ($sortAsc = !$sortAsc)}
                />
            </div>
            <button class="btn-pill btn-primary" onclick={openDialog}
                >New playlist</button
            >
        </div>
    </div>

    {#if loading}
        <Loading />
    {:else if playlists.length === 0}
        <div class="empty-state">
            <div class="empty-icon">
                <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                    <path
                        d="M15 6H3v2h12V6zm0 4H3v2h12v-2zm0 4H3v2h12v-2zm2-10v16l7-8-7-8z"
                    />
                </svg>
            </div>
            <h1 class="empty-title">No playlists yet</h1>
            <p class="empty-text">
                Create a manual playlist or build one from a folder.
            </p>
        </div>
    {:else}
        {#each playlistGroups as group (group.key)}
            <section
                class="playlist-group"
                aria-labelledby={`playlist-group-${group.key}`}
            >
                <h2 id={`playlist-group-${group.key}`} class="group-header">
                    {group.key}
                </h2>
                <ul class="card-grid">
                    {#each group.items as playlist, index (playlist.id)}
                        <li
                            class="card-grid-item split card-enter"
                            style="animation-delay: {index * 40}ms"
                        >
                            <div class="card-grid-thumb-wrap">
                                <a
                                    class="thumb-link"
                                    href={`/playlists/${playlist.id}`}
                                    aria-label="Open {playlist.name}"
                                    tabindex="-1"
                                >
                                    <div
                                        class="card-grid-thumb placeholder playlist-thumb"
                                        use:lazyCollage={playlist.id}
                                    >
                                        <svg
                                            viewBox="0 0 24 24"
                                            fill="currentColor"
                                            aria-hidden="true"
                                        >
                                            <path
                                                d="M15 6H3v2h12V6zm0 4H3v2h12v-2zm0 4H3v2h12v-2zm2-10v16l7-8-7-8z"
                                            />
                                        </svg>
                                        <CoverCollage
                                            albumIds={collageIds[playlist.id] ??
                                                []}
                                        />
                                    </div>
                                </a>
                                <button
                                    class="card-play-button"
                                    type="button"
                                    aria-label={`Play ${playlist.name}`}
                                    onclick={(e: MouseEvent) =>
                                        handlePlay(e, playlist)}
                                    disabled={playlist.track_count === 0}
                                >
                                    <svg
                                        viewBox="0 0 24 24"
                                        fill="currentColor"
                                        aria-hidden="true"
                                    >
                                        <path d="M8 5v14l11-7z" />
                                    </svg>
                                </button>
                            </div>
                            <a
                                class="card-text-link"
                                href={`/playlists/${playlist.id}`}
                            >
                                <div class="card-grid-title ellipsis">
                                    {playlist.name}
                                </div>
                                <div class="card-grid-meta ellipsis">
                                    {playlist.track_count} track{playlist.track_count ===
                                    1
                                        ? ""
                                        : "s"}
                                    {#if playlist.live_mix}
                                        · Live mix
                                    {:else if playlist.folder_path}
                                        · Folder
                                    {:else}
                                        · Manual
                                    {/if}
                                </div>
                            </a>
                            {#if !playlist.live_mix}
                                <button
                                    class="delete-btn"
                                    aria-label={`Delete ${playlist.name}`}
                                    onclick={(e: MouseEvent) =>
                                        handleDelete(playlist.id, e)}
                                    disabled={deleting === playlist.id}
                                >
                                    {#if deleting === playlist.id}
                                        <Loading variant="inline" />
                                    {:else}
                                        <svg
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="2"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                            aria-hidden="true"
                                        >
                                            <path d="M3 6h18" />
                                            <path
                                                d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"
                                            />
                                            <path
                                                d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"
                                            />
                                            <line
                                                x1="10"
                                                x2="10"
                                                y1="11"
                                                y2="17"
                                            />
                                            <line
                                                x1="14"
                                                x2="14"
                                                y1="11"
                                                y2="17"
                                            />
                                        </svg>
                                    {/if}
                                </button>
                            {/if}
                        </li>
                    {/each}
                </ul>
            </section>
        {/each}
    {/if}
</div>

{#if dialogOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
        class="dialog-overlay"
        role="presentation"
        tabindex="-1"
        onclick={() => (dialogOpen = false)}
        onkeydown={(e: KeyboardEvent) => {
            if (e.key === "Escape") dialogOpen = false;
        }}
    >
        <div
            class="dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="playlist-dialog-title"
            tabindex="-1"
            onclick={(e: MouseEvent) => e.stopPropagation()}
        >
            <h2 id="playlist-dialog-title" class="dialog-title">
                New playlist
            </h2>

            <div class="dialog-body">
                <div class="field">
                    <label for="playlist-name">Name</label>
                    <input
                        id="playlist-name"
                        type="text"
                        bind:value={name}
                        placeholder="My playlist"
                    />
                </div>

                <div class="field">
                    <label for="playlist-description">Description</label>
                    <input
                        id="playlist-description"
                        type="text"
                        bind:value={description}
                        placeholder="Optional"
                    />
                </div>

                <div class="field">
                    <span class="label">Type</span>
                    <div class="type-options">
                        <label class="type-option">
                            <input
                                type="radio"
                                bind:group={mode}
                                value="manual"
                            />
                            <span>Manual</span>
                        </label>
                        <label class="type-option">
                            <input
                                type="radio"
                                bind:group={mode}
                                value="folder"
                            />
                            <span>From folder</span>
                        </label>
                    </div>
                </div>

                {#if mode === "folder"}
                    <div class="field">
                        <label for="playlist-folder">Folder</label>
                        <div class="folder-picker">
                            <button
                                class="btn-pill btn-secondary"
                                onclick={handlePickFolder}
                            >
                                {folderPath ? "Change folder" : "Select folder"}
                            </button>
                            {#if folderPath}
                                <span class="folder-path">{folderPath}</span>
                            {/if}
                        </div>
                    </div>
                {/if}
            </div>

            <div class="dialog-actions">
                <button
                    class="btn-pill btn-secondary"
                    onclick={() => (dialogOpen = false)}
                    disabled={saving}>Cancel</button
                >
                <button
                    class="btn-pill btn-primary"
                    onclick={handleCreate}
                    disabled={saving || !name.trim()}
                >
                    {saving ? "Creating..." : "Create"}
                </button>
            </div>
        </div>
    </div>
{/if}

<style>
    .playlists-page {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xl);
    }

    .header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
        flex-wrap: wrap;
    }

    .controls {
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
    }

    .sort-field {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
    }

    .sort-label {
        font-size: var(--font-size-sm);
        color: var(--color-text-secondary);
        font-weight: var(--font-weight-medium);
    }

    .playlist-thumb {
        position: relative;
        overflow: hidden;
    }

    .playlist-thumb svg {
        width: 40%;
        height: 40%;
    }

    .card-grid-item {
        position: relative;
    }

    .card-play-button:disabled {
        opacity: 0;
        pointer-events: none;
    }

    .delete-btn {
        position: absolute;
        top: var(--spacing-sm);
        right: var(--spacing-sm);
        z-index: 2;
        display: flex;
        align-items: center;
        justify-content: center;
        width: 1.75rem;
        height: 1.75rem;
        padding: 0.25rem;
        border-radius: var(--radius-full);
        background-color: var(--color-surface-raised);
        border: 1px solid var(--color-border);
        color: var(--color-text-muted);
        opacity: 0;
        transition:
            opacity var(--transition-fast),
            background-color var(--transition-fast),
            color var(--transition-fast);
    }

    .card-grid-item:hover .delete-btn,
    .card-grid-item:focus-within .delete-btn {
        opacity: 1;
    }

    .delete-btn:hover:not(:disabled) {
        background-color: var(--color-error);
        color: var(--color-text);
    }

    .delete-btn:disabled {
        opacity: 1;
    }

    .delete-btn svg {
        width: 0.875rem;
        height: 0.875rem;
    }

    .dialog-overlay {
        position: fixed;
        inset: 0;
        z-index: 100;
        display: flex;
        align-items: center;
        justify-content: center;
        background-color: rgba(0, 0, 0, 0.6);
        padding: var(--spacing-md);
    }

    .dialog {
        width: 100%;
        max-width: 420px;
        background-color: var(--color-surface);
        border-radius: var(--radius-lg);
        padding: var(--spacing-xl);
        display: flex;
        flex-direction: column;
        gap: var(--spacing-lg);
        box-shadow: 0 16px 40px rgba(0, 0, 0, 0.4);
    }

    .dialog-title {
        font-size: var(--font-size-xl);
        font-weight: var(--font-weight-bold);
    }

    .dialog-body {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
    }

    .field {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .field label,
    .field .label {
        font-weight: var(--font-weight-semibold);
        font-size: var(--font-size-sm);
        color: var(--color-text);
    }

    .field input[type="text"] {
        padding: var(--spacing-sm) var(--spacing-md);
        border-radius: var(--radius);
        border: 1px solid var(--color-border);
        background: var(--color-surface-elevated);
        color: var(--color-text);
    }

    .type-options {
        display: flex;
        gap: var(--spacing-md);
    }

    .type-option {
        display: flex;
        align-items: center;
        gap: var(--spacing-xs);
        cursor: pointer;
        font-weight: var(--font-weight-normal);
    }

    .folder-picker {
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
        flex-wrap: wrap;
    }

    .folder-path {
        font-size: var(--font-size-sm);
        color: var(--color-text-muted);
        word-break: break-all;
    }

    .dialog-actions {
        display: flex;
        justify-content: flex-end;
        gap: var(--spacing-md);
    }
</style>
