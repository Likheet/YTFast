//! `--save-replies FOLDER`: YouTube Music's own replies, saved as files
//! for the readers' tests (`crates/ytfast-core/tests/fixtures`), with
//! everything personal taken out first (see `scrub.rs`).
//!
//! Several readers were tested only on hand-written replies (Up next, the
//! `player` reply, search, Home, History, the account menu) or on none
//! (the library). This saves real ones: what the check reads anyway, and
//! the other pages the app reads, each asked for once, one at a time.
//! Lyrics are not saved: they are the songwriters' words.

use std::path::Path;

use serde_json::{Value, json};
use tokio::runtime::Runtime;
use ytfast_core::innertube::{ApiError, Session};
use ytfast_core::library::LibraryTab;
use ytfast_core::read::{self, Continuation};

use crate::report::{Outcome, Report};
use crate::scrub::{self, Personal};
use crate::ui;

/// What is searched for: a well-known song, so the search says nothing
/// about the account.
const SEARCH: &str = "daft punk get lucky";

/// Fetches the replies, takes out what is personal, and writes them to
/// `folder`.
pub fn save(rt: &Runtime, session: &Session, folder: &Path, report: &mut Report) {
    ui::extra_heading("Saving YouTube Music's replies (--save-replies)");
    if let Err(e) = std::fs::create_dir_all(folder) {
        ui::result(
            Outcome::Fail,
            &format!("Could not make the folder {}: {e}", folder.display()),
        );
        return;
    }
    // The account's menu first: what to take out of every reply is read
    // from it.
    let menu = match rt.block_on(session.call("account/account_menu", json!({}))) {
        Ok(menu) => menu,
        Err(e) => {
            ui::result(
                Outcome::Fail,
                &format!("Nothing was saved: your account could not be read ({e})."),
            );
            return;
        }
    };
    let Some(personal) = Personal::from_account_menu(&menu, secrets(session)) else {
        ui::result(
            Outcome::Fail,
            "Nothing was saved: YouTube's reply did not name your account, so your name could not be taken out of the others.",
        );
        return;
    };
    ui::say("Asking YouTube Music for each page once, one at a time...");
    let mut files = Files {
        folder,
        personal,
        saved: 0,
        tried: 0,
    };
    files.keep("account_menu", Ok(menu));

    let liked = files.keep("liked_songs", rt.block_on(session.browse("VLLM")));
    let tracks = liked.as_ref().map(read::tracks).unwrap_or_default();
    match liked.as_ref().and_then(read::track_continuation) {
        Some(Continuation::Body(token)) => {
            let more = session.call("browse", json!({ "continuation": token }));
            files.keep("liked_songs_more", rt.block_on(more));
        }
        // The older form goes in the address, which only the engine can
        // send; the first page shows it already.
        Some(Continuation::Address(_)) => {
            log::info!("Liked songs continue the older way; the next part is not saved")
        }
        None => {}
    }
    files.keep("history", rt.block_on(session.browse("FEmusic_history")));
    files.keep("home", rt.block_on(session.browse("FEmusic_home")));
    files.keep(
        "search",
        rt.block_on(session.call("search", json!({ "query": SEARCH }))),
    );
    for (name, tab) in [
        ("library_playlists", LibraryTab::Playlists),
        ("library_songs", LibraryTab::Songs),
        ("library_albums", LibraryTab::Albums),
        ("library_artists", LibraryTab::Artists),
        ("library_subscriptions", LibraryTab::Subscriptions),
    ] {
        files.keep(name, rt.block_on(session.browse(tab.browse_id())));
    }

    if let Some(song) = tracks.first() {
        // The `player` reply as the fast way gets it: with the player
        // code's signature timestamp.
        let sts = session.player_js_url().and_then(|url| {
            let code = rt.block_on(session.fetch_text(&url)).ok()?;
            ytfast_core::direct::signature_timestamp(&code)
        });
        files.keep(
            "player",
            rt.block_on(session.player_reply(&song.video_id, sts)),
        );
        files.keep(
            "next_radio",
            rt.block_on(session.call("next", radio_body(&song.video_id))),
        );
    }
    if let Some(album_id) = tracks.iter().find_map(|t| t.album_id.clone()) {
        let album = files.keep("album", rt.block_on(session.browse(&album_id)));
        let playlist = album
            .as_ref()
            .and_then(|reply| read::page(reply).header)
            .and_then(|header| header.library_id);
        if let Some(playlist) = playlist {
            files.keep(
                "next_album",
                rt.block_on(session.call("next", queue_body(&playlist))),
            );
        }
    }

    let Files { saved, tried, .. } = files;
    report.fact(
        "Replies saved (--save-replies)",
        format!("{saved} of {tried}"),
    );
    if saved == 0 {
        ui::result(Outcome::Fail, "No reply could be saved.");
        return;
    }
    ui::result(
        Outcome::Ok,
        &format!(
            "Saved {saved} of {tried} replies in {}.\n\
             Your name, handle, email, photo and sign-in were taken out of them.\n\
             They still show your Liked songs, History, playlists and library.",
            folder.display()
        ),
    );
}

