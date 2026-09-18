# Discord presence

Open **Settings → Sharing → Discord → Presence layout**. Changes save automatically.

- **App name** explicitly sets the activity name; the default is `Sparkle`.
- **“Listening to” status** chooses the app name, first line, or second line for Discord's member-list status.
- **First line**, **Second line**, and **Artwork text (hover)** accept custom text and `{title}`, `{artist}`, `{album}`, and `{lyrics}`. Empty fields are omitted. Text is limited to 128 UTF-8 bytes without splitting characters.
- **Show album cover** controls cover uploads/display. Discord can still use its application icon when no image is supplied.
- **Show progress bar** includes the track's start/end timestamps when its duration is known.

**Use live lyrics** puts the song and artist on the first line, and the current synced lyric on the second line and in the artwork hover text. To replace only the album text, set **Artwork text (hover)** to `{lyrics}`. The cover image is retained. **Reset layout** restores the defaults without changing the application ID or artwork storage.

Lyrics use the selected source, cached/local text, LRC offsets, and Sparkle's saved timing adjustment. An unavailable synced line falls back to the album name. The Discord worker samples the current line about every 15 seconds and skips unchanged text; it does not queue old lines or upload artwork on lyric ticks. This is an intentionally conservative refresh interval, not a guarantee of Discord's delivery timing. Fast lines may be skipped. Pause/stop clear the presence, and seeking updates its position. Lyrics already fetched for Sparkle's player are reused; the presence worker does not make additional lyric-provider requests.

## An old application name still appears

1. Compare Sparkle's saved **Discord application ID** with **General Information → Application ID** in the [Discord Developer Portal](https://discord.com/developers/applications). Names edited on another application do not affect the saved ID.
2. Keep **App name** set to `Sparkle` and **“Listening to” status** set to **App name**. Wait for the saved indicator. Sparkle sends `activity.name` explicitly and reconnects when the ID or display name changes.
3. If the old name persists after checking the ID, fully quit and reopen Discord, then toggle playback sharing off/on. Client caching is a troubleshooting possibility, not proof that the portal rename failed.

Before this customization, Sparkle omitted `activity.name` and relied on Discord's application metadata. The current integration supports the explicit override through the existing `discord-rich-presence` dependency.

References: [Discord's name-override release note](https://discord.com/developers/docs/social-sdk/release_notes.html), [activity fields and status display types](https://discord.com/developers/docs/events/gateway-events#activity-object), and [the Rust activity API](https://docs.rs/discord-rich-presence/1.1.0/discord_rich_presence/activity/struct.Activity.html).
