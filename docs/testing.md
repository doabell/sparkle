# Tests and coverage

Run all coverage checks from the repository root:

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --version 0.9.0 --locked
bun run test:coverage
```

Or run one language with `bun run test:coverage:ts` or `bun run test:coverage:rs`.
For fast tests without instrumentation, use `bun test` and
`cargo test --locked --manifest-path src-tauri/Cargo.toml`.

Rust coverage is verified on Windows, matching the native CI job. The first
instrumented build compiles into `src-tauri/target/llvm-cov-target`; later runs
reuse it. No app window, browser, headless browser, real audio output, or live
provider credentials are needed.

## Gates

| Scope                         | Lines | Functions | Per-file lines |
| ----------------------------- | ----: | --------: | -------------: |
| TypeScript production modules |   95% |       95% |            80% |
| Rust core library             |   85% |       65% |              — |
| Rust full backend library     |   35% |       25% |              — |

These are independent gates, not a blended TS/Rust score. The checker sums
executed/total lines and functions from LCOV, rather than averaging file
percentages. Empty reports and missing production modules fail the check.
Threshold failures, weighting, and source-inventory checks have their own tests.

### TypeScript scope

Every standalone `.ts` module under `src/`, including API orchestration, stores,
and utilities, is imported by the inventory test. Bun only instruments loaded
modules, so this prevents untested new modules from silently disappearing.
Declarations, test/support code, dependencies, and developer scripts do not
contribute to the production percentage. Native IPC, dialogs, media-session
initialization, and SvelteKit navigation are mocked at their external boundaries;
the app's API and stores execute normally.

Svelte component scripts, markup, CSS, and the static pre-paint JavaScript are
not part of this metric. Some existing tests inspect their contracts, but those
assertions are not component-rendering or visual coverage.

### Rust scope

The overall gate includes all `src-tauri/src` production modules except the
binary's thin `main.rs` entry point. Unit tests live in adjacent `tests/`
directories so LLVM can exclude their source files without excluding the
production modules they test. Third-party dependencies and vendored plugins are
not part of Sparkle's percentage.

The higher core gate covers complete modules: analytics, artwork storage,
backup, cache, database initialization/migrations, database writer, models,
artist normalization, settings, and local lyric dispatch/embedded/sidecar
providers. These files also count in the overall gate. Tests use in-memory
SQLite with the real schema and isolated temporary files, plus existing fake
storage adapters.

The overall floor is intentionally lower: device lifecycle/audio worker loops,
desktop startup, Discord connections, and live online-provider flows remain
under-tested. **The core percentage is not whole-backend coverage.** Keep those
gaps visible and raise the overall floor as deterministic integration seams are
added. Rust's function count also includes generated functions and individual
error closures, so its function gate is separate from its line gate.

## Reports and review

LCOV reports are written to `coverage/typescript/lcov.info` and
`coverage/rust/lcov.info`. CI runs both gates and uploads the `coverage` artifact,
including available reports on failure. Reports and instrumented builds are not
committed.

When adding behavior, test outcomes and failure paths rather than calling a
function solely to raise its percentage. Keep new Rust tests in a `tests/`
module, and do not exclude production files to make a gate pass. Visual QA
remains a separate manual step.
