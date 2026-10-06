# YTFast

**YouTube Music, native and fast.** YTFast (YouTube Music Fast) is a desktop app for YouTube Music that
aims to open in under a second and stay light on memory, in the spirit of
[Spotifast](https://github.com/crmne/spotifast). The goal is everything the
YouTube Music app does except video, with its own screens instead of a web
page.

For YouTube Music **Premium** accounts, on **Mac (Apple Silicon)** and
**Windows**.

## Status: the second version of the app

The app has the YouTube Music layout: a sidebar with Home, Explore, Library,
History and your playlists; search with suggestions and filters; album,
artist, playlist and mood pages; a player bar with likes, repeat, shuffle and
volume; Up next; and a player page with the song's cover, time-synced lyrics
and related songs, over colours taken from the cover (in the style of Better
Lyrics' Even Better Lyrics Plus theme). You can like songs, save albums and
playlists, subscribe to artists, and make, rename, delete and edit
playlists. Songs start in about a second.

The first version ran on a real account; this one is still to be tried on
the owner's laptops.

**Run the app:** [docs/run-the-app.md](docs/run-the-app.md)

Before the app, a small check program proved the YTFast way works on a real
computer with a real account (signing in, Premium-quality audio, playing,
plays reaching History). It passed on Windows; the Mac run is still to do.
**Run the check:** [docs/run-the-check.md](docs/run-the-check.md)

The plan and its risks: [docs/plan.md](docs/plan.md)

## How it works

- **Your sign-in** comes from your web browser. Only YouTube's part is read;
  it stays on your computer and is only sent to YouTube.
- **Your music** (Library, Home, search) comes from the same internal API
  music.youtube.com uses. There is no official YouTube Music API.
- **The audio** of each song is found the way the website finds it, with
  the challenge solver of [yt-dlp](https://github.com/yt-dlp/yt-dlp) kept
  running in the background, and plays while it downloads. If that fails,
  yt-dlp itself finds the song. YTFast downloads yt-dlp and Deno (which
  runs the solver) and checks both against their published fingerprints.
- **Lyrics** come from YouTube Music, or from [LRCLIB](https://lrclib.net)
  (a free lyrics database) when YouTube Music has none with times.
- **Playing** happens in YTFast itself, through the same audio code as
  Spotifast, so it uses no CPU while paused and follows your headphones.
- **Your plays are reported to YouTube**, so History, Home and your mixes keep
  learning, as they do on the website.

## For developers and AI assistants

Start with [AGENTS.md](AGENTS.md). Build and test:

```sh
cargo test --workspace
cargo run -p ytfast              # the app
cargo run -p ytfast -- --demo    # the app with made-up music, no account
cargo run -p ytfast-check        # the step 0 check
```

Linux needs `libasound2-dev` for the audio library.

## Credits

YTFast builds on [fastframe](https://github.com/crmne/fastframe) and learns
from [Spotifast](https://github.com/crmne/spotifast) (both MIT, by Carmine
Paolino), [ytmusicapi](https://github.com/sigma67/ytmusicapi) (MIT),
[Kaset](https://github.com/sozercan/kaset) (MIT) and
[yt-dlp](https://github.com/yt-dlp/yt-dlp) (Unlicense). Its look is
inspired by [Better Lyrics](https://betterlyrics.org)' Even Better Lyrics
Plus theme, recreated without its code. Lyrics come partly from
[LRCLIB](https://lrclib.net). Icons are from [Lucide](https://lucide.dev)
(ISC).

YTFast is an independent project, not affiliated with YouTube or Google.
YouTube and YouTube Music are trademarks of Google LLC. Using unofficial
apps is against YouTube's terms of service; see the risks in
[docs/plan.md](docs/plan.md).
