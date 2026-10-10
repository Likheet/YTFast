# YTFast: guide for coding agents

Read this before changing anything. [docs/plan.md](docs/plan.md) has the
product decisions, phases and risks; this file is how to work in the code.

## Who you are working with

The owner is **not a programmer**. They build YTFast by talking to AI coding
assistants. So:

- Explain in plain language. Say what changed for them, not how the code
  works. Avoid jargon; when a technical word is needed, explain it in a few
  words.
- Ask one clear question at a time, and only when the answer changes what
  you do. Recommend an option instead of listing many.
- Be exact about what was tested and how: "tested on this machine",
  "compiled only", "can only be tested on your laptop". Never claim
  something works on their Mac or Windows laptop unless they ran it there.
- They use a Mac (Apple Silicon) and a Windows laptop (Intel/AMD), and a
  YouTube Music Premium account.

## What YTFast is

YTFast (YouTube Music Fast) is a native, fast, light YouTube Music desktop
app: everything the YouTube
Music desktop app does except video, at Spotifast's speed. Premium only,
audio only, Mac and Windows. No web page inside the app, visible or hidden.

## Layout

```
crates/ytfast-core/   the engine (no user interface)
  src/cookies.rs      the YouTube sign-in, from browser cookies
  src/auth.rs         the request signature (SAPISIDHASH)
  src/ytcfg.rs        settings read from the YouTube Music page
  src/innertube.rs    requests to YouTube Music's internal API
  src/library.rs      changing the account (likes, playlists, library,
                      subscriptions); suggestions, song details, lyrics
  src/read/           reading YouTube's replies: the ONLY place that does
  src/playreport.rs   reporting plays (History, recommendations)
  src/helpers.rs      downloading yt-dlp and Deno, checked by SHA-256
  src/ytdlp.rs        running yt-dlp (sign-in from browser, song audio)
  src/solver.rs       yt-dlp's challenge solver (EJS) kept running in Deno
  src/direct.rs       finding a song's audio the website's way (fast way)
  src/prepare.rs      getting a song ready: the fast way, else yt-dlp
  src/stream.rs       downloading a song while it plays
  src/audio.rs        decoding, the player
  src/lyrics.rs       LRCLIB's lyrics and LRC text
  src/net.rs          HTTP clients
  src/redact.rs       keeping secrets out of messages
  tests/fixtures/     saved YouTube replies (see its README)
crates/ytfast/        the app: an egui window on fastframe
  src/main.rs         starts the window (`--demo`, `--verbose`); the
                      updater's helper and `--version` first
  src/update.rs       updates from the GitHub releases (fastframe-update)
  src/update_mac.rs   the Mac's updates, done by YTFast itself
  src/app.rs          the app's state, and what every action does
  src/backend.rs      network and yt-dlp work, on a thread of its own
  src/audio_thread.rs the player, on a thread of its own
  src/queue.rs        what plays now and next
  src/images.rs       album covers, loaded once and kept for a while
  src/colors.rs       a cover shrunk to a few pixels (the blurred cover
                      behind an album's page)
  src/lyrics.rs       lyrics as the player page shows them
  src/demo.rs         made-up music for `--demo`
  src/dynamic.rs      the Dynamic Background theme's background, glass
                      and buttons (its screens: views/dynamic.rs; see
                      docs/look/dynamic-background.md)
  src/theme.rs        colours, fonts, icons, drawing helpers
  src/views/          what the window draws: backdrop, sidebar, top bar,
                      page, player bar, player page (now_playing), Up
                      next, settings, dialogs, sign-in
  assets/             icons (Google's Material Symbols, Apache 2.0), the
                      Roboto font (OFL) and the app's own mark
  build.rs            the icon and name in the Windows program
crates/ytfast-check/  step 0: the guided check program
crates/ytfast-release/ making a release: the signing key, the update
                      archive, checksums.txt and its signature
                      (docs/releasing.md)
packaging/            app icon files, the Mac Info.plist and its
                      build-and-install script, the Windows resource file
docs/                 plan, how to run the app and the check
docs/look/            the look: how YouTube Music is measured (README),
                      its sizes (reference.md), and every difference
                      left, as a checklist (gaps.md); the Premium theme
                      (premium.md)
tools/look/           measuring YouTube Music in a browser (measure.js),
                      and driving the demo on Windows by button names
                      (demo.ps1)
```

