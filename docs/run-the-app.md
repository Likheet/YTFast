# How to run YtFast

YtFast is YouTube Music as a small, fast app: Home, Explore, your Library
and playlists, search, and a player with an "Up next" queue. It plays songs
only (no videos) and needs a YouTube Music **Premium** account.

## Before you start

Open **music.youtube.com** in your browser and make sure you are signed in
with your Premium account. YtFast uses that sign-in.

- **Windows:** use **Firefox**. Chrome, Edge and Brave on Windows lock their
  sign-in data so other programs cannot read it. If you don't have Firefox,
  install it from mozilla.org, then sign in to music.youtube.com in it once.
- **Mac:** Chrome, Firefox, Edge, Brave or Safari. Chrome is simplest.

## Get the app

1. Go to the repository on GitHub, open the **Actions** tab, and click the
   newest run with a green tick.
2. Scroll down to **Artifacts** and download:
   - `YtFast-for-Windows` on the Windows laptop, or
   - `YtFast-for-Mac` on the Mac.

## Windows

1. Right-click the downloaded zip, choose **Extract All**, then open the
   extracted folder.
2. Double-click **YtFast.exe**.
3. If Windows says "Windows protected your PC", click **More info**, then
   **Run anyway**. Windows asks this because YtFast is not signed with a
   paid certificate. It asks only once.
4. Choose **Firefox** and click **Continue**.

To keep it handy, move `YtFast.exe` to a folder of your choice (for example
Documents), then right-click it and choose **Pin to Start** or **Pin to
taskbar**.

## Mac

1. In Downloads, double-click what you downloaded. In the folder that
   opens, double-click **YtFast-mac.zip**. **YtFast** appears.
2. Drag **YtFast** into your **Applications** folder.
3. Open it. The first time, your Mac says it could not check the app. Click
   **Done**. Then open **System Settings**, click **Privacy & Security**,
   scroll down, and click **Open Anyway** next to the message about YtFast.
   Confirm with your password or Touch ID. You do this only once: YtFast is
   not signed by Apple because that needs a paid developer account.
4. Choose your browser and click **Continue**.
   - **Chrome, Edge or Brave:** your Mac asks whether to allow access to
     "Chrome Safe Storage" (or the browser's own). Type your Mac password and
     click **Always Allow**. This is how YtFast reads your YouTube sign-in.
   - **Safari:** your Mac may refuse at first. Open System Settings, then
     Privacy & Security, then **Full Disk Access**, turn on **YtFast**, and
     try again.

## The first time

The first time you sign in, YtFast downloads two helper programs, yt-dlp
and Deno (about 250 MB on disk). This takes a minute or two and happens only
once. If you ran the YtFast check on this computer, they are already there.

After that, YtFast remembers your browser and signs in by itself each time
it opens.

## Using it

- **Click a song** to play it. The songs after it in that list play next.
- **Right-click a song** for **Play next**, **Add to queue** and **Start
  radio** (songs like it).
- On an album or playlist, **Play** plays it all and **Shuffle** plays it in
  a random order.
- In the player bar at the bottom:
  - **Repeat** goes from off, to repeating the queue, to repeating the song.
  - **Shuffle** mixes up the songs coming up.
  - The **list** button at the far right shows **Up next**.
  - Click or drag the red line along the top of the bar to jump in the song.
- When the queue runs out, YouTube Music's own "Up next" songs follow, as on
  the website.

| Key | What it does |
|---|---|
| Space | Pause or play |
| / or Ctrl+F (Cmd+F on a Mac) | Search |
| Your keyboard's media keys | Play, pause, next, previous |

The song playing also shows in your computer's own media controls (the
Windows volume pop-up, or the Mac's Control Center).

## Is it safe?

- It reads **only YouTube's** sign-in from your browser. Every other site's
  sign-in data is dropped the moment it is read.
- That copy is kept in a private folder while YtFast is open and deleted
  when you close it. It is only ever sent to YouTube.
- Songs you play appear in your YouTube Music History, as on the website, so
  your recommendations keep working.
- YtFast uses your account through yt-dlp, and yt-dlp's makers warn that
  using an account with it can get the account banned, temporarily or
  permanently. YtFast only asks for what a listener does (the song playing
  and the next few), never bulk downloads, but the risk is not zero.
- Never paste your browser's cookies or a `cookies.txt` file into any chat,
  including with Claude.

YtFast keeps its files here. Delete the folder to remove them:

- Mac: `~/Library/Application Support/YtFast` and `~/Library/Caches/YtFast`
- Windows: `%LOCALAPPDATA%\YtFast`

## If something goes wrong

YtFast says what happened in plain words. Common fixes:

- **Sign-in fails**: open music.youtube.com in that browser, check you are
  signed in to the right account, quit the browser, and click **Try again**.
- **"Confirm you're not a bot"**: play any song on music.youtube.com in your
  browser, then try again in YtFast.
- **A song is skipped**: YouTube does not offer it to your account (removed,
  or not available in your country). YtFast moves on to the next one.
- **No sound**: check your computer's volume and output device. YtFast
  follows the default output, so plugging in headphones moves the sound to
  them.
- **Mac says the app "cannot be opened" or "is damaged"**: do step 3 of the
  Mac steps again. If there is no **Open Anyway** button, open Terminal and
  run `xattr -cr /Applications/YtFast.app`, then open YtFast again.
- **Windows Security removed the file**: open Windows Security, then
  Protection history, and allow it.

## Try it without an account

To see YtFast with made-up songs and no sign-in, start it with `--demo`.
On Windows, in the folder with YtFast.exe, type `cmd` in the address bar,
press Enter, and run `YtFast.exe --demo`. On a Mac, open Terminal and run
`/Applications/YtFast.app/Contents/MacOS/ytfast --demo`.
