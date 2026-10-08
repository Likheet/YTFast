//! YtFast's engine: everything that talks to YouTube Music or plays sound,
//! with no user interface. `ytfast-check` (step 0) and the YtFast app both
//! use it.
//!
//! - [`cookies`]: the YouTube sign-in, read from a browser's cookies.
//! - [`auth`] and [`ytcfg`]: what makes requests look like the web client.
//! - [`innertube`]: requests to YouTube Music's internal API.
//! - [`library`]: more of the account, and changing it: ratings, the
//!   library, subscriptions, playlists, search suggestions, lyrics.
//! - [`read`]: reading YouTube's replies (the only place that does), and
//!   the page model the app draws.
//! - [`lyrics`]: lyrics, and LRCLIB for songs YouTube has none for.
//! - [`playreport`]: telling YouTube what was played (History, mixes).
//! - [`direct`] and [`solver`]: finding a song's audio the website's way
//!   (the fast way), with yt-dlp's challenge solver kept running in Deno.
//! - [`helpers`] and [`ytdlp`]: yt-dlp and Deno, downloaded on first use.
//!   yt-dlp reads the browser's sign-in, and finds a song's audio when
//!   the fast way cannot.
//! - [`prepare`]: getting a song ready to play: finding its audio (the
//!   fast way, else yt-dlp), then starting its download.
//! - [`stream`]: downloading a song while it plays.
//! - [`audio`]: decoding and playing it.
//! - [`net`]: the HTTP clients.
//! - [`redact`]: keeping secrets out of messages.

pub mod audio;
pub mod auth;
pub mod cookies;
pub mod direct;
pub mod helpers;
pub mod innertube;
pub mod library;
pub mod lyrics;
pub mod net;
pub mod playreport;
pub mod prepare;
pub mod read;
pub mod redact;
pub mod solver;
pub mod stream;
pub mod ytcfg;
pub mod ytdlp;
