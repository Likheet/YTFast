# YtFast: guide for coding agents

Read this before changing anything. [docs/plan.md](docs/plan.md) has the
product decisions, phases and risks; this file is how to work in the code.

## Who you are working with

The owner is **not a programmer**. They build YtFast by talking to AI coding
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

## What YtFast is

A native, fast, light YouTube Music desktop app: everything the YouTube
Music desktop app does except video, at Spotifast's speed. Premium only,
audio only, Mac and Windows. No web page inside the app, visible or hidden.

## Layout

```
crates/ytfast-core/   the engine (no user interface)
  src/cookies.rs      the YouTube sign-in, from browser cookies
  src/auth.rs         the request signature (SAPISIDHASH)
  src/ytcfg.rs        settings read from the YouTube Music page
  src/innertube.rs    requests to YouTube Music's internal API
  src/read.rs         reading YouTube's replies: the ONLY place that does
  src/playreport.rs   reporting plays (History, recommendations)
  src/helpers.rs      downloading yt-dlp and Deno, checked by SHA-256
  src/ytdlp.rs        running yt-dlp (sign-in from browser, song audio)
  src/audio.rs        downloading audio, decoding, the player
  src/net.rs          HTTP clients
  src/redact.rs       keeping secrets out of messages
  tests/fixtures/     saved YouTube replies (see its README)
crates/ytfast-check/  step 0: the guided check program
docs/                 plan, how to run the check
```

The app itself (phase 1) will be a new crate, `crates/ytfast`, with an
egui interface on [fastframe](https://github.com/crmne/fastframe), modelled
on Spotifast. Copy Spotifast code only where it fits, crediting it (MIT,
Copyright (c) 2026 Carmine Paolino), as `audio.rs` does.

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

- All reading of YouTube's JSON lives in `read.rs`. Readers look for the
  piece they need wherever it is and return `None` or an empty list rather
  than failing. When a reply breaks a reader, save a real reply (nothing
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

- A song is downloaded whole into memory before it plays. Its link expires,
  but nothing more needs fetching once it has arrived.
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
- On Windows the app (phase 1) must start helpers without a console window.

### Where things can be tested

- **Live YouTube testing happens only on the owner's laptops.** Cloud
  machines are blocked by or suspicious to YouTube, and the owner's sign-in
  must never be put on one. In a cloud session: build, run the unit tests,
  and say plainly what still needs a run on the laptops.
- The check can be exercised in a cloud session up to the sign-in step with
  a fake Firefox profile (a `cookies.sqlite` with made-up YouTube cookies).

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

CI (`.github/workflows/ci.yml`) runs the same checks on macOS, Windows and
Linux, and uploads `ytfast-check` for the Mac and for Windows as artifacts.

## Current state

Step 0 is built and waiting for the owner to run `ytfast-check` on both
laptops (see [docs/run-the-check.md](docs/run-the-check.md)). Their report
decides whether phase 1 starts as planned.

Tested so far, in a cloud session only: unit tests (cookie handling, request
signature, page config, reading real saved replies, play reports, yt-dlp
output, checksums, unpacking, decoding and exact seeking of YouTube's audio
layout), helper download and verification on Linux, and reading a fake
Firefox sign-in through yt-dlp. Not yet tested anywhere: anything that talks
to YouTube, and sound output.
