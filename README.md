# Sparkle

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

## Install

Sparkle 0.1.0 targets Windows 10/11 x64 and is distributed as an MSI from [GitHub Releases](https://github.com/doabell/sparkle/releases). This preview is not code-signed, so Windows may show an unknown-publisher warning.

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
