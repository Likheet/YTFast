# How to run YTFast

YTFast (YouTube Music Fast) is YouTube Music as a small, fast app: Home,
Explore, your Library and playlists, search, a player page with lyrics, and
a player with an "Up next" queue. It plays songs only (no videos) and needs
a YouTube Music **Premium** account.

## Before you start

Open **music.youtube.com** in your browser and make sure you are signed in
with your Premium account. YTFast uses that sign-in.

- **Windows:** use **Firefox**. Chrome, Edge and Brave on Windows lock their
  sign-in data so other programs cannot read it. If you don't have Firefox,
  install it from mozilla.org, then sign in to music.youtube.com in it once.
- **Mac:** Chrome, Firefox, Edge, Brave or Safari. Chrome is simplest.

## Get the app

1. Go to the repository on GitHub, open the **Actions** tab, and click the
   newest run with a green tick.
2. Scroll down to **Artifacts** and download:
   - `YTFast-for-Windows` on the Windows laptop, or
   - `YTFast-for-Mac` on the Mac.

## Windows

1. Right-click the downloaded zip, choose **Extract All**, then open the
   extracted folder.
2. Double-click **YTFast.exe**.
3. If Windows says "Windows protected your PC", click **More info**, then
   **Run anyway**. Windows asks this because YTFast is not signed with a
   paid certificate. It asks only once.
4. Choose **Firefox** and click **Continue**.

To keep it handy, move `YTFast.exe` to a folder of your choice (for example
Documents), then right-click it and choose **Pin to Start** or **Pin to
taskbar**.

## Mac

1. In Downloads, double-click what you downloaded. In the folder that
   opens, double-click **YTFast-mac.zip**. **YTFast** appears.
2. Drag **YTFast** into your **Applications** folder.
3. Open it. The first time, your Mac says it could not check the app. Click
   **Done**. Then open **System Settings**, click **Privacy & Security**,
   scroll down, and click **Open Anyway** next to the message about YTFast.
   Confirm with your password or Touch ID. You do this only once: YTFast is
   not signed by Apple because that needs a paid developer account.
4. Choose your browser and click **Continue**.
   - **Chrome, Edge or Brave:** your Mac asks whether to allow access to
     "Chrome Safe Storage" (or the browser's own). Type your Mac password and
     click **Always Allow**. This is how YTFast reads your YouTube sign-in.
   - **Safari:** your Mac refuses at first, and again after each new copy
     downloaded from GitHub. Click **Open Full Disk Access** under the
     message, turn on **YTFast** in the list that opens, and let your Mac
     reopen it. (A copy that Claude builds on your Mac keeps the
     permission.)

## The first time

The first time you sign in, YTFast downloads two helper programs, yt-dlp
and Deno (about 250 MB on disk). This takes a minute or two and happens only
once. If you ran the YtFast check on this computer, they are already there.

Then it gets YouTube Music's player ready in the background (a few seconds,
again once). After that, songs start in about a second.

YTFast remembers your browser and signs in by itself each time it opens.
While it is open it also reads your browser's sign-in again by itself
whenever YouTube stops accepting the copy it has (browsers renew their
sign-in as they go), so you are not asked to sign in again. Stay signed in
to music.youtube.com in that browser.

## Using it

YTFast is laid out like YouTube Music: a bar across the top, the menu on
the left, the page in the middle, and the player bar across the bottom
once a song plays.

**Getting around**

- **Home**, **Explore** and **Library** are in the menu on the left, with
  **New playlist** and your playlists under them. The button with three
  lines at the top left closes the menu to a strip of icons, and opens it
  again.
- The arrows before the search box go back and forward.
- The round button at the top right is your account: **History**,
  **Settings** and **Sign out** are in its menu.

**Playing**

- **Click a song** to play it. The songs after it in that list play next.
- Songs start in about a second. A song you rest the pointer on, and the top
  search result, are made ready ahead of time, so they start at once.
- When the queue runs out, songs like the last one follow, as YouTube
  Music's autoplay does (you can turn this off in Settings).

**The player page**

- Click the song in the player bar to open it: the cover large on the
  left, over its own colours, and on the right the tabs **Up next**,
  **Lyrics** and **Related**.
- Lyrics follow the song: the line being sung is lit. Click any line to jump
  there. Scroll to read ahead; it starts following again a moment later.
- **Esc**, the back arrow, or another click on the song in the player bar
  closes it.

**Menus**

- **Right-click a song**, or click the three dots that show at its right
  when the pointer is on it: Start radio, Play next, Add to queue, Add to
  liked songs, Add to playlist, Go to album, Go to artist, Copy link. On
  your own playlists, also **Remove from playlist**. The three dots at the
  right of the player bar give the same for the song playing.
- **Right-click an album or playlist**: Play, Shuffle play, Start radio,
  Play next, Add to queue, Save to library, Copy link.
- **Right-click a song in Up next**: Play next, Move up, Move down, Remove
  from queue.

**Your music**

