# Tests and coverage

Run checks from the repository root after `bun install --frozen-lockfile`:

```sh
bun run check
bun run test
cargo test --workspace --lib --locked --manifest-path src-tauri/Cargo.toml
bun run format:check
bun run version:check
```

The test scripts run `svelte-kit sync` first so `$lib` aliases resolve on a
fresh checkout. Use `bun run test -- <file>` to select a test file.

Dev/test builds retain Sparkle's debug symbols and omit dependency symbols.
To debug dependency code, pass `--config 'profile.dev.package."*".debug=2'` to Cargo.

For library, scanner, settings, database, and provider work, run the independent
core suite without compiling Tauri, WebView2, the updater, or audio-device bindings:

```sh
cargo test -p sparkle-core --lib --locked --manifest-path src-tauri/Cargo.toml
```

The workspace command above tests both crates. `src-tauri/core` owns the core
logic; `src-tauri/src` owns IPC, windows, playback devices, and desktop adapters.
Both use the same database schema, settings types, and provider implementations.

Use the relevant tests during development. Frontend, app, and installer builds
are needed when validating build or packaging behavior; they are not required
for routine code checks.

## Coverage

On Windows, install the coverage tools once:

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --version 0.9.0 --locked
bun run test:coverage
```

Run one language with `bun run test:coverage:ts` or `bun run test:coverage:rs`.
Rust instrumentation uses `src-tauri/target/llvm-cov-target`. LCOV reports are
written to `coverage/typescript/lcov.info` and `coverage/rust/lcov.info`.

| Scope                         | Lines | Functions | Per-file lines |
| ----------------------------- | ----: | --------: | -------------: |
| TypeScript production modules |   95% |       95% |            80% |
| Rust data and storage         |   85% |       65% |              — |
| Rust full backend library     |   45% |       35% |              — |

Gates use executed/total counts, not averages of file percentages. Empty reports
and missing production modules fail. The core Rust files also count toward the
full backend gate; `scripts/check-coverage.mjs` defines the inventories and gates.

## Scope and fixtures

- TypeScript coverage includes every standalone production `.ts` module under
  `src/`. The inventory test imports all modules. Native IPC, dialogs, and
  navigation are mocked at their external boundaries.
- Svelte component scripts, markup, CSS, and pre-paint JavaScript are outside
  that metric. Source-contract and server-rendering tests do not provide visual
  or browser-interaction coverage.
- Rust coverage includes `src-tauri/src` and `src-tauri/core/src`, except the thin
  `main.rs` entry point and the core's module-only `lib.rs`. The higher gate
  covers data, storage, settings, analytics, and local
  lyrics modules. Device worker loops, desktop startup, Discord, and live
  provider integration have less coverage.
- Tests use in-memory SQLite, temporary files, synthetic audio, fake storage,
  and bounded loopback HTTP fixtures. They need no player window, real audio
  output, or provider credentials. The taskbar-icon test uses a hidden native
  window. FFmpeg is needed only to regenerate the committed FLAC fixture.
- `test/fixtures/lrc.json` supplies the shared frontend/native lyric contract.
  Rust unit tests live in adjacent `tests/` directories so coverage excludes
  test code without excluding production modules.
- Shared file fixtures live in `src-tauri/test-support`; HTTP fixtures belong
  to the core tests. Coverage runs both workspace suites and checks both source
  inventories, so extracting a module cannot silently remove it from the report.

Test behavior and failure paths; do not exclude production files to pass a gate.
Visual QA is a separate manual check.

CI runs affected gates on `main` and uploads the available reports as the
`coverage` artifact, including on failure. See [Windows releases](windows-releases.md)
for CI path selection and packaging.
