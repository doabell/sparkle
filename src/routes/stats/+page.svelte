<script lang="ts">
    import { onMount } from "svelte";
    import {
        getListeningStats,
        type ListeningStats,
        type PlayStatBucket,
    } from "$lib/api";
    import { uiPref } from "$lib/stores/uiPrefs";
    import Loading from "$lib/components/Loading.svelte";
    import Artwork from "$lib/components/Artwork.svelte";
    import ArtistAvatar from "$lib/components/ArtistAvatar.svelte";
    import { plural } from "$lib/utils/text";

    const RANGES = [
        { days: 7, label: "7 days" },
        { days: 30, label: "30 days" },
        { days: 90, label: "90 days" },
        { days: 0, label: "All time" },
    ];

    const rangeDays = uiPref<number>("stats.rangeDays", 30);

    let stats = $state<ListeningStats | null>(null);
    let loading = $state(true);
    let error = $state<string | null>(null);

    async function load(days: number) {
        loading = true;
        error = null;
        try {
            stats = await getListeningStats(days > 0 ? days : undefined);
        } catch (e) {
            error = String(e);
        } finally {
            loading = false;
        }
    }

    onMount(() => load($rangeDays));

    function chooseRange(days: number) {
        if (days === $rangeDays) return;
        $rangeDays = days;
        load(days);
    }

    function formatMinutes(ms: number): string {
        const minutes = Math.round(ms / 60_000);
        return minutes === 1 ? "1 min" : `${minutes.toLocaleString()} min`;
    }

    function minutesValue(ms: number): string {
        return Math.round(ms / 60_000).toLocaleString();
    }

    function percentage(part: number, whole: number): number {
        return whole > 0 ? Math.round((part / whole) * 100) : 0;
    }

    function formatHour(hour: number | null): string {
        if (hour === null) return "—";
        if (hour === 0) return "midnight";
        if (hour === 12) return "noon";
        return hour < 12 ? `${hour} am` : `${hour - 12} pm`;
    }

    // Fill the gaps the backend leaves: every day in the range for day mode,
    // every month from the first active month to now for month mode.
    let activitySeries = $derived.by<PlayStatBucket[]>(() => {
        if (!stats) return [];
        const byLabel = new Map(stats.activity.map((b) => [b.label, b]));
        const out: PlayStatBucket[] = [];
        if (!stats.activity_by_month) {
            const days = $rangeDays > 0 ? $rangeDays : 30;
            const today = new Date();
            for (let i = days - 1; i >= 0; i--) {
                const d = new Date(
                    today.getFullYear(),
                    today.getMonth(),
                    today.getDate() - i,
                );
                const label = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
                out.push(byLabel.get(label) ?? { label, plays: 0, ms: 0 });
            }
            return out;
        }
        if (stats.activity.length === 0) return [];
        const [fy, fm] = stats.activity[0].label.split("-").map(Number);
        const now = new Date();
        let y = fy;
        let m = fm;
        while (
            y < now.getFullYear() ||
            (y === now.getFullYear() && m <= now.getMonth() + 1)
        ) {
            const label = `${y}-${String(m).padStart(2, "0")}`;
            out.push(byLabel.get(label) ?? { label, plays: 0, ms: 0 });
            m += 1;
            if (m > 12) {
                m = 1;
                y += 1;
            }
        }
        return out;
    });

    let maxBucketMs = $derived(Math.max(1, ...activitySeries.map((b) => b.ms)));

    const MONTH_NAMES = [
        "Jan",
        "Feb",
        "Mar",
        "Apr",
        "May",
        "Jun",
        "Jul",
        "Aug",
        "Sep",
        "Oct",
        "Nov",
        "Dec",
    ];

    function bucketLabel(label: string, byMonth: boolean): string {
        if (byMonth) {
            const [y, m] = label.split("-").map(Number);
            return `${MONTH_NAMES[(m ?? 1) - 1]} ${y}`;
        }
        const [, m, d] = label.split("-").map(Number);
        return `${MONTH_NAMES[(m ?? 1) - 1]} ${d}`;
    }

    function bucketTitle(bucket: PlayStatBucket): string {
        const label = bucketLabel(
            bucket.label,
            stats?.activity_by_month ?? false,
        );
        return `${label} — ${formatMinutes(bucket.ms)}, ${plural(bucket.plays, "play")}`;
    }

    // Only label a few bars so the axis stays readable at any bucket count.
    function showAxisLabel(index: number, total: number): boolean {
        if (total <= 12) return true;
        return (
            index === 0 ||
            index === total - 1 ||
            index === Math.floor((total - 1) / 2)
        );
    }

    let topTime = $derived({
        tracks: Math.max(1, ...(stats?.top_tracks.map((t) => t.ms) ?? [1])),
        artists: Math.max(1, ...(stats?.top_artists.map((a) => a.ms) ?? [1])),
        albums: Math.max(1, ...(stats?.top_albums.map((a) => a.ms) ?? [1])),
    });

    type Insight = { kicker: string; title: string; detail: string };
    let insights = $derived.by<Insight[]>(() => {
        if (!stats || stats.total_ms <= 0) return [];
        const dayparts = [
            { name: "morning", ms: stats.morning_ms },
            { name: "afternoon", ms: stats.afternoon_ms },
            { name: "evening", ms: stats.evening_ms },
            { name: "late night", ms: stats.late_night_ms },
        ].sort((a, b) => b.ms - a.ms);
        const leading = dayparts[0];
        const daypartTitle: Record<string, string> = {
            morning: "First-light listener",
            afternoon: "Daylight drifter",
            evening: "Golden-hour regular",
            "late night": "Night owl",
        };
        const variety = stats.unique_tracks / Math.max(1, stats.total_plays);
        const varietyTitle =
            variety >= 0.75
                ? "Open-road ears"
                : variety <= 0.35
                  ? "Comfort-loop connoisseur"
                  : "Curious regular";
        const finishRate = percentage(stats.completed_plays, stats.total_plays);
        const finishTitle =
            finishRate >= 75
                ? "Full-song loyalist"
                : finishRate <= 35
                  ? "Hook hunter"
                  : "Selective finisher";
        const weekendShare = percentage(stats.weekend_ms, stats.total_ms);
        const weekendTitle =
            weekendShare >= 45
                ? "Weekend headliner"
                : weekendShare <= 18
                  ? "Weekday soundtrack"
                  : "Seven-day soundtrack";

        const result: Insight[] = [
            {
                kicker: "Listening Time",
                title: daypartTitle[leading.name],
                detail: `${percentage(leading.ms, stats.total_ms)}% in the ${leading.name}. Peak: ${formatHour(stats.peak_hour)}.`,
            },
            {
                kicker: "Variety",
                title: varietyTitle,
                detail: `${stats.unique_tracks.toLocaleString()} distinct tracks across ${stats.total_plays.toLocaleString()} meaningful listens.`,
            },
            {
                kicker: "Completion",
                title: finishTitle,
                detail: `${finishRate}% of meaningful listens reached the final stretch.`,
            },
            {
                kicker: "Week Split",
                title: weekendTitle,
                detail: `${weekendShare}% of your minutes arrived on weekends.`,
            },
        ];
        if (stats.average_year !== null) {
            const year = Math.round(stats.average_year);
            result.push({
                kicker: "Release Years",
                title:
                    year < 1990
                        ? "Analog soul"
                        : year < 2010
                          ? "Millennium signal"
                          : year >= 2020
                            ? "Present tense"
                            : "Across the decades",
                detail: `Your time-weighted release year is ${year}.`,
            });
        }
        if (stats.top_genre) {
            result.push({
                kicker: "Top Genre",
                title: stats.top_genre,
                detail: `${percentage(stats.top_genre_ms, stats.total_ms)}% of listening time.`,
            });
        }
        return result;
    });
