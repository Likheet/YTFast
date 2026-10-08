# YouTube Music, measured

What music.youtube.com looks like, screen by screen, for copying it one to
one. Measured on 8 October 2026 in a browser 1280 by 820 (CSS pixels, which
are the app's points), signed out unless a section says otherwise, with
`tools/look/measure.js` and YouTube Music's own stylesheet (see
[README.md](README.md) for how). Positions are `x,y` from the window's top
left; sizes `width x height`. Colours are `#rrggbb`, with `@0.70` for an
alpha. A font is written `size/weight/line height`.

Where a value comes from the stylesheet rather than from a measurement, it
says so ("CSS").

## The whole window

### Fonts

- Text is **Roboto** (400, 500, 700). Big titles are **YouTube Sans**
  (500, 700): shelf titles, page titles, Explore's buttons. Both come from
  Google Fonts (`fonts.googleapis.com/css?family=Roboto`,
  `...family=YouTube+Sans:500,700`).
- Roboto's licence (Apache 2.0) lets it be shipped in the app. YouTube
  Sans is Google's own brand font with no published licence: it can be
  shown in a browser from Google, but shipping it inside YTFast is not
  clearly allowed. See the decision in [gaps.md](gaps.md).
- The body's base size is 10px; every piece of text sets its own.

### Colours (CSS and measured)

| What | Colour |
|---|---|
| Behind everything (body) | `#030303` |
| Top bar once the page scrolls | `#030303`, hairline under it `#ffffff@0.15` (both fade in; transparent at the top of a page) |
| Search box | `#ffffff@0.15`, border `#ffffff@0.15` 1px, r 8 |
| Chips (mood filters) | `#ffffff@0.10`, r 8 |
| Explore's buttons and mood buttons | `#ffffff@0.15`, r 8 |
| Chosen menu entry | `#ffffff@0.10`, r 8 |
| Secondary text (subtitles) | `#ffffff@0.70` |
| Strapline over a shelf title | `#aaaaaa`, upper case |
| Quiet text (sign-in promo) | `#909090` |
| Divider in the menu | `#ffffff@0.15` 1px |
| Pop-up menu entry under the pointer (CSS) | `#ffffff@0.05` (`--ytmusic-menu-item-hover-background-color`) |
| Left menu entry under the pointer (CSS) | `#ffffff@0.20` (`--ytmusic-guide-hover`) |
| Dropdown entry under the pointer (CSS) | `#212121` |
| Switch on (CSS) | `#3ea6ff`; off `#909090`, its bar `#fff` |

### The page grid (CSS, confirmed by measuring)

The page is not a fixed margin: YouTube Music sets one width for every
page's content, `--ytmusic-content-width`, and centres it.

- Menu (guide) width: **240** open, **72** closed (the strip of icons);
  under 936 wide it is always 72; under 616 there is none. The window
  remembers whether the user closed it (`guide-collapsed`).
- Content width, the window `W` wide, `G` the menu's width, `S` the
  scroll bar (12):
  - W ≥ 1150: `min(W − 200 − G − S, 1478)`
  - 616 ≤ W < 1150: `min(W − 112 − G − S, 1478)`
  - W < 616: `W − 32`
