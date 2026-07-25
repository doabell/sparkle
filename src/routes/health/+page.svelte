<script lang="ts">
    import { onMount } from "svelte";
    import {
        getHealthTracks,
        getLibraryHealth,
        type LibraryHealth,
        type Track,
    } from "$lib/api";
    import Loading from "$lib/components/Loading.svelte";
    import TrackRow from "$lib/components/TrackRow.svelte";
    import { loadQueue } from "$lib/stores/playback";
    import { goto } from "$app/navigation";
    let health = $state<LibraryHealth | null>(null);
    let error = $state<string | null>(null);
    let selectedKind = $state<string | null>(null);
    let detailTracks = $state<Track[]>([]);
    let detailLoading = $state(false);
    onMount(async () => {
        try {
            health = await getLibraryHealth();
        } catch (e) {
            error = String(e);
        }
    });
    const issueGroups = $derived(
        health
            ? [
                  {
                      label: "Core metadata",
                      description: "The tags that make browsing feel finished.",
                      items: [
                          {
                              kind: "titles",
                              label: "Missing titles",
                              value: health.missing_titles,
                              description:
                                  "Give every song a name you can recognize.",
                          },
                          {
                              kind: "artists",
                              label: "Missing artists",
                              value: health.missing_artists,
                              description:
                                  "Artist credits make browsing and search work.",
                          },
                          {
                              kind: "albums",
                              label: "Missing albums",
                              value: health.missing_albums,
                              description:
                                  "Album context keeps the library feeling complete.",
                          },
                          {
                              kind: "genres",
                              label: "Missing genres",
                              value: health.missing_genres,
                              description:
                                  "Genres help Sparkle make better shelves and mixes.",
                          },
                          {
                              kind: "lyrics",
                              label: "Missing lyrics",
                              value: health.missing_lyrics,
                              description:
                                  "Only indexed and cached lyrics count here.",
                          },
                          {
                              kind: "years",
                              label: "Missing release years",
                              value: health.missing_years,
                              description:
                                  "Years make timelines and browsing more useful.",
                          },
                          {
                              kind: "track_numbers",
                              label: "Missing track numbers",
                              value: health.missing_track_numbers,
                              description:
                                  "Track numbers keep albums in the right order.",
                          },
                          {
                              kind: "duplicate_titles",
                              label: "Duplicate titles",
                              value: health.duplicate_titles,
                              description:
                                  "Possible duplicates worth a quick look.",
                          },
                      ],
                  },
                  {
                      label: "Playback readiness",
                      description:
                          "Technical details Sparkle uses to describe and play your files.",
                      items: [
                          {
                              kind: "audio_properties",
                              label: "Needs technical scan",
                              value: health.missing_audio_properties,
                              description:
                                  "Rescan to read bitrate, sample rate, and channels.",
                          },
                          {
                              kind: "durations",
                              label: "Missing durations",
                              value: health.missing_durations,
                              description:
                                  "These files could not report a usable duration.",
                          },
                          {
                              kind: "low_bitrate",
                              label: "Under 192 kbps",
                              value: health.low_bitrate_tracks,
                              description:
                                  "Compressed files where artifacts may be easier to hear.",
                          },
                      ],
                  },
                  {
                      label: "Other checks",
                      description:
                          "Unusual lengths, channels, and listening gaps.",
                      items: [
                          {
                              kind: "never_played",
                              label: "Never played",
                              value: health.never_played,
                              description: "Songs with no listening history.",
                          },
                          {
                              kind: "very_short",
                              label: "Micro tracks",
                              value: health.very_short_tracks,
                              description:
                                  "Under 30 seconds: intros, transitions, or scan oddities.",
                          },
                          {
                              kind: "very_long",
                              label: "Long-form tracks",
                              value: health.very_long_tracks,
                              description:
                                  "Over 20 minutes: mixes, movements, or hidden tracks.",
                          },
                          {
                              kind: "mono",
                              label: "Mono recordings",
                              value: health.mono_tracks,
                              description:
                                  "Often intentional for archival and early recordings.",
                          },
                      ],
                  },
              ]
            : [],
    );

    function formatBytes(bytes: number): string {
        if (bytes <= 0) return "—";
        const units = ["B", "KB", "MB", "GB", "TB"];
        const index = Math.min(
            units.length - 1,
            Math.floor(Math.log(bytes) / Math.log(1024)),
        );
        const value = bytes / 1024 ** index;
        return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
    }

    function share(value: number): number {
        return health && health.track_count > 0
            ? Math.round((value / health.track_count) * 100)
            : 0;
    }

    async function showTracks(kind: string) {
        selectedKind = kind;
        detailTracks = [];
        detailLoading = true;
        try {
            detailTracks = await getHealthTracks(kind);
        } catch (e) {
            error = String(e);
        } finally {
            detailLoading = false;
        }
    }

    async function playDetail(index: number) {
        try {
            await loadQueue(
                detailTracks.map((track) => track.id),
                index,
            );
        } catch (e) {
            error = String(e);
        }
    }

    const completion = $derived(
        health
            ? Math.round(
                  100 *
                      (1 -
                          (health.missing_titles +
                              health.missing_artists +
                              health.missing_albums +
                              health.missing_genres +
                              health.missing_years +
                              health.missing_track_numbers) /
                              Math.max(1, health.track_count * 6)),
              )
            : 0,
    );
