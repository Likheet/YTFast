//! YtFast's engine: everything that talks to YouTube Music or plays sound,
//! with no user interface. `ytfast-check` (step 0) uses it today; the YtFast
//! app will use the same code.
//!
//! - [`cookies`]: the YouTube sign-in, read from a browser's cookies.
//! - [`auth`] and [`ytcfg`]: what makes requests look like the web client.
//! - [`innertube`]: requests to YouTube Music's internal API.
//! - [`read`]: reading YouTube's replies (the only place that does).
//! - [`playreport`]: telling YouTube what was played (History, mixes).
//! - [`helpers`] and [`ytdlp`]: yt-dlp, which finds each song's audio.
//! - [`audio`]: downloading and playing that audio.
//! - [`net`]: the HTTP clients.
//! - [`redact`]: keeping secrets out of messages.

pub mod audio;
pub mod auth;
pub mod cookies;
pub mod helpers;
pub mod innertube;
pub mod net;
pub mod playreport;
pub mod read;
pub mod redact;
pub mod ytcfg;
pub mod ytdlp;
