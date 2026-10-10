//! Updates: YTFast looks at its GitHub releases a minute after it starts
//! and once a day, downloads a newer version in the background (with
//! "Install updates automatically" on), checks it (the release's checksums,
//! signed with the publisher key this build carries, and the program's own
//! `--version`), and installs it when the listener restarts. The work is
//! fastframe-update's, as in Spotifast: a helper swaps the program while
//! YTFast is closed, starts the new one, and puts the old one back if the
//! new one does not start. Releases are made with `ytfast-release`
//! (docs/releasing.md).
//!
//! On Windows any copy can update itself: the `ytfast-portable.txt` marker
//! beside the program says so, and YTFast writes it there. On a Mac the
//! builds are not signed by Apple, which installing them needs: a new
//! version is shown, and installed by hand.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use fastframe_update::{MacConfig, MacTarget, Prepared, Release, UpdateConfig, Updater};

/// How YTFast's releases are found and checked.
pub const CONFIG: UpdateConfig = UpdateConfig {
    publisher_key: Some(include_str!("../assets/update-public-key.hex")),
    macos: MacConfig {
        bundle_ids: &["io.github.likheet.ytfast"],
        executable_names: &[],
        legacy_bundle_names: &[],
    },
    mac_target: MacTarget::Arm64Only,
    ..UpdateConfig::new(
        "Likheet/YTFast",
        "YTFast",
        "ytfast",
        env!("CARGO_PKG_VERSION"),
    )
};

/// The newest release's page, where a Mac copy gets a new version.
pub const DOWNLOAD_PAGE: &str = "https://github.com/Likheet/YTFast/releases/latest";

/// The first look after starting: not while the window opens.
const FIRST_LOOK: Duration = Duration::from_secs(60);

/// What the updater is doing, for Settings and the banner.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum State {
    /// Not looked yet.
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// A newer version, not downloaded: automatic installs are off, or
    /// this copy cannot install it (`note` says why).
    Available {
        version: String,
        note: Option<String>,
    },
    Downloading {
        version: String,
        /// Bytes so far, and in all (0 while unknown).
        received: u64,
        total: u64,
    },
    /// Downloaded and checked: a restart installs it.
    Ready {
        version: String,
    },
    /// The helper is waiting for YTFast to close.
    Restarting,
    Failed(String),
}

enum Command {
    Check,
    Install,
    Restart,
}

/// The updater's thread, and what it is doing.
pub struct Updates {
    commands: Sender<Command>,
    state: Arc<Mutex<State>>,
    automatic: Arc<AtomicBool>,
}

impl Updates {
    /// Starts looking for updates (`automatic`: downloading them too);
    /// `wake` draws the window again when something changed.
    pub fn start(automatic: bool, wake: impl Fn() + Send + 'static) -> Self {
        mark_portable();
        let (commands, receiver) = mpsc::channel();
        let state = Arc::new(Mutex::new(State::Idle));
        let automatic = Arc::new(AtomicBool::new(automatic));
        let (shared, auto) = (Arc::clone(&state), Arc::clone(&automatic));
        let started = std::thread::Builder::new()
            .name("ytfast-updates".into())
            .spawn(move || run(&receiver, &shared, &auto, &wake));
        if let Err(e) = started {
            log::warn!("updates: could not start: {e}");
        }
        Self {
            commands,
            state,
            automatic,
        }
    }

