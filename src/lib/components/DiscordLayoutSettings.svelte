<script lang="ts">
    import type { DiscordLayout } from "$lib/api";
    import Select from "$lib/components/Select.svelte";
    import {
        defaultDiscordLayout,
        renderDiscordTemplate,
    } from "$lib/utils/discord";

    let { layout = $bindable() }: { layout: DiscordLayout } = $props();
    const fields = [
        { key: "details", label: "First line" },
        { key: "state", label: "Second line" },
        { key: "image_text", label: "Artwork text (hover)" },
    ] as const;
    const statusOptions = [
        { value: "name", label: "App name" },
        { value: "details", label: "First line" },
        { value: "state", label: "Second line" },
    ];
    const example = {
        title: "Midnight drive",
        artist: "Sample artist",
        album: "After hours",
        lyrics: "Stay here until the morning",
    };
    let details = $derived(renderDiscordTemplate(layout.details, example));
    let state = $derived(renderDiscordTemplate(layout.state, example));
    let imageText = $derived(renderDiscordTemplate(layout.image_text, example));
    let name = $derived(
        renderDiscordTemplate("{title}", {
            ...example,
            title: layout.name.trim() || "Sparkle",
        }),
    );
    let status = $derived(
        (layout.status_display === "details"
            ? details
            : layout.status_display === "state"
              ? state
              : name) || name,
    );

    function useLyrics() {
        layout.details = "{title} — {artist}";
        layout.state = "{lyrics}";
        layout.image_text = "{lyrics}";
    }
</script>

<div class="layout-settings">
    <div class="heading">
        <h3>Presence layout</h3>
        <div class="presets">
            <button
                type="button"
                class="btn-pill btn-secondary"
                onclick={useLyrics}>Use live lyrics</button
            >
            <button
                type="button"
                class="btn-pill btn-secondary"
                onclick={() => (layout = defaultDiscordLayout())}
                >Reset layout</button
            >
        </div>
    </div>
    <label class="field" for="discord-display-name">
        <span>App name</span>
        <input
            id="discord-display-name"
            type="text"
            bind:value={layout.name}
            placeholder="Sparkle"
            maxlength="128"
        />
    </label>
    <div class="field">
        <span id="discord-status-label">“Listening to” status</span>
        <Select
            options={statusOptions}
            value={layout.status_display}
            onchange={(value) => (layout.status_display = value)}
            ariaLabel="Listening to status"
        />
    </div>
    {#each fields as field}
        <label class="field" for={`discord-layout-${field.key}`}>
            <span>{field.label}</span>
            <input
                id={`discord-layout-${field.key}`}
                type="text"
                bind:value={layout[field.key]}
                maxlength="512"
                spellcheck="false"
                aria-describedby="discord-template-help"
            />
        </label>
    {/each}
    <p class="hint" id="discord-template-help">
        Combine your own text with <code>{"{title}"}</code>,
        <code>{"{artist}"}</code>, <code>{"{album}"}</code>, or
        <code>{"{lyrics}"}</code>. Leave a line blank to hide it. Discord text
        is limited to 128 bytes.
    </p>
    <div class="toggles">
        <label
            ><input type="checkbox" bind:checked={layout.show_artwork} /> Show album
            cover</label
        >
        <label
            ><input type="checkbox" bind:checked={layout.show_progress} /> Show progress
            bar</label
        >
    </div>
    <p class="hint">
        Lyrics refresh about every 15 seconds and may skip short lines. Album
        text is used until a synced line is available. Discord may still show
        the app icon when the cover is hidden.
    </p>
    <div class="preview" aria-label="Discord presence preview">
        <span class="preview-label"
            >Sample preview · Discord’s layout may vary</span
        >
        <p class="status">Listening to {status}</p>
        <div class="preview-body">
            {#if layout.show_artwork}
                <div
                    class="cover"
                    title={imageText}
                    aria-label={imageText || "Sample album cover"}
                >
                    ♫
                </div>
            {/if}
            <div class="preview-text">
                <strong>{name}</strong>
                {#if details}<p>{details}</p>{/if}
                {#if state}<p>{state}</p>{/if}
                {#if layout.show_progress}<div
                        class="progress"
                        aria-label="Sample playback progress"
                    >
                        <span></span>
                    </div>{/if}
            </div>
        </div>
        {#if layout.show_artwork && imageText}<p class="hover-text">
                On cover hover: {imageText}
            </p>{/if}
    </div>
</div>

<style>
    .layout-settings {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
    }
    .heading {
        display: flex;
        align-items: center;
        justify-content: space-between;
        flex-wrap: wrap;
        gap: var(--spacing-sm);
    }
    h3 {
        font-size: var(--font-size-base);
    }
    .presets,
    .toggles {
        display: flex;
        flex-wrap: wrap;
        gap: var(--spacing-md);
    }
    .field {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        font-size: var(--font-size-sm);
    }
    .field input {
        width: 100%;
    }
    .hint,
    .hover-text,
    .preview-label {
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
        line-height: 1.5;
    }
    .hint {
        margin: 0;
    }
    code {
        color: var(--color-text-secondary);
    }
    .toggles label {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        font-size: var(--font-size-sm);
    }
    .preview {
        border: 1px solid var(--color-border);
        border-radius: var(--radius);
        padding: var(--spacing-md);
        min-width: 0;
    }
    .status {
        font-size: var(--font-size-sm);
        margin: var(--spacing-sm) 0;
        overflow-wrap: anywhere;
    }
    .preview-body {
        display: flex;
        gap: var(--spacing-md);
        align-items: center;
    }
    .cover {
        display: grid;
        place-items: center;
        width: 72px;
        height: 72px;
        flex: 0 0 72px;
        border-radius: var(--radius);
        background: var(--color-accent-subtle);
        color: var(--color-on-accent-subtle);
        font-size: 2rem;
    }
    .preview-text {
        flex: 1;
        min-width: 0;
        font-size: var(--font-size-sm);
        overflow-wrap: anywhere;
    }
    .preview-text p {
        margin: 0.2rem 0;
    }
    .progress {
        height: 4px;
        margin-top: var(--spacing-sm);
        border-radius: 2px;
        background: var(--color-border);
        overflow: hidden;
    }
    .progress span {
        display: block;
        width: 40%;
        height: 100%;
        background: var(--color-text-secondary);
    }
    .hover-text {
        margin: var(--spacing-sm) 0 0;
        overflow-wrap: anywhere;
    }
</style>
