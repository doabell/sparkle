# Windows releases

Sparkle uses Velopack 1.2.0 for its Windows x64 setup and in-app updates. Keep the Rust SDK in `src-tauri/Cargo.toml` and CLI in `.config/dotnet-tools.json` on the same version. The app ID is `com.doabell.sparkle`, and the feed channel is `win-x64`. Changing these breaks the installed app's update path.

## Build and release

1. Update the version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`, refresh `Cargo.lock`, and add the matching changelog section.
2. If dependencies change, run `cargo install cargo-about --version 0.8.4 --locked`, then `bun run licenses:generate`. Review and commit `licenses/dependencies.json`. This collects Windows runtime crates and packages actually included in the frontend build, retaining full texts and notices. `licenses/about.toml` records accepted licenses. Generation fails for missing/unaccepted licenses or a generic MIT fallback without an upstream copyright/license file. `licenses/upstream.json` records version-specific source files omitted from published crates; update those snapshots from the corresponding upstream revision when necessary. `CARGO_ABOUT` can point to a local cargo-about executable.
3. On Windows x64 with Bun, Rust/Tauri prerequisites, and .NET SDK 8, run `dotnet tool restore`, then `bun run package:windows`. Packages appear in `.tmp/releases`. Use a fresh output directory for each release. `VPK` can point to a local vpk executable.
4. Merge the PR, tag the version on `main` as `vX.Y.Z`, and push the tag. GitHub Actions creates a draft release with setup, full `.nupkg`, and the channel feed. Review the release and publish the draft to make it discoverable. Do not rename assets or publish just the setup: installed apps need the feed and package too. Drafts and prereleases are excluded from the stable updater.

The setup creates only a Start menu shortcut. WebView2 is bootstrapped when missing. Packages contain the app, required DLLs, `LICENSE`, `THIRD_PARTY_NOTICES.md`, and offline dependency license texts. PR checks verify feed metadata/checksums, shortcut metadata, and packaged notice contents. The release build disables Tauri's MSI bundler.

## Manual upgrade smoke test

Use a disposable Windows user or VM with two consecutive release versions; do not use a real music library for installer testing.

1. Install the older setup. Confirm the Start menu entry exists and no desktop shortcut was created. Open Sparkle and add a small test library.
2. Open Settings → About. Confirm no check/download begins automatically. Check for updates and confirm only the newer version is offered; the app keeps running and no package downloads until Download update is clicked.
3. Download the update. Close and reopen Sparkle normally: the older version must still run and Settings must offer Restart to install. Click it and confirm the new version launches with library/settings preserved.
4. Check again: the app should report up to date. Disconnect the network and check again: it should show an error. Interrupt a download and verify that retry succeeds. Check that only one update action can run at a time.
5. Test migration separately: close an old MSI install, uninstall it, and install the Velopack setup. Its library/settings should remain available in the unchanged app-data directory. Unpackaged/debug builds should link to GitHub Releases instead of attempting an in-app update.

For feed testing before a stable release, use a fork and change the repository constant in `src-tauri/src/updates.rs` in test builds. Never publish a fake higher version to the production repository: update clients compare semantic versions. The automated Rust tests cover action sequencing, retries, concurrent requests, and release validation without contacting GitHub or exiting the test process.