    /// An updater that only shows `state`, for the demo's pictures of the
    /// update badge and window (`YTFAST_DEMO_UPDATE`): nothing is looked
    /// for or downloaded.
    pub fn sample(state: State) -> Self {
        let (commands, _) = mpsc::channel();
        Self {
            commands,
            state: Arc::new(Mutex::new(state)),
            automatic: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn state(&self) -> State {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Looks now (Settings' "Check for updates").
    pub fn check(&self) {
        let _ = self.commands.send(Command::Check);
    }

    /// Downloads the version found, when it was only shown.
    pub fn install(&self) {
        let _ = self.commands.send(Command::Install);
    }

    /// Hands the downloaded version to the helper; the window then closes
    /// (`State::Restarting`), and the new version opens.
    pub fn restart(&self) {
        let _ = self.commands.send(Command::Restart);
    }

    pub fn set_automatic(&self, on: bool) {
        self.automatic.store(on, Ordering::Relaxed);
    }
}

fn run(
    commands: &Receiver<Command>,
    state: &Mutex<State>,
    automatic: &AtomicBool,
    wake: &(impl Fn() + Send),
) {
    let set = |next: State| {
        let mut now = state.lock().unwrap_or_else(PoisonError::into_inner);
        if *now != next {
            *now = next;
            wake();
        }
    };
    let client = reqwest::blocking::Client::builder().user_agent(format!(
        "YTFast/{} (+https://github.com/Likheet/YTFast)",
        env!("CARGO_PKG_VERSION")
    ));
    let updater = match fastframe_update::ReqwestTransport::new(client) {
        Ok(transport) => Updater::new(CONFIG, transport),
        Err(e) => {
            log::warn!("updates: {e}");
            set(State::Failed(
                "Updates cannot be checked on this computer.".into(),
            ));
            return;
        }
    };
    let mut found: Option<Release> = None;
    let mut prepared: Option<Prepared> = None;
    let mut next_look = Instant::now() + FIRST_LOOK;
    loop {
        let command =
            match commands.recv_timeout(next_look.saturating_duration_since(Instant::now())) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            };
        match command {
            None | Some(Command::Check) => {
                next_look = Instant::now() + fastframe_update::CHECK_INTERVAL;
                // A version waiting for a restart is not looked for again.
                if prepared.is_some() {
                    continue;
                }
                set(State::Checking);
                match updater.check() {
                    Ok(None) => set(State::UpToDate),
                    Ok(Some(release)) => {
                        log::warn!("updates: YTFast {} is out", release.version);
                        let version = release.version.clone();
                        found = Some(release);
                        match installable(&updater) {
                            Err(note) => set(State::Available {
                                version,
                                note: Some(note),
                            }),
                            Ok(()) if !automatic.load(Ordering::Relaxed) => {
                                set(State::Available {
                                    version,
                                    note: None,
                                });
                            }
                            Ok(()) => {
                                prepared = download(&updater, found.as_ref(), &set);
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("updates: could not look: {e:#}");
                        set(State::Failed(
                            "Could not look for a new version. YTFast tries again tomorrow.".into(),
                        ));
                    }
                }
            }
            // Download now (also "Retry download"): after looking again
            // when the last look failed.
            Some(Command::Install) => {
                if prepared.is_none() {
                    if found.is_none() {
                        set(State::Checking);
                        found = updater.check().ok().flatten();
                    }
                    match (found.as_ref(), installable(&updater)) {
                        (None, _) => set(State::Failed(
                            "Could not look for a new version. Try again later.".into(),
                        )),
                        (Some(release), Err(note)) => set(State::Available {
                            version: release.version.clone(),
                            note: Some(note),
                        }),
                        (Some(_), Ok(())) => prepared = download(&updater, found.as_ref(), &set),
                    }
                }
            }
            Some(Command::Restart) => {
                if let Some(ready) = prepared.take() {
                    match updater.handoff(ready, Vec::new()) {
                        Ok(()) => set(State::Restarting),
                        Err(e) => {
                            log::warn!("updates: could not hand over: {e:#}");
                            set(State::Failed(
                                "The new version could not be installed. Try again later.".into(),
                            ));
                        }
                    }
                }
            }
        }
    }
}

/// Whether this copy can install updates itself; the words for why not.
fn installable(updater: &Updater) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        return Err(
            "On a Mac, download it from its page (YTFast.dmg) and drag YTFast into Applications, as the first time."
                .into(),
        );
    }
    updater.installation().map(|_| ()).map_err(|reason| {
        log::warn!("updates: this copy cannot install them: {reason}");
        format!("This copy cannot install it itself: {reason}")
    })
}

/// Downloads and checks `release`; ready to install, or why not.
fn download(
    updater: &Updater,
    release: Option<&Release>,
    set: &impl Fn(State),
) -> Option<Prepared> {
    let release = release?;
    let version = release.version.clone();
    set(State::Downloading {
        version: version.clone(),
        received: 0,
        total: 0,
    });
    let mut last = None;
    let done = updater.download(release, |received, total| {
        let percent = received.saturating_mul(100).checked_div(total).unwrap_or(0);
        // A step of 2% at a time (and the end), not at every chunk.
        if last.is_none_or(|before| percent >= before + 2 || received == total) {
            last = Some(percent);
            set(State::Downloading {
                version: version.clone(),
                received,
                total,
            });
        }
    });
    match done {
        Ok(prepared) => {
            log::warn!("updates: YTFast {version} is ready to install");
            set(State::Ready { version });
            Some(prepared)
        }
        Err(e) => {
            log::warn!("updates: the download failed: {e:#}");
            set(State::Failed(format!(
                "YTFast {version} could not be downloaded. YTFast tries again tomorrow."
            )));
            None
        }
    }
}

/// On Windows, says beside the program that this copy may update itself
/// (fastframe-update's `ytfast-portable.txt`): YTFast has no installer, so
/// a copy anywhere is a portable one. Not for programs under Program Files
/// or the Store's folder, nor for development builds.
fn mark_portable() {
    if !cfg!(windows) || cfg!(debug_assertions) {
        return;
    }
    let Some(folder) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    else {
        return;
    };
    let lower = folder.to_string_lossy().to_lowercase();
    if lower.contains("\\program files") || lower.contains("\\windowsapps") {
        return;
    }
    let marker = folder.join("ytfast-portable.txt");
    if !marker.exists()
        && let Err(e) = std::fs::write(&marker, "ytfast-portable-v1\n")
    {
        log::warn!("updates: could not write {}: {e}", marker.display());
    }
}

/// `--update-dry-run`: looks for the newest release as if this were the
/// oldest version, downloads and checks it as an update would (signature,
/// checksum, its `--version`), says what happened, and installs nothing.
/// For checking a release once it is published.
pub fn dry_run() -> i32 {
    let config = UpdateConfig {
        current_version: "0.0.1",
        ..CONFIG
    };
    let client = reqwest::blocking::Client::builder();
    let updater = match fastframe_update::ReqwestTransport::new(client) {
        Ok(transport) => Updater::new(config, transport),
        Err(e) => {
            println!("no client: {e}");
            return 1;
        }
    };
    let release = match updater.check() {
        Ok(Some(release)) => release,
        Ok(None) => {
            println!("no release found");
            return 1;
        }
        Err(e) => {
            println!("could not look: {e:#}");
            return 1;
        }
    };
    println!("found YTFast {} ({})", release.version, release.url);
    if let Err(reason) = updater.installation() {
        println!("this copy cannot install it: {reason}");
        return 1;
    }
    match updater.download(&release, |_, _| {}) {
        Ok(prepared) => {
            println!(
                "downloaded and checked YTFast {}: signature, checksum and --version all good",
                prepared.version()
            );
            prepared.discard();
            0
        }
        Err(e) => {
            println!("the download failed its checks: {e:#}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_update_settings_are_valid() {
        CONFIG.validate().unwrap();
        assert_eq!(CONFIG.current_version, env!("CARGO_PKG_VERSION"));
    }
}
