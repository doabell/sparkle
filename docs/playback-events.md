# Playback event contracts

Sparkle has three separate contracts: semantic events in `playback_events`,
listening facts in `listens`, and live frontend state/progress notifications.
Diagnostic command logs carry timing and errors. Progress ticks and volume drags
are not persisted as semantic events.

## Semantic events

`PlaybackEvent` in `core/src/analytics.rs` defines the variant-specific payload.
`PlaybackEventKind`, `ListenStartReason`, and `ListenEndReason` define the stable
wire vocabulary used by producers, recovery, and backup import.

| Event                | Emission rule                                                            | Additional data                                       |
| -------------------- | ------------------------------------------------------------------------ | ----------------------------------------------------- |
| `queue_loaded`       | A queue is replaced, before attempting to start its selected track.      | Reason: `queue_replaced` or `single_track`.           |
| `listen_started`     | A new listen attempt starts after its source is loaded and played.       | Listen ID and typed start reason.                     |
| `playback_resumed`   | An existing listen resumes through a Play command.                       | Existing listen ID.                                   |
| `playback_paused`    | Active playback or pending playback intent is paused.                    | Listen ID when a listen has started.                  |
| `seeked`             | A seek succeeds against the player.                                      | Previous position, target position, and seek reason.  |
| `listen_ended`       | An active listen is finalized, including recovery of an interrupted one. | Listen ID and typed end reason.                       |
| `playback_stopped`   | An explicit Stop clears a selected track.                                | Snapshot of the track/listen before clearing.         |
| `shuffle_changed`    | The shuffle preference changes.                                          | Resulting `shuffle` and queue cursor.                 |
| `repeat_changed`     | The repeat preference changes.                                           | Resulting `repeat_mode`.                              |
| `queued_next`        | A track is inserted or moved to play next.                               | `target_track_id`; current listen fields stay intact. |
| `output_unavailable` | The output pipeline becomes unavailable or its observed cause changes.   | `device_changed`, `clock_stalled`, or `open_failed`.  |
| `output_restored`    | An output pipeline opens successfully, including initial opening.        | Current playback snapshot, if any.                    |
| `command_failed`     | An engine operation fails, including a failed deferred start/recovery.   | Command name, failure stage, and optional target ID.  |

`listen_started` describes a listen attempt, not proof that sound reached the
speakers. It also covers a new attempt on the same track after inactivity.
An existing listen interrupted by pipeline recovery retains its ID unless the
inactivity timeout has elapsed. Deferred initial starts retain their original
selection/advance reason and command identity.

No-op commands do not produce semantic change events. Setting shuffle to its
existing value preserves the exact queue order. Requesting Play Next for the
current track or the already-next track leaves the queue untouched. Stop with
no selected track emits nothing. Repeated output-open failures with the same
cause are coalesced. Output events describe pipeline availability even while
paused or idle; `output_restored` does not assert that playback resumed.

Every new event has an event ID and a run ID. Events caused by a command also
carry its command ID, including failures before a listen could start. Failures
store a stage token, not file paths or free-form error messages. Detailed errors
remain in diagnostic logs.

`track_id`, `position_ms`, `listen_id`, and queue cursors describe the current
playback snapshot. `target_track_id` separately identifies the requested track
for queue edits and failures; it can refer to an unavailable library ID.
`target_position_ms` is only present for seeks. Snapshot fields can be null
when there is no selected track or active listen. A queue replacement is a real
change even if the following start fails; both events share a command ID.

`reason` is defined by the event variant. Values already captured by `source`,
`shuffle`, or `repeat_mode` are not repeated as reasons. Legacy backups accept
`track_started` as an input alias for `listen_started`; unknown vocabulary is
normalized to `unknown`. Legacy queue targets are separated from the current
listen during import.

## Listening history and recovery

`listens` is the durable source for listening statistics. A listen records heard
time independently of seek position. Its `completed` flag means the final
position reached the last ten percent; the `completed` end reason means normal
automatic completion. These are distinct measurements.

After an interrupted process, open listens are finalized at their last durable
activity checkpoint. Recovery atomically writes one `listen_ended` event with
reason `interrupted` per recovered listen. The event timestamp is the time of
recovery, not an inferred crash time. Repeating recovery produces no duplicates.
`interrupted`, `legacy_import`, and `legacy_migration` belong to the shared
reason vocabulary.

## Frontend notifications

`playback-state-changed` carries the complete `PlaybackState`, including volume
and a monotonically increasing `revision`. State snapshots and command replies
use the same shape and revision. The revision increases whenever the engine
publishes state, including a seek or a restart of the same track.

`playback-progress` contains the state `revision`, a monotonic progress
`sequence`, track ID, position, and duration. Progress can update only the exact
state revision and current track already accepted by the frontend. Out-of-order
progress sequences and older/equal state snapshots are ignored. Future-revision
progress waits for a complete snapshot instead of hiding an unseen state change.
Counters belong to one audio-controller lifetime.

The playback store subscribes before requesting its initial snapshot. This
closes the startup subscription gap and prevents an older initial response,
command reply, or same-track progress event from overwriting newer state.
