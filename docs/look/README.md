# Copying YouTube Music's look

The owner wants YTFast to give the YouTube Music experience one to one:
the same screens, sizes, colours, words and behaviour, but as a small
native app that uses no CPU while idle and less than 200 MB of memory.
This folder is how that work is done.

- [reference.md](reference.md): music.youtube.com measured, screen by
  screen. The source of truth for numbers.
- [gaps.md](gaps.md): every known difference between YTFast and YouTube
  Music, as a checklist, in the order to work through it.
- `tools/look/measure.js`: measures the real page in a browser.
- `tools/look/demo.ps1`: starts the demo at the same size, presses its
  buttons by name, takes pictures, and checks CPU and memory (Windows).

Kept on this computer only (in `target/look/`, which git ignores, so it
is never committed): `ytm/ytmusic.css` (YouTube Music's own stylesheet),
`ytm/ytmusic.split.css` (the same, one rule per line, for searching),
`app/*.png` (pictures of the demo), `app-inventory.md` (what the app drew,
with file and line, on 8 October 2026; it goes stale as the code changes).

## The loop, for each item in gaps.md

1. **Look at the real thing.** Find the rule in the stylesheet (search
   `target/look/ytm/ytmusic.split.css` for the element, for example
   `ytmusic-player-bar`), then measure it in a browser 1280×820 with
   `measure.js`. Where they disagree, the measurement wins: many rules
   are for other variants (mobile, experiments). Check the breakpoints
   too (936, 1150, 1364, 1578 wide) when the item changes with width.
2. **Change the code**, in the numbers YouTube Music uses. Do not round
   or tidy them (CLAUDE.md, "The sizes are YouTube Music's").
3. **Look at the demo at the same size**: `demo.ps1 start`, then
   `demo.ps1 shot -Name <page>` (or `tour` for every screen), and compare
   with the real page. `demo.ps1 list` gives every button's place in
   points, to check positions against the measurement.
4. **Check the cost**: `demo.ps1 cost` with nothing playing must say 0%
   CPU; memory must stay well under 200 MB (the demo was 112 MB private,
   53 MB working set on 8 October 2026). A new font, picture or effect
   costs memory: measure it before and after.
5. **Tick the item** in gaps.md and say what was tested and how (demo
   only, or with the owner's account on a laptop).

Work one screen at a time and keep each change small: another assistant
may be changing the same files.

## Measuring in the browser

- Use a browser window whose inside is **1280×820** (the built-in browser:
  `resize_window` 1280×820). Note whether you are signed in: some screens
  exist only signed in (see the end of reference.md).
- Load `measure.js` into the page. In a browser's console, paste it. From
  a tool that runs JavaScript in the page, paste it once; YouTube Music
  blocks running text as code, so to reload it after a page reload, keep
  the script in the page's storage and run it through a Trusted Types
  policy:
  `trustedTypes.createPolicy('m', {createScript: s => s})` then
  `(0, eval)(policy.createScript(localStorage.ytmMeasure))`.
  Moving between pages by clicking (not reloading) keeps it loaded.
- Then `ytm.page()` for the main parts of the page, `ytm.tree(selector,
  depth)` for everything inside one part, `ytm.m(selector, n)` for the
  first n of a kind. Lines read `name [attributes] x,y width x height`
  then the font (`size/weight/line height`), colour and anything not
  default; see the top of `measure.js`.
- When the browser's window is hidden, the page stops animating, and a
  panel that slides in (the player page) is measured part way. Turn
  animation off first: add a style
  `*,*::before,*::after{transition:none!important;animation:none!important}`
  and call `document.getAnimations().forEach(a => a.finish())`. Pause any
  song that started (`document.querySelector('video').pause()`).
- To get a fresh copy of the stylesheet (its address changes when YouTube
  updates it): in the page, list `[...document.styleSheets].map(s =>
  s.href)`, then download the `music.youtube.com/s/_/ytmusicweb/...`
  address into `target/look/ytm/ytmusic.css`, and split it with
  `sed 's/}/}\n/g'`.

## Rules for this work

- YouTube Music's numbers, exactly. Measure, don't guess; when a value
  could not be measured, say so in gaps.md.
- The app keeps its own name and mark (YTFast), and plays audio only:
  YouTube Music's video parts (the Song/Video switch, captions, the
  video's player) are left out.
- Never ship YouTube Sans (see gaps.md for the font decision) or any of
  YouTube's own pictures or icons' files. Recreate icons; Roboto may be
  shipped (Apache 2.0, credit it in THIRD-PARTY-NOTICES.txt).
- Every new button gets a screen-reader name (`widget_info`): the demo is
  driven by those names.
- 0% CPU while idle: no repaint loops. Animations ask for frames only
  while they run (CLAUDE.md, "The window draws only when asked").
- Every new store in memory has a limit (CLAUDE.md, "Everything kept in
  memory has a limit").
