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
    import SearchField from "$lib/components/SearchField.svelte";
    import SearchFeedback from "$lib/components/SearchFeedback.svelte";
    import { dialogFocus } from "$lib/utils/dialogFocus";
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
    let editSection = $state<"image" | "bio">("image");
    let editInfoProvider = $state("default");
    let editImageProvider = $state("default");
    let editInfoTerm = $state("");
    let editBio = $state("");
    let editSaving = $state(false);
    let imageCandidates = $state<ImageCandidate[]>([]);
    let searchingImages = $state(false);
    let imageSearchRequest = 0;
    type ImageSearchStatus =
        "idle" | "searching" | "success" | "empty" | "partial" | "failed";
    let imageSearchStatus = $state<ImageSearchStatus>("idle");
    let imageSearchMessage = $state<string | null>(null);
    let imageSearchIssues = $state<string[]>([]);
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
                deezer: "Deezer",
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
        imageSearchRequest += 1;
        editOpen = false;
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
        if (editBusy) return;
        imageSearchRequest += 1;
        searchingImages = false;
        editSection = "image";
        imageSearchIssues = [];
        brokenCandidates = new Set();
        cropCandidate = null;
        cropLoadingUrl = null;
        editInfoProvider = artist?.info_provider ?? "default";
        editImageProvider = artist?.image_provider ?? "default";
        editInfoTerm = artist?.info_term ?? "";
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

    function closeEdit() {
        if (editBusy) return;
        imageSearchRequest += 1;
        editOpen = false;
    }

    async function findImages() {
        if (searchingImages || editBusy || !chooserQuery.trim()) return;
        const request = ++imageSearchRequest;
        searchingImages = true;
        imageCandidates = [];
        brokenCandidates = new Set();
        candidatePreviewUrls = new Map();
        candidatePreviewLoading = new Set();
        cropCandidate = null;
        imageSearchStatus = "searching";
        imageSearchMessage = "Searching online providers…";
        imageSearchIssues = [];
        try {
            const result: ImageSearchResults = await searchArtistImages(
                artistId,
                chooserQuery,
            );
            if (request !== imageSearchRequest || !editOpen) return;
            imageCandidates = result.candidates;
            imageSearchIssues = [
                ...result.timed_out_sources.map(
                    (source) => `${providerOptionLabel(source)}: timed out`,
                ),
                ...result.failed_sources.map(
                    (source) =>
                        `${providerOptionLabel(source)}: ${result.provider_errors?.[source] ?? "search unavailable"}`,
                ),
            ];
            if (imageCandidates.length > 0) {
                imageSearchStatus = imageSearchIssues.length
                    ? "partial"
                    : "success";
                imageSearchMessage = `${imageCandidates.length} result${imageCandidates.length === 1 ? "" : "s"}${imageSearchIssues.length ? " · Some providers unavailable" : ""}`;
            } else {
                imageSearchStatus = imageSearchIssues.length
                    ? "failed"
                    : "empty";
                imageSearchMessage = imageSearchIssues.length
                    ? "Search unavailable. Try again or choose a file."
                    : "No matches. Try another spelling or artist name.";
            }
        } catch (e) {
            if (request !== imageSearchRequest || !editOpen) return;
            imageSearchStatus = "failed";
            imageSearchMessage = `Image search failed: ${String(e)}`;
        } finally {
            if (request === imageSearchRequest) searchingImages = false;
        }
    }

    async function handleCandidateError(candidate: ImageCandidate) {
        const { source, url } = candidate;
        const request = imageSearchRequest;
        if (brokenCandidates.has(url) || candidatePreviewLoading.has(url)) {
            return;
        }

        // A cached fallback failed too. Do not retry it forever.
        if (candidatePreviewUrls.has(url)) {
            const nextPreviewUrls = new Map(candidatePreviewUrls);
            nextPreviewUrls.delete(url);
            candidatePreviewUrls = nextPreviewUrls;
            markCandidateBroken(url);
            return;
        }

        candidatePreviewLoading = new Set(candidatePreviewLoading).add(url);
        try {
            const image = await downloadArtistImageCandidate(url, source);
            if (request !== imageSearchRequest || !editOpen) return;
            const previewUrl = imageDataToUrl(image, "");
            if (!previewUrl) throw new Error("downloaded preview was empty");
            candidatePreviewUrls = new Map(candidatePreviewUrls).set(
                url,
                previewUrl,
            );
        } catch {
            if (request !== imageSearchRequest || !editOpen) return;
            markCandidateBroken(url);
        } finally {
            if (request === imageSearchRequest) {
                const nextLoading = new Set(candidatePreviewLoading);
                nextLoading.delete(url);
                candidatePreviewLoading = nextLoading;
            }
        }
    }

    function markCandidateBroken(url: string) {
        brokenCandidates = new Set(brokenCandidates).add(url);
        const remaining = imageCandidates.filter(
            (item) => !brokenCandidates.has(item.url),
        ).length;
        imageSearchStatus = remaining ? "partial" : "failed";
        imageSearchMessage = remaining
            ? `${remaining} results · Some previews unavailable`
            : "Images could not be loaded. Try another search or choose a file.";
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
    let editBusy = $derived(
        editSaving || cropSaving || cropLoadingUrl !== null,
    );
    let dragStart: { x: number; y: number; fx: number; fy: number } | null =
        null;

    async function startCrop(candidate: ImageCandidate) {
        if (editBusy || searchingImages) return;
        const request = imageSearchRequest;
        cropLoadingUrl = candidate.url;
        try {
            const image = await downloadArtistImageCandidate(
                candidate.url,
                candidate.source,
            );
            if (request !== imageSearchRequest || !editOpen) return;
            cropCandidate = image;
            cropUrl = imageDataToUrl(image, "");
            cropX = 0.5;
            cropY = 0.5;
            cropZoom = 1;
            try {
                const img = new Image();
                img.src = cropUrl;
                await img.decode();
                if (request !== imageSearchRequest || !editOpen) return;
                cropNatural = [img.naturalWidth || 1, img.naturalHeight || 1];
            } catch {
                if (request === imageSearchRequest) cropNatural = [1, 1];
            }
        } catch (e) {
            if (request !== imageSearchRequest || !editOpen) return;
            addToast(String(e), "error");
        } finally {
            if (request === imageSearchRequest) cropLoadingUrl = null;
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
    // The crop view has one explicit action that commits this image.
    // The command replaces all four fields. Fill untouched fields from the
    // saved artist, so changing an image cannot reset its biography source.
    function saveArtistProviderFields(
        target: Artist,
        changes: Parameters<typeof setArtistProviders>[1],
    ) {
        return setArtistProviders(target.id, {
            infoProvider: target.info_provider ?? null,
            imageProvider: target.image_provider ?? null,
            infoTerm: target.info_term ?? null,
            imageTerm: target.image_term ?? null,
            ...changes,
        });
    }

    async function applyCrop(target: Artist) {
        const targetId = target.id;
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
        await setArtistImageData(targetId, bytes);
        // A hand-picked image is custom content — persist the provider right
        // away, otherwise the old explicit provider keeps serving the old image.
        await saveArtistProviderFields(target, { imageProvider: "custom" });
        editImageProvider = "custom";
        invalidateArtistImage(targetId);
        if (artistId === targetId) {
            artist = await getArtist(targetId);
            artistImage = null;
            refreshMetadata();
        }
        cropCandidate = null;
    }

    async function confirmCrop() {
        if (!artist || !cropCandidate || editBusy) return;
        const target = artist;
        const targetId = target.id;
        cropSaving = true;
        try {
            await applyCrop(target);
            if (artistId === targetId) editOpen = false;
            addToast("Artist image updated", "success");
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            cropSaving = false;
        }
    }

    async function saveEdit() {
        if (!artist || editBusy) return;
        const target = artist;
        const targetId = target.id;
        const section = editSection;
        const newBio = editBio.trim() || undefined;
        editSaving = true;
        try {
            if (section === "image") {
                await saveArtistProviderFields(target, {
                    imageProvider:
                        editImageProvider === "default"
                            ? null
                            : editImageProvider,
                    imageTerm:
                        chooserQuery.trim() === target.name
                            ? null
                            : chooserQuery.trim() || null,
                });
            } else {
                await saveArtistProviderFields(target, {
                    infoProvider:
                        editInfoProvider === "default"
                            ? null
                            : editInfoProvider,
                    infoTerm: editInfoTerm.trim() || null,
                });
                if (newBio !== (target.bio ?? undefined))
                    await setArtistBio(targetId, newBio);
            }
            const updated = await getArtist(targetId);
            invalidateArtistImage(targetId);
            if (artistId === targetId) {
                artist = updated;
                artistInfo = null;
                artistImage = null;
                refreshMetadata();
                editOpen = false;
            }
            addToast(
                section === "image"
                    ? "Image source updated"
                    : "Biography updated",
                "success",
            );
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            editSaving = false;
        }
    }

    async function handlePickImage() {
        if (!artist || editBusy) return;
        const target = artist;
        const targetId = target.id;
        editSaving = true;
        try {
            const path = await pickImageFile();
            if (!path) return;
            await setArtistImageFile(targetId, path);
            await saveArtistProviderFields(target, { imageProvider: "custom" });
            const updated = await getArtist(targetId);
            invalidateArtistImage(targetId);
            if (artistId === targetId) {
                artist = updated;
                artistImage = null;
                refreshMetadata();
                editOpen = false;
            }
            addToast("Artist image updated", "success");
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            editSaving = false;
        }
    }

    async function handleClearImage() {
        if (!artist || editBusy) return;
        const target = artist;
        const targetId = target.id;
        editSaving = true;
        try {
            await clearArtistCustomImage(targetId);
            if (target.image_provider === "custom")
                await saveArtistProviderFields(target, { imageProvider: null });
            const updated = await getArtist(targetId);
            invalidateArtistImage(targetId);
            if (artistId === targetId) {
                artist = updated;
                editImageProvider = updated?.image_provider ?? "default";
                artistImage = null;
                refreshMetadata();
            }
            addToast("Custom image removed", "success");
        } catch (e) {
            addToast(String(e), "error");
        } finally {
            editSaving = false;
        }
    }

    // Row clicks keep the player's current shuffle mode; the header buttons
    // are explicit context switches: Play = in order, Shuffle = shuffled.
    function playTrack(index: number) {
        if (topTracks.length === 0) return;
        loadQueue(
            topTracks.map((t) => t.id),
            index,
            undefined,
            { kind: "artist", id: String(artistId) },
        );
    }

    function playArtist() {
        if (topTracks.length === 0) return;
        loadQueue(
            topTracks.map((t) => t.id),
            0,
            false,
            { kind: "artist", id: String(artistId) },
        );
    }

    function shuffleArtist() {
        if (topTracks.length === 0) return;
        const start = Math.floor(Math.random() * topTracks.length);
        loadQueue(
            topTracks.map((t) => t.id),
            start,
            true,
            { kind: "artist", id: String(artistId) },
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
        class="search-dialog-overlay"
        role="presentation"
        tabindex="-1"
        onclick={closeEdit}
    >
        <div
            class="search-dialog"
            style:--search-dialog-width="32rem"
            role="dialog"
            aria-modal="true"
            aria-labelledby="artist-edit-title"
            tabindex="-1"
            use:dialogFocus={closeEdit}
            onclick={(e) => e.stopPropagation()}
        >
            <header class="search-dialog-heading">
                <h2 id="artist-edit-title">
                    {cropCandidate ? "Crop Image" : "Edit Artist"}
                </h2>
            </header>
            {#if !cropCandidate}
                <div
                    class="segmented-control edit-sections"
                    role="group"
                    aria-label="Artist details"
                >
                    <button
                        class:active={editSection === "image"}
                        aria-pressed={editSection === "image"}
                        disabled={editBusy}
                        onclick={() => (editSection = "image")}>Image</button
                    >
                    <button
                        class:active={editSection === "bio"}
                        aria-pressed={editSection === "bio"}
                        disabled={editBusy}
                        onclick={() => (editSection = "bio")}>Biography</button
                    >
                </div>
            {/if}
            <div class="search-dialog-body">
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
                            <div class="crop-circle" aria-hidden="true"></div>
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
                        <p class="hint">
                            Drag to position, use the slider to zoom. The circle
                            previews the artist avatar crop.
                        </p>
                    </div>
                {:else if editSection === "image"}
                    <section
                        class="search-dialog-section"
                        aria-label="Artist image source"
                    >
                        <div class="search-dialog-row">
                            <span class="search-dialog-label">Source</span>
                            <Select
                                options={imageProviderOptions}
                                value={editImageProvider}
                                onchange={(v) => {
                                    if (!editBusy) editImageProvider = v;
                                }}
                                ariaLabel="Image provider"
                                disabled={editBusy}
                            />
                        </div>
                        {#if editImageProvider === "custom"}
                            <div class="search-dialog-tools">
                                <button
                                    class="btn-pill btn-secondary"
                                    onclick={handlePickImage}
                                    disabled={editBusy}>Choose File…</button
                                >
                                {#if artist.image_provider === "custom" || artistImage?.source === "custom"}
                                    <button
                                        class="btn-pill btn-secondary"
                                        onclick={handleClearImage}
                                        disabled={editBusy}
                                        >Remove Custom</button
                                    >
                                {/if}
                            </div>
                        {/if}
                    </section>
                    <section
                        class="search-dialog-section"
                        aria-label="Search artist images online"
                    >
                        <SearchField
                            bind:value={chooserQuery}
                            label="Search Online"
                            placeholder="Artist name"
                            busy={searchingImages}
                            disabled={editBusy}
                            onsearch={findImages}
                        />
                        <SearchFeedback
                            message={imageSearchMessage}
                            issues={imageSearchIssues}
                            failed={imageSearchStatus === "failed"}
                        />
                        {#if imageCandidates.length > 0}
                            <div
                                class="candidate-grid"
                                aria-label="Image results"
                            >
                                {#each imageCandidates.filter((c) => !brokenCandidates.has(c.url)) as candidate, index (candidate.url + "#" + index)}
                                    <button
                                        class="candidate"
                                        onclick={() => startCrop(candidate)}
                                        aria-label={`Preview image ${index + 1} from ${providerOptionLabel(candidate.source)}`}
                                        title={`Preview image from ${providerOptionLabel(candidate.source)}`}
                                        disabled={editBusy || searchingImages}
                                    >
                                        <span class="candidate-art">
                                            <img
                                                src={candidatePreviewUrls.get(
                                                    candidate.url,
                                                ) ?? candidate.url}
                                                alt=""
                                                loading="lazy"
                                                referrerpolicy="no-referrer"
                                                onerror={() =>
                                                    handleCandidateError(
                                                        candidate,
                                                    )}
                                            />
                                            {#if cropLoadingUrl === candidate.url || candidatePreviewLoading.has(candidate.url)}
                                                <span class="candidate-loading"
                                                    ><Loading
                                                        variant="inline"
                                                    /></span
                                                >
                                            {/if}
                                        </span>
                                        <span class="candidate-source"
                                            >{providerOptionLabel(
                                                candidate.source,
                                            )}</span
                                        >
                                    </button>
                                {/each}
                            </div>
                        {/if}
                    </section>
                {:else}
                    <section
                        class="search-dialog-section"
                        aria-label="Artist biography"
                    >
                        <div class="search-dialog-row">
                            <span class="search-dialog-label">Source</span>
                            <Select
                                options={infoProviderOptions}
                                value={editInfoProvider}
                                onchange={(v) => {
                                    if (!editBusy) editInfoProvider = v;
                                }}
                                ariaLabel="Bio provider"
                                disabled={editBusy}
                            />
                        </div>
                        {#if editInfoProvider !== "custom"}
                            <label
                                class="search-dialog-label"
                                for="artist-bio-term">Search Term</label
                            >
                            <input
                                id="artist-bio-term"
                                type="text"
                                bind:value={editInfoTerm}
                                placeholder={artist.name}
                                spellcheck="false"
                                disabled={editBusy}
                            />
                        {:else if editInfoProvider === "custom"}
                            <textarea
                                class="bio-editor"
                                bind:value={editBio}
                                rows="8"
                                placeholder="Write a biography…"
                                aria-label="Custom biography"
                                disabled={editBusy}></textarea>
                        {/if}
                    </section>
                {/if}
            </div>
            <footer class="search-dialog-actions">
                {#if cropCandidate}
                    <button
                        class="btn-pill btn-secondary"
                        onclick={() => (cropCandidate = null)}
                        disabled={editBusy}>Back</button
                    >
                    <button
                        class="btn-pill btn-primary"
                        onclick={confirmCrop}
                        disabled={editBusy}
                        >{cropSaving ? "Saving…" : "Use Image"}</button
                    >
                {:else}
                    <button
                        class="btn-pill btn-secondary"
                        onclick={closeEdit}
                        disabled={editBusy}>Cancel</button
                    >
                    <button
                        class="btn-pill btn-primary"
                        onclick={saveEdit}
                        disabled={editBusy}
                    >
                        {editSaving
                            ? "Saving…"
                            : editSection === "image"
                              ? "Save Image Source"
                              : "Save Biography"}
                    </button>
                {/if}
            </footer>
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
        transition: color var(--transition-feedback);
    }

    .info-source.link:hover {
        color: var(--color-text);
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
        transition: color var(--transition-feedback);
    }

    .info-source.link:hover {
        color: var(--color-text);
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
            color var(--transition-feedback),
            border-color var(--transition-feedback),
            background-color var(--transition-feedback),
            transform var(--transition-transform);
    }

    .edit-artist-btn:hover {
        background-color: var(--interactive-hover);
        color: var(--color-text);
        transform: scale(var(--motion-hover-scale));
    }

    .edit-artist-btn svg {
        width: 1rem;
        height: 1rem;
    }

    .hint {
        margin: 0;
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
        line-height: var(--line-height);
    }

    .edit-sections {
        align-self: flex-start;
    }
    .bio-editor {
        resize: vertical;
        min-height: 10rem;
        font-family: inherit;
    }
    .candidate-grid {
        display: grid;
        grid-template-columns: repeat(3, minmax(0, 1fr));
        gap: var(--spacing-sm);
    }
    .candidate {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        padding: var(--spacing-xs);
        min-width: 0;
        border-radius: var(--radius);
        text-align: center;
        transition: background-color var(--transition-feedback);
    }
    .candidate:hover:not(:disabled) {
        background: var(--interactive-hover);
    }
    .candidate:active:not(:disabled) {
        background: var(--interactive-active);
    }
    .candidate:focus-visible {
        outline-offset: -2px;
    }
    .candidate-art {
        position: relative;
        width: 100%;
        aspect-ratio: 1;
        overflow: hidden;
        border-radius: var(--radius-full);
        background: var(--color-surface-elevated);
    }
    .candidate img {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }
    .candidate-loading {
        position: absolute;
        inset: 0;
        display: grid;
        place-items: center;
        background: var(--color-surface-elevated);
    }
    .candidate-source {
        font-size: var(--font-size-xs);
        color: var(--color-text-secondary);
        max-width: 100%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .crop-area {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
    }

    .crop-frame {
        align-self: center;
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
</style>
