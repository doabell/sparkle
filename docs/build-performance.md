# Local build measurements

Measured on 19 September 2026 on a Ryzen 7 7840HS (16 logical processors),
32 GB RAM, Windows 10, Rust/Cargo 1.89.0, and Bun 1.3.14.

## Native rebuilds

The comparison uses the same application code and frontend assets. The original
settings are from `be0b285`: three library outputs and full LTO. The improved
settings from `6db7fe5` produce only an `rlib` and use ThinLTO.

| Cached workload               | Original | Improved | Reduction |
| ----------------------------- | -------: | -------: | --------: |
| Debug rebuild after an edit   |   16.8 s |   10.7 s |       37% |
| Release rebuild after an edit |  262.7 s |   66.8 s |       75% |
| Unchanged Cargo build         |    1.0 s |    1.0 s |         — |

Debug numbers are medians of four samples per configuration, run in both orders.
Original samples: 25.01, 15.44, 17.95, 15.68 seconds. Improved samples:
11.72, 9.62, 11.85, 9.15 seconds. Release numbers are medians of two samples:
265.33 and 260.15 seconds originally, versus 67.32 and 66.32 seconds improved.

Each sample changes a log string in the Rust library, then builds the executable.
Dependencies are already cached; each measured sample recompiles only Sparkle.
Warm-up builds are excluded. Switching to ThinLTO rebuilt 312 dependencies in the
first run, which took 145.63 seconds; that is a one-time cache transition, not a
comparable cached sample. No clean-build speedup is claimed.

The original release's final executable compilation took about 210–212 seconds.
With the improved settings it took about 38 seconds. The release executable grew
from 27.1 MiB to 29.5 MiB. Runtime performance was not benchmarked.

## Avoiding unchanged frontend rebuilds

With the compiler improvements already applied, repeated unchanged Tauri release
builds still took 73.02 and 82.50 seconds. Cargo's fingerprint log identified
rewritten frontend files as the reason for recompiling Sparkle. SvelteKit's
default build timestamp also changed asset contents and filenames on every run.

The frontend now uses the app version as its deterministic SvelteKit version.
The static adapter builds into a staging directory, then publishes changed files
to `build/` while retaining identical files and their timestamps. Removed assets
are deleted. Vite still runs on every build; this does not cache or skip source
compilation. Rust's build script tracks the embedded asset directory so new and
deleted files also invalidate the native build.

After warming the generated-asset cache, the same unchanged Tauri builds took
**6.57 and 7.07 seconds**, with **zero Rust compilation**. All 88 frontend files
retained both their contents and timestamps. That is about **91% less time**
than the previous unchanged-build median of 77.76 seconds.

The first two builds after changing the asset pipeline still took 75.56 and
77.71 seconds. The second rebuilt because Tauri had created new compressed
assets during the first compilation, after Cargo's dependency timestamp.
New compressed assets can therefore cause one extra rebuild before the cache
settles. The 6–7 second result is for subsequent unchanged builds, not the first
build after a frontend change. Installer generation is not included in these
Tauri `--no-bundle` measurements.

```sh
bun run tauri build --no-bundle --target x86_64-pc-windows-msvc -- --locked --offline --timings
```

## Reproducing the native workload

Build the frontend once with `bun run build`, then leave its output unchanged
while comparing native settings. Warm each configuration before recording
samples. Use the same small Rust edit for both configurations and restore it
afterward. Run builds sequentially, with no other compiler running.

```sh
cargo build --locked --offline --manifest-path src-tauri/Cargo.toml --timings --no-default-features
cargo build --locked --offline --manifest-path src-tauri/Cargo.toml --timings --release --target x86_64-pc-windows-msvc --features tauri/custom-protocol --bins
```

Cargo's HTML timing reports distinguish rebuilt units from cached dependencies.
The benchmark logs and reports for this measurement are in the local ignored
`.tmp/build-bench` directory. These are local measurements, not predictions of
GitHub runner timings. Routine coding validation still uses tests; these builds
were run explicitly to measure build performance.
