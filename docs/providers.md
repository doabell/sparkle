# Metadata and artwork providers

Enable and reorder sources in **Settings → Library → Online sources**. Automatic lookups try
enabled sources in order. Local selections are saved as Custom.

| Feature          | Default source order                                             |
| ---------------- | ---------------------------------------------------------------- |
| Lyrics           | Custom, Embedded, LRC file, LRCLIB, NetEase, Kashinavi, QQ Music |
| Artist biography | Custom, Wikipedia (English)                                      |
| Artist image     | Custom, Deezer, Wikipedia (English)                              |
| Album cover      | Custom, Embedded, Cover Art Archive                              |

Brave image search is optional and requires an API key. Deezer artist search
needs no key and prefers the largest available portrait, excluding placeholders.
Wikipedia supports language-specific sources.

## Manual search

Lyrics results open a text preview before **Use Lyrics** saves a selection.
Artist image results open a crop view before **Use Image**. Artist Image and
Biography save independently. **Choose File** is available with Custom selected.

Artist image search queries enabled online sources concurrently with a bounded
timeout. The UI distinguishes empty results, provider errors, and partial
results; expand provider details to inspect failures. A failed source does not
discard another source's results.

If a lookup fails, check the enabled sources, search terms, network connection,
and any required API key. Retry or choose another source or a local file.
Use [Debug logs](logging.md) for provider names, failure reasons, and timeouts;
an empty result alone does not indicate a network failure.
