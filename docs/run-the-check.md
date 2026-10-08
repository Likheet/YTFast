# How to run the YtFast check

The check is a small program that tests, on your own laptop, whether
YouTube Music can work the YtFast way. It takes about five minutes. Run it on
your Mac and on your Windows laptop.

It will:

1. Download two helper programs (about 250 MB on disk, only the first time).
2. Read your YouTube sign-in from your web browser.
3. Check your YouTube Music account and Premium.
4. Read your Liked songs.
5. Check that Premium-quality audio is offered.
6. Play three of your Liked songs, from the top of the list. You control
   them with the keyboard. (It leaves out the song you played last, and any
   song YouTube no longer offers you.)
7. Check that those plays reached your YouTube Music History.

At the end it saves a report called `ytfast-check-report.txt`. Send that file
to Claude.

## Before you start

Open **music.youtube.com** in your browser and make sure you are signed in
with your **Premium** account. Like at least a few songs if you have none.

- **Mac:** Chrome, Firefox or Safari all work. Chrome is simplest.
  **Safari** needs one setting first, or the check cannot read its sign-in:
  open System Settings, then Privacy & Security, then **Full Disk Access**,
  and turn on **Terminal** (click + and choose Terminal from Applications >
  Utilities if it is not in the list). Then quit Terminal and open it again.
- **Windows:** use **Firefox**. Chrome and Edge on Windows lock their sign-in
  data so other programs cannot read it. If you don't have Firefox, install
  it from mozilla.org, then sign in to music.youtube.com in it once.

## Get the program

1. Go to the repository on GitHub, open the **Actions** tab, and click the
   newest run with a green tick that is marked **main** (the branch name
   shows beside each run). Runs marked with any other name are unfinished
   work that has not been accepted yet. To see only main's runs, choose
   **main** in the **Branch** menu above the list.
2. Scroll down to **Artifacts** and download:
   - `ytfast-check-macos-apple-silicon` on the Mac, or
   - `ytfast-check-windows` on the Windows laptop.

GitHub keeps these downloads for 30 days. If main's newest run shows no
Artifacts (or says they have expired), make a fresh one: on the
**Actions** tab, click **CI** on the left, then **Run workflow**, leave
the branch on **main**, and click the green **Run workflow** button. When
the new run has its green tick, its Artifacts are there to download.

## Run it on the Mac

1. In Downloads, double-click what you downloaded, and keep double-clicking
   anything ending in `.zip` or `.tar.gz`, until you see a file called
   **ytfast-check**.
2. Open **Terminal** (press Cmd+Space, type `Terminal`, press Enter).
3. Type `xattr -c ` (with a space at the end). Then drag the **ytfast-check**
   file onto the Terminal window and press Enter. Nothing is shown when it
   works. This tells your Mac you trust the file.
4. Drag the **ytfast-check** file onto the Terminal window again, and press
   Enter. The check starts.
5. When it asks which browser you use, type its number and press Enter.
6. If you picked Chrome, your Mac asks whether to allow access to "Chrome Safe
   Storage". Type your Mac password and click **Always Allow**. This is how
   the check reads your YouTube sign-in.

The report is saved in your home folder. In Finder, press Cmd+Shift+H to
open it.

## Run it on Windows

1. Right-click the downloaded zip, choose **Extract All**, then open the
   extracted folder.
2. Double-click **ytfast-check.exe**.
3. If Windows says "Windows protected your PC", click **More info**, then
   **Run anyway**.
4. When it asks which browser you use, type `2` (Firefox) and press Enter.

The report is saved in the same folder as `ytfast-check.exe`.

## While songs play

| Key | What it does |
|---|---|
| Space | Pause or play |
| Left / Right arrow | Back or forward 10 seconds |
| N | Next song |
| Q | Stop playing |

Please also try this: pause for a few minutes, or close the laptop lid and
open it again, then press Space. The song should carry on.

## Is it safe?

- It reads **only YouTube's** sign-in from your browser. Every other site's
  sign-in data is dropped the moment it is read.
- That copy is kept in a private folder during the check and deleted at the
  end, also if you close the window early. It is only ever sent to YouTube.
- The report has **no account name, song titles, passwords or cookies** in
  it, so it is safe to share. It does not name your headphones or speakers
  either, and your user folder is written as `~` (Mac) or `%USERPROFILE%`
  (Windows).
- The songs it plays appear in your YouTube Music History. This is on
  purpose: it is what keeps your recommendations working.
- Never paste your browser's cookies or a `cookies.txt` file into any chat,
  including with Claude.

The helper programs are stored here. Delete the folder to remove them:

- Mac: `~/Library/Application Support/YtFast`
- Windows: `%LOCALAPPDATA%\YtFast`

## If something goes wrong

The check says what happened in plain words, and the report has the details.
Common fixes:

- **"No YouTube sign-in was found"**: open music.youtube.com in that browser,
  check you are signed in, quit the browser, and run the check again. If you
  use several Chrome profiles, see `--browser` under Options below.
- **"This computer did not allow reading that browser's data"** (Mac,
  Safari): turn on Full Disk Access for Terminal, as in "Before you start",
  then quit Terminal, open it again and run the check again.
- **It shows the wrong account**: sign in to the right account in the
  browser (or use the right browser profile), and run the check again.
- **Mac says the file "cannot be opened"**: do step 3 of the Mac steps again.
  Or open System Settings, then Privacy & Security, and click **Open Anyway**.
- **Windows Security removed a file**: open Windows Security, then
  Protection history, and allow it.
- **"Confirm you're not a bot"**: play any song on music.youtube.com in your
  browser, then run the check again.
- **History did not update**: check that YouTube History is turned on at
  myactivity.google.com (Activity controls).
- **History says "could not tell"**: the songs it played were already the
  newest in your History, so a new play looks the same as the old one. Play
  any other song on music.youtube.com, then run the check again.

Whatever happens, send the report to Claude.

## Options

Options are typed after the program's name, with a space before each.

- **Mac:** open Terminal, drag the **ytfast-check** file onto the Terminal
  window, then type a space and the options, and press Enter. For example
  (the first part is what dragging the file puts there):

  ```
  /Users/you/Downloads/ytfast-check --browser firefox
  ```

- **Windows:** open the folder that has **ytfast-check.exe** in it,
  right-click an empty part of the folder and choose **Open in Terminal**.
  Type `.\ytfast-check.exe`, a space and the options, and press Enter
  (the `.\` at the start is needed). For example:

  ```
  .\ytfast-check.exe --browser firefox
  ```

The options:

```
--browser firefox             skip the browser question
--browser "chrome:Profile 1"  a Chrome profile other than the first
--song <link>                 play this song instead (can be given more than once)
--count 5                     play five Liked songs
--no-play                     check everything except playing (the check then
                              says two steps were skipped)
--cookies FILE                use a cookies.txt file instead of a browser
--save-replies FOLDER         also save YouTube Music's replies, for YtFast's tests
```

### Saving YouTube Music's replies

Only do this when Claude asks for it. With `--save-replies replies`, the
check also asks YouTube Music for the pages YtFast reads (your Liked songs,
History, Home, library, a search, one song's details and its album) and
saves YouTube's answers as files in a folder called `replies`, next to the
report. YtFast's tests can then be checked against real answers.

Before a file is written, the check takes out your name, handle, email,
account picture and channel, every email address, and everything tied to
your sign-in (no cookies or passwords are ever saved). The files still show
which songs you liked and played, and your playlists' names. Send the
folder only to the person who asked for it, never as text pasted into a
chat.