The app is modelled on [Spotifast](https://github.com/crmne/spotifast) and
built on [fastframe](https://github.com/crmne/fastframe). Copy Spotifast
code only where it fits, crediting it (MIT, Copyright (c) 2026 Carmine
Paolino), as `audio.rs` does.

## How the app works

- Three threads. The window thread draws and never waits. The backend
  thread (`backend.rs`, a tokio runtime) does everything that waits on the
  network or yt-dlp. The audio thread (`audio_thread.rs`) owns the player.
  They talk through channels: `Request`/`Event` and `Command`/`Status`.
- Views draw from a shared `&App` and push `Action`s; `App::apply` runs
  them after the frame is drawn, so nothing changes under a view. What
  keeps the music going (backend and player news, media keys) runs in
  `App::logic`, which eframe also runs while the window is minimised or
  hidden.
- Each queue entry has its own ID. Answers about an entry (a song made
  ready, a song that ended) carry that ID, and answers about an entry no
  longer playing are ignored. Answers about the queue (more songs, a
  playlist to play) are tagged the same way, so a late one cannot change
  a queue the user has replaced.
- Songs are found the fast way (`direct.rs`): one `player` request
  carrying the player code's signature timestamp, then the stream address
  unlocked by the solver (`solver.rs`: yt-dlp's own EJS scripts, from
  inside the yt-dlp download, kept running in Deno in V8's lite mode,
  about 70 MB, and stopped after 15 minutes unused). The player code and
  its prepared form are cached on disk by the player's ID; the solver is
  restarted after preparing a new player, which leaves it larger. If any
  step fails, yt-dlp finds the song instead (`prepare.rs`) and the log says
  why. Settings can turn the fast way off.
- A song plays from its first 256 KB (`START_BYTES` in `prepare.rs`)
  while the rest downloads (`stream.rs`). A connection that breaks or
  stalls (8 s without data) is asked again for the rest, three times; a
  download nobody wants any more (the song was skipped) stops. A song whose download broke off for good
  stops the player, as `PlayState::Failed`; it is not skipped.
- The next song is made ready while the current one plays, and more songs
  are asked for (YouTube Music's Up next) when the queue is about to run
  out. At the end of a playlist or album, with autoplay on, a radio of
  its last song follows. The top search result and a song the pointer
  rests on (0.15 s still on its row, its tile, or the Play button that
  starts it; not rows scrolling past) are found ahead of time too, and its
  server is asked for one byte (`stream::touch`), so it has the song at
  hand and the connection stays open: found, not downloaded. A click on
  a song still being found waits for that answer rather than asking
  YouTube again (`direct::Underway`). Without
  that byte the first piece took 0.05 to 0.9 s after a click (the slow
  ones, songs the server did not have at hand); after it, under 0.03 s
  (measured on the owner's laptop, 10 October 2026).
- A song that cannot play is skipped only when the problem is that song's
  (removed, private, not offered here), and at most five in a row; any
  other problem stops and waits for Play (`App::song_failed`).
- The sign-in is a copy of the browser's. A browser renews its own as it
  goes, which ends the copy (within the hour, with YouTube open in the
  browser). When YouTube stops accepting it (HTTP 401, a reply as if
  nobody were signed in, or yt-dlp saying its cookies are no longer
  valid), the session reads the browser's sign-in again and sends the
  request once more, without asking the user: `Session::renew_with` and
  `Session::send` (`innertube.rs`), `Preparer::prepare` (`prepare.rs`),
  `backend::renewer`. The browser is read at most once a minute. yt-dlp
  gets a copy of the cookie file of its own for each song (it writes its
  cookies back), and a refusal still there after reading the browser again
  shows the sign-in screen (`Event::SignedOut`).
- The window is laid out as YouTube Music's own page: a bar across the
  top, the menu on the left (which closes to icons), the page, and the
  player bar across the bottom. Sizes and colours are YouTube Music's,
  measured from music.youtube.com (`theme.rs`, `views/widgets.rs`): an
  album or playlist has its cover and buttons on the left and its songs
  on the right, an artist's page begins under a wide picture, and the
  player page has the cover on the left and the tabs Up next, Lyrics and
  Related on the right. The owner wants a one-to-one copy of YouTube
  Music's look: when changing a screen, open the real one and measure.
- On Windows the window has no Windows title bar: the top bar moves it
  (drag) and maximizes it (double-click), YTFast draws Windows 11's
  minimize, maximize and close at its right end, round and on its middle
  line as the bar's other small buttons (close turns red under the
  pointer), each answering the pointer over the bar's whole height and
  close up to the window's corner, so the pointer thrown into the top
  right corner closes, as on Windows' own title bars; the edges resize it
  (`views/window_frame.rs`; the sign-in screen has a strip of its own for
  them). Windows still rounds the corners and draws the shadow. The Mac
  keeps its own title bar, and its Dock keeps YTFast.app's icon (eframe is
  given no icon there, or it would show the Windows one in its place).
- An album's or playlist's page has its cover, blurred, behind its top,
  and an artist's page its picture, both under the top bar and the menu
  (`views/backdrop.rs`, painted in a place `backdrop::paint` keeps under
  them). The player page is plain, as YouTube Music's, with lyrics that
  follow the song in the style of Better Lyrics' Even Better Lyrics Plus
  theme (recreated, not copied).
- Lyrics: YouTube Music's timed lyrics, else LRCLIB's (lrclib.net, found
  by title, artist, album and length), else YouTube Music's plain ones.
  Asked for only when the player page shows them.
- Changes to the account (`backend::Edit`) show at once and go to YouTube
  one at a time, in order; a refusal from YouTube (`Event::EditFailed`)
  undoes them (back to what was shown before) and says so.
- Two looks, chosen in Settings (Theme): YouTube Music's own, the
  default, and Premium (charcoal, rounder, a warm accent;
  `docs/look/premium.md`). Each frame draws in the chosen one
  (`theme::set`, `theme::premium()`); `PALETTE` and the sizes that differ
  follow it.
- The interface font is Roboto, YouTube Music's own (Roboto Bold stands
  in for YouTube Sans, which may not be shipped), with Inter behind it for
  any letter Roboto lacks. The computer's fonts for other scripts
  (Chinese, Japanese, Korean, Arabic, the Indian scripts...; about 60 MB
  on Windows) are read only once some text on screen needs one
  (`theme::ScriptFonts`).
- Long lists (a playlist, Liked Music) show their first songs at once and
  load the rest in the background (`Session::more_tracks`).
- Every page is a header and sections of songs or cards (`read::Page`), so
  one view draws Home, Explore, search, albums, artists and playlists.
- `--demo` replaces the account with made-up music and no network or
  sound, for trying the interface and for screenshots. It can be open
  beside YTFast in real use without disturbing it: it keeps a log of its
  own (`ytfast-demo.log`), leaves the sign-in folders alone, saves no
  settings, and keeps its window's size in a file of its own
  (`demo-window.ron` in the cache folder).
- Updates come from the repository's GitHub releases (public): a minute
  after starting and once a day YTFast looks, downloads a newer version in
  the background (Settings, "Install updates automatically", on by
  default), checks the release's `checksums.txt` signature against the
  publisher key built in, and installs it when the listener restarts. As
  in Spotifast: a toast says a version is out, an Update badge stays in the
  top bar, and it opens the update window (the download's progress, then
  "Restart to update"). `YTFAST_DEMO_UPDATE=downloading` (or `ready`)
  shows them in the demo. fastframe-update's helper swaps the program
  while YTFast is closed and puts the old one back if the new one does
  not start. On a Mac fastframe-update installs only apps signed with a
  paid Apple developer certificate, so YTFast does it itself there
  (`update_mac.rs`, from 0.6.4): it downloads the release's `YTFast.dmg`,
  checks `checksums.txt`'s signature and the image's checksum as
  fastframe-update does, copies the app out into the cache, signs it with
  the Mac's own "YTFast Local Signing" when there is one (so the Mac keeps
  Full Disk Access), asks it its `--version`; "Restart to update" starts a
  helper (`--finish-mac-update`) that waits for YTFast to close, moves the
  old app aside and the new one in (the old one back if that fails), and
  opens it; the next start says how it went. Only a copy in a folder it
  may write in (Applications) does this; any other is told a version is
  out, and its update window opens the release's page. Downloaded by
  YTFast rather than a browser, the new app is not marked as from the
  internet, so the Mac does not ask about it again. Every release carries
  both builds, `YTFast.exe` and `YTFast.dmg` (named so: Mac copies look
  for it), even when only Windows was tried. Making a release: docs/releasing.md (the private signing key
  stays on the owner's laptop).
- Problems go to `ytfast.log` in the cache folder, made new each run
  (warnings; everything with `--verbose`). Every line passes through
  `redact::urls`. Ask the owner for this file when something fails on
  their laptop.

## Rules

### Sign-in and secrets

- Never print, log, save in a report, or commit cookie values, stream
  addresses or report addresses. Types holding them hide them in `Debug`.
  Messages pass through `redact::urls`.
- Keep only YouTube's cookies (`CookieJar::youtube_only`). A browser's cookie
  export holds every site's sign-in; drop the rest as soon as it is read.
- Cookie files go in private folders (0600 files, 0700 folders on Unix) and
  are deleted after use.
- Never ask the owner to paste cookies or a cookies.txt file into a chat.

### Talking to YouTube

- All reading of YouTube's JSON lives in the `read` module (`src/read/`).
  Readers look for the piece they need wherever it is and return `None` or
  an empty list rather than failing. When a reply breaks a reader, save a real reply (nothing
  personal in it) to `tests/fixtures/` and add a test.
- Don't hard-code client versions or account details: they come from the
  page config (`ytcfg.rs`), with fallbacks. The one exception is the
  language: every request asks for English (`LANGUAGE` in
  `innertube.rs`), because some readers tell an artist from an album by
  YouTube's English words. Page and shelf titles are therefore English
  whatever the account's language.
- Keep to a normal listener's pace: never download in bulk, never loop
  requests. yt-dlp's documentation warns that accounts can be banned. The
  limits the code keeps now, which must not get looser:
  - The song after the playing one is made ready (downloaded) ahead, and
    again each time a queue change makes another song next
    (`App::prepare_next`), so quick queue edits can start a few downloads
    at once. At most three songs made ready are kept (`READY_AHEAD` in
    `backend.rs`); a download no longer kept or wanted stops.
  - The rest of a long list (a playlist, Liked Music, Library Songs) is
    asked for in up to 100 requests one straight after another
    (`MAX_BATCHES` in `backend.rs`, about 10,000 songs), each time that
    page is loaded afresh (Liked Music, for one, after a like elsewhere).
    A newer loading of the page stops the older one.
  - A playlist queued as a whole takes up to 5 more batches of its songs
    (`MAX_QUEUE_BATCHES` in `innertube.rs`), and the Library's tabs up to
    9 more batches of their cards (`MAX_LIBRARY_BATCHES`).
  - When the fast way cannot get a player ready, it rests for 15 minutes
    (`REST_AFTER_FAILURE` in `direct.rs`) and yt-dlp finds songs
    meanwhile, rather than trying again for every song.
  - A song found ahead (the pointer resting on it, the top search
    result) gets one request for its first byte only (`stream::touch`),
    once per song (`App::warm`); nothing more of it is fetched until it
    plays.
- Every song played is reported twice: when it starts, and how long it
  played (`playreport.rs`). Without this, History and recommendations stop
  learning.
- Use each playlist row's own ID (`set_video_id`) for playlist edits, never
  only the song ID: two copies of a song in one playlist share a song ID.

### Playing

- The device keeps its buffer full ahead of what is heard (200 ms on
  Windows), and pausing does not empty it. So stopping a song (for the
  next one) and jumping in one move to a fresh, empty device stream
  (`Player::empty_the_device`); otherwise the end of the old sound plays
  first. Opening one takes about 0.15 s, which made songs start later, so
  a spare is opened while a song plays and nothing waits
  (`Player::prepare_spare`): moving to it takes about 0.01 s.
- The device is Windows' default output, opened as its virtual default
  device (which follows the default by itself). When that will not open
  (seen with Bluetooth headphones: "Failed to get audio client"), the
  default output is opened by its name instead and YTFast follows the
  default itself, every 2 s (`Player::follow_default_by_name`). A failure
  inside the audio library leaves the sound thread running: it opens the
  device again and the song carries on where it was (`audio_thread::run`).
  Device moves and failures are written to the log as warnings.
- A song is downloaded whole, into memory (a song over 40 MB, into a
  temporary file instead: `IN_MEMORY` in `stream.rs`), and plays from its
  first bytes while the rest arrives. Its link expires, but nothing more
  needs fetching once it has arrived.
- The sound device must never wait. A song is decoded on a thread of its
  own, about two seconds ahead (`AHEAD_SECONDS` in `audio.rs`); the device
  takes only what is ready, and plays silence while the download is
  behind. That silence is not counted in the song's position, so the time
  and the lyrics do not run ahead. A song let go (skipped, or replaced by
  a seek) drops what was decoded for it.
- Give symphonia's MP4 reader a one-way (unseekable) stream until the
  song has fully arrived: with a seekable one it reads every top-level box
  first, which waits for the whole download. Seeking waits for the whole
  song, then opens it seekable.
- Seeking builds a new decoder at the target position (`SongSource::open`).
  Do not use rodio's seek: it cannot seek in YouTube's fragmented MP4 (it
  reads the length as zero) and it waits for the audio thread, which does
  not run while the device is paused.
- Turn loud songs down by YouTube's `loudnessDb`; never turn songs up.
- The interface is optimistic, as in Spotifast: a control shows its result
  at once, and a late answer from YouTube must not undo what the user just
  did. Tag requests so stale answers are ignored.
- A sign-in or network failure must not empty the queue or skip through it.

### Helpers

- yt-dlp: the latest release, checked against its published SHA2-256SUMS.
- Deno: pinned in `helpers.rs` (`DENO_VERSION` plus one SHA-256 per
  platform, from Deno's `.sha256sum` files). Update them together.
- On Windows, helpers start without a console window (`no_console_window`
  in `ytdlp.rs`); the app has none either in release builds.

### Where things can be tested

- **Live YouTube testing happens only on the owner's laptops.** Cloud
  machines are blocked by or suspicious to YouTube, and the owner's sign-in
  must never be put on one. In a cloud session: build, run the unit tests,
  and say plainly what still needs a run on the laptops.
- The check can be exercised in a cloud session up to the sign-in step with
  a fake Firefox profile (a `cookies.sqlite` with made-up YouTube cookies).
- The app can be seen in a cloud session in demo mode: run it under a
  virtual screen (Xvfb, with `libxkbcommon-x11-0`), click with `xdotool`
  and take screenshots with ImageMagick's `import`. Give the window
  keyboard focus first (`xdotool windowfocus`) or typing goes nowhere.
- On the owner's Windows laptop the demo can be driven without moving
  their mouse or taking the keyboard: every button has a name for screen
  readers (set with `widget_info`), so press it by name through Windows'
  UI Automation; type by posting key messages to the window; take its
  picture with `PrintWindow`. Posted mouse moves do not work (the pointer
  is taken as gone at once), so what shows only under the pointer cannot
  be seen this way. Give new buttons a name, for this and for screen
  readers. The owner uses the laptop meanwhile: `tools/look/demo.ps1`
  opens the demo off the screen (minimised, then placed far left without
  being brought forward; pictures still work), so only its taskbar button
  shows. Do every check in one run; search and tour give the window the
  keyboard, which brings it forward.
- On the owner's Mac, build and install with
  `packaging/macos/install.sh`. It puts YTFast in Applications, signed
  with that Mac's own fixed signature ("YTFast Local Signing", kept in the
  login keychain and made by the script the first time), so macOS takes
  every build for the same app and keeps the Full Disk Access that reading
  Safari's sign-in needs. CI's builds are signed ad hoc: macOS takes each
  for a new app and forgets what it allowed. Leave no other YTFast.app on
  the disk (not in `target/` either): the Mac lists every copy it finds.

### Platforms and licences

- Keep macOS (Apple Silicon) and Windows (x64) compiling; CI builds both.
  Put platform code behind `cfg`.
- Spotifast and fastframe: MIT, credit when copying. ytmusicapi fixtures:
  MIT, credited in the fixtures README. Better Lyrics: GPL, do not copy its
  code. yt-dlp and Deno are downloaded, not bundled in the repository.

## Before you push

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

On Linux the audio library needs ALSA headers: `sudo apt-get install
libasound2-dev`.

The solver's test runs only when `YTFAST_TEST_DENO` names a Deno program
and `YTFAST_TEST_EJS` a folder with yt-dlp's `core.min.js` and
`lib.min.js` (in the yt-dlp download, under
`_internal/yt_dlp_ejs/yt/solver`).

CI (`.github/workflows/ci.yml`) runs the same checks on macOS, Windows and
Linux, and uploads the app (`YTFast-for-Mac`: YTFast.app, signed ad hoc, in
a disk image, `YTFast.dmg`, beside a shortcut to Applications;
`YTFast-for-Windows`: YTFast.exe) and `ytfast-check` for both as
artifacts, each with its HOW-TO-RUN guide. A release offers the same two,
`YTFast.exe` and `YTFast.dmg`, as its downloads (docs/releasing.md).

Version numbers live in `Cargo.toml`, `packaging/macos/Info.plist` and
`packaging/windows/ytfast.rc`: change them together.

## Current state

Step 0 passed on the owner's Windows laptop: Firefox sign-in, account and
Liked songs, Premium audio (AAC 256 kbps, format 141), playing with pause
and next, and plays reaching History. The Mac run is still to do.

The first app ran on the owner's laptop: songs played, but each took about
10 seconds to start (yt-dlp for every song), and album covers flickered on
hover.

The second app (the fast way to start songs and playing while downloading;
the player page with time-synced lyrics, Up next and Related; likes;
Library tabs and History; making, renaming, deleting and editing
playlists; saving to the library and subscribing; search suggestions and
filters; Home's mood buttons; queue edits; Settings; artist Radio; see
[docs/plan.md](docs/plan.md)) ran on the owner's Windows laptop with their
account in October 2026. The owner's words: almost everything works, and
it is quick. Two things were wrong. The sign-in stopped being accepted
after the app had been open a while (the log: the fast way got "Video
unavailable", then yt-dlp refused the cookies), and signing out and in
again mended it each time. And the look was unfinished beside YouTube
Music's, which the owner wants copied one to one.

Built since, on the owner's Windows laptop: the session reads the
browser's sign-in again by itself when YouTube stops accepting it; and the
window was laid out again as YouTube Music's own page, from measurements of
music.youtube.com (signed out, in a browser, 1280 wide). Tested: the unit
tests (among them when the sign-in is read again, and that it is read once
for requests refused together), and every screen in demo mode on that
laptop, beside the real pages, at 1280 by 820 and at 960 by 600. Since
then, that build's log from a run left open overnight (7 to 8 October,
the owner's account) shows the sign-in read again from Firefox twice, by
itself, with nothing failing. Not yet tested: the new look with real pages
(the owner's library, real covers, an artist's wide picture), what shows
only under the pointer (the menu button on a song row, the volume bar),
and the Mac.

An earlier build (before the new look) ran on the owner's Mac with their
account on 7 October 2026, built there, signed in through Safari. The
owner's words: it does everything well. Three things were wrong. A tile
of one song (as on Home and Explore) showed "Song" and no cover in the
player bar. macOS asked for Full Disk Access again with every new build.
And in demo mode the release build took about 210 to 225 MB of memory,
over the 200 MB target (80 MB of it window surfaces), not yet looked
into. Built since: a song's tile gives the player its name, artist and
cover (tested against the saved Explore page); the refusal shows an Open
Full Disk Access button; and builds made on the Mac carry a fixed
signature (above). Not yet tested: any of these with the owner's account,
the button's page in System Settings, and whether Full Disk Access now
lasts from one build to the next.

On 8 October 2026 a review of the whole code (seventeen readers, each
finding then checked by two more; the usage limit stopped about half of
the checks) found, above all, that the music stopped after the current
song while the window was minimised, and several ways the app could skip
through the queue or download in bulk. Built since, on the owner's Windows
laptop: songs advance and media keys work while the window is minimised
(`App::logic`); only a song's own problem skips it, at most five in a
row; downloads stop when no longer wanted, ask again after a broken or
stalled connection, and a song whose download broke off stops instead of
skipping; songs are found ahead only after the pointer rests on that row;
late answers about a queue are dropped, a finished album no longer starts
over (autoplay follows with a radio), and Play works after the queue
ends; lyrics are looked up with the right song's length, and lyrics and
Related that did not load are asked for again; yt-dlp gets its own copy
of the cookies, a renewal by another request counts for yt-dlp, a newer
yt-dlp that cannot be downloaded leaves the installed one in use, and a
second YTFast leaves the first's sign-in alone; changes to the account go
in order, and a refusal puts back what was shown; the sound device rests
when nothing plays, and a jump in a song still arriving no longer freezes
the controls; the fonts for other scripts are read only when some text
needs them (58 MB on that laptop: most of the demo's excess memory); the
demo keeps its window in a file of its own. Tested: the unit tests (128,
among them a download broken off and asked again, one refused, one
stopped when unwanted; yt-dlp's words for one song's problem and for
every song's; the queue's order and generations; a renewal by another
request; which characters need the other fonts), and in demo mode on that
laptop: the next song starts while the window is minimised, Enter makes a
playlist, the other fonts are read (in the background) only once such a
title shows, and at the same maximised window the demo's memory went from
about 190 to 132 MB (private) and from about 135-150 to 92 MB (working
set). Not yet tested: anything with the owner's account (a real dropped
connection, real yt-dlp failures, a renewal with yt-dlp, two windows
open), and the Mac. What that round left of the review was fixed in the
next (below), except that closing the window while minimised forgets its
size (eframe saves the minimised window's).

The next round (version 0.4.0, 8 October 2026) fixed the rest of the
review. Playing: each song decodes on a thread of its own, so the sound
device never waits for the network; songs over 40 MB go to a temporary
file; a broken fast way rests for 15 minutes instead of being retried for
every song, and a remembered address that fails is found again the fast
way; without a sound device, Play says so instead of playing silently
through the queue; the listening report counts what was heard after a
jump, and the last song's report is sent before the window closes.
Reading YouTube: greyed-out rows stay on their page but are not queued; a
whole playlist queued takes up to 5 more batches, the Library's tabs up
to 9; an artist's Shuffle and Radio, an episode's Play and Top songs'
"Show all" use YouTube's own; a video as top result says so; only AAC-LC
stereo audio is chosen, in the song's original language; every request
asks for English. The app: a renewal is refused when the browser has
another account signed in; an older loading of a page is dropped and
stops loading its batches; errors shown are plain words; "Add to
playlist" offers only the account's own playlists, scrolls, and starts
with New playlist; "Remove from playlist" only on that playlist's page;
long lists draw only the rows on screen; Page Up, Page Down, Home and End
scroll the page, and Shift+F10 opens a focused row's menu; "/" opens the
search box without typing itself into it; one-line titles are cut at the
edge, mid-word; the log's first line and Settings name the build
("YTFast 0.4.0 (abc1234)"). The check: the report keeps out names and
home folders, History counts only new plays, the sign-in copy goes when
the window closes, and `--save-replies` saves scrubbed replies for
fixtures. CI pins its actions to commits and ships the licence notices.
Tested on the owner's Windows laptop: the unit tests (176, among them the
app's own rules, now testable without its threads: stale answers, skip or
stop, repeat, likes and refusals, suggestions, page loadings), and in demo
mode: search's filters, an album's Play next (its own three songs), Page
Up and End in Liked Music, the version in Settings, and memory (122 MB
private, 93 MB working set at 1280 by 820, after browsing and playing;
0.02 s of processor time in 10 s paused). Once, a demo window closed by
itself during testing and could not be made to again; the log now says
why the window stops if it does. Not yet tested: everything with the
owner's account (the new decoding with real songs and real dropped
connections, greyed-out rows, batches of a real library, the artist
buttons, an episode), Shift+F10 (posted keys carry no Shift), and the Mac.

The next round (version 0.5.0, 8 October 2026) copied YouTube Music's
look screen by screen, from measurements of music.youtube.com (signed
out, 1280 wide, in a browser) and its stylesheet. The method, the sizes
and the checklist of differences (about 250; 200 done, the rest listed
there) are in `docs/look/`. Roboto and Material Symbols replaced Inter and
Lucide. Built: the page grid and how many cards fit, by window width; the
top bar, the menu and its strip of icons; every menu, and Save to
playlist as a dialog; the player bar; Home's chips and shelves, Explore's
buttons and moods; album and playlist pages (the blurred cover, two
columns by YouTube's formula, plays, like, dislike and ⋮ under the
pointer, greyed-out songs); artist pages (the picture under the bar, the
description, Subscribe with its count, Top songs in columns with Show
all); search (the top result card, one mixed list, YouTube's own filter
buttons, suggestions with pictures, the arrow keys and Enter in them);
the player page (plain, sliding up, YouTube's spacing and tabs, Up next
with what it plays from, every song, and the Autoplay switch); Shuffle as
a mode that stays on; toasts. Read anew from YouTube: descriptions, owner
pictures, plays, subscriber counts, search's filter buttons, suggestions
with pictures, and the queue's name. Tested on the owner's Windows laptop:
the unit tests (184, among them reading a real search and suggestions
reply, the filter buttons, and shuffle turned off), and every changed
screen in demo mode beside the real page at 1280 by 820 (some at 1100 and
1440): 0% processor time when idle, about 120 MB private memory. The
owner then ran CI's 0.5.0 builds with their account (8 October 2026):
"It works perfectly." 0.5.0 is YTFast's first public release, offered
from the owner's own website.

After 0.5.0 (8 October 2026, not yet released), the screens only a
signed-in page has were measured on music.youtube.com with the owner's
account, in the built-in browser (only measured: nothing on the account
changed, nothing played), and built: the account's menu with its photo;
the Library's front page, its six chips (Profiles and Podcasts are new)
and each tab's sort button with its own orders, kept per tab in Settings;
the playlist form (title, description, privacy) for New and Edit; an own
playlist's buttons; YouTube Music's keyboard shortcuts and their list
(`?`); past searches in the search box (a clock, and a bin that removes
one from the account's history); the explicit "E" on songs and cards; the
card of the album or playlist playing keeps its button; toasts as wide as
their words. Tested on the owner's Windows laptop: the unit tests (191,
among them the sort orders read from real tokens, a removed past search
staying gone, and a tab loading again in its order), and the demo at 1280
by 820 and 960 by 600: 0% processor time when idle, about 118 MB private
memory. Not yet tested: any of it in YTFast with the owner's account, and
the Mac.

Then, at the owner's word (8 October 2026): a moment of the old song
played when another was chosen (the device's buffer, above), now emptied;
on Windows, YTFast's own title bar and window buttons; on the Mac, the
Dock keeps YTFast.app's icon while it runs. Tested on the owner's Windows
laptop: the unit tests, and in the demo the buttons (Maximize fills the
screen above the taskbar, Restore and Minimize), the corners and shadow,
and 0% processor time when idle. Not yet tested: the sound fix (the demo
plays no sound), dragging, double-clicking and resizing the window (they
need the real mouse), the sign-in screen's strip, and the Mac's Dock.

The owner then found songs starting later (the fresh stream, above, now
ready ahead: a switch measured 8 ms with the spare, 120 ms without, at
volume 0 on that laptop's device), and songs that showed no name: an
artist's Shuffle and Mix, Start mix and some Play buttons name only the
first song's ID, and the placeholder ("Song", no artist or cover) was
never filled. Now the player's answer gives it its name and artist, and
Up next's its cover and album (`App::fill_in`, `queue::fill_in`; tested:
`a_song_started_by_its_id_alone_gets_its_name`, and an artist's Mix in
the demo). Not yet tested: both with the owner's account.

On 9 October 2026 (after Likheet/YTFast#7 was merged), at the owner's
word, more of YouTube Music's differences, measured signed in at the
owner's maximised window (1707 by 1019): the search box's placeholder is
16 and centred; text is 16 from a window 1364 wide wherever YouTube Music
grows it (`theme::responsive`: cards but the Library's grid, every song
row, Up next, the player bar's song, straplines, the top result,
suggestions, Save to playlist); a search of one kind loads its next
results when its end comes into view, one request at a time
(`Request::MoreResults`); Up next's rows are dragged to a new place (Move
up and Move down are gone, as on YouTube Music); the Moods & genres page
is grids of striped buttons under smaller titles; below 1150 wide the
player bar's volume, repeat and shuffle sit in a strip behind a "…"
button. The owner decided to keep YTFast's scroll bars. Tested: the unit
tests (194), and the demo at 1707 by 1019 and 1000 by 700 (0% processor
time once settled). Not yet tested: dragging in Up next (it needs the
real mouse), and a real search's next results (a made-up reply in
YouTube's shape was used, as a real one would add a search to the owner's
history).

Then, at the owner's word (9 October 2026): tick boxes on an album's or
playlist's rows and the bar for the ticked songs (Save to playlist, Play
next, Add to queue, Remove from playlist), as YouTube Music's
(`views/selection_bar.rs`); a playlist's Sort (Default ordering, Title,
Artist, Album; newest and oldest added first are left out); the page's
menu entry stays lit under the player page. The owner decided to keep
YTFast's lyrics, the 960 smallest window, and no Home picture. Tested: the
unit tests (195) and Sort in the demo; the tick boxes need the real
mouse. A playlist sorted in YTFast still plays in YouTube's order from its
Play button.

