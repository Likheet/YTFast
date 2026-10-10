# Premium theme

A second look, chosen in Settings (Theme). YouTube Music's own look stays
the default, and everything in this folder's other notes is about that
one. Premium was first made on `codex/premium-ui` (9 October 2026),
became a setting on 10 October 2026, and was reworked twice that day at
the owner's word.

- One neutral charcoal (`#111113`) behind everything. The menu and the
  top bar are see-through (the top bar stays so when the page scrolls,
  with only a hairline under it: nothing scrolls under it), so there are
  no grey bands. Text `#f5f5f7`, quieter text `#aeaeb4`.
- A background of its own (`backdrop::ambient`): the playing song's
  colours across the window's top, fading out by 70% of its height; with
  nothing playing, a deep wine-to-indigo glow. Pages with a background of
  their own (an album's cover, an artist's picture) keep theirs.
- The player page: the playing song's colours fill the whole window,
  behind the menu, the bars and the page (`backdrop::listening`): the
  cover kept at 16 by 16, drawn up to 48 by 48 and blurred, so its light
  and dark parts stay where they were, its colours made richer and kept
  dark enough for white words, darkened toward the foot and the corners.
  The cover (at most 480, r 16) with the song's name and artist centred
  under it; beside it the tabs and what they show, straight on the song's
  colours with no box round them, as YouTube Music sets a playlist's songs
  beside its cover. The tabs are clear glass: a capsule barely filled with
  no edge, the chosen tab a clear capsule with one thin rim. Up next's
  covers are 48 (rows 64); the playing song keeps its cover, lightly
  darkened, with a white disc and its mark on it (bars while it sounds).
  Lyrics are large and fade out over the last 56 at the column's top and
  foot.
- Everything stays still, so an idle window costs nothing (measured: 0%
  of a core over 10 s, paused on the player page).
- The accent is Apple Music's pink-red (`#fa2d48`), which sits well beside
  the red logo: the main Play buttons (white icon on it), switches, the
  song's line, focus rings.
- Glass for whatever can be pressed: the search box (white@0.07, more
  under the pointer and while typing; solid while its suggestions show),
  New playlist, the menu's lit entry, the Liked Music tile, the tabs,
  Settings' cards (white@0.05 with a white@0.06 edge).
- The top bar, balanced and on one middle line: back and forward just past
  the menu, the search box (at most 560) in the middle of the page, the
  account 16 before the window's buttons.
- The player bar is a glass panel floating over the page (inset 12 and 8,
  corners 16, `#202024` at 90%, a white@0.12 edge, a light along its top,
  a shadow). The song at the left; a middle column (36% of the bar, 28%
  under 1150 wide; 240 to 560) with previous, play on a white disc and
  next, and under them the song's line (4 thick, 6 under the pointer, on
  white@0.18, a knob only then) between the time played and the length,
  12 in the quieter grey; the other buttons at the right. Like, dislike
  and the menu are 22 and quieter, so play stands out; the player page's
  button is a chevron drawn in lines, not a solid triangle.
- Roboto and Material Symbols, as the default look; no new fonts.
- Shelf titles 28 (24 under 1150); page titles 36; an album's or a
  playlist's title 48 (32 in less room).
- Even margins (56 from a window 1364 wide, 40 from 1150, else 24), at
  most 1440 wide; covers about 200 wide, 20 apart; songs in up to three
  columns. Settings' column is at most 760.
- Corners 8 to 16 everywhere, no hairlines between songs, and a thin
  accent line round whatever the keyboard is on.
- The menu: "YOUR LIBRARY", each playlist a small cover and its name (who
  made it shows under the pointer).
- Albums and playlists: the cover (248, r 12) beside the words, which are
  at most 760 wide (the description 3 lines), then the songs at full
  width; in a narrow page the cover over the words. Their blurred cover at
  the top is veiled less than YouTube Music's (black 43%, not 60%).
- Dialogs share one frame (corners 16); the playlist form has boxed
  fields.

## In every look

- The window's own buttons (Windows) are as tall as the top bar, their
  glyphs on its middle line with the bar's other buttons, in its white.
- Settings is one column in the middle of the page (at most 640;
  Premium's 760).
- A mood button on Home (or a filter over Liked Music) keeps the page
  showing, dimmed, with the button lit at once, until the new page
  arrives (`page::stand_in`), instead of emptying it.

## In the code

`theme::set` is called at the start of each frame with the chosen theme
(`Settings.theme`); `theme::premium()` says which is drawn. `PALETTE`
gives the chosen theme's colours, and the sizes that differ are functions
(`top_bar_height()`, `player_bar_height()`, `guide_width()`,
`page_foot()`, `menu_edge()`, `Row::themed`). So:

- Read colours and those sizes while drawing, never into a `const`.
- A screen that differs in Premium keeps YouTube Music's code as it is
  and adds its own behind `theme::premium()`.
- egui's own colours are set again when the theme changes
  (`theme::restyle`).
- The panels and the page all paint on egui's background layer, in order,
  so whatever must lie under the menu and the bars goes in one of the two
  places `backdrop::paint` keeps at the frame's start (the album's cover,
  then Premium's colours), not painted later. A page with a background of
  its own says so (`App::page_backdrop`), and Premium's ambient colours
  then stay off.

Playback, sign-in, account changes and the limits on requests are the
same in both.
