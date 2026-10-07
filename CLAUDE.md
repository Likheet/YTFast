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
```

- Tests sit beside the code, in a `mod tests` at the foot of each file.
  `crates/ytfast-core/tests/` holds only the saved replies and the test
  tone; there are no separate test programs.
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

One frame (`App::ui`) runs in this order: events, media keys, shortcuts,
cover colours, lyrics and related when wanted, the views, then the actions
the views pushed.

## Easy to get wrong

- **The sizes are YouTube Music's, not taste.** The numbers in `theme.rs`
  and `views/widgets.rs` (`Row`) and in each view were measured on
  music.youtube.com at 1280 wide (bar heights, row heights, cover sizes,
  margins, colours). Do not round or "tidy" them; to change a screen,
  open the real one, measure it (the page's computed styles), then check
  the result in `--demo` beside it.
- **Give every new button a name** (`response.widget_info`). Screen
  readers need it, and it is how the demo is driven on the owner's laptop
  (see AGENTS.md, "Where things can be tested").
- **A stale sign-in is mended in one place.** `Session::send` reads the
  browser's sign-in again and repeats the request; `Preparer::prepare`
  does the same when yt-dlp refuses its cookies. Do not add retries or
  "sign in again" prompts elsewhere; `Event::SignedOut` now means reading
  it again did not help either.
- **egui is a pinned fork.** The root `Cargo.toml` patches egui, eframe and
  winit to fixed commits of Spotifast's forks, and both crates take
  fastframe at one tag. Move all of them together. The frame's entry point
  here is `eframe::App::ui`, not the older `update`: follow the calls
  already in `views/` rather than older egui examples.
- **The window draws only when asked.** The backend and audio threads wake
  it when they have something; `App::ui` asks again after 250 ms only while
  a song plays. Views ask with `request_repaint_after` and a reason to stop
  (typing pause, lyrics following). An unconditional repaint makes an idle
  window use CPU; one such loop has been fixed already.
- **Playlist IDs come in two forms.** A playlist's page is `VL` plus its ID
  (Liked Music is `VLLM`); changes to it take the ID without `VL`
  (`library::bare`). `Route::browse` turns `VLLM` into `Route::Liked`, so
  Liked Music is one page however it is reached.
- **Skip or stop.** `backend::classify` marks a failure `song_only` (the
  song is gone or not offered here). Only then does the app move to the
  next song; anything else leaves the queue where it is, as
  `PlayState::Failed`, and Play tries again.
- **One fetch per song.** `Shared.prepared` keeps one cell per song, so a
  song made ready ahead and then played is fetched once (at most
  `READY_AHEAD` kept). `Shared.details` does the same for what likes,
  lyrics and Related all need.
- **Everything kept in memory has a limit** (pages 24, with Home, Library
  and Liked Music always kept; lyrics, related, details and covers too).
  Give any new store one: the target is about 200 MB.
- **What the user chose sits on top of what YouTube said.** `App.likes`,
  `saved` and `subscribed` hold this run's choices over the page's own
  state; a late `Event::Liked` never replaces one, and `EditFailed` takes
  it back out.
- **The folder on disk is `YtFast`**, not `YTFast`
  (`ProjectDirs::from("", "", "YtFast")`, in the app and the check alike).
  Renaming it would lose the settings and download the helpers again.
- **Settings** are saved by eframe under the key `ytfast`; `Settings` is
  `#[serde(default)]`, so a new field needs a default.
