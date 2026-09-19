<script lang="ts">
    import { tick } from "svelte";
    import {
        getDiscordPreview,
        type DiscordLayout,
        type DiscordPreview,
    } from "$lib/api";
    import { playback, interpolatedPositionMs } from "$lib/stores/playback";
    import { cachedImageToUrl } from "$lib/utils/base64";
    import { formatTime } from "$lib/utils/formatTime";
    import Select from "$lib/components/Select.svelte";
    import {
        defaultDiscordLayout,
        discordTemplateFields,
        discordPreviewValues,
        renderDiscordTemplate,
    } from "$lib/utils/discord";

    let {
        layout = $bindable(),
        active = true,
    }: { layout: DiscordLayout; active?: boolean } = $props();
    const labels = {
        name: "Card name",
        details: "Title",
        state: "Subtitle",
        image_text: "Cover text",
    } as const;
    type EditableField = keyof typeof labels;
    let activeField = $state<EditableField>("details");
    let editor: HTMLInputElement;
    const statusOptions = [
        { value: "name", label: "Card name" },
        { value: "details", label: "Title" },
        { value: "state", label: "Subtitle" },
    ];
    let preview = $state<DiscordPreview | null>(null);
    let trackId = $derived($playback.current_track?.id ?? null);
    let currentPreview = $derived(
        preview?.track_id === trackId ? preview : null,
    );
    let values = $derived(
        discordPreviewValues($playback.current_track, currentPreview),
    );
    let details = $derived(renderDiscordTemplate(layout.details, values));
    let subtitle = $derived(renderDiscordTemplate(layout.state, values));
    let imageText = $derived(renderDiscordTemplate(layout.image_text, values));
    let artwork = $derived(
        currentPreview ? currentPreview.artwork : $playback.album_art,
    );
    let coverUrl = $derived(
        artwork ? cachedImageToUrl(artwork, "/sparkle.svg") : "/sparkle.svg",
    );
    let failedCover = $state("");
    let displayedCover = $derived(
        failedCover === coverUrl ? "/sparkle.svg" : coverUrl,
    );
    let duration = $derived(
        trackId === null
            ? 215_000
            : Math.max(
                  $playback.duration_ms,
                  $playback.current_track?.duration_ms ?? 0,
              ),
    );
    let position = $derived(
        trackId === null
            ? 30_000
            : Math.max(0, Math.min($interpolatedPositionMs, duration)),
    );
    let progress = $derived(duration > 0 ? (position / duration) * 100 : 0);

    $effect(() => {
        const id = trackId;
        if (!active || id === null) return;
        let cancelled = false;
        let pending = false;
        async function refresh() {
            if (pending || document.hidden) return;
            pending = true;
            try {
                const result = await getDiscordPreview(id!);
                if (!cancelled) preview = result;
            } catch {
                // Keep basic current-track text available and retry on the next tick.
            } finally {
                pending = false;
            }
        }
        void refresh();
        const timer = setInterval(refresh, 1000);
        return () => {
            cancelled = true;
            clearInterval(timer);
        };
    });
    let name = $derived(
        renderDiscordTemplate("{title}", {
            title: layout.name.trim() || "Sparkle",
        }),
    );
    let status = $derived(
        (layout.status_display === "details"
            ? details
            : layout.status_display === "state"
              ? subtitle
              : name) || name,
    );

    async function selectField(field: EditableField) {
        activeField = field;
        await tick();
        editor.focus();
    }

    async function insertField(key: string) {
        const value = layout[activeField];
        const start = editor.selectionStart ?? value.length;
        const end = editor.selectionEnd ?? start;
        const token = `{${key}}`;
        const next = value.slice(0, start) + token + value.slice(end);
        if (next.length > 512) return;
        layout[activeField] = next;
        await tick();
        editor.focus();
        editor.setSelectionRange(start + token.length, start + token.length);
    }
</script>

