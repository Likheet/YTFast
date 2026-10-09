# Dynamic Background theme

A third look, chosen in Settings (Theme), beside YouTube Music's own and
Premium. Added on 10 October 2026.

It recreates chengggit's "Dynamic Background" theme for the Better Lyrics
extension (<https://github.com/chengggit/YouTube-Music-Dynamic-Theme>, MIT
licence, Copyright (c) chengggit), the second most installed theme in Better
Lyrics' store, with its default settings. The sizes and colours were
measured on music.youtube.com wearing the theme, signed out, 1280 wide. The
moving background follows Better Lyrics' kawarp (MIT): the cover, blurred,
its coordinates moved by slow waves.

## What it looks like

- Behind the whole window, the playing song's cover: blurred (60), its
  colours richer (×1.5) and darker (×0.7), zoomed 1.2 times. A very light
  cover is darkened further so white words stay readable. A new song's
  colours fade in over 1.5 s. Before anything plays, a plain `#0e0e0e`.
- The colours drift slowly while a song plays, and stop when it is paused
  or the window is hidden. Settings' "Moving background" keeps them still.
- The top bar, the menu on the left and the player bar are clear: the
  background shows through them. Menus, dialogs, the search suggestions,
  toasts and the bar of ticked songs are glass (the background under them,
  a breath of white), corners 24 (12 for the suggestions and toasts), with
  the theme's shadow (`0 4px 16px` black at 20%).
- Text, icons and the accent are white; second lines white at 70%. The
  progress line is white.
- Buttons are white at 10% (20% under the pointer): chips corners 12,
  buttons with words corners 16, the shelves' arrows rounded squares,
  Subscribe white at 20%.
- Everything is set in Inter (already in the app; Roboto in the other two
  looks).
- Covers: cards and large covers corners 20, middling 12, small 4.
- Song rows: an album's and a playlist's 56 high (cover or number 38, 24
  after it); an artist's top songs 54; Up next 58, 8 apart, covers 42; no
  lines between rows.
- An artist's picture at 80%, fading out over its lower 40% into the
  background, across the whole window.
- The player page: the cover at most 400 (corners 20, shadow) and the tabs
  in white. Lyrics as Better Lyrics sets them in the theme: Inter 40
  semibold (smaller in a narrow window), the line being sung white with a
  short glow, the next one half lit, the others at 30% and blurred; the
  pointer lights a line, and scrolling by hand lifts the blur.

## In the code

- `src/dynamic.rs`: the colours, the background (`paint`, `wash`), glass,
  and the theme's buttons, chips and switch.
- `src/views/dynamic.rs`: the player page and lyrics, toasts, the row
  sizes, cover corners and the other pieces that differ.
- Elsewhere, each place that differs asks `theme::dynamic()` and hands
  over to those two files, so the other looks' code stays as it is.
- `backend::Picture::soft` keeps each cover at 24 × 24 (about 2 KB) for
  the background (`Images::soft`).
- The choice is saved as YouTube Music's look plus `dynamic_background:
  true` (`Settings::loaded`, `dynamic::save_theme`), so a build without
  this theme still reads the settings, showing YouTube Music's look,
  instead of forgetting them all (the sign-in's browser among them).

## What it costs

Measured in the demo on the owner's Windows laptop (release build, 1280 by
820): playing with the background moving, about 9% of one processor core
on Home (15 frames a second); with it still, 1.7%; minimised while
playing, 0.3%; paused, 0%. Memory
about 118 MB, as in the other looks. With the lyrics showing, the window
draws 30 times a second in every look (about 37% in YouTube Music's look,
47% in this one, with the blurred lines).
