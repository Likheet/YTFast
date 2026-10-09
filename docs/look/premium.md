# Premium theme

A second look, chosen in Settings (Theme). YouTube Music's own look stays
the default, and everything in this folder's other notes is about that
one. Premium was first made on `codex/premium-ui` (9 October 2026) and
became a setting on 10 October 2026.

- Charcoal background, raised dark surfaces and a warm accent (`#dec59b`).
- Roboto and Material Symbols, as the default look; no new fonts.
- Page titles 28 to 36; shelf titles 20 to 22.
- Even margins (40, or 24 under 1150), at most 1440 wide; covers about 200
  wide, 20 apart; songs in up to three columns.
- Corners 8 to 16 everywhere, and a thin accent line round whatever the
  keyboard is on.
- The menu: the panel's colour, "YOUR LIBRARY", each playlist a small
  cover and its name (who made it shows under the pointer).
- Albums and playlists: the cover beside the words across the top, then
  the songs at full width; in a narrow page the cover over the words.
- The player bar floats: the song on the left, previous, play and next in
  the middle with the time under them, the other buttons on the right,
  and the progress line in the accent along its foot.
- The player page: the cover (at most 420) with the song's name under it,
  over a still wash of the cover; the tabs as one rounded strip; larger
  lyrics.
- Dialogs share one frame (corners 16); the playlist form has boxed
  fields. Settings sit on cards.

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

Playback, sign-in, account changes and the limits on requests are the
same in both.
