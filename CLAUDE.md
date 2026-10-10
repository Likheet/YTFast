# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

@AGENTS.md

AGENTS.md (above) is the guide every coding assistant reads: who the owner
is, what YTFast is, the layout, the rules, and what has been tested. Keep it
the source of truth. What follows is only what it leaves out.

## Commands

AGENTS.md has the three checks to run before pushing. The rest:

```sh
cargo run -p ytfast                 # the app (needs a signed-in browser)
cargo run -p ytfast -- --demo       # made-up music, no account, no sound
cargo run -p ytfast -- --verbose    # everything in ytfast.log, not only warnings
cargo run -p ytfast-check           # the step 0 check

cargo test -p ytfast-core read::page          # one module's tests
cargo test -p ytfast-core seeking_lands_exactly   # one test, by name
cargo test -p ytfast queue                    # the app crate's tests

cargo build --release --locked -p ytfast -p ytfast-check   # what CI builds

# a real YouTube H.264 video decoded in time (never commit the file)
YTFAST_TEST_VIDEO=path/to/video.mp4 cargo test --release -p ytfast-core decodes_a_real_video_in_time
```


- Tests sit beside the code, in a `mod tests` at the foot of each file.
  `crates/ytfast-core/tests/` holds only the saved replies and the test
  tone; there are no separate test programs.
- The app's own rules are tested in `app.rs` without its threads:
  `App::with` takes `Backend::for_tests()` and `Audio::for_tests()`, whose
  receivers show what the app asked for, and `Harness::answer` hands it an
  `Event` as the next frame would. Add a test there for a new rule about
  stale answers, the queue or the account.
- Two checkouts (git worktrees) must not share one `CARGO_TARGET_DIR`:
  cargo names a workspace's crates the same in each, so one checkout's
  crates pass as fresh in the other's build.
- The compiler is pinned (`rust-toolchain.toml`, 1.98.0). CI names the same
  version in `RUST_TOOLCHAIN` (`.github/workflows/ci.yml`): change both.
- CI builds with `--locked`, so a dependency change must come with its
  `Cargo.lock` change. It checks formatting on Linux only.
- A debug build is fine to listen to: dependencies are built optimised even
  in debug (`[profile.dev.package."*"]`).

## How one feature runs through the code

Nearly every feature is the same round trip through five places:

1. A view (`views/`) draws from `&App` and calls `app.act(Action::...)`.
2. `App::apply` (`app.rs`) shows the result at once and sends a
   `Request` to the backend.
3. `backend.rs` starts each request as a task of its own, so answers come
   back in any order. The task calls a `Session` method (`innertube.rs` to
   read, `library.rs` to change the account), which builds the request and
   hands YouTube's reply to a reader in `read/`.
4. The task sends an `Event`, and `App::handle_events` takes it in at the
   start of the next frame.
5. Every backend task answers from `demo.rs` when `shared.demo` is set. A
   new `Request` needs that branch too, or `--demo` shows nothing for it.

One frame runs `App::logic` first: events, media keys, and the actions
they pushed. Then `App::ui`: shortcuts, cover colours, lyrics and related
when wanted, the views, then the actions the views pushed. eframe also
runs `App::logic` alone while the window is minimised or hidden (it draws
nothing then), so whatever keeps the music going belongs there, not in
`App::ui`.

## Easy to get wrong

- **The sizes are YouTube Music's, not taste.** The numbers in `theme.rs`
  and `views/widgets.rs` (`Row`) and in each view were measured on
  music.youtube.com at 1280 wide (bar heights, row heights, cover sizes,
  margins, colours). Do not round or "tidy" them; to change a screen,
  open the real one, measure it (the page's computed styles), then check
  the result in `--demo` beside it.
- **Three themes.** `PALETTE` and the sizes that differ by theme
  (`top_bar_height()`, `player_bar_height()`, `guide_width()`,
  `page_foot()`, `Row::themed`) follow the theme chosen in Settings, so
  read them while drawing, never into a `const`. A screen that looks
  different in Premium keeps YouTube Music's code untouched and adds its
  own behind `theme::premium()` (`docs/look/premium.md`). Dynamic
  Background does the same behind `theme::dynamic()`, with its own
  screens in `views/dynamic.rs` (`docs/look/dynamic-background.md`).
  Tests that draw or size things in Premium or Dynamic Background call
  `theme::set` themselves (it is per thread).
- **The pointer is YouTube Music's** (read from its stylesheet): a hand on
  whatever can be pressed (`theme::pointing`; round buttons, chips, pills,
  switches and menus already do it), and "not allowed" on the player's
  buttons
  and the player page's tabs when off (`theme::pointing_or_not`). One
  difference, the owner's choice: Up next's rows (dragged to a new place)
  show the open hand, cover included, closed while one is dragged, where
  YouTube Music shows move arrows; their ⋮ keeps the pointing hand (a
  button's hand is set after the row's, so it wins).
- **Give every new button a name** (`response.widget_info`). Screen
  readers need it, and it is how the demo is driven on the owner's laptop
  (see AGENTS.md, "Where things can be tested").
- **Long lists draw only the rows on screen** (`widgets::rows`). Each row
  must add exactly one widget of its own, as `track_row_in` does: the rows
  not drawn are counted as one each, which keeps every row's ID (its open
  menu, its keyboard focus) the same wherever the list is scrolled.
