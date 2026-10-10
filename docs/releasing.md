# Releasing YTFast

A release is a GitHub release on `Likheet/YTFast` (public). Every copy of
YTFast on Windows looks at the newest one a minute after it starts and once
a day, downloads it in the background, checks it, and installs it when the
listener restarts (`crates/ytfast/src/update.rs`, fastframe-update). A Mac
copy says a new version is out; it is installed by hand.

## The signing key

The updater installs nothing whose `checksums.txt` is not signed with the
publisher key. Its public half is built into the app
(`crates/ytfast/assets/update-public-key.hex`); its private half is on the
owner's Windows laptop only:

    %APPDATA%\YTFast-release\update-signing-key.hex

It never goes in the repository, a release, a chat or a log. Keep a copy
somewhere safe (a password manager, a USB stick): without it, no update can
be signed for the copies already out there, and everyone would have to
download YTFast by hand again. A new key (`ytfast-release key`) only works
for builds made after its public half is put in the app.

## Steps

1. Raise the version in `Cargo.toml`, `packaging/macos/Info.plist` and
   `packaging/windows/ytfast.rc` (three numbers, higher than the last
   release), with `Cargo.lock`. Merge to `main` once the owner has tried
   the build.
2. Wait for CI on that `main` commit, then download its two apps (each
   can be downloaded as soon as its own job has finished):

       gh run download <run id> -n YTFast-for-Windows -D dist/windows
       gh run download <run id> -n YTFast-for-Mac -D dist/mac

3. Pack and sign (the version without a `v`):

       cargo run -p ytfast-release -- pack 0.6.0 "%APPDATA%\YTFast-release\update-signing-key.hex" dist/release dist/windows/YTFast.exe dist/mac/YTFast.dmg

   It writes the two downloads, `YTFast.exe` and `YTFast.dmg` (CI's disk
   image: the app beside a shortcut to Applications);
   `ytfast-v0.6.0-x86_64-pc-windows-msvc.zip` (a folder of that name with
   `ytfast.exe` and the `ytfast-portable.txt` marker: the updater's
   layout, which installed Windows copies look for, so it must be on every
   release); `checksums.txt` and `checksums.txt.sig`; and checks the
   signature. The zip carries no clock time: the same program packs to
   the same bytes, so a release can be packed again to add a file without
   changing what copies update from.

   Every release carries both builds, even when only Windows was tried.
   Mac copies (from 0.6.4) update themselves from `YTFast.dmg`, checked
   against the same signed `checksums.txt`: keep that name.

4. Publish, tagged `v` and the version, on `main`'s commit:

       gh release create v0.6.0 --target <commit> --title "YTFast 0.6.0" --notes-file <notes> dist/release/*

   The notes are for listeners: what changed, in plain words; that the
   builds are not signed by Microsoft or Apple (Windows' "protected your
   PC" and the Mac's "could not check" warnings); that YouTube's terms do
   not allow apps like this.

5. Check it as an update would, from a copy of the release's Windows
   program in a folder with its marker (no window opens):

       ytfast.exe --update-dry-run > dry-run.txt

   It finds the newest release as if it were the oldest version, downloads
   it, checks the signature, the checksum and the program's `--version`,
   and installs nothing. `dry-run.txt` should end with "all good".

## What the updater needs

- `ytfast --version` prints `ytfast <version>` (the helper asks the
  downloaded program before installing it).
- On Windows the `ytfast-portable.txt` marker beside the program; YTFast
  writes it itself (not in Program Files), so a copy sent as a bare
  `YTFast.exe` updates too.
- Release files named as above; the tag is `v<version>`.
- The update flags (`--apply-update`, `--update-receipt`, `--update-error`)
  are a contract between versions: see fastframe-update's README.