/// The replies written so far.
struct Files<'a> {
    folder: &'a Path,
    personal: Personal,
    saved: usize,
    tried: usize,
}

impl Files<'_> {
    /// Writes `reply` as `name.json`, without what is personal, and hands
    /// the reply back for the next requests.
    fn keep(&mut self, name: &str, reply: Result<Value, ApiError>) -> Option<Value> {
        self.tried += 1;
        let reply = match reply {
            Ok(reply) => reply,
            Err(e) => {
                ui::result(Outcome::Fail, &format!("{name}: not saved ({e})"));
                return None;
            }
        };
        let mut clean = reply.clone();
        scrub::reply(&mut clean, &self.personal);
        let text = match serde_json::to_string_pretty(&clean) {
            Ok(text) => text + "\n",
            Err(e) => {
                ui::result(Outcome::Fail, &format!("{name}: not saved ({e})"));
                return Some(reply);
            }
        };
        if self.personal.holds_secret(&text) {
            ui::result(
                Outcome::Fail,
                &format!("{name}: not saved, because part of your sign-in was still in it."),
            );
            return Some(reply);
        }
        let file = format!("{name}.json");
        match crate::write_private(&self.folder.join(&file), &text) {
            Ok(()) => {
                self.saved += 1;
                ui::say(&format!("Saved {file}"));
            }
            Err(e) => ui::result(Outcome::Fail, &format!("{file}: not saved ({e})")),
        }
        Some(reply)
    }
}

/// What must never be written: the sign-in's cookie values and the
/// session's own IDs.
fn secrets(session: &Session) -> Vec<String> {
    let mut secrets: Vec<String> = session
        .cookies()
        .to_netscape()
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            (fields.len() == 7).then(|| fields[6].to_string())
        })
        .collect();
    let config = session.config();
    secrets.extend(
        [
            &config.visitor_data,
            &config.delegated_session_id,
            &config.user_session_id,
        ]
        .into_iter()
        .flatten()
        .cloned(),
    );
    secrets
}

/// A `next` request for a song's radio, as `innertube::next_body` builds
/// it for the app's Up next (ytmusicapi's `get_watch_playlist`).
fn radio_body(video_id: &str) -> Value {
    json!({
        "enablePersistentPlaylistPanel": true,
        "isAudioOnly": true,
        "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
        "videoId": video_id,
        "playlistId": format!("RDAMVM{video_id}"),
        "watchEndpointMusicSupportedConfigs": {
            "watchEndpointMusicConfig": {
                "hasPersistentPlaylistPanel": true,
                "musicVideoType": "MUSIC_VIDEO_TYPE_ATV"
            }
        }
    })
}

/// A `next` request for an album's or playlist's songs, as
/// `Session::playlist_queue` asks.
fn queue_body(playlist_id: &str) -> Value {
    json!({
        "enablePersistentPlaylistPanel": true,
        "isAudioOnly": true,
        "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
        "playlistId": playlist_id,
    })
}
