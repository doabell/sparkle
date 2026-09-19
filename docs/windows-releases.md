# Windows builds and releases

Sparkle targets Windows x64 with Velopack 1.2.0. Keep the Rust SDK in
`src-tauri/Cargo.toml` and CLI in `.config/dotnet-tools.json` aligned. Installed
updates use app ID `com.doabell.sparkle` and feed channel `win-x64`.

## Release

1. Update `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`,
   `Cargo.lock`, and the matching `CHANGELOG.md` section. Run `bun run version:check`.
2. For dependency changes, install `cargo-about` 0.8.4 with `--locked`, run
   `bun run licenses:generate`, and commit the reviewed `licenses/dependencies.json`.
   `licenses/about.toml` lists accepted licenses; `licenses/upstream.json` supplies
   version-specific license files missing from published crates.
3. To test packaging locally, install the Tauri prerequisites and .NET SDK 8,
   run `dotnet tool restore`, then `bun run package:windows`. Use a fresh
   `.tmp/releases` output directory. `VPK` and `CARGO_ABOUT` can select local tools.
   `VPK` also accepts the pinned CLI's DLL when using an installed .NET runtime.
   Add `--test` to compile the app and native library tests together, then run the
   tests before creating the installer.
4. Merge and wait for main CI, then tag that commit `vX.Y.Z` and push the tag.
   Actions creates a draft release. Review and publish its setup, full `.nupkg`,
   and channel feed together, preserving asset names. Stable updates exclude
   drafts and prereleases.

Packages include the app, required DLLs, license notices, and offline dependency
license texts. Setup creates a Start menu shortcut and installs WebView2 if
needed. Workflows verify feed checksums, shortcut metadata, and packaged notices.

## CI and artifact reuse

- Every PR and main push checks formatting and version consistency.
  `scripts/ci-changes.mjs` selects other checks against the PR base or the
  workflow's last successful main push, retaining work from failed/canceled runs.
- Frontend changes run frontend checks; native changes run Rust tests. Shared
  fixtures run both. Docs-only changes skip compilation. Unknown paths, shared
  configuration, scripts, workflows, or missing history select all checks.
- Main builds the installer for changes to app code, embedded frontend, release
  notes, or packaged licenses. Test-only and docs-only changes skip packaging
  when no earlier work is outstanding. Packaging builds the frontend once;
  standalone native unit tests and coverage do not require frontend assets.
  `sparkle-windows-x64` artifacts last 30 days, and `rust-build-timings` reports
  last 14 days.
- Tags reuse only a successful main CI artifact from the exact tagged commit in
  this repository, then verify it against the tagged source. Missing or expired
  artifacts trigger a source build; invalid packages fail verification.
- Main packaging uses `--test` when native tests are required, including a check
  of embedded frontend assets. PRs and test-only changes use the dev profile.
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
