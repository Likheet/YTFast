# YtFast plan

YtFast is a fast, light YouTube Music app for the desktop, in the spirit of
[Spotifast](https://github.com/crmne/spotifast): its own native screens
instead of a web page, starting in under a second and using a few hundred MB
of memory at most.

## Decisions made

| Decision | Why |
|---|---|
| YouTube Music **Premium** accounts only | Premium removes YouTube's extra "proof" tokens for streaming, gives 256 kbps audio, and means YtFast never skips ads someone would otherwise see. |
| **Audio only**, no videos | Videos are what makes YouTube Music heavy. Music videos still play, as sound. |
| **Mac (Apple Silicon)** and **Windows (Intel/AMD)** | The two laptops YtFast is for. Both are built from one codebase. |
| **No web page inside the app**, not even a hidden one | A hidden YouTube Music page would play reliably, but it brings back the memory use YtFast exists to avoid. |
| **Rust**, with the UI built like Spotifast's (egui + [fastframe](https://github.com/crmne/fastframe)) | Spotifast proves this combination is fast and light. fastframe is Spotifast's shared foundation (media keys, audio output, tray, updates), MIT licensed. |
| **yt-dlp** finds each song's audio | The best-maintained tool for YouTube's changing defences. It runs for a moment per song, then exits. |
| YouTube Music's own internal API for everything else | It is what music.youtube.com uses. There is no official YouTube Music API. |
| No casting, no offline downloads (for now) | Casting to TVs and speakers is very hard to rebuild. The YouTube Music desktop app has no downloads either. |

## The phases

Each phase ends with something you can run and check.

### Step 0: prove it works (now)

`ytfast-check` (see [run-the-check.md](run-the-check.md)) proves, on both
laptops: reading the sign-in, the right account and Premium, Liked songs,
Premium-quality audio, playing with pause and seek, carrying on after a
long pause or a closed lid, and plays reaching History.

**Done when** the check passes on the Mac and on Windows. If it fails on
something fundamental (sign-in or playback), we rethink before building any
screens.

### Phase 1: the app shell and playback

A window with your Library (playlists, Liked songs), a queue, a player bar,
media keys and the tray, playing through the same engine the check uses.
Read-only: nothing in your account changes except History.

Also decided here: how YtFast signs in for everyday use. Today the check
borrows the browser's sign-in, which is simple and keeps working as long as
you stay signed in there. The alternative is a one-time sign-in window.

### Phase 2: the rest of YouTube Music, read-only

Home (Quick picks, Listen again, mixes), Explore (new releases, charts,
moods), search with suggestions, artist and album pages, Up Next, Start
radio, podcasts, History.

### Phase 3: lyrics and the look

Time-synced lyrics (YouTube Music's own, then LRCLIB), with a Better
Lyrics-inspired style recreated natively. Better Lyrics' code is GPL and its
lyrics server is private, so neither is used directly.

### Phase 4: changing things

Like and dislike, saving to the library, subscribing to artists, creating and
editing playlists. Every edit is tested on a throwaway playlist first, and
edits use each row's own ID so duplicates are never confused.

### Phase 5: polish and release builds

Packaged apps for both laptops, self-update, settings, keyboard shortcuts,
gapless albums, performance measured on the real apps (startup time, memory,
including the helper programs).

## Risks

### Could stop the project (step 0 tests these)

1. **Sign-in.** Google can block sign-in windows inside apps, which is why
   the check borrows the browser's sign-in instead. On Windows only Firefox
   allows that.
2. **Playback.** YouTube changes its defences every few months. yt-dlp
   usually catches up quickly, but there is no promise of how quickly.
3. **The account.** yt-dlp's own documentation warns that using an account
   with it can get the account banned, temporarily or permanently. YtFast
   keeps to a normal listener's pace (one song at a time, the next one
   prepared ahead, never bulk downloads), but the risk is not zero. Using any
   unofficial app is against YouTube's terms, Premium or not.

### Designed in from the start (expensive to add later)

4. **The sign-in is a master key.** The saved cookies are as powerful as being
   signed in to Google in a browser. Only YouTube's cookies are kept, in
   private files or the system's password store, never in logs, reports,
   chats or git.
5. **Expiring audio links.** YouTube's audio links stop working after a few
   hours and depend on the network. YtFast downloads each song whole into
   memory, so pausing, sleeping or changing Wi-Fi cannot break the current
   song; a song that hasn't started yet is fetched again if needed.
6. **History and recommendations.** Every play is reported to YouTube (when
   it starts, and how long it played), or Home and the mixes stop learning.
7. **Loudness.** YouTube turns loud songs down; YtFast applies the same
   figure YouTube sends.
8. **YouTube's data format changes.** All reading of YouTube's replies lives
   in one file (`crates/ytfast-core/src/read.rs`), tested against saved real
   replies, and it tolerates missing pieces instead of failing.
9. **Seeking.** Rust's usual audio decoder cannot seek in YouTube's file
   layout, and its seek freezes while the sound device is paused. YtFast
   decodes with its own small decoder that seeks exactly (tested).

### Given up, on purpose

Casting, offline downloads, videos and the Samples feed.
