# Build workflow

Use GitHub Actions CI for builds, Windows packaging, and Rust tests or coverage
that compile native code. Do not run local builds or download build-only SDKs
and toolchains unless the user explicitly requests them.

Local source inspection, formatting, type checks, and lightweight frontend tests
are fine. Keep temporary files small and remove generated artifacts when done.
