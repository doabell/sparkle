# Sparkle

[![CI](https://github.com/doabell/sparkle/actions/workflows/ci.yml/badge.svg)](https://github.com/doabell/sparkle/actions/workflows/ci.yml)
[![Coverage](https://github.com/doabell/sparkle/actions/workflows/coverage.yml/badge.svg?branch=main)](https://github.com/doabell/sparkle/actions/workflows/coverage.yml)
[![MIT License](https://img.shields.io/github/license/doabell/sparkle)](LICENSE)
[![Latest release](https://img.shields.io/github/v/release/doabell/sparkle)](https://github.com/doabell/sparkle/releases/latest)

Sparkle is a local-first Windows music player with native playback, an album-focused interface, and listening insights.

> [!CAUTION]
> Sparkle is highly personalized. It works for me; it might not work for you.

## Highlights

- Albums, artists, playlists, search, queue controls, and media keys.
- Sound Check normalization, synchronized lyrics, and lyrics editing/export.
- Listening stats, library health checks, and selective `.sparklebackup` restores.
- Optional metadata, artwork, and [Discord presence](docs/discord-artwork.md) integrations.

## Install

For Windows 10/11 x64, download the v0.5.0+ `Setup.exe` from [GitHub Releases](https://github.com/doabell/sparkle/releases). It creates a Start menu shortcut, no desktop shortcut, and installs WebView2 if needed. The preview is unsigned.

**Upgrading from MSI:** close Sparkle, uninstall the MSI, then run the setup. Your library and settings remain available.

Updates are manual: **Settings → About → Check for updates → Download update → Restart to install**.

## Privacy

Your database, cache, and listening history stay local. Online providers are contacted for features you configure or invoke. Backups exclude music files, credentials, and music-folder paths.

## Develop

Install the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), Bun 1.3.14, and Rust 1.89:

```sh
bun install --frozen-lockfile
bun run tauri dev
```

See [tests and coverage](docs/testing.md), [Windows builds and releases](docs/windows-releases.md), and the [changelog](CHANGELOG.md).

## License

[MIT](LICENSE). Dependency licenses and [third-party notices](THIRD_PARTY_NOTICES.md) are included offline in **Settings → About**.
