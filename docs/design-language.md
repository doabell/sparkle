# Interface language

Use the semantic tokens in `src/app.css` and shared components. Sparkle uses
large artwork, clear type, rounded controls, and quiet navigation. Album art is
square; artist imagery is circular.

## Interaction and controls

- Rows, cards, and contained controls change background across their full hit
  area using `--interactive-hover` and `--interactive-active`. Text and icon
  links change color. Feedback is immediate; borders describe structure.
- Prominent actions and interactive artwork may scale with
  `--motion-hover-scale` and `--motion-press-scale`. Metadata and passive surfaces
  stay still. Avoid lifting, bouncing, and decorative hover glows.
- Standard controls use `--control-height` and pill-shaped `--control-radius`;
  compact controls use `--control-height-sm`. Inputs and navigation rows use
  smaller surface radii.
- Use `.segmented-control` for mutually exclusive choices and `.control-cluster`
  for related adjustments. Each group has one quiet fill without nested borders.
- Layered rows have one full-row hit target, with independent actions above it.
  Clickable areas must look clickable.

Search uses `SearchField`, `SearchFeedback`, and shared `search-dialog` styles.
Keep headings and actions visible while results scroll. Lyrics search is 28rem,
artist editing 32rem, and the LRC editor 44rem, bounded by the viewport. Empty,
failed, and partial results have distinct states with expandable provider details.

## Surfaces and color

Use spacing, headings, and row dividers before adding containers. Reserve filled
surfaces for artwork, inputs, previews, and selected options. Settings uses open
sections separated by a single rule.

Appearance supports System (default), Light, and Dark, with a validated preference
restored before first paint. Light mode uses a white canvas, neutral surfaces,
solid gray hover fills, and small shadows. Artwork backdrops stay at or below
6% opacity over white. The light player bar is opaque with a thin divider.
Dark mode uses its own surfaces, translucency, and shadows.

The accent is a seed. Use its semantic roles:

| Role                      | Token                                           |
| ------------------------- | ----------------------------------------------- |
| Text                      | `--color-accent-content`                        |
| Icons and indicators      | `--color-accent-graphic`                        |
| Filled actions            | `--color-accent-fill` and hover/active variants |
| Text/icons on accent fill | `--color-on-accent-fill`                        |
| Selection                 | `--color-accent-subtle`                         |
| Keyboard focus            | `--color-accent-focus`                          |

Avoid hard-coded theme colors. Success and error states use their own roles.

## Motion and lyrics

| Purpose                                    | Token                    | Duration |
| ------------------------------------------ | ------------------------ | -------- |
| Background, color, border, shadow feedback | `--transition-feedback`  | 0 ms     |
| Hover/press transforms                     | `--transition-transform` | 220 ms   |
| Opacity feedback                           | `--motion-duration-fast` | 140 ms   |
| Menus and notifications                    | `--motion-duration-base` | 220 ms   |
| Page/section entrance                      | `--motion-duration-slow` | 360 ms   |

Use `--motion-ease-standard` for interaction and `--motion-ease-enter` for
entrances. Entrances use opacity or a small zoom and release their transform on
completion. Toasts dismiss immediately. Progress tracks and scrollbars keep a
constant hover size.

The OS reduced-motion preference and Sparkle's Reduce motion setting disable
animations, transitions, hover/press scaling, and smooth scrolling. Keep transforms
needed for positioning, switch state, or artwork cropping. First paint stays still
until the preference is available.

Synced lyrics use `LYRIC_TRANSITION_DURATION_MS` for both animation and playback
anticipation. Fixed font metrics prevent active lines from rewrapping; only the
lyric panel auto-scrolls. Reduced motion uses immediate emphasis at the timestamp.
Blank timed cues are ignored, and the first nonempty sentence remains visible
during the intro. **Adjust** applies the offset to timestamps and resets it;
**Save** preserves the offset; **Export** does not change the library.

## Layout and accessibility

- Use `.page-shell`, `.page-header`, `.page-heading`, `.page-title`, and
  `.page-enter`. Add subtitles only when they explain a constraint or next step.
- Labels use title case and normal letter spacing; preserve acronyms and metadata.
- Back and window controls are unboxed glyphs with fixed hit areas. Close includes
  the top-right corner. Keep hover tiles and focus outlines inside those areas.
- Hero backdrops extend behind the chrome using `--content-padding-top` and
  `--content-padding-inline`. Plain pages use the app background.
- Page scrolling is native with an accessible overlay thumb; local panels retain
  their own scrollbars. Now Playing fits the grid above the player, keeps artwork
  square, and gives lyrics their own scroll viewport. All credits stay reachable.
- Preserve visible keyboard focus. Icon buttons need accessible names; selected
  controls expose native checked state or appropriate ARIA state. Contrast comes
  from semantic foreground/background pairs, including custom accents.
