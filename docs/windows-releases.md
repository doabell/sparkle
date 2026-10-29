# Windows builds and releases

Sparkle targets Windows x64 with Velopack 1.2.161. Keep the Rust SDK in
`src-tauri/Cargo.toml` and CLI in `.config/dotnet-tools.json` aligned. Installed
updates use app ID `com.doabell.sparkle` and feed channel `win-x64`.

## Release

Use GitHub Actions for builds, Windows packaging, and native Rust tests or
coverage. Local release preparation can run formatting, version/license checks,
type checks, and lightweight frontend tests.

1. Update `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`,
   `src-tauri/Cargo.lock`, and the matching `CHANGELOG.md` section. Run
   `bun run version:check`.
2. Run `bun run licenses:check`. A version-only bump also changes the report's
   input fingerprint; refresh it only after verifying that the other dependency,
   notice, and generator inputs are unchanged. For dependency changes, regenerate
   the report in CI with `cargo-about` 0.8.4 installed using `--locked` and
   `bun run licenses:generate`, then commit the reviewed `licenses/dependencies.json`.
   `licenses/about.toml` lists accepted licenses; `licenses/upstream.json` supplies
   version-specific license files missing from published crates.
3. Open a release PR and wait for CI. Download `sparkle-windows-x64` from that
   run to test installation, playback, and the release's new features in a
   disposable Windows user or VM. Complete the upgrade smoke test below; use a
   fork for feed testing because the production updater excludes draft releases.
4. Merge the release PR and wait for both CI and Coverage on that exact main
   commit. Confirm that CI uploaded `sparkle-windows-x64` for the new version,
   then tag the verified commit `vX.Y.Z` and push the tag.
5. Actions creates a draft release, reusing the verified main package when
   available. Review the release notes and publish its setup, full `.nupkg`,
   and channel feed together, preserving asset names. Publishing makes the
   version visible to stable in-app update checks; drafts and prereleases stay
   excluded.

Packages include the app, required DLLs, license notices, and offline dependency
license texts. Setup creates a Start menu shortcut and installs WebView2 if
needed. Workflows verify feed checksums, shortcut metadata, and packaged notices.

`bun run build` reuses the frontend when source, configuration, environment,
installed dependencies, generated assets, and the bundled-license report match
the last successful build. Use `bun run build --force` to rebuild it explicitly.
Release builds and Velopack packages disable LTO. The audio processing crates
use one code generation unit to preserve decoding and Sound Check throughput.

## CI and artifact reuse

- Every PR and main push checks formatting and version consistency.
  `scripts/ci-changes.mjs` selects other checks against the PR base or the
  workflow's last successful main push, retaining work from failed/canceled runs.
- Frontend changes run frontend checks; native changes run Rust tests. Shared
  fixtures run both. Docs-only changes skip compilation. Unknown paths, shared
  configuration, scripts, workflows, or missing history select all checks.
- PRs and main pushes build the installer for changes to app code, embedded
  frontend, release notes, or packaged licenses. Test-only and docs-only changes skip packaging
  when no earlier work is outstanding. Packaging builds the frontend once;
  standalone native unit tests and coverage do not require frontend assets.
  `sparkle-windows-x64` artifacts last 30 days, and `rust-build-timings` reports
  last 14 days.
- Tags reuse only a successful main CI artifact from the exact tagged commit in
  this repository, then verify it against the tagged source. Missing or expired
  artifacts trigger a source build; invalid packages fail verification.
- Packaging uses `--test` when native tests are required, running both the
  desktop and core suites, including a check of embedded frontend assets.
  Test-only changes use the dev profile and test the full workspace.
  Release, dev, and coverage caches are separate and save after successful runs.
- CI and releases reuse an installed .NET SDK 8, installing it only when missing.
  Coverage downloads the pinned, checksum-verified `cargo-llvm-cov` Windows binary
  when its tool cache is empty.

## Upgrade smoke test

Use a disposable Windows user or VM, a small test library, and two consecutive
release versions.

1. Install the older version. Check the Start menu shortcut, absence of a desktop
   shortcut, and library playback.
2. In **Settings → About**, check for updates, download, then restart to install.
   Each stage requires its own click. An ordinary restart before installation
   keeps the older version and pending update.
3. Confirm the new version preserves the library/settings and reports up to date.
   Check offline failure, interrupted-download retry, and concurrent-action blocking.
4. For MSI migration, close and uninstall the MSI before running setup. Confirm
   library/settings survive. Unpackaged builds should link to GitHub Releases.

For feed testing, use a fork and the repository constant in
`src-tauri/src/updates.rs`. Do not publish test versions to the production feed.
