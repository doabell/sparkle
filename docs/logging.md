# Logging

Choose **Settings → Advanced → Diagnostics → Log level**. Changes save automatically and apply
without restarting. **Info** is the default; use **Debug** to reproduce a problem
and **Trace** for playback or media-control timing.

| Level | Includes                                                                                              |
| ----- | ----------------------------------------------------------------------------------------------------- |
| Error | Failed operations that cannot complete.                                                               |
| Warn  | Errors plus degraded behavior, recovery, and partial failures.                                        |
| Info  | Warnings plus startup/shutdown, device changes, scan summaries, and backup operations.                |
| Debug | Info plus playback transitions, commands, provider failures, cache decisions, and per-track analysis. |
| Trace | Debug plus frequent state publications, volume changes, media updates, and database write details.    |

Dependency logs are capped at Warn at every verbosity. Sparkle's native media
adapter follows the app's level. The saved level is loaded as soon as the
database opens; database initialization uses Info. An existing enabled verbose
switch maps to Debug until a level is saved.

## Files and troubleshooting

**Settings → Advanced → Diagnostics → Log file** shows the active `sparkle.log` path and opens its
location. On Windows it is under `%LOCALAPPDATA%\com.doabell.sparkle\logs`.
Development builds use `sparkle-dev.log` with separate archives.
The log keeps the active file and two archives, each rotating at approximately
2 MiB. Frontend failures and native events share this file and stdout. Each
record includes a UTC timestamp, target, severity, and `event` name.

Reproduce the problem once, note the time, and collect the active log plus any
archive covering that time. Return to Info afterward. Logs can include local
paths and library IDs; review them before sharing. HTTP(S) URLs are replaced
with `[url]`, control characters are escaped or removed, and long records are
truncated. Logs are not uploaded automatically.

## Adding events

- Native code uses `log` macros with `target: "sparkle::<subsystem>"` and
  `event=snake_case` followed by useful `key=value` fields.
- Frontend code uses `logger` from `$lib/logger`, with stable scope/event names.
  Uncaught errors and unhandled promise rejections are captured by the layout.
  Bridge failures fall back to a console notice without retrying.
- Log a failure where its outcome is known. Expected misses and successful
  fallbacks belong at Debug; a partial or degraded operation belongs at Warn.
- Use counts, elapsed milliseconds, and internal IDs. Do not log settings
  objects, credentials, request/response bodies, lyrics, or track metadata.
  URL redaction is a safeguard, not permission to log secrets.
- Keep per-tick traffic at Trace. Log repeated recoverable failures once, then
  retries at Debug. Never log from the audio sample callback or while holding a
  playback-state lock; snapshot the required fields first.

Filtering, formatting, retention, and frontend IPC live in
`src-tauri/src/logging.rs`; frontend helpers live in `src/lib/logger.ts`.