Then (10 October 2026), at the owner's word, the Premium look made on
`codex/premium-ui` became a theme to choose in Settings, YouTube Music's
own staying the default, with every feature of `main` kept in both.
Tested: the unit tests (197, among them Premium's page sizes and player
page), and in the demo on the owner's Windows laptop: choosing each theme
changes the whole window at once and back again, every main screen in
Premium, and 0% processor time when idle. Not yet tested: Premium with
the owner's account, and the Mac. The owner means to develop Premium
further. The same day Premium was reworked (`docs/look/premium.md`): one
charcoal with see-through menu and bars, a pink-red accent, the playing
song's colours behind the whole player page, lyrics fading at the edges,
a centred scrubber with the times, a larger album header. Tested: the
unit tests (197) and the demo at 1707 by 1019. Running the demo on the
owner's laptop takes their screen while they use it: open it once per
task at most.

The owner then ran it with their account and found it not premium
enough; reworked again (10 October 2026, `docs/look/premium.md`): the
player bar a glass panel with a clear edge and calmer buttons; the player
page's tabs and what they show on one glass panel beside the cover; the
song's colours a real blur of its cover (not one averaged grey), and a
background of the playing song's colours on the main pages too; the
search box glass; the top bar balanced (back and forward by the menu, the
search box in the middle); Settings centred; roomier margins. In both
looks: the window's buttons on the top bar's middle line, and a mood
button on Home keeps the page (dimmed) until the new one arrives. Tested:
the unit tests (199), and the demo off the screen at 1707 by 1019 and
960 by 600 (0% processor time paused on the player page). Not yet tested:
any of it with the owner's account (real covers, a real mood button's
page), and the Mac.

