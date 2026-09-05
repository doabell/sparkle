# Sparkle interface language

This document is the contract for Sparkle's interface. It describes the roles
behind the CSS tokens in `src/app.css`; individual pages should consume those
roles instead of inventing nearby colors, shapes, or motion.

## Character

Sparkle takes its cues from Apple Music: generous artwork, rounded controls,
clear type, and quiet navigation. Album art stays square and artist imagery
stays circular unless the source itself calls for another crop. Chrome supports
the music rather than competing with it.

## Interaction grammar

Use one primary response for each interaction:

- Rows, cards, and contained controls use a background change over their full
  hit area.
- Text and icon links use a color change. App navigation and metadata links do
  not underline.
- Prominent contained actions may scale to `--motion-hover-scale`; pressed
  actions scale to `--motion-press-scale`. Metadata text does not scale on hover.
- Borders describe structure. They do not brighten on hover. Focus outlines,
  validation borders, and selection indicators are semantic exceptions.
- Non-interactive surfaces do not lift, glow, or otherwise react to the mouse.

An element must look clickable everywhere it is clickable. Layered rows use one
full-row link, with any independent action raised above that hit target.

## Controls and shape

- Standard actions are `--control-height` with pill-shaped `--control-radius`.
- Compact controls are `--control-height-sm`.
- Circles are reserved for avatars, artist imagery, artwork-level play buttons,
  and icon-only controls where the icon is the visual object.
- Related mutually exclusive actions use `.segmented-control`.
- Related adjustment actions use `.control-cluster`.
- Segments and adjustment buttons share the pill radius inside a single quiet
  fill. Do not outline both the group and its individual controls.
- The lyrics source action and timing controls share `.control-cluster` for
  identical height, fill, type, radius, and interaction states. Headers and
  controls use the UI font; the lyrics font applies only to lyric content.
- Primary filled actions use the accent fill roles. Secondary actions use the
  neutral interactive backgrounds.
- Text inputs, artwork previews, and navigation rows use the smaller surface
  radii. Rounded controls do not require every surface to become a pill.

## Surface hierarchy

Use spacing and headings before adding a container. Settings has open sections
separated by a single rule; provider, cache, and diagnostic lists use row
dividers. Avoid a framed section around a framed list around framed rows.

Filled surfaces are useful when they communicate something specific: a theme
preview, artwork, a selected option, or an editable input. Supporting text and
save status remain plain text. A selectable preview has one background state,
not another enclosing frame.

## Accent roles

Appearance offers System, Light, and Dark. System is the default and follows
live OS changes; an explicit mode overrides the OS until System is selected
again. Save the preference with settings and restore its validated cache before
first paint. Both modes retain the same semantic accent and interaction roles.

The selected accent is a seed, not a component color. Consume its semantic
roles:

- `--color-accent-content` for accent text.
- `--color-accent-graphic` for icons and indicators.
- `--color-accent-fill` and its hover/active roles for filled actions.
- `--color-on-accent-fill` for text and icons inside filled actions.
- `--color-accent-subtle` for selected surfaces.
- `--color-accent-focus` for keyboard focus.

Do not add hard-coded theme colors to product UI. Success and error states use
their own semantic colors.

## Motion

- `--motion-duration-fast` (140 ms): hover, press, icon, and color feedback.
- `--motion-duration-base` (220 ms): panels and state changes.
- `--motion-duration-slow` (360 ms): page and section entrance.
- `--motion-ease-standard`: interactive movement.
- `--motion-ease-enter`: entrance and reveal movement.

Motion changes opacity or transform and must not create layout shift. The OS
reduced-motion preference and Sparkle's Reduce motion setting both collapse
transitions, animation durations, and stagger delays globally. A component must
remain understandable when all motion is removed. Entrance animations must
release their transform on completion so they do not override hover states.

Synchronized lyrics are a timing exception: their CSS duration comes from the
same `LYRIC_TRANSITION_DURATION_MS` value as their playback anticipation. Lyric
emphasis uses transforms at a fixed font size. Synced lines keep medium-weight
glyph metrics; a subtle text stroke gives the active line its bolder appearance
without changing glyph advances. Focus must never rewrap a line, including with
custom lyric fonts. Auto-centering scrolls only the lyric panel and respects
both motion preferences.

## Page structure

Top-level content uses `.page-shell`. Major pages use `.page-header` with a
`.page-heading` and `.page-title`. Add a subtitle only when it explains a real
constraint or next step; do not repeat the heading in an eyebrow or tagline.
Use `.page-enter` for the initial reveal. Settings-like subsections may replay
the same entrance when the visible section changes.

UI labels use title case and normal letter spacing, not CSS uppercase or
widely tracked capitals. Preserve intentional acronyms (FLAC, UI, QQ Music)
and the original casing of song, album, and artist names.

The larger sidebar wordmark is optically centered in its own 4rem-high brand
area, with a slight left/down offset. Back navigation sits one small spacing
step below the top edge; it and the window controls are unboxed glyphs in the top chrome
row, with soft rounded hover backgrounds. Window controls keep fixed hit areas
flush with the top and right edges; Close includes the exact top-right corner.
Paint matching inset rounded tiles for hover, separate from the rectangular
hit targets. Keep the glyph sizes and stroke weights optically consistent.
Animate glyphs only, and draw focus outlines inside the edge so they stay
visible. Do not enclose these controls in a separate tinted title bar.

Hero backdrops extend behind the chrome to the top edge using the shared
`--content-padding-top` and `--content-padding-inline` insets. Plain pages use
the app background without a separate decorative header band.

Page scrolling remains native, with an overlay thumb instead of a reserved
scrollbar gutter. Backgrounds reach the full right edge beneath it. The thumb
starts below the window controls, ends above the player, and supports dragging,
track paging, wheel input, and keyboard navigation. It disappears when the page
does not overflow. Local scroll areas, such as lyrics and menus, keep their own
scrollbars. Do not hide scroll affordances without an accessible replacement.

Now playing fits the actual grid space above the player, not a guessed player
height subtracted from `100vh`. Album and artist layouts anchor the artwork and
credits at the top of their panel; the lyrics layout keeps its centered context.
Artwork shrinks within that space, preserving
one square composition and one crop per image. Lyrics own their scroll viewport;
the page does not scroll. Only exceptionally long metadata needs a local scroll
area: all credited artists must remain accessible.

## Accessibility

- Keep visible `:focus-visible` outlines; hover is never the only state signal.
- Icon-only buttons need an accessible name and a tooltip or `title` when their
  meaning is not obvious.
- Selected controls expose `aria-pressed`, `aria-current`, or the appropriate
  native checked state.
- Minimum contrast is determined by semantic foreground/background roles, not
  by assuming light or dark text on a custom accent.
