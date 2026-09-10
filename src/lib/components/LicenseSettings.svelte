<script lang="ts">
    import sparkleLicense from "../../../LICENSE?raw";
    import thirdPartyNotices from "../../../THIRD_PARTY_NOTICES.md?raw";
    import apacheLicense from "../../../licenses/Apache-2.0.txt?raw";
    import dependencies from "../../../licenses/dependencies.json";

    let search = $state("");
    const licenses = dependencies.licenses;
    let filtered = $derived(
        licenses.filter((entry) => {
            const query = search.trim().toLowerCase();
            return (
                !query ||
                entry.license.toLowerCase().includes(query) ||
                entry.components.some((component) =>
                    component.name.toLowerCase().includes(query),
                )
            );
        }),
    );
</script>

<div class="license-settings">
    <h3>Licenses &amp; third-party notices</h3>
    <details>
        <summary>Sparkle <span class="badge">MIT</span></summary>
        <a
            href="https://github.com/doabell/sparkle"
            target="_blank"
            rel="noreferrer"
        >
            Source on GitHub ↗
        </a>
        <pre>{sparkleLicense}</pre>
    </details>
    <details>
        <summary>Adapted projects &amp; attribution notices</summary>
        <a
            href="https://github.com/doabell/sparkle/blob/main/THIRD_PARTY_NOTICES.md"
            target="_blank"
            rel="noreferrer">Project links and notices on GitHub ↗</a
        >
        <pre>{thirdPartyNotices}</pre>
        <details class="nested-license">
            <summary>Apache License 2.0</summary>
            <pre>{apacheLicense}</pre>
        </details>
    </details>

    <div class="dependency-heading">
        <h4>Dependencies</h4>
        <label for="license-search">Find a library or license</label>
        <input
            id="license-search"
            type="search"
            bind:value={search}
            placeholder="Search dependencies…"
        />
    </div>
    <p class="count" role="status">
        {filtered.length} license and notice entries
    </p>
    <div class="dependency-list">
        {#each filtered as entry (entry.key)}
            <details>
                <summary>
                    <span class="names">
                        {entry.components
                            .slice(0, 3)
                            .map((component) => component.name)
                            .join(", ")}
                        {#if entry.components.length > 3}
                            <span class="more">
                                +{entry.components.length - 3}</span
                            >
                        {/if}
                    </span>
                    <span class="badge">{entry.license}</span>
                </summary>
                <ul>
                    {#each entry.components as component}
                        <li>
                            <a
                                href={component.source}
                                target="_blank"
                                rel="noreferrer"
                            >
                                {component.name}
                                {component.version} ↗
                            </a>
                            <span class="ecosystem">{component.ecosystem}</span>
                        </li>
                    {/each}
                </ul>
                <pre>{entry.text}</pre>
            </details>
        {/each}
        {#if !filtered.length}
            <p class="hint">No matching libraries or licenses.</p>
        {/if}
    </div>
</div>

<style>
    .license-settings {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
        min-width: 0;
    }
    h3,
    h4 {
        font-size: var(--font-size-base);
        font-weight: var(--font-weight-semibold);
    }
    p {
        margin: 0;
    }
    .hint {
        color: var(--color-text-muted);
        font-size: var(--font-size-sm);
        line-height: 1.6;
    }
    details {
        padding: var(--spacing-sm) 0;
        border-bottom: 1px solid var(--color-border);
        min-width: 0;
    }
    summary {
        cursor: pointer;
        font-size: var(--font-size-sm);
        line-height: 1.8;
        overflow-wrap: anywhere;
    }
    summary:hover {
        color: var(--color-accent-content);
    }
    .badge {
        display: inline-block;
        margin-left: var(--spacing-sm);
        padding: 0 var(--spacing-sm);
        border-radius: var(--radius-full);
        background: var(--color-surface-raised);
        color: var(--color-text-secondary);
        font-size: var(--font-size-xs);
        vertical-align: middle;
    }
    .more,
    .ecosystem {
        color: var(--color-text-muted);
        font-size: var(--font-size-xs);
    }
    .dependency-heading {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
        margin-top: var(--spacing-sm);
    }
    label {
        font-size: var(--font-size-sm);
    }
    input {
        width: min(100%, 28rem);
        padding: var(--spacing-sm) var(--spacing-md);
        background: var(--color-surface);
        color: var(--color-text);
        border: 1px solid var(--color-border);
        border-radius: var(--radius);
        font: inherit;
        font-size: var(--font-size-sm);
    }
    .count {
        font-size: var(--font-size-xs);
        color: var(--color-text-muted);
    }
    ul {
        display: grid;
        gap: var(--spacing-xs);
        padding: var(--spacing-sm) 0;
        list-style: none;
    }
    li {
        display: flex;
        flex-wrap: wrap;
        gap: var(--spacing-sm);
        align-items: baseline;
    }
    a {
        display: inline-block;
        margin-top: var(--spacing-xs);
        color: var(--color-accent-content);
        font-size: var(--font-size-sm);
        overflow-wrap: anywhere;
    }
    a:hover {
        text-decoration: underline;
    }
    pre {
        max-height: 26rem;
        overflow-y: auto;
        margin: var(--spacing-md) 0;
        padding: var(--spacing-md);
        border-radius: var(--radius);
        background: var(--color-surface);
        color: var(--color-text-secondary);
        white-space: pre-wrap;
        overflow-wrap: anywhere;
        font-family: var(--font-family);
        font-size: var(--font-size-sm);
        line-height: 1.7;
    }
    .nested-license {
        border-bottom: 0;
    }
</style>
