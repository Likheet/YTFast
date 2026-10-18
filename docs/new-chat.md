# Starting a new chat on YTFast

For any coding assistant (Claude, Codex) opening YTFast in a fresh chat.
[AGENTS.md](../AGENTS.md) is how the code works and its rules; this is the
order of work the owner expects, from the first command to the last
message. Keep it up to date when that changes.

## 1. Know which laptop you are on

The owner works from two laptops, one GitHub repository
(`Likheet/YTFast`), and one branch, `main`.

| | Mac (Apple Silicon) | Windows laptop |
|---|---|---|
| Build, test, push to `main` | yes | yes |
| Sign and publish a release | no: the key is not there | yes |
| Rust | Homebrew's rustup: if `cargo` is not found, run `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"` first | as installed |
| Put a build in front of the owner | `packaging/macos/install.sh` (AGENTS.md says why) | the release's installer |

The release signing key is only on the Windows laptop
(`%APPDATA%\YTFast-release\update-signing-key.hex`). Never ask for it in a
chat, and never copy it anywhere yourself.

## 2. Start from GitHub's main

The owner moves between laptops, so the other one may have pushed since
this one last looked.

```sh
git fetch origin
git switch main
git pull --ff-only
gh release list --limit 3
```

- If `main` will not move forward to `origin/main`, stop and say so; do
  not merge or force anything. On the Mac, `old-main-0.1.0` is an old line
  with no history in common with GitHub's: never build on it or push it.
- Changes already in the folder that are not committed came from another
  chat: ask the owner before touching them.
- A **draft** release in the list was started and not finished: see
  step 5.

## 3. Change it and test it yourself

The owner is not a programmer and does not try builds before they ship.
Testing is your job:

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace` (AGENTS.md, "Before you push").
- A test for each new rule; the app's own rules are tested in `app.rs`
  (CLAUDE.md says how).
- The demo (`cargo run -p ytfast -- --demo`) for anything on screen.

Then tell the owner plainly what was tested, and how and where (AGENTS.md,
"Who you are working with").

## 4. Ship it straight to main

- Commit to `main` and push. A short-lived local branch is fine; pull
  requests are not: the owner does not want them.
- Write commit messages as the owner's own: no "Co-Authored-By" or
  "Generated with" lines, no mention of AI.
- Add what changed and what was tested to AGENTS.md's "Current state",
  as each earlier round did.

## 5. Release it

Every change to the app ends in a release, so the owner gets it through
YTFast's Update button. Changes to documents alone need none. The steps
are in [releasing.md](releasing.md).

- **On the Windows laptop:** all the steps.
- **On the Mac:** raise the version (step 1), push, then make the release
  as a draft with its notes and no files, on that commit
  (`gh release create v<version> --draft --target <commit> --title "YTFast <version>" --notes-file <notes>`).
  End by telling the owner, in one line, to open a chat on the Windows
  laptop and say "finish the <version> release".
- **Asked to finish a release:** follow "Finishing a draft" in
  releasing.md.

## 6. Before the chat ends

- Nothing is left only on this laptop: everything is committed and
  pushed, and `git status` is clean.
- Say what changed for the owner, what was tested and where, and what
  waits for them, if anything (a release to finish on Windows, something
  only their account can show).
