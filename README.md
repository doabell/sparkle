# Sparkle

[![PR checks](https://github.com/doabell/sparkle/actions/workflows/ci.yml/badge.svg)](https://github.com/doabell/sparkle/actions/workflows/ci.yml)
[![Coverage](https://github.com/doabell/sparkle/actions/workflows/coverage.yml/badge.svg?branch=main)](https://github.com/doabell/sparkle/actions/workflows/coverage.yml)
[![MIT License](https://img.shields.io/github/license/doabell/sparkle)](LICENSE)
[![Latest release](https://img.shields.io/github/v/release/doabell/sparkle)](https://github.com/doabell/sparkle/releases/latest)

Sparkle is a local-first Windows music player for your own library. It pairs native playback with a polished, album-focused interface and useful listening insights.

> [!CAUTION]
> Sparkle is highly personalized. It works for me; it might not work for you.

## Highlights

- Browse songs, albums, artists, genres, playlists, and search results.
- Native queue, shuffle, repeat, media-key, lyrics, and artwork support.
- Minutes-first listening stats with habits and patterns—not just play counts.
- Library health checks for formats, metadata, artwork, and audio quality.
- Compressed `.sparklebackup` exports with a preview and selective restore.
- Optional online metadata, artwork, and Discord presence integrations.

## Privacy

Your library database, cache, and listening history stay on your computer. Online providers are contacted only for features you configure or invoke. Backups do not contain music files, API keys, provider tokens, or music-folder paths.

## Discord artwork storage

New Discord artwork uploads can use any S3-compatible object store. Configure
the explicit Artwork storage mode in Settings → Discord Rich Presence:
Disabled, Catbox, or S3-compatible storage. The endpoint and bucket are required;
the public URL is useful when objects are served through a CDN or custom public
domain. Authenticated stores can use an access key, secret key, and optional
session token. Region defaults to `us-east-1`, and the object prefix defaults
to `sparkle/`. Credentials are stored locally and excluded from backups.

For deployments that launch Sparkle with a preconfigured environment, the
equivalent `SPARKLE_ARTWORK_S3_ENDPOINT`, `SPARKLE_ARTWORK_S3_BUCKET`,
`SPARKLE_ARTWORK_S3_PUBLIC_URL`, `SPARKLE_ARTWORK_S3_ACCESS_KEY`,
`SPARKLE_ARTWORK_S3_SECRET_KEY`, `SPARKLE_ARTWORK_S3_SESSION_TOKEN`,
`SPARKLE_ARTWORK_S3_REGION`, and `SPARKLE_ARTWORK_S3_PREFIX` variables remain
supported when all S3 Settings fields are empty.

The Settings test action performs a real list/access check and uploads a small
test object, which it leaves in the selected store.

In S3 mode, the Discord worker lists that prefix once, uses the existing object
named from the artwork's content hash when available, and uploads a
deterministic `<hash>.jpg` only when it is missing. In Catbox mode, the exact
URL returned by Catbox is preserved. The v8 artwork cache keeps independent
Catbox and S3 URLs for each artwork key, so switching stores does not overwrite
either provider's cached filename or cause a repeat upload.

## Install

Sparkle targets Windows 10/11 x64. Download the `Setup.exe` from [GitHub Releases](https://github.com/doabell/sparkle/releases). The Velopack setup creates a Start menu shortcut and no desktop shortcut, and installs WebView2 if needed. This preview is not code-signed, so Windows may show an unknown-publisher warning.

For an existing MSI installation, close Sparkle, uninstall the MSI, then run the new setup. Sparkle keeps the same app-data location, so the library and settings remain available. MSI installations cannot use the new updater directly.

In **Settings → About**, choose **Check for updates**, then **Download update**, then **Restart to install**. Each step is manual. Sparkle does not check on launch or install a downloaded update on a later launch. Only published stable GitHub releases are offered.

Other platforms are not released or tested yet.

## Develop

Install the [Tauri prerequisites for Windows](https://v2.tauri.app/start/prerequisites/), Bun 1.3.14, and Rust 1.89. Then run:

```sh
bun install --frozen-lockfile
bun run tauri dev
```

Useful checks:

```sh
bun run version:check
bun run format:check
bun run check
bun run test
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

Run `bun run test:coverage` for the TypeScript and Rust coverage gates.
See [Tests and coverage](docs/testing.md) for setup, scope, thresholds, and reports.

To build the Windows setup and update feed, install .NET SDK 8 and restore the pinned Velopack CLI:

```sh
dotnet tool restore
bun run package:windows
```

Packages are written to `.tmp/releases`. The `v*` tag workflow verifies that the tag belongs to `main` and matches the app version, then creates a **draft** GitHub release with the setup, full update package, and `releases.win-x64.json` feed. Publish that draft when ready; retain the original asset names. PR checks also build and verify these packages and upload a test artifact. See [Windows releases](docs/windows-releases.md) for release and upgrade testing.

## License

[MIT](LICENSE)

Complete dependency license texts and third-party attribution notices are available offline in **Settings → About** and included in the installed files. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and [licenses/dependencies.json](licenses/dependencies.json).
