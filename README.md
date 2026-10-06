# YtFast

**YouTube Music, native and fast.** A desktop app for YouTube Music that
opens in under a second and stays light on memory, in the spirit of
[Spotifast](https://github.com/crmne/spotifast). It does everything the
YouTube Music app does except video, with its own screens instead of a web
page.

For YouTube Music **Premium** accounts, on **Mac (Apple Silicon)** and
**Windows**.

## Status: step 0

Before building the app, a small check program proves that the YtFast way
works on a real computer with a real account: signing in, Premium-quality
audio, playing with pause and seek, and plays reaching your History.

**Run the check:** [docs/run-the-check.md](docs/run-the-check.md)

The plan and its risks: [docs/plan.md](docs/plan.md)

## How it works

- **Your sign-in** comes from your web browser. Only YouTube's part is read;
  it stays on your computer and is only sent to YouTube.
- **Your music** (Library, Home, search) comes from the same internal API
  music.youtube.com uses. There is no official YouTube Music API.
- **The audio** of each song is found by [yt-dlp](https://github.com/yt-dlp/yt-dlp),
  which YtFast downloads and checks against its published fingerprint. It
  runs for a moment per song.
- **Playing** happens in YtFast itself, through the same audio code as
  Spotifast, so it uses no CPU while paused and follows your headphones.
- **Your plays are reported to YouTube**, so History, Home and your mixes keep
  learning, as they do on the website.

## For developers and AI assistants

Start with [AGENTS.md](AGENTS.md). Build and test:

```sh
cargo test --workspace
cargo run -p ytfast-check
```

Linux needs `libasound2-dev` for the audio library.

## Credits

YtFast builds on [fastframe](https://github.com/crmne/fastframe) and learns
from [Spotifast](https://github.com/crmne/spotifast) (both MIT, by Carmine
Paolino), [ytmusicapi](https://github.com/sigma67/ytmusicapi) (MIT),
[Kaset](https://github.com/sozercan/kaset) (MIT) and
[yt-dlp](https://github.com/yt-dlp/yt-dlp) (Unlicense).

YtFast is an independent project, not affiliated with YouTube or Google.
YouTube and YouTube Music are trademarks of Google LLC. Using unofficial
apps is against YouTube's terms of service; see the risks in
[docs/plan.md](docs/plan.md).
