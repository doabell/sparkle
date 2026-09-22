# Discord presence

Enable sharing in **Settings → Sharing → Discord** and set the Discord application
ID. **Presence layout** edits the card using the loaded track, cached artwork,
lyrics, and progress; sample values appear when no track is loaded. Changes save
automatically. The preview does not publish presence or fetch provider data.

| Control                       | Behavior                                                                       |
| ----------------------------- | ------------------------------------------------------------------------------ |
| Card name                     | Card heading; defaults to `Sparkle`.                                           |
| Under avatar                  | Compact status: Card name, Title, or Subtitle. Default: Subtitle (`{artist}`). |
| Title / Subtitle / Cover text | Editable templates, limited to 128 UTF-8 bytes. Empty fields are omitted.      |
| Album cover                   | Enables cover uploads/display; see [artwork storage](discord-artwork.md).      |
| Progress bar                  | Shows track timestamps when duration is known.                                 |
| Default                       | Resets the layout, preserving application ID and artwork storage.              |

Click a field to edit it. Metadata buttons insert at the cursor or replace the
selection. Supported tokens:

| Metadata        | Tokens                                                                              |
| --------------- | ----------------------------------------------------------------------------------- |
| Track credits   | `{title}`, `{artist}`, `{album}`, `{album_artist}`                                  |
| Lyrics and tags | `{lyrics}`, `{year}`, `{genre}`, `{track}`, `{disc}`                                |
| Audio           | `{duration}`, `{format}`, `{bitrate}`, `{sample_rate}`, `{bit_depth}`, `{channels}` |

Values come from playback/library metadata. Missing optional values are empty;
audio values include units. Album artist prefers explicit track album credits,
then library album credits, preserving credit order. Discord controls the final
rendering.

## Lyrics and playback

Set **Cover text** to `{lyrics}` to show lyrics while retaining the album image.
Lyrics use the selected source, cached/local text, and saved timing adjustment.
An unavailable synced line falls back to the album name.

The worker checks the current line about every 15 seconds and skips unchanged
text. Fast lines may be skipped. It makes no additional lyric-provider requests
or artwork uploads for lyric updates. Pause/stop clear presence; seeking updates
its position.

## Troubleshooting

- For a stale name, verify the saved application ID against **General Information**
  in the [Discord Developer Portal](https://discord.com/developers/applications).
  Card name controls the heading; Under avatar controls the compact status.
  If settings are correct, fully restart Discord and toggle sharing off/on.
- Without album artwork, Sparkle sends the asset key `logo`. That Rich Presence
  asset and the application's icon are separate uploads under the saved ID.
  Sparkle's application ID is `1081306379488346142`.
- Development and installed builds have separate settings. Check the ID and
  storage settings in the running profile. Use [Debug logs](logging.md) to inspect
  connection and artwork failures.
