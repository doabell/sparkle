# Sparkle

[![CI](https://github.com/doabell/sparkle/actions/workflows/ci.yml/badge.svg)](https://github.com/doabell/sparkle/actions/workflows/ci.yml)
[![MIT License](https://img.shields.io/github/license/doabell/sparkle)](LICENSE)
[![Latest release](https://img.shields.io/github/v/release/doabell/sparkle)](https://github.com/doabell/sparkle/releases/latest)

Sparkle is a local-first Windows music player for your own library. It pairs native playback with a polished, album-focused interface and useful listening insights.

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

Sparkle targets Windows 10/11 x64 and is distributed as an MSI from [GitHub Releases](https://github.com/doabell/sparkle/releases). This preview is not code-signed, so Windows may show an unknown-publisher warning.

Other platforms are not released or tested yet.

## Develop

Install the [Tauri prerequisites for Windows](https://v2.tauri.app/start/prerequisites/), Node.js 24, and Rust 1.89. Then run:

```sh
npm ci
npm run tauri dev
```

Useful checks:

```sh
npm run version:check
npm run format:check
npm run check
npm test
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

Build the MSI locally with:

```sh
npm run tauri build -- --bundles msi
```

## License

[MIT](LICENSE)
