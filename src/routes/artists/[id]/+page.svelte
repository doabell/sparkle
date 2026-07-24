<script lang="ts">
    import { page } from "$app/stores";
    import {
        getArtist,
        getAlbums,
        getTracksByArtist,
        getArtistInfo,
        getArtistImage,
        getOnlineSettings,
        setArtistProviders,
        setArtistBio,
        setArtistImageFile,
        setArtistImageData,
        searchArtistImages,
        downloadArtistImageCandidate,
        clearArtistCustomImage,
        invalidateArtistImage,
        pickImageFile,
        getRelatedArtists,
        type Artist,
        type Album,
        type Track,
        type ArtistInfo,
        type CachedImage,
        type ImageData,
        type ImageCandidate,
        type ImageSearchResults,
    } from "$lib/api";
    import { cachedImageToUrl, imageDataToUrl } from "$lib/utils/base64";
    import { plural } from "$lib/utils/text";
    import { loadQueue } from "$lib/stores/playback";
    import Loading from "$lib/components/Loading.svelte";
    import TrackRow from "$lib/components/TrackRow.svelte";
    import Artwork from "$lib/components/Artwork.svelte";
    import ArtistAvatar from "$lib/components/ArtistAvatar.svelte";
    import Select from "$lib/components/Select.svelte";
    import { onMount } from "svelte";
    import { addToast } from "$lib/stores/toast";
    import { windowPageTitle } from "$lib/stores/windowPageTitle";
    import { openUrl } from "@tauri-apps/plugin-opener";

    let artist = $state<Artist | null>(null);
    let albums = $state<Album[]>([]);
    let topTracks = $state<Track[]>([]);
    let relatedArtists = $state<Artist[]>([]);
    let artistInfo = $state<ArtistInfo | null>(null);
    let artistImage = $state<CachedImage | null>(null);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let editOpen = $state(false);
    let editInfoProvider = $state("default");
    let editImageProvider = $state("default");
    let editInfoTerm = $state("");
    let editImageTerm = $state("");
    let editBio = $state("");
    let editSaving = $state(false);
    let imageCandidates = $state<ImageCandidate[]>([]);
    let searchingImages = $state(false);
    type ImageSearchStatus =
        "idle" | "searching" | "success" | "empty" | "partial" | "failed";
    let imageSearchStatus = $state<ImageSearchStatus>("idle");
    let imageSearchMessage = $state<string | null>(null);
    let chooserQuery = $state("");
    // URLs that fail both direct and Rust-backed preview loading are hidden.
    let brokenCandidates = $state<Set<string>>(new Set());
    // Most candidates render directly. Only URLs rejected by the webview are
    // downloaded through Rust, keeping the normal path cheap and responsive.
    let candidatePreviewUrls = $state<Map<string, string>>(new Map());
    let candidatePreviewLoading = $state<Set<string>>(new Set());
    let cropLoadingUrl = $state<string | null>(null);

    // Provider options mirror what's enabled in Settings (plus Custom) —
    // disabled providers never show up here.
    let infoProviderOptions = $state([
        { value: "default", label: "Default (settings order)" },
    ]);
    let imageProviderOptions = $state([
        { value: "default", label: "Default (settings order)" },
    ]);

    function providerOptionLabel(source: string): string {
        if (source.startsWith("wikipedia:")) {
            return `Wikipedia (${source.slice("wikipedia:".length)})`;
        }
        return (
            {
                brave: "Brave Image Search",
                duckduckgo: "DuckDuckGo Images",
                embedded: "Embedded tags",
                cover_art_archive: "Cover Art Archive",
                lrclib: "LRCLIB",
                netease: "NetEase",
                qq: "QQ Music",
                lrc: "Sidecar .lrc files",
            }[source] ?? source
        );
    }

    async function loadProviderOptions() {
        try {
            const settings = await getOnlineSettings();
            const info = settings.artist_info_sources.filter(
                (s) => s !== "custom",
            );
            const image = settings.artist_image_sources.filter(
                (s) => s !== "custom",
            );
            infoProviderOptions = [
                { value: "default", label: "Default (settings order)" },
                { value: "custom", label: "Custom (write your own)" },
                ...info.map((s) => ({
                    value: s,
                    label: providerOptionLabel(s),
                })),
            ];
            imageProviderOptions = [
                { value: "default", label: "Default (settings order)" },
                { value: "custom", label: "Custom (choose a file)" },
                ...image.map((s) => ({
                    value: s,
                    label: providerOptionLabel(s),
                })),
            ];
        } catch {
            // keep the minimal defaults
        }
    }

    const artistId = $derived(Number($page.params.id));

    $effect(() => {
        windowPageTitle.set(artist?.name ?? null);
    });

    onMount(() => {
        loadProviderOptions();
    });

    // Reload when the id changes — artist-to-artist navigation reuses this
    // component, so onMount alone would leave stale content behind.
    $effect(() => {
        load(artistId);
    });

    async function load(id: number) {
        loading = true;
        error = null;
        artist = null;
        albums = [];
        topTracks = [];
        relatedArtists = [];
        artistInfo = null;
        artistImage = null;
        try {
            artist = await getArtist(id);
        } catch (e) {
            error = String(e);
            loading = false;
            return;
        }

        const [albumsData, tracksData, relatedData] = await Promise.all([
            getAlbums(id).catch((e) => {
                console.error("Failed to load albums:", e);
                return [];
            }),
            getTracksByArtist(id).catch((e) => {
                console.error("Failed to load tracks:", e);
                return [];
            }),
            getRelatedArtists(id).catch((e) => {
                console.error("Failed to load related artists:", e);
                return [];
            }),
        ]);
        albums = albumsData;
        topTracks = tracksData;
        relatedArtists = relatedData;
        loading = false;

        refreshMetadata();
    }

    function refreshMetadata() {
        getArtistInfo(artistId)
            .then((info) => (artistInfo = info))
            .catch((e) => {
                console.error("Artist info not available:", e);
                artistInfo = null;
            });

        getArtistImage(artistId)
            .then((image) => (artistImage = image))
            .catch((e) => {
                console.error("Artist image not available:", e);
                artistImage = null;
            });
    }

    function openEdit() {
        editInfoProvider = artist?.info_provider ?? "default";
        editImageProvider = artist?.image_provider ?? "default";
        editInfoTerm = artist?.info_term ?? "";
        editImageTerm = artist?.image_term ?? "";
        editBio = artist?.bio ?? "";
        chooserQuery =
            artist?.image_term || artist?.info_term || artist?.name || "";
        imageCandidates = [];
        imageSearchStatus = "idle";
        imageSearchMessage = null;
        candidatePreviewUrls = new Map();
        candidatePreviewLoading = new Set();
        editOpen = true;
    }

    async function findImages() {
        if (searchingImages || !chooserQuery.trim()) return;
        searchingImages = true;
        imageCandidates = [];
        brokenCandidates = new Set();
        candidatePreviewUrls = new Map();
        candidatePreviewLoading = new Set();
        cropCandidate = null;
        imageSearchStatus = "searching";
        imageSearchMessage = "Searching enabled image providers…";
        try {
            const result: ImageSearchResults = await searchArtistImages(
                artistId,
                chooserQuery,
            );
            imageCandidates = result.candidates;
            const failures = [
                result.timed_out_sources.length > 0
                    ? `Timed out waiting for ${result.timed_out_sources.join(", ")}.`
                    : null,
                result.failed_sources.length > 0
                    ? `Could not search ${result.failed_sources.join(", ")}.`
                    : null,
            ].filter((message): message is string => message !== null);
            if (failures.length > 0) {
                const availability =
                    imageCandidates.length > 0
                        ? "Showing available results."
                        : "No usable image results were found.";
                imageSearchStatus =
                    imageCandidates.length > 0 ? "partial" : "failed";
                imageSearchMessage = `${failures.join(" ")} ${availability}`;
                addToast(imageSearchMessage, "error");
            } else if (imageCandidates.length === 0) {
                imageSearchStatus = "empty";
                imageSearchMessage = "No images found online for this artist.";
                addToast("No images found online for this artist", "error");
            } else {
                imageSearchStatus = "success";
                imageSearchMessage = `Found ${imageCandidates.length} image${imageCandidates.length === 1 ? "" : "s"}.`;
            }
        } catch (e) {
            imageSearchStatus = "failed";
            imageSearchMessage = `Image search failed: ${String(e)}`;
            addToast(imageSearchMessage, "error");
        } finally {
            searchingImages = false;
        }
    }

    async function handleCandidateError(candidate: ImageCandidate) {
        const { source, url } = candidate;
        if (brokenCandidates.has(url) || candidatePreviewLoading.has(url)) {
            return;
        }

        // A cached fallback failed too. Do not retry it forever.
        if (candidatePreviewUrls.has(url)) {
            const nextPreviewUrls = new Map(candidatePreviewUrls);
            nextPreviewUrls.delete(url);
            candidatePreviewUrls = nextPreviewUrls;
            brokenCandidates = new Set(brokenCandidates).add(url);
            return;
        }

        candidatePreviewLoading = new Set(candidatePreviewLoading).add(url);
        try {
            const image = await downloadArtistImageCandidate(url, source);
            const previewUrl = imageDataToUrl(image, "");
            if (!previewUrl) throw new Error("downloaded preview was empty");
            candidatePreviewUrls = new Map(candidatePreviewUrls).set(
                url,
                previewUrl,
            );
        } catch {
            const nextBrokenCandidates = new Set(brokenCandidates).add(url);
            brokenCandidates = nextBrokenCandidates;
            const remaining = imageCandidates.filter(
                (item) => !nextBrokenCandidates.has(item.url),
            ).length;
            if (remaining > 0) {
                imageSearchStatus = "partial";
                imageSearchMessage =
                    "Some image hosts blocked thumbnails; remaining results are shown.";
            } else {
                imageSearchStatus = "failed";
                imageSearchMessage =
                    "The image hosts blocked every result. Try another search.";
                addToast(imageSearchMessage, "error");
            }
        } finally {
            const nextLoading = new Set(candidatePreviewLoading);
            nextLoading.delete(url);
            candidatePreviewLoading = nextLoading;
        }
    }

    // --- Crop & focus --------------------------------------------------------
    // Picking a candidate downloads just that one image (the grid itself only
    // loads URLs) and opens a light cropper: drag to set the focal point,
    // slider to zoom; the circle previews the avatar crop. The square crop is
    // baked into the stored custom image (no focus metadata needed).
    let cropCandidate = $state<ImageData | null>(null);
    let cropUrl = $state("");
    let cropX = $state(0.5);
    let cropY = $state(0.5);
    let cropZoom = $state(1);
    let cropNatural = $state<[number, number]>([1, 1]);
    let cropSaving = $state(false);
    let dragStart: { x: number; y: number; fx: number; fy: number } | null =
        null;

    async function startCrop(candidate: ImageCandidate) {
        if (cropLoadingUrl) return;
        cropLoadingUrl = candidate.url;
        try {
            const image = await downloadArtistImageCandidate(
                candidate.url,
                candidate.source,
            );
            cropCandidate = image;
            cropUrl = imageDataToUrl(image, "");
            cropX = 0.5;
            cropY = 0.5;
            cropZoom = 1;
            try {
                const img = new Image();
                img.src = cropUrl;
                await img.decode();
                cropNatural = [img.naturalWidth || 1, img.naturalHeight || 1];
            } catch {
                cropNatural = [1, 1];
            }
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            cropLoadingUrl = null;
        }
    }

    function cropPointerDown(e: PointerEvent) {
        (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
        dragStart = { x: e.clientX, y: e.clientY, fx: cropX, fy: cropY };
    }

    function cropPointerMove(e: PointerEvent) {
        if (!dragStart) return;
        const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
        const [nw, nh] = cropNatural;
        const cover = Math.max(rect.width / nw, rect.height / nh) * cropZoom;
        const overflowX = Math.max(
            0.001,
            (nw * cover - rect.width) / rect.width,
        );
        const overflowY = Math.max(
            0.001,
            (nh * cover - rect.height) / rect.height,
        );
        cropX = Math.min(
            1,
            Math.max(
                0,
                dragStart.fx -
                    (e.clientX - dragStart.x) / rect.width / overflowX,
            ),
        );
        cropY = Math.min(
            1,
            Math.max(
                0,
                dragStart.fy -
                    (e.clientY - dragStart.y) / rect.height / overflowY,
            ),
        );
    }

    function cropPointerUp() {
        dragStart = null;
    }

    // Bakes the crop into a 512px square and stores it as the custom image.
    // Shared by the "Use image" button and Save (a crop left open is part of
    // saving — one click commits everything).
    async function applyCrop() {
        if (!cropCandidate) return;
        const img = new Image();
        img.src = cropUrl;
        await img.decode();
        const size = 512;
        const canvas = document.createElement("canvas");
        canvas.width = size;
        canvas.height = size;
        const ctx = canvas.getContext("2d");
        if (!ctx) throw new Error("canvas unavailable");
        const cover =
            Math.max(size / img.naturalWidth, size / img.naturalHeight) *
            cropZoom;
        const w = img.naturalWidth * cover;
        const h = img.naturalHeight * cover;
        ctx.drawImage(img, (size - w) * cropX, (size - h) * cropY, w, h);
        const mime =
            cropCandidate.mime_type === "image/png"
                ? "image/png"
                : "image/jpeg";
        const blob = await new Promise<Blob | null>((resolve) =>
            canvas.toBlob(resolve, mime, 0.92),
        );
        if (!blob) throw new Error("failed to encode image");
        const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
        await setArtistImageData(artistId, bytes);
        // A hand-picked image is custom content — persist the provider right
        // away, otherwise the old explicit provider keeps serving the old image.
        await setArtistProviders(artistId, { imageProvider: "custom" });
        editImageProvider = "custom";
        artistImage = null;
        invalidateArtistImage(artistId);
        refreshMetadata();
        cropCandidate = null;
    }

    async function confirmCrop() {
        if (!cropCandidate || cropSaving) return;
        cropSaving = true;
        try {
            await applyCrop();
            addToast("Artist image updated", "success");
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            cropSaving = false;
        }
    }

    async function saveEdit() {
        if (!artist) return;
        editSaving = true;
        try {
            // A crop still open in the chooser is part of the save.
            if (cropCandidate) {
                await applyCrop();
            }
            await setArtistProviders(artistId, {
                infoProvider:
                    editInfoProvider === "default" ? null : editInfoProvider,
                imageProvider:
                    editImageProvider === "default" ? null : editImageProvider,
                infoTerm: editInfoTerm.trim() || null,
                imageTerm: editImageTerm.trim() || null,
            });
            const newBio = editBio.trim() || undefined;
            if (newBio !== (artist.bio ?? undefined)) {
                await setArtistBio(artistId, newBio);
            }
            artist = await getArtist(artistId);
            artistInfo = null;
            artistImage = null;
            invalidateArtistImage(artistId);
            refreshMetadata();
            addToast("Artist updated", "success");
            editOpen = false;
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            editSaving = false;
        }
    }

    async function handlePickImage() {
        if (!artist) return;
        const path = await pickImageFile();
        if (!path) return;
        try {
            await setArtistImageFile(artistId, path);
            // A hand-picked image is custom content — persist the provider right
            // away, otherwise the old explicit provider keeps serving the old image.
            await setArtistProviders(artistId, { imageProvider: "custom" });
            editImageProvider = "custom";
            artistImage = null;
            invalidateArtistImage(artistId);
            refreshMetadata();
            addToast("Artist image updated", "success");
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    async function handleClearImage() {
        if (!artist) return;
        try {
            await clearArtistCustomImage(artistId);
            artistImage = null;
            invalidateArtistImage(artistId);
            refreshMetadata();
            addToast("Custom image removed", "success");
        } catch (e) {
            addToast(String(e), "error");
        }
    }

    // Row clicks keep the player's current shuffle mode; the header buttons
    // are explicit context switches: Play = in order, Shuffle = shuffled.
    function playTrack(index: number) {
        if (topTracks.length === 0) return;
        loadQueue(
            topTracks.map((t) => t.id),
            index,
        );
    }

    function playArtist() {
        if (topTracks.length === 0) return;
        loadQueue(
            topTracks.map((t) => t.id),
            0,
            false,
        );
    }

    function shuffleArtist() {
        if (topTracks.length === 0) return;
        const start = Math.floor(Math.random() * topTracks.length);
        loadQueue(
            topTracks.map((t) => t.id),
            start,
            true,
        );
    }

    let wikipediaUrl = $derived.by(() => {
        if (!artistInfo?.source.startsWith("wikipedia:")) return null;
        const lang = artistInfo.source.split(":")[1];
        const title = artist?.info_term || artist?.name;
        if (!lang || !title) return null;
        return `https://${lang}.wikipedia.org/wiki/${encodeURIComponent(title).replace(/%20/g, "_")}`;
    });

    function openWikipedia() {
        if (wikipediaUrl) {
            openUrl(wikipediaUrl).catch((e) => addToast(String(e), "error"));
        }
    }
</script>

<div class="artist-detail page-enter">
    {#if error}
        <div class="error">{error}</div>
    {/if}

    {#if loading}
        <Loading />
    {:else if artist}
        <section
            class="hero-section"
            style:--hero-image={artistImage?.file_path
                ? `url(${cachedImageToUrl(artistImage, "")})`
                : "none"}
        >
            <div class="hero-art round">
                {#if artistImage?.file_path}
                    <img
                        src={cachedImageToUrl(artistImage, "")}
                        decoding="async"
                        alt={artist.name}
                    />
                {:else}
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.5"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        aria-hidden="true"
                    >
                        <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
                        <circle cx="12" cy="7" r="4" />
                    </svg>
                {/if}
            </div>
            <div class="hero-info">
                <span class="hero-label">Artist</span>
                <h1 class="page-title">{artist.name}</h1>
                <p class="hero-meta">
                    {plural(artist.track_count ?? 0, "track")} · {plural(
                        artist.album_count ?? 0,
                        "album",
                    )}
                </p>
                <div class="hero-actions">
                    <button
                        class="btn-pill btn-primary"
                        onclick={playArtist}
                        disabled={topTracks.length === 0}
                    >
                        <svg
                            viewBox="0 0 24 24"
                            fill="currentColor"
                            aria-hidden="true"
                        >
                            <path d="M8 5v14l11-7z" />
                        </svg>
                        Play
                    </button>
                    <button
                        class="btn-pill btn-secondary"
                        onclick={shuffleArtist}
                        disabled={topTracks.length === 0}
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
                            <path
                                d="M2 18h1.4c1.3 0 2.5-.6 3.3-1.7l6.1-8.6c.8-1.1 2-1.7 3.3-1.7H22"
                            />
                            <path d="m18 2 4 4-4 4" />
                            <path d="M2 6h1.9c1.5 0 2.9.9 3.6 2.2" />
                            <path
                                d="M22 18h-5.9c-1.3 0-2.6-.7-3.3-1.8l-.5-.8"
                            />
                            <path d="m18 14 4 4-4 4" />
                        </svg>
                        Shuffle
                    </button>
                    <button
                        class="edit-artist-btn"
                        aria-label="Edit artist metadata"
                        title="Edit artist metadata"
                        onclick={openEdit}
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
                            <path
                                d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"
                            />
                            <path d="m15 5 4 4" />
                        </svg>
                    </button>
                </div>
            </div>
        </section>

        {#if artistInfo?.summary}
            <section class="section">
                <p class="summary">{artistInfo.summary}</p>
                {#if artistInfo.source === "custom"}
                    <span class="info-source">Custom bio</span>
                {:else if artistInfo.source.startsWith("wikipedia")}
                    <button class="info-source link" onclick={openWikipedia}>
                        From Wikipedia
                        <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            aria-hidden="true"
                        >
                            <path d="M15 3h6v6" />
                            <path d="M10 14 21 3" />
                            <path
                                d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"
                            />
                        </svg>
                    </button>
                {/if}
            </section>
        {/if}

        {#if topTracks.length > 0}
            <section class="section">
                <h2 class="section-title">Top Tracks</h2>
                <div class="track-header artist">
                    <span class="header-cover"></span>
                    <span class="header-title">Title</span>
                    <span class="header-duration">
                        <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            aria-hidden="true"
                        >
                            <circle cx="12" cy="12" r="10" />
                            <polyline points="12 6 12 12 16 14" />
                        </svg>
                    </span>
                </div>
                <ul class="track-list">
                    {#each topTracks as track, index (track.id)}
                        <TrackRow
                            {track}
                            {index}
                            variant="artist"
                            onPlay={playTrack}
                            showAddToPlaylist={true}
                        />
                    {/each}
                </ul>
            </section>
        {:else}
            <section class="section">
                <h2 class="section-title">Top Tracks</h2>
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
                            <path d="M9 18V5l12-2v13" />
                            <circle cx="6" cy="18" r="3" />
                            <circle cx="18" cy="16" r="3" />
                        </svg>
                    </div>
                    <p class="empty-title">No top tracks</p>
                    <p class="empty-text">
                        This artist doesn't have any tracks in your library yet.
                    </p>
                </div>
            </section>
        {/if}

        <section class="section">
            <h2 class="section-title">Albums</h2>
            {#if albums.length === 0}
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
                            <path d="M9 18V5l12-2v13" />
                            <circle cx="6" cy="18" r="3" />
                            <circle cx="18" cy="16" r="3" />
                        </svg>
                    </div>
                    <p class="empty-title">No albums</p>
                    <p class="empty-text">
                        This artist doesn't have any albums in your library yet.
                    </p>
                </div>
            {:else}
                <ul class="card-grid">
                    {#each albums as album, index (album.id)}
                        <li
                            class="card-grid-item card-enter"
                            style="animation-delay: {index * 50}ms"
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
                                    {#if album.year}{album.year} ·
                                    {/if}
                                    {album.artist_names?.join(", ") ?? ""}
                                </div>
                            </a>
                        </li>
                    {/each}
                </ul>
            {/if}
        </section>
        {#if relatedArtists.length > 0}
            <section class="section">
                <h2 class="section-title">Related Artists</h2>
                <ul class="card-grid">
                    {#each relatedArtists as related, index (related.id)}
                        <li
                            class="card-grid-item artist card-enter"
                            style="animation-delay: {index * 40}ms"
                        >
                            <a href={`/artists/${related.id}`}>
                                <ArtistAvatar
                                    artistId={related.id}
                                    alt={related.name}
                                    class="artist-avatar"
                                />
                                <div class="card-grid-title ellipsis">
                                    {related.name}
                                </div>
                                <div class="card-grid-meta ellipsis">
                                    {related.track_count ?? 0}
                                    {(related.track_count ?? 0) === 1
                                        ? "track"
                                        : "tracks"}
                                </div>
                            </a>
                        </li>
                    {/each}
                </ul>
            </section>
        {/if}
    {/if}
</div>

{#if editOpen && artist}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
        class="dialog-overlay"
        role="presentation"
        tabindex="-1"
        onclick={() => (editOpen = false)}
        onkeydown={(e: KeyboardEvent) => {
            if (e.key === "Escape") editOpen = false;
        }}
    >
        <div
            class="dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="artist-edit-title"
            tabindex="-1"
            onclick={(e: MouseEvent) => e.stopPropagation()}
        >
            <h2 id="artist-edit-title" class="dialog-title">Edit artist</h2>

            <div class="dialog-body">
                <div class="field">
                    <div class="field-head">
                        <span class="field-label">Bio</span>
                        <div class="field-control">
                            <Select
                                options={infoProviderOptions}
                                value={editInfoProvider}
                                onchange={(v) => (editInfoProvider = v)}
                                ariaLabel="Bio provider"
                            />
                        </div>
                    </div>
                    {#if editInfoProvider.startsWith("wikipedia:")}
                        <input
                            type="text"
                            bind:value={editInfoTerm}
                            placeholder={artist.name}
                            spellcheck="false"
                            aria-label="Wikipedia search term"
                        />
                        <p class="hint">
                            Search term for the Wikipedia page. Leave empty to
                            use the artist name.
                        </p>
                    {:else if editInfoProvider === "custom"}
                        <textarea
                            bind:value={editBio}
                            rows="4"
                            placeholder="Write your own bio"
                            aria-label="Custom bio"></textarea>
                        <p class="hint">
                            Your own text, shown instead of an online biography.
                        </p>
                    {:else}
                        <p class="hint">
                            Providers from Settings are tried in order.
                        </p>
                    {/if}
                </div>

                <div class="field">
                    <div class="field-head">
                        <span class="field-label">Image</span>
                        <div class="field-control">
                            <Select
                                options={imageProviderOptions}
                                value={editImageProvider}
                                onchange={(v) => (editImageProvider = v)}
                                ariaLabel="Image provider"
                            />
                        </div>
                    </div>
                    {#if editImageProvider.startsWith("wikipedia:") || editImageProvider === "brave"}
                        <input
                            type="text"
                            bind:value={editImageTerm}
                            placeholder={editInfoTerm || artist.name}
                            spellcheck="false"
                            aria-label="Image search term"
                        />
                        <p class="hint">
                            Search term for the image lookup. Leave empty to use
                            the artist name.
                        </p>
                    {:else if editImageProvider === "custom"}
                        <div class="image-actions">
                            <button
                                class="btn-pill btn-secondary"
                                onclick={handlePickImage}
                                disabled={editSaving}
                            >
                                Choose file...
                            </button>
                            <button
                                class="btn-pill btn-secondary"
                                onclick={handleClearImage}
                                disabled={editSaving}
                            >
                                Remove image
                            </button>
                        </div>
                        <p class="hint">
                            A local image file, shown instead of an online
                            image.
                        </p>
                    {:else}
                        <p class="hint">
                            Providers from Settings are tried in order.
                        </p>
                    {/if}

                    <div class="chooser">
                        <span class="chooser-title">
                            <svg
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                aria-hidden="true"
                            >
                                <rect
                                    x="3"
                                    y="3"
                                    width="18"
                                    height="18"
                                    rx="2"
                                    ry="2"
                                />
                                <circle cx="8.5" cy="8.5" r="1.5" />
                                <path d="m21 15-5-5L5 21" />
                            </svg>
                            Find an image online
                        </span>
                        <div class="chooser-search">
                            <input
                                type="text"
                                bind:value={chooserQuery}
                                placeholder={`Search the web for images of ${artist.name}…`}
                                spellcheck="false"
                                aria-label="Image search term"
                                onkeydown={(e) => {
                                    if (e.key === "Enter") findImages();
                                }}
                            />
                            <button
                                class="btn-pill btn-secondary"
                                onclick={findImages}
                                disabled={searchingImages ||
                                    !chooserQuery.trim()}
                            >
                                {#if searchingImages}
                                    <Loading variant="inline" />
                                    Searching…
                                {:else if imageSearchStatus === "failed" || imageSearchStatus === "empty"}
                                    Retry search
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
                                        <circle cx="11" cy="11" r="8" />
                                        <path d="m21 21-4.3-4.3" />
                                    </svg>
                                    Search images
                                {/if}
                            </button>
                        </div>
                        {#if imageSearchMessage}
                            <p class="hint" role="status">
                                {imageSearchMessage}
                            </p>
                        {/if}
                        {#if cropCandidate}
                            <div class="crop-area">
                                <div
                                    class="crop-frame"
                                    role="application"
                                    aria-label="Drag to position the crop"
                                    onpointerdown={cropPointerDown}
                                    onpointermove={cropPointerMove}
                                    onpointerup={cropPointerUp}
                                >
                                    <img
                                        src={cropUrl}
                                        alt=""
                                        draggable="false"
                                        style:object-position={`${cropX * 100}% ${cropY * 100}%`}
                                        style:transform={`scale(${cropZoom})`}
                                        style:transform-origin={`${cropX * 100}% ${cropY * 100}%`}
                                    />
                                    <div
                                        class="crop-circle"
                                        aria-hidden="true"
                                    ></div>
                                </div>
                                <div class="crop-zoom">
                                    <span class="crop-zoom-label">Zoom</span>
                                    <input
                                        type="range"
                                        min="1"
                                        max="3"
                                        step="0.01"
                                        bind:value={cropZoom}
                                        aria-label="Zoom"
                                    />
                                </div>
                                <div class="crop-actions">
                                    <button
                                        class="btn-pill btn-secondary"
                                        onclick={() => (cropCandidate = null)}
                                        disabled={cropSaving}
                                    >
                                        Back
                                    </button>
                                    <button
                                        class="btn-pill btn-primary"
                                        onclick={confirmCrop}
                                        disabled={cropSaving}
                                    >
                                        {cropSaving ? "Saving…" : "Use image"}
                                    </button>
                                </div>
                                <p class="hint">
                                    Drag to position, use the slider to zoom.
                                    The circle previews the artist avatar crop.
                                </p>
                            </div>
                        {:else if imageCandidates.length > 0}
                            <div class="candidate-grid">
                                {#each imageCandidates.filter((c) => !brokenCandidates.has(c.url)) as candidate, index (candidate.url + "#" + index)}
                                    <button
                                        class="candidate"
                                        onclick={() => startCrop(candidate)}
                                        title={`Crop this image (${candidate.source})`}
                                        disabled={cropLoadingUrl !== null}
                                    >
                                        <img
                                            src={candidatePreviewUrls.get(
                                                candidate.url,
                                            ) ?? candidate.url}
                                            alt=""
                                            loading="lazy"
                                            referrerpolicy="no-referrer"
                                            onerror={() =>
                                                handleCandidateError(candidate)}
                                        />
                                        {#if cropLoadingUrl === candidate.url || candidatePreviewLoading.has(candidate.url)}
                                            <span class="candidate-loading">
                                                <Loading variant="inline" />
                                            </span>
                                        {/if}
                                        <span class="candidate-source"
                                            >{candidate.source}</span
                                        >
                                    </button>
                                {/each}
                            </div>
                            <p class="hint">
                                Click an image to adjust the crop — it becomes
                                this artist's custom image.
                            </p>
                        {/if}
                    </div>
                </div>
            </div>

            <div class="dialog-actions">
                <button
                    class="btn-pill btn-secondary"
                    onclick={() => (editOpen = false)}
                    disabled={editSaving}>Cancel</button
                >
                <button
                    class="btn-pill btn-primary"
                    onclick={saveEdit}
                    disabled={editSaving}
                >
                    {editSaving ? "Saving..." : "Save"}
                </button>
            </div>
        </div>
    </div>
{/if}

<style>
    .artist-detail {
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

    .section {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-lg);
    }

    .summary {
        color: var(--color-text-secondary);
        line-height: var(--line-height);
        max-width: 64ch;
    }

    .info-source {
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
    }

    .info-source.link {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-xs);
        transition: color var(--transition-fast);
    }

    .info-source.link:hover {
        color: var(--color-text);
        text-decoration: underline;
    }

    .info-source.link svg {
        width: 0.75rem;
        height: 0.75rem;
    }

    .hero-actions {
        display: flex;
        gap: var(--spacing-md);
        margin-top: var(--spacing-md);
        align-items: center;
    }

    .hero-actions .btn-pill svg {
        width: 1rem;
        height: 1rem;
    }

    .info-source.link {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-xs);
        transition: color var(--transition-fast);
    }

    .info-source.link:hover {
        color: var(--color-text);
        text-decoration: underline;
    }

    .info-source.link svg {
        width: 0.75rem;
        height: 0.75rem;
    }

    .edit-artist-btn {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 2.25rem;
        height: 2.25rem;
        border-radius: var(--radius-full);
        border: 1px solid var(--color-border);
        background-color: rgba(255, 255, 255, 0.08);
        color: var(--color-text-secondary);
        transition:
            color var(--transition-fast),
            border-color var(--transition-fast),
            background-color var(--transition-fast),
            transform var(--transition-fast);
    }

    .edit-artist-btn:hover {
        background-color: rgba(255, 255, 255, 0.12);
        border-color: rgba(255, 255, 255, 0.18);
        color: var(--color-text);
        transform: scale(1.04);
    }

    .edit-artist-btn svg {
        width: 1rem;
        height: 1rem;
    }

    .dialog-overlay {
        position: fixed;
        inset: 0;
        z-index: 100;
        display: flex;
        align-items: center;
        justify-content: center;
        background-color: rgba(0, 0, 0, 0.6);
        backdrop-filter: blur(8px);
        -webkit-backdrop-filter: blur(8px);
        padding: var(--spacing-md);
    }

    .dialog {
        width: 100%;
        max-width: 440px;
        background-color: var(--color-surface);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-xl);
        padding: var(--spacing-xl);
        display: flex;
        flex-direction: column;
        gap: var(--spacing-lg);
        box-shadow: var(--shadow-lg);
        max-height: calc(100vh - 2 * var(--spacing-xl));
        overflow-y: auto;
    }

    .dialog-title {
        font-size: var(--font-size-xl);
        font-weight: var(--font-weight-bold);
        letter-spacing: -0.01em;
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

    .field .field-label {
        font-weight: var(--font-weight-semibold);
        font-size: var(--font-size-sm);
        color: var(--color-text);
    }

    .field textarea {
        resize: vertical;
        min-height: 4rem;
        font-family: inherit;
    }

    .hint {
        margin: 0;
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
        line-height: var(--line-height);
    }

    .image-actions {
        display: flex;
        gap: var(--spacing-sm);
        flex-wrap: wrap;
    }

    .field-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
    }

    .chooser {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
        margin-top: var(--spacing-xs);
        padding: var(--spacing-md);
        border: 1px dashed var(--color-border);
        border-radius: var(--radius-lg);
        background: rgba(var(--color-surface-rgb), 0.4);
    }

    .chooser-title {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-xs);
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-semibold);
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-text-secondary);
    }

    .chooser-title svg {
        width: 0.875rem;
        height: 0.875rem;
    }

    .chooser-search {
        display: flex;
        gap: var(--spacing-sm);
    }

    .chooser-search input {
        flex: 1;
        min-width: 0;
    }

    .chooser-search button svg {
        width: 0.875rem;
        height: 0.875rem;
    }

    .candidate-grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(5rem, 1fr));
        gap: var(--spacing-sm);
    }

    .candidate {
        position: relative;
        aspect-ratio: 1;
        border-radius: var(--radius);
        overflow: hidden;
        border: 2px solid transparent;
        transition:
            border-color var(--transition-fast),
            transform var(--transition-fast);
        background-color: var(--color-surface-elevated);
    }

    .candidate:hover {
        border-color: var(--color-accent-graphic);
        transform: scale(1.03);
    }

    .candidate:disabled {
        cursor: default;
    }

    .candidate img {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }

    .candidate-loading {
        position: absolute;
        inset: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        background: rgba(0, 0, 0, 0.55);
        color: var(--color-text);
    }

    .candidate-source {
        position: absolute;
        bottom: 0;
        left: 0;
        right: 0;
        padding: 1px var(--spacing-xs);
        font-size: 0.625rem;
        background-color: rgba(0, 0, 0, 0.65);
        color: rgba(255, 255, 255, 0.85);
        text-align: center;
    }

    .crop-area {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
    }

    .crop-frame {
        position: relative;
        width: 100%;
        max-width: 16rem;
        aspect-ratio: 1;
        overflow: hidden;
        border-radius: var(--radius);
        background-color: var(--color-surface-elevated);
        cursor: grab;
        touch-action: none;
        user-select: none;
    }

    .crop-frame:active {
        cursor: grabbing;
    }

    .crop-frame img {
        width: 100%;
        height: 100%;
        object-fit: cover;
        pointer-events: none;
    }

    .crop-circle {
        position: absolute;
        inset: 6%;
        border-radius: var(--radius-full);
        border: 2px solid rgba(255, 255, 255, 0.9);
        box-shadow: 0 0 0 999px rgba(0, 0, 0, 0.35);
        pointer-events: none;
    }

    .crop-zoom {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        max-width: 16rem;
    }

    .crop-zoom-label {
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
        flex-shrink: 0;
    }

    .crop-zoom input[type="range"] {
        flex: 1;
        accent-color: var(--color-accent-native);
    }

    .crop-actions {
        display: flex;
        gap: var(--spacing-sm);
    }

    .dialog-actions {
        display: flex;
        justify-content: flex-end;
        gap: var(--spacing-md);
    }
</style>
