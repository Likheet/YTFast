//! "Start with the computer" (Settings): YTFast opens when the user signs
//! in, without its window (`--hidden`), its icon by the clock. On Windows
//! an entry in the user's own Run list in the registry, as other apps add
//! (Task Manager's Startup apps lists it); on a Mac a login item of the
//! user's own, a LaunchAgent (System Settings lists it under Login Items).
//! Each start puts this copy of YTFast there while the setting is on, so
//! the entry follows a copy moved or installed anew.

use std::path::Path;

/// The flag a start at sign-in carries: no window, only the icon.
pub const HIDDEN: &str = "--hidden";

/// Starts YTFast with the computer (`on`), or not, changing only what
/// differs. Not for development builds, whose program is in `target`.
pub fn set(on: bool) {
    if cfg!(debug_assertions) {
        log::info!("start with the computer ({on}): not for development builds");
        return;
    }
    let Ok(program) = std::env::current_exe() else {
        return;
    };
    #[cfg(windows)]
    if let Err(e) = windows::set(windows::RUN, windows::NAME, on.then(|| command(&program))) {
        log::warn!("start with the computer: {e}");
    }
    #[cfg(target_os = "macos")]
    mac::set(on, &program);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (on, program);
}

/// What the Run list starts: the program, quoted, and `--hidden`.
#[cfg_attr(not(windows), allow(dead_code))]
fn command(program: &Path) -> String {
    format!("\"{}\" {HIDDEN}", program.display())
}

#[cfg(windows)]
mod windows {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

    /// The user's own Run list (no administrator needed).
    pub const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// YTFast's entry in it. The installer takes it away when YTFast is
    /// uninstalled (`packaging/windows/ytfast.iss`).
    pub const NAME: &str = "YTFast";

    /// Sets the entry `name` under `key` to `command`, or takes it away.
    pub fn set(key: &str, name: &str, command: Option<String>) -> std::io::Result<()> {
        let (run, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey_with_flags(key, KEY_READ | KEY_WRITE)?;
        let now: Option<String> = run.get_value(name).ok();
        match command {
            Some(wanted) if now.as_deref() != Some(wanted.as_str()) => run.set_value(name, &wanted),
            None if now.is_some() => run.delete_value(name),
            _ => Ok(()),
        }
    }

    /// The registry, written and read back: only where asked
    /// (`YTFAST_TEST_REGISTRY`, set in CI), under a key of the test's own,
    /// never the Run list.
    #[test]
    fn the_entry_is_written_and_taken_away() {
        if std::env::var_os("YTFAST_TEST_REGISTRY").is_none() {
            return;
        }
        let key = r"Software\YTFast-test";
        let read = || -> Option<String> {
            RegKey::predef(HKEY_CURRENT_USER)
                .open_subkey(key)
                .ok()?
                .get_value("YTFast")
                .ok()
        };
        set(key, "YTFast", Some("\"C:\\YTFast.exe\" --hidden".into())).unwrap();
        assert_eq!(read().as_deref(), Some("\"C:\\YTFast.exe\" --hidden"));
        set(key, "YTFast", None).unwrap();
        assert_eq!(read(), None);
        set(key, "YTFast", None).unwrap();
        RegKey::predef(HKEY_CURRENT_USER)
            .delete_subkey(key)
            .unwrap();
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::path::{Path, PathBuf};

    /// The login item's name: YTFast's bundle id.
    const LABEL: &str = "io.github.likheet.ytfast";

    pub fn set(on: bool, program: &Path) {
        let Some(file) = directories::BaseDirs::new().map(|dirs| {
            dirs.home_dir()
                .join("Library/LaunchAgents")
                .join(format!("{LABEL}.plist"))
        }) else {
            return;
        };
        let result = if on {
            let Some(app) = bundle(program) else {
                log::warn!(
                    "start with the computer: YTFast runs from a disk image; move it to Applications first"
                );
                return;
            };
            let wanted = super::launch_agent(LABEL, &app);
            if std::fs::read_to_string(&file).ok().as_deref() == Some(wanted.as_str()) {
                return;
            }
            fastframe_file::write(&file, wanted)
        } else if file.exists() {
            std::fs::remove_file(&file)
        } else {
            return;
        };
        if let Err(e) = result {
            log::warn!("start with the computer: {e}");
        }
    }

    /// YTFast.app, from the program inside it, unless it runs from a disk
    /// image (or the Mac's copy of one): such a place is gone after a
    /// restart.
    fn bundle(program: &Path) -> Option<PathBuf> {
        let app = program.parent()?.parent()?.parent()?;
        let text = app.to_string_lossy();
        let passing = text.starts_with("/Volumes/") || text.contains("/AppTranslocation/");
        (app.extension()? == "app" && !passing).then(|| app.to_path_buf())
    }
}

/// A Mac's login item: `open` starts the app in the background (`-g`) at
/// sign-in, with `--hidden`, as any app is opened.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn launch_agent(label: &str, app: &Path) -> String {
    let escape = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let app = escape(&app.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{label}</string>
	<key>ProgramArguments</key>
	<array>
		<string>/usr/bin/open</string>
		<string>-g</string>
		<string>-a</string>
		<string>{app}</string>
		<string>--args</string>
		<string>{HIDDEN}</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
</dict>
</plist>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_run_list_starts_this_program_hidden() {
        assert_eq!(
            command(Path::new(
                r"C:\Users\A B\AppData\Local\Programs\YTFast\YTFast.exe"
            )),
            r#""C:\Users\A B\AppData\Local\Programs\YTFast\YTFast.exe" --hidden"#
        );
    }

    #[test]
    fn the_login_item_opens_the_app_in_the_background() {
        let plist = launch_agent(
            "io.github.likheet.ytfast",
            Path::new("/Applications/R&B.app"),
        );
        assert!(plist.contains("<string>/Applications/R&amp;B.app</string>"));
        assert!(plist.contains("<string>-g</string>"));
        assert!(plist.contains("<string>--hidden</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"));
    }
}