<div class="layout-settings">
    <div class="heading">
        <h3>Presence layout</h3>
        <button
            type="button"
            class="btn-pill btn-secondary"
            onclick={() => (layout = defaultDiscordLayout())}>Default</button
        >
    </div>
    <div class="preview" role="group" aria-label="Discord presence preview">
        <div class="activity-heading">
            <span>Listening to</span>
            <button
                type="button"
                class="preview-field app-name"
                class:selected={activeField === "name"}
                aria-label="Edit card name"
                aria-pressed={activeField === "name"}
                onclick={() => selectField("name")}>{name}</button
            >
        </div>
        <div class="preview-body">
            {#if layout.show_artwork}
                <button
                    type="button"
                    class="cover"
                    title={imageText}
                    aria-label="Edit cover text"
                    onclick={() => selectField("image_text")}
                >
                    <img
                        src={displayedCover}
                        alt=""
                        class:fallback={displayedCover === "/sparkle.svg"}
                        onerror={() => (failedCover = coverUrl)}
                    />
                </button>
            {/if}
            <div class="preview-text">
                {#each [{ key: "details", text: details }, { key: "state", text: subtitle }, ...(layout.show_artwork ? [{ key: "image_text", text: imageText }] : [])] as row}
                    {@const field = row.key as EditableField}
                    <button
                        type="button"
                        class="preview-field"
                        class:track-title={field === "details"}
                        class:empty={!row.text}
                        class:selected={activeField === field}
                        aria-label={`Edit ${labels[field].toLowerCase()}`}
                        aria-pressed={activeField === field}
                        onclick={() => selectField(field)}
                        >{row.text || labels[field]}</button
                    >
                {/each}
                {#if layout.show_progress && duration > 0}
                    <div class="playback" aria-label="Playback progress">
                        <span>{formatTime(position)}</span>
                        <div class="progress">
                            <span style:width={`${progress}%`}></span>
                        </div>
                        <span>{formatTime(duration)}</span>
                    </div>
                {/if}
            </div>
        </div>
    </div>
    <label class="field" for="discord-layout-editor">
        <span>{labels[activeField]}</span>
        <input
            id="discord-layout-editor"
            bind:this={editor}
            bind:value={layout[activeField]}
            type="text"
            maxlength={activeField === "name" ? 128 : 512}
            placeholder={activeField === "name" ? "Sparkle" : "Hidden"}
            spellcheck="false"
        />
    </label>
    {#if activeField !== "name"}
        <div class="tokens" role="group" aria-label="Insert metadata">
            {#each discordTemplateFields as field}
                <button
                    type="button"
                    class="token"
                    title={`{${field.key}}`}
                    onclick={() => insertField(field.key)}>{field.label}</button
                >
            {/each}
        </div>
    {/if}
    <div class="options">
        <div class="field status-field">
            <span id="discord-status-label">Under avatar</span>
            <div role="group" aria-labelledby="discord-status-label">
                <Select
                    options={statusOptions}
                    value={layout.status_display}
                    onchange={(value) => (layout.status_display = value)}
                    ariaLabel="Status under avatar"
                />
            </div>
            <span class="member-status"
                ><span aria-hidden="true">🎵</span> {status}</span
            >
        </div>
        <div class="toggles">
            <label
                ><input type="checkbox" bind:checked={layout.show_artwork} /> Album
                cover</label
            >
            <label
                ><input type="checkbox" bind:checked={layout.show_progress} /> Progress
                bar</label
            >
        </div>
    </div>
</div>

<style>
    .layout-settings {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
    }
    .heading,
    .options {
        display: flex;
        align-items: center;
        justify-content: space-between;
        flex-wrap: wrap;
        gap: var(--spacing-sm);
    }
    h3 {
        font-size: var(--font-size-base);
    }
    .tokens,
    .toggles {
        display: flex;
        flex-wrap: wrap;
        gap: var(--spacing-sm);
    }
    .field {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        font-size: var(--font-size-sm);
        min-width: 0;
    }
    .field input {
        width: 100%;
    }
    .member-status {
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
    }
    .member-status {
        overflow-wrap: anywhere;
    }
    .token {
        border: 1px solid var(--color-border);
        border-radius: var(--radius);
        background: var(--color-surface-elevated);
        color: var(--color-text-secondary);
        padding: 0.35rem 0.65rem;
        font-size: var(--font-size-xs);
        cursor: pointer;
    }
    .token:hover {
        border-color: var(--color-text-muted);
        color: var(--color-text);
    }
    .status-field {
        flex: 1 1 180px;
        max-width: 300px;
    }
    .toggles {
        flex-direction: column;
    }
    .toggles label {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        font-size: var(--font-size-sm);
    }
    .preview {
        border-radius: 16px;
        padding: 18px;
        background: #1c1c1f;
        color: #dbdce0;
        min-width: 0;
    }
    .activity-heading {
        display: flex;
        align-items: baseline;
        gap: 0;
        font-size: 13px;
        margin-bottom: 12px;
    }
    .activity-heading > span {
        flex-shrink: 0;
    }
    .preview-body {
        display: flex;
        gap: 14px;
        align-items: flex-start;
    }
    .cover {
        display: grid;
        place-items: center;
        width: 80px;
        height: 80px;
        flex: 0 0 80px;
        border: 0;
        border-radius: 10px;
        background: #25252a;
        overflow: hidden;
        cursor: pointer;
    }
    .cover img {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }
    .cover img.fallback {
        padding: 14px;
        object-fit: contain;
    }
    .preview-text {
        flex: 1;
        min-width: 0;
    }
    .preview-field {
        display: block;
        max-width: 100%;
        border: 1px solid transparent;
        border-radius: 4px;
        padding: 1px 4px;
        margin-left: -5px;
        background: transparent;
        color: inherit;
        font: inherit;
        font-size: 15px;
        line-height: 1.4;
        text-align: left;
        overflow-wrap: anywhere;
        cursor: pointer;
    }
    .preview-field:hover,
    .preview-field.selected {
        background: #ffffff0d;
        border-color: #ffffff36;
    }
    .preview-field:focus-visible,
    .cover:focus-visible {
        outline: 2px solid #aeb9ff;
        outline-offset: 2px;
    }
    .app-name {
        font-size: inherit;
        margin-left: 0;
        padding-inline: 0.25em 0;
        border: 0;
    }
    .track-title {
        font-size: 17px;
        font-weight: 700;
        color: #f2f3f5;
    }
    .empty {
        font-style: italic;
        color: #a9aab0;
    }
    .playback {
        display: flex;
        align-items: center;
        gap: 9px;
        margin-top: 9px;
        font-size: 12px;
        font-variant-numeric: tabular-nums;
    }
    .progress {
        flex: 1;
        height: 4px;
        border-radius: 2px;
        background: #38383d;
        overflow: hidden;
        min-width: 12px;
    }
    .progress span {
        display: block;
        height: 100%;
        background: #dbdce0;
        border-radius: inherit;
    }
    @media (max-width: 420px) {
        .cover {
            width: 60px;
            height: 60px;
            flex-basis: 60px;
        }
        .preview {
            padding: 14px;
        }
    }
</style>
