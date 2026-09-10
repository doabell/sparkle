# Changelog

## 0.5.0 — 2026-09-10

- Replace the Windows MSI with a Velopack setup that creates a Start menu shortcut and no desktop shortcut.
- Add manual update checks, downloads, and restart-to-install actions in Settings → About.
- Include complete offline dependency licenses and third-party notices in Settings and Windows packages.
- Preserve track identities, playlists, custom metadata, and listening history when music files move, change tags, or disappear temporarily.
- Add lyrics editing and export, and improve synchronized lyrics parsing, offsets, seeking, and refresh after scans.
- Improve artist credits and artwork search with Deezer and Wikimedia providers.
- Unify interface motion and hover feedback, respect reduced motion, and improve light-theme contrast and artwork backdrops.
- Simplify backup, update, and license settings.
- Optimize release builds with LTO; build and verify Windows setup/update artifacts on main and release tags, with faster PR checks and shared build caches.

## 0.4.0 — 2026-09-05

- Add alternate now-playing layouts with a unified Apple Music-inspired visual system across the app.
- Improve synchronized lyrics timing, transitions, and playback state recovery.
- Add artist credits, lyrics offset controls, custom page scrollbars, and refined window controls.
- Expand TypeScript and Rust test coverage with enforced coverage baselines and backend integration tests.

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
