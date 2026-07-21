<script lang="ts">
    import { getGenreCollageAlbumIds, getGenres, type Genre } from "$lib/api";
    import { uiPref } from "$lib/stores/uiPrefs";
    import Loading from "$lib/components/Loading.svelte";
    import CoverCollage from "$lib/components/CoverCollage.svelte";
    import Select from "$lib/components/Select.svelte";
    import SortDirButton from "$lib/components/SortDirButton.svelte";
    import { onMount } from "svelte";

    const SORT_OPTIONS = [
        { value: "name", label: "Name" },
        { value: "tracks", label: "Tracks" },
    ];

    let genres = $state<Genre[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);
    let collageIds = $state<Record<string, number[]>>({});
    const collageLoading = new Set<string>();
    const sortBy = uiPref<string>("genres.sortBy", "name");
    const sortAsc = uiPref("genres.sortAsc", true);

    function chooseSort(v: string) {
        // Direction is the user's choice — switching fields never touches it.
        $sortBy = v;
    }

    let sortedGenres = $derived.by<Genre[]>(() => {
        const dir = $sortAsc ? 1 : -1;
        const sortItems = (list: Genre[]) =>
            [...list].sort((a, b) => {
                if ($sortBy === "tracks") {
                    return (
                        dir * (a.track_count - b.track_count) ||
                        a.name.localeCompare(b.name)
                    );
                }
                return dir * a.name.localeCompare(b.name);
            });
        const sorted = sortItems(genres);
        return sorted;
    });

    onMount(async () => {
        loading = true;
        try {
            genres = await getGenres();
        } catch (e) {
            error = String(e);
        } finally {
            loading = false;
        }
    });

    async function loadCollage(genre: string) {
        if (genre in collageIds || collageLoading.has(genre)) return;
        collageLoading.add(genre);
        try {
            const ids = await getGenreCollageAlbumIds(genre);
            collageIds = { ...collageIds, [genre]: ids };
        } catch {
            // The placeholder remains useful if a single collage cannot load.
        } finally {
            collageLoading.delete(genre);
        }
    }

    function lazyCollage(node: HTMLElement, genre: string) {
        if (typeof IntersectionObserver === "undefined") {
            void loadCollage(genre);
            return;
        }
        const observer = new IntersectionObserver(
            (entries) => {
                if (!entries.some((entry) => entry.isIntersecting)) return;
                observer.disconnect();
                void loadCollage(genre);
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
</script>

<div class="genres-page page-enter">
    <div class="header">
        <h1 class="page-title">Genres</h1>
        <div class="controls">
            <div class="sort-field">
                <span class="sort-label">Sort</span>
                <Select
                    options={SORT_OPTIONS}
                    value={$sortBy}
                    onchange={chooseSort}
                    ariaLabel="Sort genres"
                />
                <SortDirButton
                    ascending={$sortAsc}
                    ontoggle={() => ($sortAsc = !$sortAsc)}
                />
            </div>
        </div>
    </div>

    {#if error}
        <div class="error">{error}</div>
    {/if}

    {#if loading}
        <Loading />
    {:else if genres.length === 0}
        <div class="empty-state">
            <div class="empty-icon">
                <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.5"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    <path
                        d="M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82z"
                    />
                    <line x1="7" y1="7" x2="7.01" y2="7" />
                </svg>
            </div>
            <h1 class="empty-title">No genres found</h1>
            <p class="empty-text">
                Add folders from the Folders page to start listening.
            </p>
        </div>
    {:else}
        <ul class="card-grid">
            {#each sortedGenres as genre, index (genre.name)}
                <li
                    class="card-grid-item card-enter"
                    style="animation-delay: {index * 40}ms"
                >
                    <a href={`/genres/${encodeURIComponent(genre.name)}`}>
                        <div class="card-grid-thumb-wrap">
                            <div
                                class="card-grid-thumb placeholder genre-thumb"
                                use:lazyCollage={genre.name}
                            >
                                <svg
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="1.5"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                    aria-hidden="true"
                                >
                                    <path
                                        d="M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82z"
                                    />
                                    <line x1="7" y1="7" x2="7.01" y2="7" />
                                </svg>
                                <CoverCollage
                                    albumIds={collageIds[genre.name] ?? []}
                                />
                            </div>
                        </div>
                        <div class="card-grid-title ellipsis">
                            {genre.name}
                        </div>
                        <div class="card-grid-meta ellipsis">
                            {genre.track_count}
                            {genre.track_count === 1 ? "song" : "songs"}
                        </div>
                    </a>
                </li>
            {/each}
        </ul>
    {/if}
</div>

<style>
    .genres-page {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xl);
    }

    .header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
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

    .error {
        background-color: var(--color-error);
        color: var(--color-text);
        padding: var(--spacing-md);
        border-radius: var(--radius-lg);
        font-size: var(--font-size-sm);
    }

    .genre-thumb {
        position: relative;
        overflow: hidden;
    }

    .genre-thumb > svg {
        width: 40%;
        height: 40%;
    }
</style>