Version 0.6.1 (10 October 2026), at the owner's word. Songs had become
slow to start again (half a second to a second). The code that starts
them was the same as in the fast build; measured on the owner's laptop
with their sign-in (nothing played or reported), the wait was YouTube's
server sending the first piece: 0.05 to 0.9 s, the slow ones songs it
did not have at hand. A song found ahead now has its server get it ready
(one byte, above), and song tiles and an album's or playlist's Play
button find their song ahead too: the first piece then came in under
0.03 s for every song tried. The player itself starts in 0.01 to 0.03 s
(0.11 s for the first song, which opens the device). And the window's
buttons sit 4 from the edge and answer the pointer up to the corner.
Tested: the unit tests (among them that a song is found ahead from its
row, a tile and the Play button in each theme, and that a press in the
window's very corner closes it in each theme), the timings above, and
the demo off the screen. Not yet tested: the Mac. (Bluetooth is left to
the owner's brother, who uses it and will say if something is wrong.)

Version 0.6.2 (10 October 2026), at the owner's word. Releases offer
`YTFast.exe` and `YTFast.dmg` (the app beside a shortcut to Applications,
made by CI) instead of zips; the updater's zip and the signed checksums
stay beside them for the copies already out there. On a Mac, when a
browser's sign-in cannot be found or read and YTFast has no Full Disk
Access (`cookies::full_disk_access`: a file only that permission opens),
it says so for any browser, not only Safari, with the button that opens
the permission's switch (`backend::sign_in_problem`); without that
permission a Mac hides other apps' data, which yt-dlp reads as missing.
The update window of a copy that cannot install updates (every Mac copy)
opens the release's page. Tested: the unit tests (among them the words
for every browser with and without the permission, and the release
tool's files, packed again to the same bytes), compiled on Windows; the
Mac's own code and the disk image only in CI's Mac build. Not yet tested:
any of it on the owner's Mac.

Then, at the owner's word (10 October 2026, released in 0.6.3): a song is
found ahead once the pointer rests on it 0.15 s (was 0.35 s), and a click
on a song still being found waits for that answer instead of asking
YouTube again. Measured on the owner's laptop with their sign-in (nothing
played or reported), from the click until the song was ready: 0.37 to
0.46 s clicked straight away, 0.24 to 0.29 s with the pointer stopped 0.1 s
before the click. YouTube's answer about a song takes 0.21 to 0.29 s of
its own and the network trip 0.005 s, so a song clicked before anything
was found ahead cannot start much sooner. Tested: the unit tests (among
them work under way shared between two askers, and a song found after
0.2 s of rest, which fails at 0.35 s).

Then, at the owner's word (10 October 2026, released in 0.6.3): a
right-click on the player bar (not on its buttons) opens the playing
song's menu where the pointer is, as its ⋮ does and as a song's row
already did; and a song row's ⋮, like and dislike show the hand, in Up
next too, where the rest of the row keeps the move arrows. Tested: the
unit tests (a right-click on the player bar, and the pointer over an Up
next row and its ⋮, in each theme; both fail on the code before).

Where the look still differs from YouTube Music's (the rest is in
`docs/look/gaps.md`): YouTube Sans is not shipped (Roboto Bold stands in);
there is no Comments tab; back and forward arrows sit beside the account
(a browser has its own); on Windows the window's own buttons take the
top bar's right corner, so the account and the arrows stand 16 before
them when the content would reach there; History and Settings are in the
account's menu; the playing song's bars do not move (moving ones would keep the window
drawing). The signed-in screens (the account's menu, Library, the playlist
form, an own playlist's buttons) were measured on 8 October 2026 with the
owner's account; nothing of theirs is in the notes. What it does
differently: disliking the playing song does not skip it; clicking a song
in History queues the rest of that list; the playlist form has no
Collaborate switch; Up next shows the Autoplay switch for radios too.

Tested earlier, in a cloud session: unit tests (cookie handling, request
signature, page config, reading real saved replies, play reports, yt-dlp
output, checksums, unpacking, decoding and exact seeking of YouTube's
audio layout, playing a song still arriving, the queue, account changes'
requests, lyrics), the solver with Deno and yt-dlp's real EJS scripts on a
stand-in player, helper download and verification on Linux, and reading a
fake Firefox sign-in through yt-dlp. Memory, measured in demo mode on a
virtual screen before the look changed: the release build about 157 MB,
of which about 70 MB is the software renderer (libLLVM, libgallium) that
a real graphics card replaces; the solver about 70 MB with a player
loaded (a synthetic 2 MB player; the real one is untested). Measure both
on the laptops.
