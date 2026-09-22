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

**Capture playback diagnostics** in the same Diagnostics section marks the
incident and freezes playback state before opening a save dialog. It saves a
local JSON file containing:

- Output availability/configuration, pending playback intent, the current
  command, and history-writer health.
- The latest 200 command results, including queue wait, execution and loading
  stage timings. Consecutive successful volume updates are coalesced.
- Up to 2,000 playback events from the 30 minutes ending at the incident.
- The last 512 KiB of the active log and up to two archives for this profile.

The capture records missing sections and truncation explicitly. It remains
useful when the database or logs cannot be read. Pending database writes are
reported rather than waiting for them to flush; the newest transitions may
therefore appear only in the runtime command records. The file is saved only
to the destination you choose and is never uploaded. Review local paths before
sharing it.

## Playback outcomes and retention

See [Playback event contracts](playback-events.md) for the semantic event
catalog, correlation fields, recovery rules, and frontend ordering contract.

Every playback command carries a `command_id` from the frontend or native media
entry point through the audio worker and reply. Results distinguish `applied`,
`deferred` (waiting for output), `noop`, and `failed`. Failures identify the
command, target track when known, and stage. Reply timeouts do not cancel a
command that is already queued; a later completion uses the same ID.

`first_playback_progress` measures the time from enqueueing a start/resume
command to forward movement of the engine clock. It is not proof that sound
reached the speakers. `playback_stalled` reports an observed stationary clock;
output loss is a suspected cause. Device recovery logs attempts and elapsed
time, including an open call that remains pending.

Diagnostic playback events expire after seven days and are capped at 50,000
rows, with cleanup at startup and every 512 analytics writes. Listening history
has no automatic retention limit. Normal `.sparklebackup` exports include
listening history but omit diagnostic events; older backups containing events
remain importable.

History writes use a bounded, nonblocking queue of 1,024 requests. The Diagnostics
section shows recorder availability, pending writes, and failed/dropped counts.
If the queue is exhausted or the writer stops, playback continues and dropped
writes are counted and logged. Captures include the oldest pending age, last
successful write, and last failure. Failed writes never produce success logs.

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
`src-tauri/core/src/logging.rs`; the Tauri plugin and IPC adapter live in
`src-tauri/src/logging.rs`. Frontend helpers live in `src/lib/logger.ts`.