- The content is centred in what is left of the window, so the margin on
  the left is `(W − G − S − content) / 2`: **100** from 1150 wide (98.3
  measured, as this browser's scroll bar is 15.3, not 12), **56** below.
  At 1280 with the menu open the content is 828 wide; with it closed, 996.
- The search page is narrower: `min(860, W − 112 − G − S)` (from 616
  wide); `--ytmusic-search-width` is 860 from 1150, 720 from 936, 560
  from 616.
- Player bar height: 72 (64 under 936). Page padding at the bottom: 112
  (`--ytmusic-base-page-padding-bottom`), so the last row clears the bar.
- Header padding (`--ytmusic-header-padding`): `32 0 16 0` from 1150,
  `32 0 24 0` from 1578, `16 0 16 0` from 936, `16 0 8 0` below.

YTFast today: `page_margin(width) = clamp(width × 0.09, 24, 100)`
(`theme.rs`), which is close at 1280 but not the same rule.

## Top bar (`ytmusic-nav-bar`)

64 high, the whole window wide.

| Part | Measured |
|---|---|
| Left part | padding-left 16 |
| Menu button | 40×40 at 16,12, round; icon 24 at 24,20; margin-right 12 |
| Logo | 71×24 at 68,20 (an image: the red play mark and "Music") |
| Middle part | starts at the guide's right edge (72 or 240), padding-left 100 (`center-content`) in both menu states from 1150 wide; under 1150, 56 with the menu open, or the box centred with it closed (CSS) |
| Search box | 480 wide, **42 high (40 + a 1px border above and below) at y 11**, at x 172 (menu closed) or 340 (open); fill `#ffffff@0.15`, border 1 `#ffffff@0.15`, r 8. (This browser at 150% drew the border 0.67 wide, so it measured 41.3 high.) |
| Search icon | a 32×32 button 11 in from the box's left (inside the border, margin 4 10), y 16; glyph 18 |
| Search words | 16/400/19.2, placeholder `#ffffff@0.50` "Search songs, albums, artists, podcasts"; the box for typing starts 53 in from the box's left |
| Right part | padding-right 100 (at 1280) |
| Cast button | 40×32 (shows only where casting is possible) |
| ⋮ menu (signed out) | 24×24, margin 8 |
| Sign in (signed out) | 74.6×32, white, r 16, 14/500 `#030303`, padding 0 16 |

## Menu on the left (`ytmusic-guide-renderer`)

### Open (240 wide)

- Starts under the top bar (y 64), padding 8 top and bottom; each section
  has 8 on each side.
- Each entry: 224×48, r 8, padding 0 16; icon 24 at 16 in, then 20 gap;
  words 16/400/24 white (500 when it is the open page). The open page's
  entry has `#ffffff@0.10` behind it.
- Under the three main entries: a divider 192 wide, 1px `#ffffff@0.15`,
  margin 24 16.
- Signed out, then: a "Sign in" button 192×36 (r 18, `#ffffff@0.10`,
  14/500 `#f1f1f1`) and a line under it (12/400/16.8 `#909090`).
- Signed in: "New playlist" and the playlists. (To measure, signed in.)

### Closed (72 wide)

- Entries 56×65, at x 8, one under the other from y 72; r 8, padding 12
  0; icon 24 centred, 5 under it the word 10/500/12 white.
- The open page's entry has `#ffffff@0.10` behind it.

## Home

- Chips (moods: Podcasts, Relax, Sad...): the row sits at the top of the
  content; each chip is a box 36 high (r 8, `#ffffff@0.10`), words 14/400
  white with 12 each side, 12 apart (margin 6). The row's first chip
  starts at y 110 (padding 40 above inside a 84-high slot from y 70).
- Shelves (`ytmusic-carousel-shelf-renderer`), each followed by 24:
  - Header: padding 32 above. Optional strapline 14/400 `#aaaaaa`, upper
    case, 2 under it; then the title 28/700/33.6 **YouTube Sans** white.
    At the right, the two arrows: 36×36 round, border 1 `#ffffff@0.20`,
    icon 18.7, 16 apart; the one that cannot move is at opacity 0.4.
    (Some shelves have a "More" button there too.)
  - Cards start 16 under the header.
- Cards (`ytmusic-two-row-item-renderer`, medium size): **the cover's width
  is set so that a whole number of cards fill the content**: 5 across at
  1280 (180 wide with the menu closed, 146.4 with it open), 24 apart.
  - Cover square, r 8 (measured on Home's playlists).
  - Under the pointer: a gradient from `#000000@0.50` at the top to clear
    at a third of the way down; a round play button 40×40
    (`#000000@0.60`, icon 24) whose centre is 32 in from the cover's right
    and bottom; a ⋮ button 36×36 (r 18) 4 from the right and 8 from the
    top. (Their opacity goes from 0 to 1.)
  - Title 16 under the cover: 14/500/16.8 white, up to 2 lines.
  - Subtitle 3 under the title: 14/400/16.8 `#ffffff@0.70`, up to 2 lines.
- The page's top: the chips start right under the top bar; Home has a
  wide picture behind its top on some days (`ytmusic-fullbleed-thumbnail-renderer`).

### How many cards fit (measured on Home, menu open, signed out)

| Window | Content | Cards | Card width | Between |
|---|---|---|---|---|
| 960 | 596 | (fixed size, the row scrolls) | 180 | 16 |
| 1150 | 698 | 5 | 120.4 | 24 |
| 1280 | 828 | 5 | 146.4 | 24 |
| 1280, menu closed | 996 | 5 | 180 | 24 |
| 1440 | 988 | 6 | 144.7 | 24 |
| 1700 | 1248 | 6 | 188 | 24 |
| 1920 | 1468 | 6 | 224.7 | 24 |

So the number of cards goes by the **window's** width, not the content's:
5 from 1150 to 1363, 6 from 1364 (1578 and up still 6 at 1700 and
1920), each `(content − 24 × (n − 1)) / n` wide. Under 1150 the cards
keep 180 with 16 between. (The stylesheet should hold these as media
rules on the medium card size.) YTFast today: `round(window / 256)`
cards, 120 to 226 wide.

## Explore

- Three big buttons at the top (New releases, Charts, Moods & genres):
  each 260×56 at 1280 with the menu open (a grid with 24 between),
  `#ffffff@0.15`, r 8, padding 8 16; icon 24 then 12; words 18.4/700/22.1
  **YouTube Sans** white. The grid has margin 32 above and 56 below.
- "New albums & singles": a shelf of cards, as on Home.
- "Moods & genres": buttons in 4 rows that scroll sideways together, each
  146.4×48 (the card width), 16 apart down and 24 across; `#ffffff@0.15`,
  r 8, padding 0 12, a **6 wide coloured stripe on the left** (each mood
  its own colour, e.g. Chill `#a4c5ff`, Commute `#ffc200`, Energize
  `#ffe780`); words 14/500 white.
- "Trending": songs in columns of 4 that scroll sideways. Each row 436×48
  (64 apart), padding 0 8: cover 48 (r 4), then the place number (14/500
  white, in a 52-wide column, 16 after it), then the title 14/500/16.8
  white and under it (3 between) the artist and views 14/400/16.8
  `#ffffff@0.70`.
- "New music videos": a shelf of wide cards (16:9).

## Search box and suggestions

- Typing opens the list under the box; the box's lower corners go square
  (r 8 8 0 0) and its fill turns `#030303`, with a shadow
  (`0 8px 10px 1px #000@0.14`...). A clear (×) button 32×32 shows at the
  right (icon 18).
- The list (`#suggestion-list`): 480 wide, right under the box, `#030303`,
  border 1 `#ffffff@0.15`, r 0 0 8 8, padding 8 0.
- First, up to 6 text suggestions: rows 48 high; a search icon 18 in a
  50-wide column (padding 0 16); the words 14/400/16.8 `#ffffff@0.50`
  with what was typed in 500 weight, padding-left 5.
- Then up to 4 rich suggestions (an artist, songs, albums): rows 56 high,
  padding 0 12; picture 32 (round for an artist, r 4 otherwise), 16
  after it; title 14/500/16.8 white; under it (3 between) "Song •
  Coldplay • 2.4B plays • Parachutes" or "332M monthly audience" in
  14/400/16.8 `#ffffff@0.70`.

## Search results (`ytmusic-search-page`)

- The page is 860 wide from 1150 (`--ytmusic-search-width`), with the
  usual left margin (x 322.3 at 1280 with the menu open).
- Filter chips at the top, one row that scrolls sideways (arrows at its
  ends): Artists, Community playlists, Songs, Albums, Videos, Profiles,
  Featured playlists, Podcasts, Episodes. Each 32 high (not 36 as on
  Home), r 8, `#ffffff@0.10`, words 14/400 white with 12 each side, 12
  apart. The row starts at y 70.
- Top result: a card 860×232 with 32 under it, r 8, in two halves of
  430:
  - Left (`#ffffff@0.10`): picture 100×100 (round for an artist), 16 in
    from the left, centred down; 16 after it the name 24/700/28.8 **YouTube
    Sans**, 8 under it "Artist • 332M monthly audience" 14/400/16.8
    `#ffffff@0.70`; 12 under that the buttons, 36 high, r 18, 16 apart:
    Shuffle (filled `#f1f1f1`, words 14/500 `#0f0f0f`, icon 24 with 6
    after) and Mix (outlined 1 `#ffffff@0.20`, words `#f1f1f1`). A small
    ⋮ button (24, icon 16) at the left half's top right.
  - Right (`#ffffff@0.05`, padding 16, 16 between): three songs, rows 56
    high: cover 56 (r 4), 16 after it the title 14/500/16.8, under it
    (4 between) "Song • 3:37" and "53M plays", 14/400/16.8
    `#ffffff@0.70`.
- Then **one flat list** of everything else (artists, playlists, songs,
  albums, videos, podcasts mixed, in YouTube's order), not grouped under
  headings: each row 80 high, padding 0 8; picture 56 (round for an
  artist, r 4 otherwise), 16 after it; title 14/500/16.8 white; under it
  (4 between) "Artist • 72.5M monthly audience", "Song • The Chainsmokers
  & Coldplay" then "3.3B plays" (a column of its own, 4 apart), in
  14/400/16.8 `#ffffff@0.70`.
- Under the pointer a song row shows a round play button 32×32 over its
  cover (the cover darkened by `#000000@0.80`... see CSS).

### Search results narrowed to one kind (a chip chosen, e.g. Songs)

- The chip row then starts with a white square 32×32 (r 8, `#ffffff`,
  an × icon 24 in black) that clears the choice; the other kinds follow
  as before.
- No top result card. One heading ("Songs") 24/700/28.8 **YouTube Sans**,
  16 above and below, then rows 80 high with a hairline `#ffffff@0.10`
  under each (80.7 in all), padding 0 8, laid out as in the flat list.
  More rows load as the list scrolls.

## Artist (`ytmusic-immersive-header-renderer`)

- A wide picture across the top, from the window's top (behind the top
  bar), 24 under it. The header's height is not the picture's shape: it
  is 228 of padding + 60 + the words and buttons (167.6), so 455.6 at
  1280. The picture fills it (`fit: cover`); measured with the menu open
  it starts at x 168 (margin-left 168 = 240 − 72), so its left part lies
  under the open menu (with the menu closed it starts at 0, under the
  icon strip). Its lower half (from 228 down) is covered by a gradient,
  `linear-gradient(0deg, #030303 9%, transparent 100%)`, with padding 60
  above the words and the page's left margin; the CSS adds a flat
  `#000000@0.40` layer over the picture (see gaps.md).
- Name 28/700/33.6 **YouTube Sans** white; under it "332M monthly
  audience" 14/400/16.8 `#ffffff@0.70`; 16 under, the description
  14/400/19.6 white, 640 wide, cut to 2 lines (links in it `#3ea6ff`);
  18 under, the buttons, 36 high, 8 apart:
  - Shuffle and Mix: filled `#f1f1f1`, r 18, 136 wide, icon 24 (6 after
    it), words 14/500 `#0f0f0f`.
  - Subscribe: outlined 1 `#ff5577`, words 14/500 `#ff5577` with the
    count ("Subscribe 28.6M"), r 18, padding 0 15.
  - ⋮ 36×36 round.
- Top songs (`ytmusic-shelf-renderer`, 32 under it): heading 24/700/28.8
  **YouTube Sans**, 16 above and below; 5 rows 48 high (the hairline
  makes 48.7), padding 0 8, a hairline `#ffffff@0.10` under each but the
  last; cover 32 (r 4), 24 after it; then columns: title 14/500/16.8
  white (a column 245 wide at this width), then artist, plays, album
  (each 106.7, 16 apart), all 14/400/16.8 `#ffffff@0.70`. At the right
  (shown under the pointer): like and dislike 36×36 and ⋮ 36×36. Under
  the rows a "Show all" button: outlined 1 `#ffffff@0.20`, 36 high, r 18,
  words 14/500 `#f1f1f1`.
- Then shelves of cards (Albums, Singles, Videos, Fans might also like,
  Featured on...) as on Home, headings 28 YouTube Sans... (Albums'
  heading box is 36 high here, without a strapline.)

## Album (`ytmusic-two-column-browse-results-renderer`, `is-album-detail-page`)

- Two columns side by side, the pair centred: at 1280 with the menu open
  the pair is 882 wide at x 311.3 (margins 71.3). CSS:
  `--max-width-lhs` 424, `--max-width-rhs` 602, `--margin-between-sections`
  24, `--minimum-page-side-margin` 56, and the left column's content
  `min(100% − 32, 424 − 32 − 24)` = 280 wide with 16 each side.
- Behind the page's top: the album's cover itself (`#background`,
  `immersive-background`), blurred by 80px over the top 49% of the
  window, under `linear-gradient(#000000@0.60, #030303)` (996 high at
  1280×820: padding 64 top, 112 bottom). It scrolls away with the page,
  and the top bar turns solid `#030303` once the page scrolls.
- Left column (`ytmusic-responsive-header-renderer`, 280 wide), centred
  text, 64 from the top bar:
  - Strapline: the artist's small round picture and name, 14/400/16.8
    white.
  - The cover: a 264-wide box (margin 16 8) holding the picture
    **240×240** at 1280, r 12 (245 from 1364 wide, 264 from 1578; see
    gaps.md).
  - Title 28/700/33.6 **YouTube Sans** white (the `h1` 20/700).
  - 8 under: "Album • 2024" 14/400/16.8 `#ffffff@0.70`; then "10 songs •
    44 minutes", same.
  - 8 under: the description, 14/400/16.8 `#ffffff@0.70`, cut to 2 lines
    (no "more" link: clicking the block opens it whole in a dialog).
  - 16 under: the buttons, a row 64 high: Save (40×40 round,
    `#ffffff@0.10`, icon 20×18), 32, Play (64×64 white disc, icon 32
    black), 32, ⋮ (40×40 round, `#ffffff@0.10`, icon 24).
- Right column (546 wide at 1280, from x 647.3), 64 from the top bar: the
  songs, rows 68 high with 8 between, r 8, padding 0 16: the track number
  (48 wide, where a cover would be) 16 before the title 14/500/16.8 white;
  under it (8 between) the artists and "7.6M plays" 14/400/16.8
  `#ffffff@0.70`; at the right the length 14/400 `#ffffff@0.70`. Rows
  get **no fill** under the pointer (the playing row has `#ffffff@0.10`).
  Under the pointer: the number turns into a play button, the length
  hides, and a checkbox (to pick several) and like/dislike/⋮ (36 each)
  show. 32 under the list, "You might also
  like" and more shelves, in the right column only.

## Playlist made by a person (`is-playlist-detail-page`)

The same two columns as an album (pair 882 wide at x 311.3 at 1280 with
the menu open). Differences from the album page:

- Left column: no strapline; the cover (240×240 at 1280, r 12) starts 64 from the
  top bar, 16 under it the title 28/700/33.6 **YouTube Sans** (up to 2
  lines); 8 under, the owner as a small avatar 24 and name 12/400/18 white
  (`has-facepile`); 8 under, "Playlist • 2023", then "34M views • 38
  tracks • 2+ hours", both 14/400/16.8 `#ffffff@0.70`; 16 under, the same
  three buttons (Save, Play 64, ⋮).
- Right column: first a **Sort** button (icon 24, 8, "Sort" 14/500/22
  white) 24 high, 16 under it the songs. Song rows as on the album (68
  high, 8 apart, r 8, padding 0 16) but with the **cover 48 (r 4)**
  instead of the number; title 14/500/16.8 white, 8 under it the artist
  14/400/16.8 `#ffffff@0.70`; the length at the right 14/400
  `#ffffff@0.70`; a checkbox under the pointer. Long playlists load more
  as the list scrolls (a spinner 28 at the end).

## Player bar (`ytmusic-player-bar`)

72 high, `#212121`, the whole window wide, under everything. **Its order
is not YTFast's**: the play controls are at the left, the song in the
middle, and the rest at the right.

- Progress line along its top edge: 2 high, the rest of the line
  `#ffffff@0.10`, what has downloaded `#ffffff@0.10` more
  (`secondaryProgress`), what has played red. Its slider is 32 high
  (padding 15) for the pointer; the knob shows under the pointer (CSS).
- Left (`left-controls`, 241 wide at 1280):
  - Previous: 36×36 (icon 24), margin 4 4 4 8.
  - Play/pause: a 40×40 spot, margin 0 12 (shows a 16 spinner while
    loading; CSS for the play icon's size, about 40 icon).
  - Next: 36×36, margin 4 4 4 8.
  - The time "0:00 / 4:38": 12/400/14.4 `#aaaaaa`, margin 0 16 0 8.
- Middle (`middle-controls`, from x 241 to 936 at 1280): the song's picture
  40 high (40×40 for a song; 71×40 for a video), r 2; 16 after it the
  title 14/500/16.8 white and under it "Coldplay & Jon Hopkins • 2.4M
  views • 22K likes" 14/400/16.8 `#ffffff@0.70` (the artist names are
  links); then like and dislike 36×36 each, and ⋮ 36×36 (margin 0 16 0
  8). The group is centred in the middle part.
- Right (`right-controls`, 328 wide at 1280, margin-right 4):
  - Volume slider 100 wide (68 of rail: 2 high, `#909090`, filled white),
    shown only under the pointer, left of the volume button.
  - Buttons 36×36 (icon 24), margin 4 8 4 4: volume, captions (videos
    only), repeat, shuffle.
  - The arrow that opens and closes the player page: 36×36, padding 4.

## Player page (`ytmusic-player-page`)

- Fills the space between the top bar and the player bar, right of the
  menu: at 1280 with the menu open, 1040×684 from 240,64, padding 32 56 0.
- Main panel 537.9×620 at 296,96 (margin-bottom 32): the song's picture,
  square, as big as fits and centred (537.9 at 1280×820); a video is 16:9
  (537.9×302.6). Over it, under the pointer, a gradient (`#000000@0.60` to
  clear at 25%) and two small buttons at the top right (minimise, full
  screen).
- Side panel 334.1 wide, 56 after the main panel, the whole height.
  - Tabs: 48 high, a hairline `#ffffff@0.10` under the row. Each tab is
    as wide as its word + 12 each side; words 14/500 upper case: the open
    tab white, the others `#ffffff@0.70` at opacity 0.8; one that cannot
    be opened (Lyrics, when there are none) `#ffffff@0.30`. A white line
    under the open tab (selection bar). Tabs: **Up next, Lyrics, Comments,
    Related** (Comments appears for videos and some songs).
  - Up next: a header 16 under the tabs: "Playing from" 12/400/16.8
    `#ffffff@0.70`, under it the list's name 14/500/16.8 white; at the
    right a chip (transparent, 32 high: "Save"). Then an **Autoplay** row
    (title 14/500 white, under it "Add similar content to the end of the
    queue" 12/400/14.4 `#aaaaaa`, a switch 36×14 at the right), margin 16
    16 12 8. Then the queue: rows 56 high (+ hairline `#ffffff@0.10`),
    padding 0 8; cover 32 (r 2), 16 after it; title 14/500/16.8 white,
    under it (4 between) the artists 14/400/16.8 `#aaaaaa`; at the right
    the length 14/400 `#aaaaaa` (padding-left 8). The playing row is
    marked `selected` (see CSS for its fill). Under the pointer, the cover
    darkens (`#000000@0.80`) and shows a play button 24.
  - Related: shelves as on Home but narrow: headings 24/700/28.8 (YouTube
    Sans), 32 above; songs in columns 216 wide of 4 rows 48 high (64
    apart), 16 between columns; cards 160 wide, 16 apart; artists round.
  - Lyrics: to measure, signed in, with a song that has them.

## Menus (`ytmusic-menu-popup-renderer`)

- A song's ⋮ menu (from the player bar), signed out, in this order: Start
  mix, Play next, Add to queue, Add to liked songs, Download, Save to
  playlist, Remove from queue, Go to artist, Share, Report, Stats for
  nerds. Every entry has an icon before its words.
- Entries 48 high, padding 0 8 (the rest of the look, fill, corners and
  width, is in the CSS: `ytmusic-menu-popup-renderer`,
  `tp-yt-paper-listbox`, `ytmusic-menu-navigation-item-renderer`,
  `ytmusic-menu-service-item-renderer`). Entry under the pointer
  `#ffffff@0.05`... per `--ytmusic-menu-item-hover-background-color`.
- Note YouTube Music's words: "Start mix" (not "Start radio"), "Save to
  playlist" (not "Add to playlist"), "Share" (not "Copy link").
- A card's menu (right-click or its ⋮), signed out: an album: Shuffle
  play, Start mix, Play next, Add to queue, Save album to library, Save to
  playlist, Go to artist, Share; a playlist: the same with "Save playlist
  to library", without Go to artist, and "Not interested" last. A video
  row in Explore's Trending: Start mix, Play next, Add to queue, Download,
  Save to playlist, Go to artist, Share.
- YouTube names each entry's icon in its replies (`icon.iconType`): Start
  mix MIX, Play next QUEUE_PLAY_NEXT, Add to queue ADD_TO_REMOTE_QUEUE,
  Add to liked songs FAVORITE, Save to playlist ADD_TO_PLAYLIST, Go to
  album ALBUM, Go to artist ARTIST, View song credits PEOPLE_GROUP, Share
  SHARE, Shuffle play MUSIC_SHUFFLE, Subscribe SUBSCRIBE, Save ... to
  library BOOKMARK_BORDER, Go to podcast BROADCAST. Icons are 18 in menus
  (the `yt-icon` is 18×18).

## Keyboard shortcuts (YouTube Music's own list, opened with `?`)

| Does | Keys |
|---|---|
| Play/pause | Space or `;` |
| Next song | `j` or Shift+N |
| Previous song | `k` or Shift+P |
| Forward 10 s | `l` or Shift+→ |
| Back 10 s | `h` or Shift+← |
| Forward 1 s | Shift+L or Ctrl+Shift+→ |
| Back 1 s | Shift+H or Ctrl+Shift+← |
| Shuffle queue | `s` |
| Repeat (next mode) | `r` |
| Volume up / down | `=` / `-` |
| Mute | `m` |
| Open/close the player page | `q` or Esc |
| Full screen | `f` |
| Like / dislike the song playing | `+` / `_` |
| Go to Home / Explore / Library / Settings | `g` then `h` / `e` / `l` / `,` |
| Search | `/` |
| This list | `?` |

YTFast today (AGENTS.md, run-the-app.md): Space, ←/→ (10 s), ↑/↓
(volume), Shift+N/P, M, L (like), `/` or Ctrl+F, Esc. Plain arrows are
not YouTube Music's (they scroll the page there).

## Not measured yet (needs YouTube Music signed in)

Library (its tabs and grids), Liked Music, the account's own playlist
page (edit, privacy), the account menu, History, the menu's playlists and
"New playlist", Home's personal shelves (Quick picks, Listen again),
lyrics on the player page, the "Save to playlist" dialog, the "New
playlist" dialog, toasts, the Settings dialog, and the playlist page of a
playlist made by a person (signed out it looks like the album page).
