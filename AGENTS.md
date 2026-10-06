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
packaging/            app icon files, the Mac Info.plist, the Windows
                      resource file
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
  them after the frame is drawn, so nothing changes under a view.
- Each queue entry has its own ID. Answers about an entry (a song made
  ready, a song that ended) carry that ID, and answers about an entry no
  longer playing are ignored.
- Songs are found the fast way (`direct.rs`): one `player` request
  carrying the player code's signature timestamp, then the stream address
  unlocked by the solver (`solver.rs`: yt-dlp's own EJS scripts, from
  inside the yt-dlp download, kept running in Deno and stopped after 15
  minutes unused). The player code is cached on disk by its ID. If any
  step fails, yt-dlp finds the song instead (`prepare.rs`) and the log says
  why. Settings can turn the fast way off.
- A song plays from its first 256 KB while the rest downloads
  (`stream.rs`).
- The next song is made ready while the current one plays, and more songs
  are asked for (YouTube Music's Up next) when the queue is about to run
  out. The top search result and a song the pointer rests on are found
  ahead of time too (found only, not downloaded).
- The window takes its colours from the playing song's cover
  (`colors.rs`, `views/backdrop.rs`), in the style of Better Lyrics' Even
  Better Lyrics Plus theme (recreated, not copied).
- Lyrics: YouTube Music's timed lyrics, else LRCLIB's (lrclib.net, found
  by title, artist, album and length), else YouTube Music's plain ones.
  Asked for only when the player page shows them.
- Changes to the account (`backend::Edit`) show at once; a refusal from
  YouTube (`Event::EditFailed`) undoes them and says so.
- Long lists (a playlist, Liked Music) show their first songs at once and
  load the rest in the background (`Session::more_tracks`).
- Every page is a header and sections of songs or cards (`read::Page`), so
  one view draws Home, Explore, search, albums, artists and playlists.
- `--demo` replaces the account with made-up music and no network or
  sound, for trying the interface and for screenshots.
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
hover. Both are fixed since, untested on the laptops.

Built since (see [docs/plan.md](docs/plan.md)): the fast way to start songs
and playing while downloading; the Even Better Lyrics Plus look; the player
page with time-synced lyrics, Up next and Related; likes; Library tabs and
History; making, renaming, deleting and editing playlists; saving to the
library and subscribing; search suggestions and filters; Home's mood
buttons; queue edits; Settings; artist Radio.

Tested so far, in a cloud session only: unit tests (cookie handling,
request signature, page config, reading real saved replies, play reports,
yt-dlp output, checksums, unpacking, decoding and exact seeking of
YouTube's audio layout, playing a song still arriving, the queue, account
changes' requests, lyrics), the solver with Deno and yt-dlp's real EJS
scripts on a stand-in player, helper download and verification on Linux,
reading a fake Firefox sign-in through yt-dlp, and the app in demo mode
under a virtual screen. Not yet tested: anything the app asks of YouTube
or LRCLIB with a real account (the fast way, likes, playlist changes,
lyrics, related songs, suggestions), sound from the app, and the Mac and
Windows builds beyond CI compiling and packaging them. Memory in demo mode
was about 160 MB (a debug build on a software-drawn virtual screen);
measure the release build on the laptops.
