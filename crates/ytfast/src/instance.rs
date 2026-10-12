//! One YTFast at a time (fastframe-instance, as Spotifast's): a second
//! launch (the Start menu, a shortcut, the program opened again) hands its
//! request to the YTFast running, which shows its window, and ends. Running
//! on with its window closed, YTFast would otherwise open a second copy,
//! playing over the first. The demo has a slot of its own, so it runs beside
//! the real app.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The slot's name: YTFast's bundle id, as on a Mac.
const APP_ID: &str = "io.github.likheet.ytfast";

/// What a launch asks the YTFast running.
pub mod request {
    /// Show the window (an ordinary launch).
    pub const SHOW: &str = "show";
    /// Nothing: a launch at sign-in finds YTFast already running.
    pub const START: &str = "start";
    /// Quit (`--quit`, for scripts such as `tools/look/demo.ps1`).
    pub const QUIT: &str = "quit";
}

/// What other launches asked for, until the app takes it in.
#[derive(Default)]
pub struct Asked {
    show: AtomicBool,
    quit: AtomicBool,
}

impl Asked {
    /// A launch asked for the window since the last look.
    pub fn take_show(&self) -> bool {
        self.show.swap(false, Ordering::Relaxed)
    }

    /// A launch asked YTFast to quit since the last look.
    pub fn take_quit(&self) -> bool {
        self.quit.swap(false, Ordering::Relaxed)
    }

    /// Takes in one request; the answer goes back to the launch.
    pub fn handle(&self, request: &str, wake: &dyn Fn()) -> Option<String> {
        match request {
            request::SHOW => self.show.store(true, Ordering::Relaxed),
            request::QUIT => self.quit.store(true, Ordering::Relaxed),
            request::START => {}
            _ => return None,
        }
        wake();
        Some("ok".to_string())
    }
}

/// The real app's slot, or the demo's beside it.
fn slot(demo: bool) -> fastframe_instance::Slot {
    let slot = fastframe_instance::Slot::new(APP_ID);
    if demo { slot.scoped("demo") } else { slot }
}

/// Becomes the YTFast running (keep the guard while it runs), or hands
/// `request` to the one running and returns `None`: this launch then ends.
pub fn claim(
    demo: bool,
    request: &str,
    asked: &Arc<Asked>,
    waker: &fastframe_shell::Waker,
) -> Option<fastframe_instance::Guard> {
    let (asked, waker) = (Arc::clone(asked), waker.clone());
    match slot(demo).claim(request, move |request| {
        asked.handle(request, &|| waker.wake())
    }) {
        fastframe_instance::Claim::First(guard) => Some(guard),
        // The YTFast running took it, or did not answer (it may be
        // starting): either way this launch is not wanted.
        fastframe_instance::Claim::Running(_)
        | fastframe_instance::Claim::Declined
        | fastframe_instance::Claim::Unanswered => None,
    }
}

/// Asks the YTFast running (the demo, with `demo`) to quit; nothing when
/// none runs.
pub fn quit_running(demo: bool) {
    let _ = slot(demo).send(request::QUIT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_kept_until_taken() {
        let asked = Asked::default();
        let woken = std::cell::Cell::new(0);
        let wake = || woken.set(woken.get() + 1);
        assert_eq!(asked.handle(request::SHOW, &wake).as_deref(), Some("ok"));
        assert_eq!(asked.handle(request::START, &wake).as_deref(), Some("ok"));
        assert_eq!(asked.handle("open-link x", &wake), None, "declined");
        assert_eq!(woken.get(), 2);
        assert!(asked.take_show());
        assert!(!asked.take_show(), "taken once");
        assert!(!asked.take_quit());
        asked.handle(request::QUIT, &wake);
        assert!(asked.take_quit());
    }
}
