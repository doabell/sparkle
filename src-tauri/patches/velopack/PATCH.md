# Velopack 1.2.161

Source: the crates.io `velopack` 1.2.161 package, with its MIT license from
<https://github.com/velopack/velopack/blob/1.2.161/LICENSE>.

The ZIP dependency uses `zip` 9.1.3 instead of 8.6 to fix
[GHSA-m665-v7pm-7grj](https://github.com/advisories/GHSA-m665-v7pm-7grj).
`src/bundle.rs` handles the new fallible filename API, propagating invalid-name
errors during extraction and skipping invalid names during optional searches.
The former Windows bindings patch is no longer needed: the updater, CPAL, and
Tauri now use `windows` 0.62 upstream.

Keep the SDK and the pinned `vpk` CLI at the same version. Remove this patch
when upstream requires a fixed ZIP release (9.0.5 or later).
