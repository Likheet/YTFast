# YTFast plan

YTFast (YouTube Music Fast) is a fast, light YouTube Music app for the desktop, in the spirit of
[Spotifast](https://github.com/crmne/spotifast): its own native screens
instead of a web page. The targets are opening in under a second and using
a few hundred MB of memory at most, measured on the finished app.

## Decisions made

| Decision | Why |
|---|---|
| YouTube Music **Premium** accounts only | Premium removes YouTube's extra "proof" tokens for streaming, gives 256 kbps audio, and means YTFast never skips ads someone would otherwise see. |
| **Audio only**, no videos | Videos are what makes YouTube Music heavy. Music videos still play, as sound. |
| **Mac (Apple Silicon)** and **Windows (Intel/AMD)** | The two laptops YTFast is for. Both are built from one codebase. |
| **No web page inside the app**, not even a hidden one | A hidden YouTube Music page would play reliably, but it brings back the memory use YTFast exists to avoid. |
| **Rust**, with the UI built like Spotifast's (egui + [fastframe](https://github.com/crmne/fastframe)) | Spotifast proves this combination is fast and light. fastframe is Spotifast's shared foundation (media keys, audio output, tray, updates), MIT licensed. |
| Songs are found **the website's way**, with **yt-dlp's** solver; yt-dlp itself as the fallback | yt-dlp is the best-maintained tool for YouTube's changing defences, but running it for every song took about 10 seconds. Its challenge solver, kept running, does the same work in well under a second. |
| YouTube Music's own internal API for everything else | It is what music.youtube.com uses. There is no official YouTube Music API. |
| No casting, no offline downloads (for now) | Casting to TVs and speakers is very hard to rebuild. The YouTube Music desktop app has no downloads either. |
| Memory: **about 200 MB** at most | The owner's limit. Measured on the release builds on the laptops. |
| The look is **YouTube Music's own**, copied one to one | It is the app its users know. Sizes and colours are measured from music.youtube.com. The player page keeps the cover's colours behind it and lyrics that follow the song, after Better Lyrics. |

## The phases

Each phase ends with something you can run and check.

### Step 0: prove it works (passed on Windows; the Mac run is still to do)

`ytfast-check` (see [run-the-check.md](run-the-check.md)) proves, on both
laptops: reading the sign-in, the right account and Premium, Liked songs,
Premium-quality audio, playing with pause and seek, carrying on after a
long pause or a closed lid, and plays reaching History.

**Done when** the check passes on the Mac and on Windows. If it fails on
something fundamental (sign-in or playback), we rethink before building any
screens.

### Phase 1: the app shell and playback (ran on a real account)

A window with your Library (playlists, Liked songs), a queue, a player bar,
media keys, playing through the same engine the check uses: the sidebar,
search, album, artist, playlist and mood pages, long playlists loading in
full, the player bar (repeat, shuffle, volume, seeking), Up next with Play
next, Add to queue and Start radio, the system media controls, keyboard
shortcuts, and a Mac app and a Windows program with their own icon. A demo
mode (`--demo`) shows it all with made-up music.

Sign-in for everyday use borrows the browser's sign-in, as the check does:
simple, and it keeps working as long as you stay signed in there. The
browser renews its sign-in as it goes, and YouTube then stops accepting
the copy YTFast took (within the hour, in the first runs); YTFast reads
the browser's sign-in again by itself when that happens (built; to be
confirmed on the laptops).

The first run worked, but songs took about 10 seconds to start and album
covers flickered on hover. Both are fixed: songs are now found the
website's way and play while they download, and most start in about a
second (to be confirmed on the laptops).

### Phase 2: the rest of YouTube Music, read-only (built)

Search suggestions and filters (Songs, Albums, Artists, Playlists),
History, the Library's tabs (Playlists, Songs, Albums, Artists), Home's
mood buttons, "Go to album" and "Go to artist" from a song, Radio on
artist pages, and related songs. Podcasts only if wanted.

### Phase 3: lyrics and the look (built; the new look to be tried with a real account)

Time-synced lyrics (YouTube Music's own, then LRCLIB), and YouTube Music's
own look: the bar across the top, the menu on the left, an album or
playlist with its cover on the left and its songs on the right, an artist
under a wide picture, the player bar with its controls in the middle, and
the player page with the cover beside the tabs Up next, Lyrics and
Related. (The first look, rounded glass panels over a blur of the playing
song's cover, was replaced: it did not look like YouTube Music.) From
Better Lyrics' Even Better Lyrics Plus theme, recreated natively, the
player page keeps the soft blur of the cover behind it and lyrics that
light up as they are sung. Better Lyrics' code is GPL and its lyrics
server is private, so neither is used directly.

Still different from YouTube Music: the font (Inter, not Roboto and
YouTube Sans), the icons (Lucide's), and no like counts, play counts,
descriptions or Comments.

### Phase 4: changing things (built; to be tried on the laptops)

Like and dislike, saving to the library, subscribing to artists, making,
renaming and deleting playlists, adding songs to them and taking songs out.
Edits use each row's own ID so duplicates are never confused. Try playlist
edits on a throwaway playlist first.

### Phase 5: polish and release builds (partly done)

Done: packaged apps for both laptops, Settings, keyboard shortcuts.
Still to do: self-update, the tray, gapless albums, and performance
measured on the real apps (startup time, memory, including the helper
programs).

## Risks

### Could stop the project (step 0 tests these)

1. **Sign-in.** Google can block sign-in windows inside apps, which is why
   the check borrows the browser's sign-in instead. On Windows only Firefox
   allows that.
2. **Playback.** YouTube changes its defences every few months. yt-dlp
   usually catches up quickly, but there is no promise of how quickly.
3. **The account.** yt-dlp's own documentation warns that using an account
   with it can get the account banned, temporarily or permanently. YTFast
   keeps to a normal listener's pace (one song at a time, the next one
   prepared ahead, never bulk downloads), but the risk is not zero. Using any
   unofficial app is against YouTube's terms, Premium or not.

### Designed in from the start (expensive to add later)

4. **The sign-in is a master key.** The saved cookies are as powerful as being
   signed in to Google in a browser. Only YouTube's cookies are kept: in
   memory, and in a private file only while YTFast (or the check) runs.
   The file is deleted when it closes, and one left by a run that did not
   close properly is deleted at the next start. They are never put in the
   system's password store, logs, reports, chats or git.
5. **Expiring audio links.** YouTube's audio links stop working after a few
   hours and depend on the network. YTFast downloads each song whole into
   memory (it plays while the rest arrives, which takes seconds), so
   pausing, sleeping or changing Wi-Fi cannot break the current song once
   it has arrived; a song that hasn't started yet is fetched again if
   needed.
6. **History and recommendations.** Every play is reported to YouTube (when
   it starts, and how long it played), or Home and the mixes stop learning.
7. **Loudness.** YouTube turns loud songs down; YTFast applies the same
   figure YouTube sends.
8. **YouTube's data format changes.** All reading of YouTube's replies lives
   in one place (`crates/ytfast-core/src/read/`), tested against saved real
   replies, and it tolerates missing pieces instead of failing.
9. **Seeking.** Rust's usual audio decoder cannot seek in YouTube's file
   layout, and its seek freezes while the sound device is paused. YTFast
   decodes with its own small decoder that seeks exactly (tested).

### Given up, on purpose

Casting, offline downloads, videos and the Samples feed.