- **A song's menu knows where it was opened** (`widgets::Place`): only a
  row on a playlist's own page offers "Remove from playlist", only Up next
  edits the queue. Greyed-out songs (`Track::playable` false) stay on
  their page but are left out of whatever plays.
- **Every page loading is numbered** (`Request::Page { load }`). An
  answer or `Event::MoreRows` for an older loading is dropped, and the
  backend stops following an older loading's batches.
- **A stale sign-in is mended in one place.** `Session::send` reads the
  browser's sign-in again and repeats the request; `Preparer::prepare`
  does the same when yt-dlp refuses its cookies. Do not add retries or
  "sign in again" prompts elsewhere; `Event::SignedOut` now means reading
  it again did not help either.
- **egui is a pinned fork.** The root `Cargo.toml` patches egui, eframe and
  winit to fixed commits of Spotifast's forks, and both crates take
  fastframe at one tag. Move all of them together. The frame's entry points
  here are `eframe::App::logic` and `eframe::App::ui`, not the older
  `update`: follow the calls already in `views/` rather than older egui
  examples.
- **The window draws only when asked.** The backend and audio threads wake
  it when they have something; `App::logic` asks again after 250 ms only
  while a song plays. Views ask with `request_repaint_after` and a reason to
  stop (typing pause, synced lyrics following). An unconditional repaint
  makes an idle window use CPU; two such loops have been fixed already.
- **Playlist IDs come in two forms.** A playlist's page is `VL` plus its ID
  (Liked Music is `VLLM`); changes to it take the ID without `VL`
  (`library::bare`). `Route::browse` turns `VLLM` into `Route::Liked`, so
  Liked Music is one page however it is reached.
- **Skip or stop.** Only a problem with the song itself moves on to the
  next: `PrepareError::Unavailable` (yt-dlp's words for a removed, private
  or blocked song, `YtDlpError::song_unavailable`), `NoPlayableAudio`, or
  audio that cannot be decoded (`PlayFailure::song_only`). Anything else
  (the sign-in, the network, YouTube slowing the account down, a download
  that broke off part way) leaves the queue where it is, as
  `PlayState::Failed`, and Play tries again. `App::song_failed` also stops
  after `MAX_SKIPS` songs in a row, so a problem every song has can never
  run through the queue.
- **One fetch per song.** `Shared.prepared` keeps one cell per song, so a
  song made ready ahead and then played is fetched once (at most
  `READY_AHEAD` kept). `Shared.details` does the same for what likes,
  lyrics and Related all need. A download holds its song weakly
  (`stream::fetch`): once the player and the songs made ready ahead let go
  of it, it stops. Keep it that way, or skipping downloads in bulk.
- **The video mode keeps to the queue.** Only `App::play_tracks` turns it
  off (every new queue passes through it); Next, Previous, a song ending
  and Up next keep it. `Playback::version` is the ID asked for and
  `App::version_of` the one wanted: a `Prepared` for the other version
  (the switch moved meanwhile) starts the right one instead
  (`App::prepared`). A moment moves between a song and its video only by
  YouTube's map (`App::moment_in`, `App::lyrics_clock`), never as the
  same second. The video's thread decodes only while the player page
  draws it (`App::video_drawn`), and the window is asked to draw again
  only while the music runs.
- **Answers about the queue are tagged.** Up next answers carry the queue's
  generation (`Queue::generation`) and playlist answers a ticket
  (`App::wanted_playlist`); one for a queue since replaced is dropped. Do
  the same for any new answer that changes what plays.
- **Fonts for other scripts load when needed.** `theme::ScriptFonts` adds
  the computer's fonts for Chinese, Arabic, the Indian scripts and so on
  (about 60 MB) only once some text needs them. Text that reaches the
  screen from somewhere new must pass through `ScriptFonts::want_for`
  (see `words` in `app.rs`), or it may show as empty boxes.
- **Everything kept in memory has a limit** (pages 24, with Home, Library
  and Liked Music always kept; lyrics, related, details and covers too).
  Give any new store one: the target is about 200 MB.
- **What the user chose sits on top of what YouTube said.** `App.likes`,
  `saved` and `subscribed` hold this run's choices over the page's own
  state; a late `Event::Liked` never replaces one. Changes go to YouTube
  one at a time, in order (`backend::serve`), and an `EditFailed` undoes
  what was shown only when no newer change to the same thing is on its way
  (`App::edit_answered`); a like goes back to what it was before.
- **The folder on disk is `YtFast`**, not `YTFast`
  (`ProjectDirs::from("", "", "YtFast")`, in the app and the check alike).
  Renaming it would lose the helpers and download them again. Each open
  run keeps its sign-in folder (`app-sessions/<pid>`) locked, so a second
  YTFast leaves it alone.
- **Settings** are saved by eframe under the key `ytfast`, in eframe's own
  folder named after the app id (`%APPDATA%\ytfast\data\app.ron` on
  Windows, `~/Library/Application Support/ytfast` on a Mac), not in
  `YtFast`. The demo keeps its window state in `demo-window.ron` in
  YtFast's cache folder, so it never writes the real file. `Settings` is
  `#[serde(default)]`, so a new field needs a default.