- The thumbs in the player bar like or dislike the song playing.
- **Library** has Playlists, Songs, Albums and Artists.
- An album or a playlist shows its cover on the left with three buttons
  under it: the big one plays it, the one with three dots has Shuffle
  play, Play next and Add to queue, and the third saves it to your library
  (a plus, or a tick once saved). On your own playlists the third is a
  pencil, which renames it, and **Delete playlist** is under the three
  dots. Try changes on a playlist you don't mind first.
- On an artist: **Shuffle**, **Radio** and **Subscribe**.
- **Search** suggests as you type; the results can be narrowed to Songs,
  Albums, Artists or Playlists.

**In the player bar**

- In the middle: **Shuffle** (mixes up the songs coming up), previous,
  play or pause, next, and **Repeat** (off, then the queue, then the
  song; it is lit while on).
- On the right: the time, like and dislike, the volume (click it to mute;
  rest the pointer on it for the volume bar, or scroll over it), and the
  song's menu.
- Click or drag the red line along the top of the bar to jump in the song.

| Key | What it does |
|---|---|
| Space | Pause or play |
| ← / → | Back or forward 10 seconds |
| ↑ / ↓ | Volume up or down |
| Shift+N / Shift+P | Next or previous song |
| M | Mute |
| L | Like the song playing |
| / or Ctrl+F (Cmd+F on a Mac) | Search |
| Esc | Close the player page |
| Your keyboard's media keys | Play, pause, next, previous |

The song playing also shows in your computer's own media controls (the
Windows volume pop-up, or the Mac's Control Center).

**Settings** (in your account's menu, at the top right): start songs the
fast way, keep playing when the queue ends, even out loudness, sign out,
and where problems are noted.

Lyrics come from YouTube Music, or from LRCLIB (lrclib.net, a free lyrics
site) when YouTube Music has none that follow the song. LRCLIB is only
asked about the song playing, when the Lyrics tab is open.

## Is it safe?

- It reads **only YouTube's** sign-in from your browser. Every other site's
  sign-in data is dropped the moment it is read.
- That copy is kept in a private folder while YTFast is open and deleted
  when you close it. It is only ever sent to YouTube.
- Songs you play appear in your YouTube Music History, as on the website, so
  your recommendations keep working.
- YTFast finds songs the way the website does, and sometimes through yt-dlp.
  yt-dlp's makers warn that using an account with it can get the account
  banned, temporarily or permanently. YTFast only asks for what a listener
  does (the song playing and the next few), never bulk downloads, but the
  risk is not zero.
- Never paste your browser's cookies or a `cookies.txt` file into any chat,
  including with Claude.

YTFast keeps its files here. Delete the folders to remove them:

- Mac: `~/Library/Application Support/YtFast` and `~/Library/Caches/YtFast`
- Windows: `%LOCALAPPDATA%\YtFast`

## If something goes wrong

YTFast says what happened in plain words. Common fixes:

- **Sign-in fails**: open music.youtube.com in that browser, check you are
  signed in to the right account, quit the browser, and click **Try again**.
- **A song says YouTube no longer accepts the saved sign-in**: YTFast could
  not read a fresh sign-in from your browser. Check you are still signed
  in to music.youtube.com there, wait a minute, and press play again. If
  it keeps happening, send Claude the log (below).
- **Songs take about ten seconds to start, or fail to start**: the fast way
  did not work (YouTube may have changed something), and YTFast used
  yt-dlp instead. Send Claude the log (below). If songs fail to start at
  all, turn off **Start songs the fast way** in Settings.
- **"Confirm you're not a bot"**: play any song on music.youtube.com in your
  browser, then try again in YTFast.
- **A song is skipped**: YouTube does not offer it to your account (removed,
  or not available in your country). YTFast moves on to the next one.
- **No sound**: check your computer's volume and output device. YTFast
  follows the default output, so plugging in headphones moves the sound to
  them.
- **Mac says the app "cannot be opened" or "is damaged"**: do step 3 of the
  Mac steps again. If there is no **Open Anyway** button, open Terminal and
  run `xattr -cr /Applications/YTFast.app`, then open YTFast again.
- **Windows Security removed the file**: open Windows Security, then
  Protection history, and allow it.

YTFast notes problems in a file called `ytfast.log`, made new each time it
opens. It has no passwords or cookies in it, and web addresses are cut
short, so it is safe to send to Claude. **Settings → Open that folder**
opens the folder it is in:

- Mac: `~/Library/Caches/YtFast/ytfast.log`
- Windows: `%LOCALAPPDATA%\YtFast\cache\ytfast.log`

To see how a song was found and how long it took, hold the pointer over the
artist's name in the player bar.

## Try it without an account

To see YTFast with made-up songs and no sign-in, start it with `--demo`.
On Windows, in the folder with YTFast.exe, type `cmd` in the address bar,
press Enter, and run `YTFast.exe --demo`. On a Mac, open Terminal and run
`/Applications/YTFast.app/Contents/MacOS/ytfast --demo`. The demo can be
open at the same time as YTFast itself: it does not touch your sign-in or
your settings, and notes its problems in a file of its own
(`ytfast-demo.log`).