</script>

<div class="health-page page-enter">
    <div class="header">
        <div>
            <p class="eyebrow">Library care</p>
            <h1 class="page-title">Library health</h1>
            <p class="subtitle">
                A quick audit of your local music collection.
            </p>
        </div>
        <button class="btn-pill btn-secondary" onclick={() => goto("/folders")}
            >Scan folders</button
        >
    </div>
    {#if error}<div class="error">{error}</div>{:else if !health}<Loading
        />{:else}
        <div class="health-overview">
            <div class="score-card">
                <div class="score-ring" style={`--score: ${completion}%`}>
                    <strong>{completion}%</strong>
                </div>
                <div>
                    <h2>Tag completeness</h2>
                    <p>Core tags filled across your library.</p>
                </div>
            </div>
            <div class="summary">
                <div>
                    <strong>{health.track_count.toLocaleString()}</strong><span
                        >tracks</span
                    >
                </div>
                <div>
                    <strong>{health.album_count.toLocaleString()}</strong><span
                        >albums</span
                    >
                </div>
                <div>
                    <strong>{health.artist_count.toLocaleString()}</strong><span
                        >artists</span
                    >
                </div>
            </div>
        </div>
        <section class="section sound-section">
            <div class="section-heading">
                <div>
                    <p class="eyebrow">Technical</p>
                    <h2 class="section-title">Audio profile</h2>
                    <p class="subtitle">
                        Format, resolution, channels, and disk use.
                    </p>
                </div>
            </div>
            <div class="sound-grid">
                <button
                    class="sound-card lossless"
                    onclick={() => showTracks("lossless")}
                >
                    <span class="sound-value"
                        >{health.lossless_tracks.toLocaleString()}</span
                    >
                    <strong>Lossless</strong>
                    <small>{share(health.lossless_tracks)}% · FLAC, ALAC</small>
                </button>
                <button class="sound-card" onclick={() => showTracks("lossy")}>
                    <span class="sound-value"
                        >{health.lossy_tracks.toLocaleString()}</span
                    >
                    <strong>Lossy</strong>
                    <small
                        >{share(health.lossy_tracks)}% · MP3, AAC, OGG, Opus</small
                    >
                </button>
                <button
                    class="sound-card hires"
                    onclick={() => showTracks("high_resolution")}
                >
                    <span class="sound-value"
                        >{health.high_resolution_tracks.toLocaleString()}</span
                    >
                    <strong>Hi-res</strong>
                    <small>24-bit or 96 kHz+ lossless</small>
                </button>
                <div class="sound-card">
                    <span class="sound-value"
                        >{formatBytes(health.total_size_bytes)}</span
                    >
                    <strong>On disk</strong>
                    <small>Indexed audio size</small>
                </div>
            </div>
            {#if health.formats.length > 0}
                <div class="format-row" aria-label="Audio formats">
                    {#each health.formats as item (item.format)}
                        <span class="format-pill">
                            <strong>{item.format}</strong>
                            {item.tracks.toLocaleString()}
                        </span>
                    {/each}
                </div>
            {/if}
            {#if health.unclassified_tracks > 0}
                <p class="format-note">
                    {health.unclassified_tracks.toLocaleString()} files remain unclassified.
                    M4A is kept neutral because its container can hold either AAC
                    or ALAC.
                </p>
            {/if}
        </section>
        <section class="section">
            <h2 class="section-title">Library checkup</h2>
            {#each issueGroups as group (group.label)}
                <div class="issue-group">
                    <h3>{group.label}</h3>
                    <p class="group-description">{group.description}</p>
                    <div class="issue-grid">
                        {#each group.items as issue (issue.label)}<button
                                class="issue"
                                class:selected={selectedKind === issue.kind}
                                onclick={() => showTracks(issue.kind)}
                            >
                                <span class="issue-copy"
                                    ><strong>{issue.label}</strong><small
                                        >{issue.description}</small
                                    ></span
                                ><span
                                    class="issue-count"
                                    class:healthy={issue.value === 0}
                                    >{issue.value === 0
                                        ? "✓"
                                        : issue.value.toLocaleString()}</span
                                >
                            </button>{/each}
                    </div>
                </div>
            {/each}
        </section>
        {#if selectedKind}
            <section class="section detail-section">
                <div class="section-heading">
                    <div>
                        <h2 class="section-title">Tracks to review</h2>
                        <p class="subtitle">
                            Up to 100 songs. This view makes no changes.
                        </p>
                    </div>
                    <button
                        class="btn-pill btn-secondary"
                        onclick={() => (selectedKind = null)}>Close</button
                    >
                </div>
                {#if detailLoading}<Loading
                    />{:else if detailTracks.length === 0}<div
                        class="empty-detail"
                    >
                        Nothing needs attention here.
                    </div>{:else}<ul class="track-list">
                        {#each detailTracks as track, index (track.id)}<TrackRow
                                {track}
                                {index}
                                variant="songs"
                                showAddToPlaylist={true}
                                onPlay={() => playDetail(index)}
                            />{/each}
                    </ul>{/if}
            </section>
        {/if}
        <p class="note">Scans read file headers and do not edit audio files.</p>
    {/if}
</div>

<style>
    .health-page {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xl);
        max-width: 1120px;
        padding-bottom: var(--spacing-xl);
    }
    .header {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: var(--spacing-md);
    }
    .eyebrow {
        color: var(--color-accent-content);
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-bold);
        letter-spacing: 0.1em;
        text-transform: uppercase;
        margin-bottom: var(--spacing-xs);
    }
    .subtitle,
    .note {
        color: var(--color-text-muted);
        margin-top: var(--spacing-xs);
    }
    .error {
        padding: var(--spacing-md);
        border-radius: var(--radius-lg);
        background: var(--color-error);
    }
    .summary {
        display: grid;
        grid-template-columns: repeat(3, 1fr);
        gap: 1px;
        overflow: hidden;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-lg);
        background: var(--color-border);
    }

    .health-overview {
        display: grid;
        grid-template-columns: minmax(250px, 0.8fr) 1.5fr;
        gap: var(--spacing-md);
    }

    .score-card {
        display: flex;
        align-items: center;
        gap: var(--spacing-lg);
        padding: var(--spacing-lg);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-lg);
        background: linear-gradient(
            135deg,
            var(--color-surface-raised),
            var(--color-surface)
        );
        box-shadow: var(--shadow-sm);
    }

    .score-card h2 {
        font-size: var(--font-size-lg);
    }
    .score-card p {
        color: var(--color-text-muted);
        margin-top: var(--spacing-xs);
        font-size: var(--font-size-sm);
    }

    .score-ring {
        --score: 0%;
        display: grid;
        place-items: center;
        width: 5.5rem;
        height: 5.5rem;
        flex: 0 0 5.5rem;
        border-radius: 50%;
        background: conic-gradient(
            var(--color-accent-graphic) var(--score),
            var(--color-border) 0
        );
        position: relative;
    }

    .score-ring::after {
        content: "";
        position: absolute;
        inset: 0.45rem;
        border-radius: 50%;
        background: var(--color-surface-raised);
    }
    .score-ring strong {
        z-index: 1;
        font-size: var(--font-size-xl);
    }
    .summary div {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        padding: var(--spacing-lg);
        background: var(--color-surface);
    }
    .summary strong {
        font-size: var(--font-size-3xl);
    }
    .summary span {
        color: var(--color-text-muted);
    }
    .section {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
    }

    .sound-section {
        padding: clamp(var(--spacing-lg), 3vw, var(--spacing-xl));
        border: 1px solid var(--color-border);
        border-radius: var(--radius-xl);
        background:
            radial-gradient(
                circle at 90% 0%,
                color-mix(in srgb, var(--color-accent-seed) 14%, transparent),
                transparent 42%
            ),
            var(--color-surface);
        box-shadow: var(--shadow-sm);
    }

    .sound-grid {
        display: grid;
        grid-template-columns: repeat(4, minmax(0, 1fr));
        gap: var(--spacing-sm);
    }

    .sound-card {
        display: flex;
        min-width: 0;
        min-height: 9.5rem;
        flex-direction: column;
        align-items: flex-start;
        justify-content: flex-end;
        gap: 0.15rem;
        padding: var(--spacing-md);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-lg);
        background: color-mix(
            in srgb,
            var(--color-surface-elevated) 72%,
            transparent
        );
        text-align: left;
        transition:
            transform var(--transition-fast),
            border-color var(--transition-fast),
            background-color var(--transition-fast);
    }

    button.sound-card:hover {
        transform: translateY(-2px);
        border-color: var(--color-accent-graphic);
        background-color: var(--color-surface-elevated);
    }

    .sound-card.lossless {
        background: linear-gradient(
            145deg,
            color-mix(in srgb, #8e5cff 18%, var(--color-surface-elevated)),
            var(--color-surface)
        );
    }

    .sound-card.hires {
        background: linear-gradient(
            145deg,
            color-mix(in srgb, #ff9f0a 16%, var(--color-surface-elevated)),
            var(--color-surface)
        );
    }

    .sound-value {
        margin-bottom: auto;
        font-size: clamp(var(--font-size-2xl), 3vw, var(--font-size-3xl));
        font-weight: var(--font-weight-bold);
        line-height: 1;
        letter-spacing: -0.04em;
        font-variant-numeric: tabular-nums;
    }

    .sound-card strong {
        font-size: var(--font-size-sm);
    }

    .sound-card small,
    .format-note,
    .group-description {
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
    }

    .format-row {
        display: flex;
        flex-wrap: wrap;
        gap: var(--spacing-xs);
    }

    .format-pill {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-xs);
        padding: 0.3rem 0.65rem;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-full);
        background: var(--color-surface-elevated);
        color: var(--color-text-secondary);
        font-size: var(--font-size-xs);
        font-variant-numeric: tabular-nums;
    }

    .format-pill strong {
        color: var(--color-text);
        font-size: 0.65rem;
        letter-spacing: 0.06em;
    }

    .format-note {
        max-width: 46rem;
    }
    .issue-grid {
        display: grid;
        grid-template-columns: repeat(3, 1fr);
        gap: var(--spacing-sm);
    }
    .issue-group + .issue-group {
        margin-top: var(--spacing-lg);
    }
    .issue-group h3 {
        margin: 0;
        color: var(--color-text-secondary);
        font-size: var(--font-size-sm);
        font-weight: var(--font-weight-bold);
    }

    .group-description {
        margin: 0 0 var(--spacing-sm);
    }
    .issue {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
        padding: var(--spacing-md);
        border: 1px solid var(--color-border);
        border-radius: var(--radius);
        background: var(--color-surface);
        text-align: left;
        cursor: pointer;
        transition:
            border-color var(--transition-fast),
            background-color var(--transition-fast),
            transform var(--transition-fast);
        min-height: 6.5rem;
    }

    .issue:hover,
    .issue.selected {
        border-color: var(--color-accent-graphic);
        background: var(--color-surface-raised);
        transform: translateY(-1px);
    }
    .issue-copy {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        min-width: 0;
    }
    .issue-copy strong {
        color: var(--color-text);
        font-weight: var(--font-weight-bold);
    }
    .issue-copy small {
        color: var(--color-text-muted);
        font-size: var(--font-size-sm);
    }
    .issue-count {
        color: var(--color-text);
        font-weight: var(--font-weight-bold);
        font-variant-numeric: tabular-nums;
    }

    .issue-count.healthy {
        color: var(--color-success);
    }
    .section-heading {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: var(--spacing-md);
    }
    .track-list {
        display: flex;
        flex-direction: column;
        gap: 1px;
        margin: 0;
        padding: 0;
        list-style: none;
    }
    .empty-detail {
        padding: var(--spacing-xl);
        border: 1px dashed var(--color-border);
        border-radius: var(--radius-lg);
        color: var(--color-text-muted);
        text-align: center;
    }
    @media (max-width: 600px) {
        .header {
            flex-direction: column;
        }
        .summary,
        .issue-grid {
            grid-template-columns: 1fr;
        }
        .health-overview {
            grid-template-columns: 1fr;
        }
        .sound-grid {
            grid-template-columns: repeat(2, minmax(0, 1fr));
        }
    }

    @media (max-width: 400px) {
        .sound-grid {
            grid-template-columns: 1fr;
        }
    }
</style>
