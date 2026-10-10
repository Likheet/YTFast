# How to run YTFast

YTFast (YouTube Music Fast) is YouTube Music as a small, fast app: Home,
Explore, your Library and playlists, search, a player page with lyrics, and
a player with an "Up next" queue. It plays songs, and in its video mode
their music videos, and needs a YouTube Music **Premium** account.

## Before you start

Open **music.youtube.com** in your browser and make sure you are signed in
with your Premium account. YTFast uses that sign-in.

- **Windows:** use **Firefox**. Chrome, Edge and Brave on Windows lock their
  sign-in data so other programs cannot read it. If you don't have Firefox,
  install it from mozilla.org, then sign in to music.youtube.com in it once.
- **Mac:** Chrome, Firefox, Edge, Brave or Safari. Chrome is simplest.

## Get the app

Open YTFast's newest release,
[github.com/Likheet/YTFast/releases/latest](https://github.com/Likheet/YTFast/releases/latest),
and under **Assets** download the file for your computer:

- **Windows** (64-bit, Intel or AMD): **YTFast.exe**.
- **Mac** (Apple Silicon: M1 or newer): **YTFast.dmg**. Macs with an
  Intel processor are not supported yet.

The other files there are for YTFast itself: installed copies update from
the `ytfast-v…zip` and the two `checksums` files, and GitHub adds the
"Source code" ones to every release. You need none of them.

(Building it yourself: every push to the repository on GitHub builds both.
Open its **Actions** tab, click the newest run with a green tick marked
**main**, and download `YTFast-for-Windows` or `YTFast-for-Mac` from
**Artifacts**. GitHub hands each one over zipped (extract it first) and
keeps them for 30 days; **CI**, **Run workflow** makes a fresh run. Beside
the app are this guide, `HOW-TO-RUN.md`, and `THIRD-PARTY-NOTICES.txt`,
the licences of the work YTFast is built with.)

## Windows

1. Move **YTFast.exe** from Downloads to a folder of your choice (for
   example Documents), then double-click it.
2. If Windows says "Windows protected your PC", click **More info**, then
   **Run anyway**. Windows asks this because YTFast is not signed with a
   paid certificate. It asks once for each new copy you download.
3. Choose **Firefox** and click **Continue**.

To keep it handy, right-click **YTFast.exe** and choose **Pin to Start** or
**Pin to taskbar**.

YTFast updates itself: once a day it looks for a new version and
downloads it in the background. An **Update** button then shows at the
top; click it, then **Restart to update** to install it. It keeps a small
file, `ytfast-portable.txt`, next to `YTFast.exe`: leave it there, it is
what lets YTFast update itself. **Settings**, **Updates** turns automatic
installs off.

## Mac

1. In Downloads, double-click **YTFast.dmg**. A window opens with
   **YTFast** and **Applications** in it.
2. Drag **YTFast** onto **Applications**. Then eject the disk image (the
   ⏏ beside **YTFast** in the Finder's sidebar); YTFast.dmg can go in the
   Bin.
3. Open YTFast from Applications. The first time, your Mac says it could
   not verify the app, and offers only **Move to Bin** and **Done**. Click
   **Done** (not Move to Bin). Then open **System Settings**, click
   **Privacy & Security**, scroll down to **Security**, and click **Open
   Anyway** next to the message about YTFast. Confirm with your password
   or Touch ID, then click **Open**. You do this once for each new copy you
   download: YTFast is not checked by Apple, because that needs a paid
   Apple developer account.
4. Choose your browser and click **Continue**.
   - **Chrome, Edge or Brave:** your Mac asks whether to allow access to
     "Chrome Safe Storage" (or the browser's own). Type your Mac password and
     click **Always Allow**. This is how YTFast reads your YouTube sign-in.
   - **Safari,** and any browser whose data your Mac keeps from YTFast:
     YTFast says Full Disk Access is not given. This happens at first, and
     again after each new copy downloaded from GitHub. Click **Open Full
     Disk Access** under the message, turn on **YTFast** in the list that
     opens (if it is not there, click **+** and choose it in
     Applications), and let your Mac reopen it. (A copy that Claude builds
     on your Mac should keep the permission from one build to the next;
     that is not yet confirmed.)

From version 0.6.4, YTFast updates itself on a Mac too, as long as it is
in **Applications**: once a day it looks for a new version and downloads
it in the background. An **Update** button then shows at the top; click
it, then **Restart to update**. YTFast closes, the new version takes its
place, and it opens again, saying it was updated. If your Mac has the
signature that a copy built on it uses ("YTFast Local Signing"), the new
version is signed with it, so your Mac should keep Full Disk Access.
Otherwise you may need to turn Full Disk Access on again after an update.
**Settings**, **Updates** turns automatic downloads off.

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
  Music's autoplay does (turn this off at the end of Up next, or in
  Settings).

**The player page**

- Click the player bar (or the ▲ at its right) to open it: it slides up,
  with the cover large on the left and on the right the tabs **Up next**,
  **Lyrics** and **Related**. Click the cover to pause or play.
- **Up next** says what the songs play from ("Playing from"), lists the
  whole queue (songs already played too, the playing one marked), and
  ends with the **Autoplay** switch.
- Lyrics follow the song: the line being sung lights word by word as it is
  sung. Click any line to jump there. Scroll to read ahead; it starts
  following again a moment later.
- **Translate**, above the lyrics, shows each line in English under it.
  For a song in another script (Japanese, Korean, Hindi...), it also shows
  the line in Latin letters. It stays on for the songs after it until you
  press it again (it is in Settings too).
- **Song** and **Video** above the cover: **Video** plays the songs in the
  queue as their music videos (the video takes the cover's place), also
  after Next, Previous or a click in Up next. Switching picks up at the
  same music, even when the video has an intro of its own, and the
  lyrics stay in time with the video. A song you start anywhere else
  plays as a song again. A song with no video keeps its cover, with
  Video greyed out. Videos need the fast way (Settings) to be on.
- **Esc**, the back arrow, the ▼ at the player bar's right, or another
  click on the player bar closes it.

**Menus**

- Menus look and read as YouTube Music's.
- **Right-click a song**, or click the three dots that show at its right
  when the pointer is on it: Start mix, Play next, Add to queue, Add to
  liked songs, Save to playlist, Go to album, Go to artist, Share. On
  your own playlists, also **Remove from playlist**. The three dots in the
  middle of the player bar give the same for the song playing.
- **Save to playlist** opens a window with your playlists (click one to
  add the song) and a **New playlist** button.
- **Share** copies the song's link.
- **Right-click an album or playlist**: Shuffle play, Start mix, Play
  next, Add to queue, Save album (or playlist) to library, Share.
- **Right-click a song in Up next**: Start mix, Play next, Move up, Move
  down, ..., Remove from queue.

**Your music**

- The thumbs in the middle of the player bar like or dislike the song
  playing.
- **Library** has Playlists, Songs, Albums and Artists.
- An album or a playlist shows its cover on the left with three buttons
  under it: the big one plays it, the one with three dots has Shuffle
  play, Play next and Add to queue, and the third saves it to your library
  (a plus, or a tick once saved). On your own playlists the third is a
  pencil, which renames it, and **Delete playlist** is under the three
  dots. Try changes on a playlist you don't mind first.
- On an artist: **Shuffle**, **Mix**, **Subscribe** (with the number of
  subscribers) and a menu; the description opens with **More**. Top songs
  shows plays, and **Show all** under it.
- **Search** suggests as you type: words, then the artist and songs it
  finds, with pictures (a song plays, an artist opens). The results can be
  narrowed with YouTube Music's own buttons (Songs, Albums, Community
  playlists...); the × before them goes back to all results.

**In the player bar**

- Laid out as YouTube Music's. On the left: previous, play or pause,
  next, and the time.
- In the middle: the song playing (click the artist's or the album's name
  to go there), like and dislike, and the song's menu.
- On the right: the volume (click it to mute; rest the pointer on it for
  the volume bar, or scroll over it), **Repeat** (off, then the queue,
  then the song; white while on), **Shuffle** (white while on: the songs
  coming up play in a random order, in this queue and the next; off puts
  them back in order), and ▲ to open the player page.
- Click or drag the red line along the top of the bar to jump in the song;
  the time under the pointer shows above it.

| Key | What it does |
|---|---|
| Space or ; | Pause or play |
| J / K (or Shift+N / Shift+P) | Next or previous song |
| L / H (or → / ←) | Forward or back 10 seconds |
| Shift+L / Shift+H | Forward or back 1 second |
| = / - (or ↑ / ↓) | Volume up or down |
| M | Mute |
| S | Shuffle on or off |
| R | Repeat |
| Q | Open or close the player page (Esc closes it) |
| F | Full screen on or off, with the player page (Esc leaves it) |
| + / _ | Like or dislike the song playing |
| G then H, E, L or , | Home, Explore, Library or Settings |
| / or Ctrl+F (Cmd+F on a Mac) | Search |
| ? | This list, in the app |
| Your keyboard's media keys | Play, pause, next, previous |

These are YouTube Music's own keys (L no longer likes a song: use +).

The song playing also shows in your computer's own media controls (the
Windows volume pop-up, or the Mac's Control Center).

**Full screen** (F): the player page fills the screen, with the cover
or the video as large as it fits and Up next, Lyrics and Related beside
it. F or Esc leaves it, and so does closing the player page.

**Settings** (in your account's menu, at the top right): start songs the
fast way, keep playing when the queue ends, even out loudness, skip songs
you dislike (on: disliking the song playing moves on to the next, as
YouTube Music does), ask before closing while a song plays, translate
lyrics, sign out, and where problems are noted.

**Closing while a song plays** asks first: "Do you really want to close?
There's a song playing.", with **Do not ask again** ticked. Yes closes
YTFast (and, ticked, it never asks again; Settings can turn the question
back on); No keeps it playing.

Lyrics come from Musixmatch (timed word by word, for many songs), from
YouTube Music, or from LRCLIB (lrclib.net, a free lyrics site). Lyrics
timed only by the line also light word by word, at the song's own pace (an
estimate). YTFast asks for the playing song's lyrics when the player page
opens. With the Lyrics tab open, it also asks for the next song's lyrics,
so they show as soon as that song starts. Translations come from Musixmatch
(written by people) when it has them, else from Google Translate.
Musixmatch and Google Translate are asked through the addresses their own
apps use, not through an official service. They can stop answering one
day; the lyrics then come from the other sources, without translations.

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

- Mac: `~/Library/Application Support/YtFast` (your settings are in it
  too) and `~/Library/Caches/YtFast`
- Windows: `%LOCALAPPDATA%\YtFast`, and your settings in `%APPDATA%\ytfast`

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
