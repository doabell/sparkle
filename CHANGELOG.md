# Changelog

## 0.3.0 — 2026-08-24

- Add Sound Check loudness normalization with scan, rescan, progress, and library analysis controls.
- Reorganize Settings with consistent controls, concise copy, and clearer Diagnostics and Licenses sections.
- Improve playback recovery and add richer diagnostics and logging for playback failures.
- Fix synchronized lyrics behavior and isolate development data from production libraries.
- Upgrade application and development dependencies.

## 0.2.0 — 2026-07-25

- Add configurable S3-compatible storage for Discord artwork, with settings UI, access testing, and environment-variable fallback.
- Preserve Discord artwork cache entries across restarts and between Catbox and S3 storage backends.
- Improve artwork upload safety, playback controls, library scanning, database writes, lyrics/cache behavior, and command-palette interactions.
- Add accessible semantic accent themes with light/dark foreground preferences and regression coverage.

## 0.1.0 — 2026-07-21

First public preview.

- Local-library browsing, search, playlists, queue, and native playback.
- Lyrics, custom artwork, artist details, and optional online providers.
- Minutes-first listening stats with richer listening patterns.
- Library health checks for metadata, artwork, formats, and audio quality.
- Compressed, selective backup and restore using stable library IDs.
- Refined Apple Music-style interface across the app.