</script>

<div class="stats-page page-shell page-enter">
    <div class="header page-header">
        <div class="header-text page-heading">
            <h1 class="page-title">Stats</h1>
        </div>
        <div
            class="range-segment segmented-control accent"
            role="group"
            aria-label="Time range"
        >
            {#each RANGES as range (range.days)}
                <button
                    class="segment"
                    class:active={$rangeDays === range.days}
                    onclick={() => chooseRange(range.days)}
                    aria-pressed={$rangeDays === range.days}
                >
                    {range.label}
                </button>
            {/each}
        </div>
    </div>

    {#if error}
        <div class="error">{error}</div>
    {/if}

    {#if loading}
        <Loading />
    {:else if !stats || stats.total_ms === 0}
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
                    <path d="M3 3v18h18" />
                    <path d="m7 15 4-6 4 3 5-8" />
                </svg>
            </div>
            <p class="empty-title">No listening time in this range</p>
            <p class="empty-text">
                Stats appear after a meaningful listen. Quick previews are
                ignored.
            </p>
        </div>
    {:else}
        <div class="summary-grid">
            <div class="stat-card primary">
                <span class="stat-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="currentColor">
                        <path
                            d="M12 2a10 10 0 1 0 10 10A10 10 0 0 0 12 2Zm1 11H7v-2h4V6h2Z"
                        />
                    </svg>
                </span>
                <span class="stat-value">{minutesValue(stats.total_ms)}</span>
                <span class="stat-label">Minutes Listened</span>
                <span class="stat-detail"
                    >{plural(stats.total_plays, "meaningful listen")}</span
                >
            </div>
            <div class="stat-card">
                <span class="stat-icon" aria-hidden="true">
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <circle cx="12" cy="12" r="10" />
                        <polyline points="12 6 12 12 16 14" />
                    </svg>
                </span>
                <span class="stat-value"
                    >{stats.unique_tracks.toLocaleString()}</span
                >
                <span class="stat-label">Tracks Explored</span>
                <span class="stat-detail"
                    >{stats.discovery_tracks.toLocaleString()} first-time
                    {stats.discovery_tracks === 1 ? "spin" : "spins"}</span
                >
            </div>
            <div class="stat-card">
                <span class="stat-icon" aria-hidden="true">
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <rect
                            x="3"
                            y="4"
                            width="18"
                            height="18"
                            rx="2"
                            ry="2"
                        />
                        <line x1="16" y1="2" x2="16" y2="6" />
                        <line x1="8" y1="2" x2="8" y2="6" />
                        <line x1="3" y1="10" x2="21" y2="10" />
                    </svg>
                </span>
                <span class="stat-value">{stats.active_days}</span>
                <span class="stat-label"
                    >{stats.active_days === 1
                        ? "Active Day"
                        : "Active Days"}</span
                >
                <span class="stat-detail"
                    >{plural(stats.longest_streak_days, "day")} longest streak</span
                >
            </div>
            <div class="stat-card">
                <span class="stat-icon" aria-hidden="true">
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                    >
                        <path d="M4 19V9m5 10V5m5 14v-7m5 7V3" />
                    </svg>
                </span>
                <span class="stat-value">{stats.session_count}</span>
                <span class="stat-label">Listening Sessions</span>
                <span class="stat-detail"
                    >{stats.unique_artists.toLocaleString()}
                    {stats.unique_artists === 1 ? "artist" : "artists"}</span
                >
            </div>
        </div>

        {#if insights.length > 0}
            <section class="insights-section">
                <div class="section-heading">
                    <div>
                        <h2 class="section-title">Listening patterns</h2>
                        <p class="section-subtitle">
                            When, what, and how you listen.
                        </p>
                    </div>
                </div>
                <div class="insight-grid">
                    {#each insights as insight, i (insight.kicker)}
                        <article class="insight-card" style={`--i: ${i}`}>
                            <span class="insight-number"
                                >{String(i + 1).padStart(2, "0")}</span
                            >
                            <p class="insight-kicker">{insight.kicker}</p>
                            <h3>{insight.title}</h3>
                            <p class="insight-detail">{insight.detail}</p>
                        </article>
                    {/each}
                </div>
            </section>
        {/if}

        {#if activitySeries.length > 1}
            <section class="chart-card">
                <div class="section-heading">
                    <div>
                        <h2 class="section-title">Activity</h2>
                    </div>
                    <span class="chart-peak"
                        >{formatMinutes(maxBucketMs)} peak</span
                    >
                </div>
                <div
                    class="chart"
                    role="img"
                    aria-label="Listening minutes over time"
                >
                    {#each activitySeries as bucket, i (bucket.label)}
                        <div class="bar-wrap" title={bucketTitle(bucket)}>
                            <div
                                class="bar"
                                class:empty={bucket.ms === 0}
                                class:peak={bucket.ms === maxBucketMs &&
                                    bucket.ms > 0}
                                style:height={`${Math.max(bucket.ms > 0 ? 4 : 1, (bucket.ms / maxBucketMs) * 100)}%`}
                            ></div>
                        </div>
                    {/each}
                </div>
                <div class="chart-axis">
                    {#each activitySeries as bucket, i (bucket.label)}
                        <span class="axis-label">
                            {#if showAxisLabel(i, activitySeries.length)}
                                {bucketLabel(
                                    bucket.label,
                                    stats.activity_by_month,
                                )}
                            {/if}
                        </span>
                    {/each}
                </div>
            </section>
        {/if}

        <div class="tops-grid">
            {#if stats.top_artists.length > 0}
                <section class="tops-card">
                    <h2 class="section-title">Top artists</h2>
                    <ol class="tops-list">
                        {#each stats.top_artists as artist, i (artist.artist_id)}
                            <li>
                                <a
                                    class="top-row"
                                    href={`/artists/${artist.artist_id}`}
                                >
                                    <span
                                        class="top-meter"
                                        style:width={`${(artist.ms / topTime.artists) * 100}%`}
                                    ></span>
                                    <span
                                        class="top-rank"
                                        class:top-rank-peak={i < 3}
                                        >{i + 1}</span
                                    >
                                    <ArtistAvatar
                                        artistId={artist.artist_id}
                                        alt={artist.name}
                                        class="top-art round"
                                    />
                                    <span class="top-name ellipsis"
                                        >{artist.name}</span
                                    >
                                    <span class="top-meta"
                                        >{formatMinutes(artist.ms)} · {plural(
                                            artist.plays,
                                            "play",
                                        )}</span
                                    >
                                </a>
                            </li>
                        {/each}
                    </ol>
                </section>
            {/if}

            {#if stats.top_tracks.length > 0}
                <section class="tops-card">
                    <h2 class="section-title">Top tracks</h2>
                    <ol class="tops-list">
                        {#each stats.top_tracks as track, i (track.track_id)}
                            <li>
                                <a
                                    class="top-row"
                                    href={track.album_id
                                        ? `/albums/${track.album_id}`
                                        : undefined}
                                    aria-label={`Open album for ${track.title ?? "Unknown"}`}
                                >
                                    <span
                                        class="top-meter"
                                        style:width={`${(track.ms / topTime.tracks) * 100}%`}
                                    ></span>
                                    <span
                                        class="top-rank"
                                        class:top-rank-peak={i < 3}
                                        >{i + 1}</span
                                    >
                                    <Artwork
                                        albumId={track.album_id}
                                        alt=""
                                        class="top-art"
                                    />
                                    <span class="top-text">
                                        <span class="top-name ellipsis"
                                            >{track.title ?? "Unknown"}</span
                                        >
                                        <span class="top-sub ellipsis"
                                            >{track.artist_names.join(
                                                ", ",
                                            )}</span
                                        >
                                    </span>
                                    <span class="top-meta"
                                        >{formatMinutes(track.ms)} · {plural(
                                            track.plays,
                                            "play",
                                        )}</span
                                    >
                                </a>
                            </li>
                        {/each}
                    </ol>
                </section>
            {/if}

            {#if stats.top_albums.length > 0}
                <section class="tops-card">
                    <h2 class="section-title">Top albums</h2>
                    <ol class="tops-list">
                        {#each stats.top_albums as album, i (album.album_id)}
                            <li>
                                <a
                                    class="top-row"
                                    href={`/albums/${album.album_id}`}
                                >
                                    <span
                                        class="top-meter"
                                        style:width={`${(album.ms / topTime.albums) * 100}%`}
                                    ></span>
                                    <span
                                        class="top-rank"
                                        class:top-rank-peak={i < 3}
                                        >{i + 1}</span
                                    >
                                    <Artwork
                                        albumId={album.album_id}
                                        alt=""
                                        class="top-art"
                                    />
                                    <span class="top-text">
                                        <span class="top-name ellipsis"
                                            >{album.title}</span
                                        >
                                        <span class="top-sub ellipsis"
                                            >{album.artist_names.join(
                                                ", ",
                                            )}</span
                                        >
                                    </span>
                                    <span class="top-meta"
                                        >{formatMinutes(album.ms)} · {plural(
                                            album.plays,
                                            "play",
                                        )}</span
                                    >
                                </a>
                            </li>
                        {/each}
                    </ol>
                </section>
            {/if}
        </div>
    {/if}
</div>

<style>
    .stats-page {
        position: relative;
        display: flex;
        flex-direction: column;
        gap: clamp(var(--spacing-lg), 3vw, var(--spacing-2xl));
    }

    .header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-xl);
        flex-wrap: wrap;
    }

    .header-text {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .segment {
        white-space: nowrap;
    }

    .error {
        background-color: var(--color-error);
        color: var(--color-text);
        padding: var(--spacing-md);
        border-radius: var(--radius-lg);
        font-size: var(--font-size-sm);
    }

    .summary-grid {
        display: grid;
        grid-template-columns: 1.25fr repeat(3, minmax(0, 1fr));
        gap: var(--spacing-lg);
    }

    .stat-card {
        position: relative;
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        min-height: 10.5rem;
        padding: var(--spacing-lg);
        border-radius: var(--radius-xl);
        background: linear-gradient(
            145deg,
            color-mix(in srgb, var(--color-surface-elevated) 94%, white 6%),
            var(--color-surface)
        );
        border: 1px solid
            color-mix(in srgb, var(--color-border) 70%, transparent);
        box-shadow:
            inset 0 1px 0 rgba(255, 255, 255, 0.05),
            var(--shadow-sm);
        overflow: hidden;
        transition:
            transform var(--transition-base),
            box-shadow var(--transition-base);
    }

    .stat-card:nth-child(1) {
        background: linear-gradient(
            145deg,
            color-mix(
                in srgb,
                var(--color-accent-seed) 22%,
                var(--color-surface-elevated)
            ),
            var(--color-surface)
        );
    }

    .stat-card:nth-child(1) .stat-icon {
        background: color-mix(
            in srgb,
            var(--color-accent-seed) 28%,
            transparent
        );
    }

    .stat-card.primary .stat-value {
        color: var(--color-accent-content);
    }

    .stat-icon {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 2.25rem;
        height: 2.25rem;
        border-radius: var(--radius-full);
        background: var(--color-accent-subtle);
        color: var(--color-on-accent-subtle);
        margin-bottom: var(--spacing-sm);
    }

    .stat-icon svg {
        width: 1.125rem;
        height: 1.125rem;
    }

    .stat-value {
        font-size: clamp(2rem, 1.4rem + 2vw, 2.75rem);
        font-weight: var(--font-weight-bold);
        letter-spacing: -0.04em;
        line-height: 1;
        font-variant-numeric: tabular-nums;
    }

    .stat-label {
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-medium);
        letter-spacing: normal;
        color: var(--color-text-muted);
    }

    .stat-detail {
        margin-top: auto;
        padding-top: var(--spacing-sm);
        color: var(--color-text-secondary);
        font-size: var(--font-size-xs);
        font-variant-numeric: tabular-nums;
    }

    .insights-section {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
    }

    .insight-grid {
        display: grid;
        grid-template-columns: repeat(3, minmax(0, 1fr));
        gap: var(--spacing-md);
    }

    .insight-card {
        --card-hue: calc(var(--i) * 20deg);
        position: relative;
        display: flex;
        min-height: 13rem;
        flex-direction: column;
        justify-content: flex-end;
        padding: var(--spacing-lg);
        overflow: hidden;
        border: 1px solid
            color-mix(in srgb, var(--color-border) 65%, transparent);
        border-radius: var(--radius-xl);
        background:
            radial-gradient(
                circle at 88% 12%,
                color-mix(in srgb, var(--color-accent-seed) 22%, transparent),
                transparent 48%
            ),
            linear-gradient(
                145deg,
                color-mix(
                    in srgb,
                    var(--color-surface-elevated) 88%,
                    transparent
                ),
                var(--color-surface)
            );
        box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.05);
    }

    .insight-card:nth-child(3n + 2) {
        background:
            radial-gradient(
                circle at 88% 12%,
                color-mix(in srgb, var(--color-accent-seed) 16%, transparent),
                transparent 48%
            ),
            linear-gradient(
                145deg,
                var(--color-surface-elevated),
                var(--color-surface)
            );
    }

    .insight-card:nth-child(3n) {
        background:
            radial-gradient(
                circle at 88% 12%,
                color-mix(in srgb, var(--color-accent-seed) 10%, transparent),
                transparent 48%
            ),
            linear-gradient(
                145deg,
                var(--color-surface-elevated),
                var(--color-surface)
            );
    }

    .insight-number {
        position: absolute;
        top: var(--spacing-md);
        right: var(--spacing-md);
        color: color-mix(in srgb, var(--color-text) 12%, transparent);
        font-size: clamp(2.75rem, 6vw, 5rem);
        font-weight: var(--font-weight-bold);
        line-height: 1;
        letter-spacing: -0.08em;
    }

    .insight-kicker {
        margin-bottom: var(--spacing-xs);
        color: var(--color-accent-content);
        font-size: 0.6875rem;
        font-weight: var(--font-weight-bold);
        letter-spacing: normal;
    }

    .insight-card h3 {
        max-width: 90%;
        font-size: clamp(var(--font-size-xl), 2.4vw, var(--font-size-2xl));
        line-height: 1.05;
        letter-spacing: -0.035em;
    }

    .insight-detail {
        max-width: 30rem;
        margin-top: var(--spacing-sm);
        color: var(--color-text-secondary);
        font-size: var(--font-size-sm);
    }

    .chart-card,
    .tops-card {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
        padding: clamp(var(--spacing-lg), 3vw, var(--spacing-xl));
        border-radius: var(--radius-xl);
        background: linear-gradient(
            145deg,
            color-mix(in srgb, var(--color-surface-elevated) 62%, transparent),
            color-mix(in srgb, var(--color-surface) 86%, transparent)
        );
        border: 1px solid
            color-mix(in srgb, var(--color-border) 72%, transparent);
        box-shadow:
            inset 0 1px 0 rgba(255, 255, 255, 0.04),
            0 10px 32px rgba(0, 0, 0, 0.08);
        min-width: 0;
    }

    .section-heading {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: var(--spacing-md);
    }

    .section-subtitle {
        margin-top: var(--spacing-xs);
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
    }

    .chart-peak {
        flex-shrink: 0;
        padding: var(--spacing-xs) var(--spacing-sm);
        border-radius: var(--radius-full);
        background: var(--color-accent-subtle);
        color: var(--color-on-accent-subtle);
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-semibold);
        font-variant-numeric: tabular-nums;
    }

    .chart {
        display: flex;
        align-items: flex-end;
        gap: 4px;
        height: clamp(10rem, 24vw, 14rem);
        padding-top: var(--spacing-sm);
    }

    .bar-wrap {
        flex: 1;
        min-width: 2px;
        height: 100%;
        display: flex;
        align-items: flex-end;
    }

    .bar {
        width: 100%;
        border-radius: 4px 4px 2px 2px;
        background: linear-gradient(
            to top,
            var(--color-accent-graphic),
            var(--color-accent-content)
        );
        transition:
            height var(--transition-slow),
            filter var(--transition-fast);
    }

    .bar.peak {
        background: linear-gradient(
            to top,
            var(--color-accent-graphic),
            var(--color-accent-content)
        );
        box-shadow: 0 0 16px
            color-mix(in srgb, var(--color-accent-seed) 45%, transparent);
    }

    .bar.empty {
        background: var(--color-surface-raised);
    }

    .bar-wrap:hover .bar {
        filter: brightness(1.25);
    }

    .chart-axis {
        display: flex;
        gap: 4px;
        border-top: 1px solid var(--color-border);
        padding-top: var(--spacing-sm);
    }

    .axis-label {
        flex: 1;
        min-width: 2px;
        font-size: 0.625rem;
        color: var(--color-text-muted);
        text-align: center;
        white-space: nowrap;
        overflow: visible;
    }

    .tops-grid {
        display: grid;
        grid-template-columns: repeat(3, minmax(0, 1fr));
        gap: var(--spacing-lg);
        align-items: start;
    }

    .tops-list {
        display: flex;
        flex-direction: column;
        gap: 2px;
    }

    .top-row {
        position: relative;
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
        padding: var(--spacing-sm) var(--spacing-xs);
        border-radius: var(--radius-lg);
        overflow: hidden;
        isolation: isolate;
        transition: background-color var(--transition-fast);
    }

    .top-row:hover {
        background-color: var(--color-surface-elevated);
    }

    a.top-row:focus-visible {
        outline: 2px solid var(--color-accent-focus);
        outline-offset: 2px;
    }

    .top-meter {
        position: absolute;
        left: 0;
        top: 0;
        bottom: 0;
        z-index: -1;
        border-radius: var(--radius-lg);
        background: color-mix(in srgb, var(--color-text) 6%, transparent);
    }

    .top-rank {
        width: 1.5rem;
        flex-shrink: 0;
        font-size: var(--font-size-lg);
        font-weight: var(--font-weight-bold);
        color: var(--color-text-muted);
        text-align: center;
        font-variant-numeric: tabular-nums;
        letter-spacing: -0.02em;
    }

    .top-rank-peak {
        color: var(--color-accent-content);
        text-shadow: 0 0 14px
            color-mix(in srgb, var(--color-accent-seed) 45%, transparent);
    }

    .tops-list :global(.top-art) {
        width: 3rem;
        height: 3rem;
        flex-shrink: 0;
        border-radius: var(--radius-sm);
        object-fit: cover;
        overflow: hidden;
        box-shadow: var(--shadow-sm);
    }

    .tops-list :global(.top-art.round) {
        border-radius: var(--radius-full);
    }

    .top-text {
        display: flex;
        flex-direction: column;
        gap: 1px;
        min-width: 0;
        flex: 1;
    }

    .top-name {
        font-size: var(--font-size-sm);
        font-weight: var(--font-weight-semibold);
        color: var(--color-text);
        min-width: 0;
        flex: 1;
    }

    .top-text .top-name {
        flex: none;
    }

    .top-sub {
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
    }

    .top-meta {
        flex-shrink: 0;
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
        font-variant-numeric: tabular-nums;
        white-space: nowrap;
    }

    @media (max-width: 960px) {
        .summary-grid,
        .insight-grid,
        .tops-grid {
            grid-template-columns: repeat(2, minmax(0, 1fr));
        }
    }

    @media (max-width: 640px) {
        .summary-grid,
        .insight-grid,
        .tops-grid {
            grid-template-columns: 1fr;
        }

        .header {
            align-items: flex-start;
        }

        .range-segment {
            width: 100%;
            justify-content: space-between;
        }

        .segment {
            flex: 1;
            padding-inline: var(--spacing-sm);
        }
    }
</style>
