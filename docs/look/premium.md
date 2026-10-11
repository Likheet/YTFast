# Premium theme

A second look, chosen in Settings (Theme). YouTube Music's own look stays
the default, and everything in this folder's other notes is about that
one. Premium was first made on `codex/premium-ui` (9 October 2026),
became a setting on 10 October 2026, and was reworked twice that day at
the owner's word.

On 11 October 2026 it took YouTube Music's layout: the owner's brother
found things moving around when the look changed (the player's buttons
jumped 560 across, Settings' rows 229 down). Every part is now where
YouTube Music's look puts it, at the same size, and Premium differs only
in how it draws them: colours, glass, corners and its background. A test
checks it (`every_look_puts_things_in_the_same_places` in `app.rs`).

- One neutral charcoal (`#111113`) behind everything. The menu and the
  top bar are see-through (the top bar stays so when the page scrolls,
  with only a hairline under it), so there are no grey bands. Text
  `#f5f5f7`, quieter text `#aeaeb4`.
- A background of its own (`backdrop::ambient`): the playing song's
  colours across the window's top, fading out by 70% of its height; with
  nothing playing, a deep wine-to-indigo glow. Pages with a background of
  their own (an album's cover, an artist's picture) keep theirs.
- The player page: the playing song's colours fill the whole window,
  behind the menu, the bars and the page (`backdrop::listening`): the
  cover kept at 16 by 16, drawn up to 48 by 48 and blurred, so its light
  and dark parts stay where they were, its colours made richer and kept
  dark enough for white words, darkened toward the foot and the corners.
  The cover and the video have corners 16. The tabs are clear glass: a
  capsule barely filled with no edge, the chosen tab a clear capsule with
  one thin rim. Lyrics are larger and fade out at the column's top and
  foot.
- Everything stays still, so an idle window costs nothing (measured: 0%
  of a core over 10 s, paused on the player page).
- The accent is Apple Music's pink-red (`#fa2d48`), which sits well beside
  the red logo: the main Play buttons (white icon on it), switches, the
  song's line, focus rings.
- Glass for whatever can be pressed: the search box (white@0.07, more
  under the pointer and while typing; solid while its suggestions show,
  corners 12), New playlist, the menu's lit entry, the tabs, Settings'
  cards (white@0.05 with a white@0.06 edge, in the room between the
  settings, so the settings stay where they are).
- The player bar is drawn as a glass panel over the page (4 in from the
  bar's edges, corners 14, `#202024` at 90%, a white@0.12 edge, a light
  along its top, a shadow), its buttons in YouTube Music's places. Play
  is on a white disc; like, dislike and the menu are 22 and quieter, so
  play stands out; the song's line is the accent.
- Roboto and Material Symbols, as the default look; no new fonts.
- Corners 8 to 16 everywhere (a row's cover 8, Up next's 6), no hairlines
  between songs, an album's and a playlist's songs lit under the pointer,
  and a thin accent line round whatever the keyboard is on.
- Dialogs share one frame (corners 16, the window behind darker); the
  playlist form has boxed fields, a little taller than YouTube Music's.
  (No look can be chosen while a dialog is open, so nothing moves.)

## In every look

- The window's own buttons (Windows) are as tall as the top bar, their
  glyphs on its middle line with the bar's other buttons, in its white,
  the last 4 from the window's edge; close answers the pointer up to the
  window's top right corner.
- Settings is one column in the middle of the page, at most 640 wide.
- A mood button on Home (or a filter over Liked Music) keeps the page
  showing, dimmed, with the button lit at once, until the new page
  arrives (`page::stand_in`), instead of emptying it.

## In the code

`theme::set` is called at the start of each frame with the chosen theme
(`Settings.theme`); `theme::premium()` says which is drawn. `PALETTE`
gives the chosen theme's colours. The sizes are YouTube Music's in every
look (`top_bar_height()`, `player_bar_height()`, `guide_width()`,
`page_foot()`, `theme::Grid`, `Row::themed` changes only corners, lines
and the fill under the pointer). So:

- Read colours while drawing, never into a `const`.
- A part that looks different in Premium is drawn differently behind
  `theme::premium()`, in the same place and at the same size.
- egui's own colours are set again when the theme changes
  (`theme::restyle`).
- The panels and the page all paint on egui's background layer, in order,
  so whatever must lie under the menu and the bars goes in one of the two
  places `backdrop::paint` keeps at the frame's start (the album's cover,
  then Premium's colours), not painted later. A page with a background of
  its own says so (`App::page_backdrop`), and Premium's ambient colours
  then stay off.

Playback, sign-in, account changes and the limits on requests are the
same in every look.
