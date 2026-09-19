# Discord presence

Open **Settings → Sharing → Discord → Presence layout**. Changes save automatically.

Click the app name, title, subtitle, or cover text in the preview to edit that field. Metadata buttons insert at the cursor or replace selected text. The preview uses sample metadata, with the same field order as Discord's listening card.

- **App name** explicitly sets the activity name; the default is `Sparkle`.
- **Member list** chooses the app name, title, or subtitle for Discord's member-list status.
- **Title**, **Subtitle**, and **Cover text** accept custom text and the metadata below. Empty fields are omitted. Text is limited to 128 UTF-8 bytes without splitting characters. Discord controls the final rendering.
- **Album cover** controls cover uploads/display. Discord can still use its application icon when no image is supplied.
- **Progress bar** includes the track's start/end timestamps when its duration is known.

| Field        | Token            | Example            |
| ------------ | ---------------- | ------------------ |
| Title        | `{title}`        | Midnight drive     |
| Artist       | `{artist}`       | Sample artist      |
| Album        | `{album}`        | After hours        |
| Album artist | `{album_artist}` | Sample ensemble    |
| Lyrics       | `{lyrics}`       | Stay until morning |
| Year         | `{year}`         | 2024               |
| Genre        | `{genre}`        | Pop                |
| Track        | `{track}`        | 3                  |
| Disc         | `{disc}`         | 1                  |
| Duration     | `{duration}`     | 3:35               |
| Format       | `{format}`       | FLAC               |
| Bitrate      | `{bitrate}`      | 921 kbps           |
| Sample rate  | `{sample_rate}`  | 44.1 kHz           |
| Bit depth    | `{bit_depth}`    | 16-bit             |
| Channels     | `{channels}`     | Stereo             |

These fields use existing playback and library metadata; they do not scan files or fetch additional metadata. Album artist prefers the track's explicit album credits, then the library's album credits, in credit order. Missing optional values are empty. Audio values include their units; channels use Mono, Stereo, or a channel count. Durations longer than an hour include hours.

**Live lyrics** puts the song and artist in the title, and the current synced lyric in the subtitle and cover text. To replace only the album text, select **Cover text** and replace it with `{lyrics}`. The cover image is retained. **Reset** restores the defaults without changing the application ID or artwork storage.

Lyrics use the selected source, cached/local text, LRC offsets, and Sparkle's saved timing adjustment. An unavailable synced line falls back to the album name. The Discord worker samples the current line about every 15 seconds and skips unchanged text; it does not queue old lines or upload artwork on lyric ticks. This is an intentionally conservative refresh interval, not a guarantee of Discord's delivery timing. Fast lines may be skipped. Pause/stop clear the presence, and seeking updates its position. Lyrics already fetched for Sparkle's player are reused; the presence worker does not make additional lyric-provider requests.

## An old application name still appears

1. Compare Sparkle's saved **Discord application ID** with **General Information → Application ID** in the [Discord Developer Portal](https://discord.com/developers/applications). Names edited on another application do not affect the saved ID.
2. Keep **App name** set to `Sparkle` and **Member list** set to **App name**. Wait for the saved indicator. Sparkle sends `activity.name` explicitly and reconnects when the ID or display name changes.
3. If the old name persists after checking the ID, fully quit and reopen Discord, then toggle playback sharing off/on. Client caching is a troubleshooting possibility, not proof that the portal rename failed.

Before this customization, Sparkle omitted `activity.name` and relied on Discord's application metadata. The current integration supports the explicit override through the existing `discord-rich-presence` dependency.

References: [Discord's name-override release note](https://discord.com/developers/docs/social-sdk/release_notes.html), [activity fields and status display types](https://discord.com/developers/docs/events/gateway-events#activity-object), and [the Rust activity API](https://docs.rs/discord-rich-presence/1.1.0/discord_rich_presence/activity/struct.Activity.html).
