//! YtFast's engine: everything that talks to YouTube Music or plays sound,
//! with no user interface. `ytfast-check` (step 0) and the YtFast app both
//! use it.
//!
//! - [`cookies`]: the YouTube sign-in, read from a browser's cookies.
//! - [`auth`] and [`ytcfg`]: what makes requests look like the web client.
//! - [`innertube`]: requests to YouTube Music's internal API.
//! - [`read`]: reading YouTube's replies (the only place that does), and
//!   the page model the app draws.
//! - [`playreport`]: telling YouTube what was played (History, mixes).
//! - [`helpers`] and [`ytdlp`]: yt-dlp, which finds each song's audio.
//! - [`audio`]: downloading and playing that audio.
//! - [`prepare`]: getting a song ready to play (find, details, download).
//! - [`net`]: the HTTP clients.
//! - [`redact`]: keeping secrets out of messages.

pub mod audio;
pub mod auth;
pub mod cookies;
pub mod direct;
pub mod helpers;
pub mod innertube;
pub mod net;
pub mod playreport;
pub mod prepare;
pub mod read;
pub mod redact;
pub mod solver;
pub mod stream;
pub mod ytcfg;
pub mod ytdlp;
