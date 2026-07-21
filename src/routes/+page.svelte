<script lang="ts">
    import {
        getArtists,
        getAlbums,
        getAlbumArt,
        getListeningStats,
        type Artist,
        type Album,
    } from "$lib/api";
    import { playback } from "$lib/stores/playback";
    import { cachedImageToUrl } from "$lib/utils/base64";
    import { plural } from "$lib/utils/text";
    import { onMount } from "svelte";
    import Loading from "$lib/components/Loading.svelte";
    import Artwork from "$lib/components/Artwork.svelte";
    import ArtistAvatar from "$lib/components/ArtistAvatar.svelte";
    import { goto } from "$app/navigation";

    let artists = $state<Artist[]>([]);
    let albums = $state<Album[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);
    let heroArtUrl = $state("");

    onMount(async () => {
        try {
            const [artistsData, albumsData, listeningStats] = await Promise.all(
                [
                    getArtists(),
                    getAlbums(),
                    getListeningStats().catch(() => null),
                ],
            );
            const artistsById = new Map(
                artistsData.map((artist) => [artist.id, artist]),
            );
            const frequentArtists = (listeningStats?.top_artists ?? [])
                .map((artist) => artistsById.get(artist.artist_id))
                .filter((artist): artist is Artist => Boolean(artist))
                .slice(0, 6);
            // Keep the existing library order until listening history has
            // produced a usable ranking.
            artists = (
                frequentArtists.length > 0 ? frequentArtists : artistsData
            ).slice(0, 6);
            // Recent albums = newest first.
            albums = albumsData
                .sort(
                    (a, b) =>
                        (b.year ?? 0) - (a.year ?? 0) ||
                        a.title.localeCompare(b.title),
                )
                .slice(0, 6);
            if (albums[0]) {
                try {
                    const art = await getAlbumArt(albums[0].id);
                    heroArtUrl = cachedImageToUrl(art, "");
                } catch {
                    heroArtUrl = "";
                }
            }
        } catch (e) {
            error = String(e);
        } finally {
            loading = false;
        }
    });

    const hero = $derived.by(() => {
        if ($playback.current_track) {
            return {
                label: "Now Playing",
                href: "/now-playing" as const,
                action: "Open Now Playing" as const,
            };
        }
        if (albums.length > 0) {
            return {
                label: "Welcome Back",
                href: `/albums/${albums[0].id}` as const,
                action: "Go to Album" as const,
            };
        }
        return {
            label: "Sparkle",
            href: "/folders" as const,
            action: "Add Folders" as const,
        };
    });

    const heroTitle = $derived.by(() => {
        if ($playback.current_track)
            return $playback.current_track.title ?? "Unknown";
        if (albums.length > 0) return albums[0].title;
        return "Your music library";
    });

    const heroMeta = $derived.by(() => {
        if ($playback.current_track) {
            let meta = $playback.current_track.artist_names?.join(", ") ?? "";
            if ($playback.current_track.album_title)
                meta += ` — ${$playback.current_track.album_title}`;
            return meta;
        }
        if (albums.length > 0) {
            let meta = albums[0].artist_names?.join(", ") ?? "";
            if (albums[0].year) meta += ` · ${albums[0].year}`;
            return meta;
        }
        return "Add folders from the Folders page to start listening.";
    });

    let lastHeroAlbumId = $state<number | null>(null);
    let heroNowPlayingArtUrl = $state("");

    async function updateHeroArt(albumId: number | null | undefined) {
        if (albumId === lastHeroAlbumId) return;
        lastHeroAlbumId = albumId ?? null;
        if (!albumId) {
            heroNowPlayingArtUrl = "";
            return;
        }
        const requestedAlbumId = albumId;
        try {
            const art = await getAlbumArt(requestedAlbumId);
            if (requestedAlbumId !== lastHeroAlbumId) return;
            heroNowPlayingArtUrl = cachedImageToUrl(art, "");
        } catch {
            if (requestedAlbumId !== lastHeroAlbumId) return;
            heroNowPlayingArtUrl = "";
        }
    }

    $effect(() => {
        updateHeroArt($playback.current_track?.album_id);
    });

    const heroImageUrl = $derived.by(() => {
        if ($playback.current_track?.album_id) {
            return heroNowPlayingArtUrl;
        }
        return heroArtUrl;
    });
</script>

<div class="home page-enter">
    {#if error}
        <div class="error">{error}</div>
    {/if}

    {#if loading}
        <Loading />
    {:else}
        <section
            class="hero-section compact"
            class:playing={$playback.current_track}
        >
            <button
                class="hero-art"
                onclick={() => goto(hero.href)}
                aria-label={hero.action}
            >
                {#if heroImageUrl}
                    <img
                        src={heroImageUrl}
                        alt={$playback.current_track?.album_title ??
                            albums[0]?.title ??
                            "Album art"}
                    />
                {:else}
                    <div class="art-placeholder">
                        <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="1.5"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            aria-hidden="true"
                        >
                            <path d="M9 18V5l12-2v13" />
                            <circle cx="6" cy="18" r="3" />
                            <circle cx="18" cy="16" r="3" />
                        </svg>
                    </div>
                {/if}
            </button>

            <div class="hero-info">
                <span class="hero-label">{hero.label}</span>
                <h1 class="page-title">{heroTitle}</h1>
                <p class="hero-meta">{heroMeta}</p>
                <a class="btn-pill btn-primary" href={hero.href}
                    >{hero.action}</a
                >
            </div>
        </section>

        {#if albums.length > 0}
            <section class="section">
                <h2 class="section-title">Recent Albums</h2>
                <ul class="card-grid">
                    {#each albums as album, index (album.id)}
                        <li
                            class="card-grid-item card-enter"
                            style="animation-delay: {index * 40}ms"
                        >
                            <a href={`/albums/${album.id}`}>
                                <Artwork
                                    albumId={album.id}
                                    alt={album.title}
                                    class="card-grid-thumb"
                                />
                                <div class="card-grid-title ellipsis">
                                    {album.title}
                                </div>
                                <div class="card-grid-meta ellipsis">
                                    {album.artist_names?.join(", ") ?? ""}
                                    {#if album.year}
                                        · {album.year}{/if}
                                </div>
                            </a>
                        </li>
                    {/each}
                </ul>
            </section>
        {/if}

        {#if artists.length > 0}
            <section class="section">
                <h2 class="section-title">Featured Artists</h2>
                <ul class="card-grid">
                    {#each artists as artist, index (artist.id)}
                        <li
                            class="card-grid-item artist card-enter"
                            style="animation-delay: {index * 40}ms"
                        >
                            <a href={`/artists/${artist.id}`}>
                                <ArtistAvatar
                                    artistId={artist.id}
                                    alt={artist.name}
                                    class="artist-avatar"
                                />
                                <div class="card-grid-title ellipsis">
                                    {artist.name}
                                </div>
                                <div class="card-grid-meta ellipsis">
                                    {plural(artist.track_count ?? 0, "track")} · {plural(
                                        artist.album_count ?? 0,
                                        "album",
                                    )}
                                </div>
                            </a>
                        </li>
                    {/each}
                </ul>
            </section>
        {/if}
    {/if}
</div>

<style>
    .home {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-2xl);
    }

    .error {
        background-color: var(--color-error);
        color: var(--color-text);
        padding: var(--spacing-md);
        border-radius: var(--radius-lg);
        font-size: var(--font-size-sm);
    }

    .art-placeholder {
        width: 5rem;
        height: 5rem;
    }

    .art-placeholder svg {
        width: 100%;
        height: 100%;
    }

    .section {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-lg);
    }
</style>
