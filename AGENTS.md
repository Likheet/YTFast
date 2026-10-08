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
  src/main.rs         starts the window (`--demo`, `--verbose`)
  src/app.rs          the app's state, and what every action does
  src/backend.rs      network and yt-dlp work, on a thread of its own
  src/audio_thread.rs the player, on a thread of its own
  src/queue.rs        what plays now and next
  src/images.rs       album covers, loaded once and kept for a while
  src/colors.rs       colours taken from a cover (backdrop, accent)
  src/lyrics.rs       lyrics as the player page shows them
  src/demo.rs         made-up music for `--demo`
  src/theme.rs        colours, fonts, icons, drawing helpers
  src/views/          what the window draws: backdrop, sidebar, top bar,
                      page, player bar, player page (now_playing), Up
                      next, settings, dialogs, sign-in
  assets/             icons (Lucide, ISC) and the app's own mark
  build.rs            the icon and name in the Windows program
crates/ytfast-check/  step 0: the guided check program
packaging/            app icon files, the Mac Info.plist and its
                      build-and-install script, the Windows resource file
docs/                 plan, how to run the app and the check
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
- A song plays from its first 256 KB while the rest downloads
  (`stream.rs`). A connection that breaks or stalls (8 s without data) is
  asked again for the rest, three times; a download nobody wants any more
  (the song was skipped) stops. A song whose download broke off for good
  stops the player, as `PlayState::Failed`; it is not skipped.
- The next song is made ready while the current one plays, and more songs
  are asked for (YouTube Music's Up next) when the queue is about to run
  out. At the end of a playlist or album, with autoplay on, a radio of
  its last song follows. The top search result and a song the pointer
  rests on (0.35 s on that row, not rows scrolling past) are found ahead
  of time too (found only, not downloaded).
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
- An album's or playlist's page is washed at the top with its cover's
  colour, and the player page lies over the playing song's cover, blurred
  (`colors.rs`, `views/backdrop.rs`), with lyrics that follow the song in
  the style of Better Lyrics' Even Better Lyrics Plus theme (recreated,
  not copied).
- Lyrics: YouTube Music's timed lyrics, else LRCLIB's (lrclib.net, found
  by title, artist, album and length), else YouTube Music's plain ones.
  Asked for only when the player page shows them.
- Changes to the account (`backend::Edit`) show at once and go to YouTube
  one at a time, in order; a refusal from YouTube (`Event::EditFailed`)
  undoes them (back to what was shown before) and says so.
- The interface font is Inter. The computer's fonts for other scripts
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
  page config (`ytcfg.rs`), with fallbacks.
- Keep to a normal listener's pace: prepare at most the next one or two
  songs, never download in bulk, never loop requests. yt-dlp's documentation
  warns that accounts can be banned.
- Every song played is reported twice: when it starts, and how long it
  played (`playreport.rs`). Without this, History and recommendations stop
  learning.
- Use each playlist row's own ID (`set_video_id`) for playlist edits, never
  only the song ID: two copies of a song in one playlist share a song ID.

### Playing

- A song is downloaded whole into memory, and plays from its first bytes
  while the rest arrives. Its link expires, but nothing more needs
  fetching once it has arrived.
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
  readers.
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
a zip; `YTFast-for-Windows`: YTFast.exe) and `ytfast-check` for both as
artifacts, each with its HOW-TO-RUN guide.

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
open), and the Mac. Not fixed yet, from the review: an album's or
playlist's Play takes only the songs loaded so far (and Up next's first
batch), greyed-out playlist rows are queued as playable, "Add to
playlist" offers playlists saved but not owned, a renewal takes whichever
account the browser now has, the sound callback can wait on the network,
very long tracks (over 200 MB) stop early, long lists are laid out in full
every frame, and closing the window while minimised forgets its size
(eframe saves the minimised window's).

Where the look still differs from YouTube Music's: the font is Inter, not
Roboto and YouTube Sans; the icons are Lucide's; there are no like or play
counts, no descriptions, and no Comments tab; back and forward arrows
stand before the search box (a browser has its own); History and Settings
are in the account's menu; and what only a signed-in page shows was not
seen while measuring.

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
