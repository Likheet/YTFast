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
server is private, so neither is used directly. Since October 2026 the
lyrics light word by word: Musixmatch's own word times where it has them,
estimated from the line's elsewhere. Translate shows each line in English
and in Latin letters (Musixmatch's translations by people, else Google
Translate's).

Still different from YouTube Music: the font (Inter, not Roboto and
YouTube Sans), the icons (Lucide's), and no like counts, play counts,
descriptions or Comments.

### Phase 4: changing things (built; to be tried on the laptops)

Like and dislike, saving to the library, subscribing to artists, making,
renaming and deleting playlists, adding songs to them and taking songs out.
Edits use each row's own ID so duplicates are never confused. Try playlist
edits on a throwaway playlist first.

### Phase 5: polish and release builds (partly done)

Done: packaged apps for both laptops, Settings, keyboard shortcuts,
self-update, the icon by the clock and the installer (1.3.0), and the
sound upgrade (1.4.0: gapless, crossfade, the equalizer, the Discord
status). Still to do: performance measured on the real apps (startup
time, memory, including the helper programs).

## Radar: YTFast's own recommendations (steps 1 and 2 built, 1.5.0; the station, 1.6.0)

The owner finds YouTube Music's recommendations poor, and wants better
ones without changing what YouTube Music's users know: Home, Explore and
the Library stay exactly as they are. Planned with the owner on 20
October 2026; the name is the owner's choice ("Discover" was left out,
as YouTube Music has a "Discover Mix").

**Where:** a fourth item in the left menu, under Library: **Radar**. A
page of rows, as Home's, drawn by the same view (`views/page.rs`), so it
works in all three looks and needs no new kind of screen.

**How it chooses:** YouTube finds the songs (its radios, "Fans might
also like", similar artists, new releases); YTFast chooses among them by
what the user does:

- A song finished, repeated or liked counts for it; one skipped in its
  first seconds, or disliked, counts against it. Kept in a small file
  on the computer only (under 1 MB, at most a few thousand songs), never
  sent anywhere; a button in Settings forgets it.
- Only songs new to the user; at most two of one artist in a row of the
  page; never a song skipped early or disliked, nor, after a few
  dislikes, its artist; official songs before fans' uploads and covers.
- A song suggested by the radios of several favourites ranks above one
  suggested by one.
- Every row says why ("Because you played Lemon and Idol"), and a song's
  menu there has "Not interested", which takes effect at once.

**Its first rows:** New to you (from the radios of the user's
favourites); Artists to try (similar to the artists played most, not yet
played); New from your artists (new songs and albums from the artists
actually played).

**The station (27 October 2026, at the owner's word).** The owner found
rows of "songs like this song" dull, and wanted Radar to think in genres
and in time (this week, this month), to tell people which genres they
play ("most people do not know what genre they like"), and above all one
button that plays songs the listener did not choose, steered by likes
and dislikes as they come. Built in 1.6.0: the record keeps each play's
date and each song's genre (Musixmatch's); the page says "This week you
played mostly ..." and has one Play button; "New to you" became the
station's pool; "Artists to try" is one row mixed by genre; "New from
your artists" has one release per artist (the owner had found
duplicates). Deezer's listeners give a second opinion, with no key (the
owner wanted only sources that need nothing from them: no API keys, no
accounts; Last.fm and ListenBrainz's radios need one, Spotify's are
closed). How the station chooses is in AGENTS.md.

**Later:** an endless Radar station (new songs, a familiar one now and
then), Deep cuts (album songs the user has not heard from artists they
play), Rediscover (songs once finished often, not played for months).

**Limits:** built once a day from about ten requests, one at a time (as
a listener opening ten radios); never in a loop, never a song's audio
fetched ahead for it. The first day starts from YouTube's History and
Liked Music, so the page is useful at once. The demo answers with
made-up rows. AGENTS.md's list of request limits gets Radar's.

**Steps:**

1. Note what the user finishes, skips and likes (on the computer).
   **Built** (1.5.0).
2. The Radar page with its first three rows and "Not interested",
   tried in the demo: released as 1.5.0. **Built** (20 October 2026),
   with Settings' Forget; how it chooses is in AGENTS.md.
3. The station. **Built** (1.6.0). Deep cuts and Rediscover: not yet.
4. Measured on the owner's laptop (Radar's songs finished against
   skipped), and tuned.
5. Later, the owner's ideas: one record for both laptops (a folder both
   sync, as OneDrive), one record per account on one computer (the
   owner's brother signing in there), and a page of the listener's own
   numbers (plays, replays, skips, likes, by week and month: the record
   already keeps them).

## Ideas for later

The owner's (19 October 2026). Not started.

- **A tab for AI music.** The user connects an AI music service of their
  own choosing (their own account and key, kept as safely as the
  sign-in), picks a few songs, and gets a new song made from them. To
  work out first: which services offer this, free or not, and their
  terms; what may be sent to them (a song's name and style, never its
  audio, unless the service's terms and copyright allow it); and where
  the songs made are kept.

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
