//! The app's state, and what every click does. Views (in `views/`) draw
//! from a shared borrow of [`App`] and push [`Action`]s; the actions are
//! applied after drawing, so nothing changes under a view mid-frame.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use fastframe_now_playing as now_playing;
use serde::{Deserialize, Serialize};
use ytfast_core::playreport::PlayReport;
use ytfast_core::read::{Item, Page, PlayerInfo, Section, Target, Track, TrackKind};
use ytfast_core::ytdlp::Browser;

use crate::audio_thread::{Audio, Command, Status};
use crate::backend::{Backend, Edit, Event, Request, Route};
use crate::images::Images;
use crate::queue::{Entry, Queue};
use crate::views;

/// What the app remembers between runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The browser whose YouTube sign-in YtFast uses (its name).
    pub browser: Option<String>,
    pub volume: f32,
    pub repeat: Repeat,
    /// Start songs the website's way (off: always through yt-dlp).
    pub fast_way: bool,
    /// When the queue ends, carry on with songs like the last one.
    pub autoplay: bool,
    /// Turn loud songs down, as YouTube Music does.
    pub even_loudness: bool,
    /// The menu on the left is closed to its icons.
    pub mini_guide: bool,
    /// Shuffle is on: every queue plays its songs to come in a random
    /// order, as YouTube Music's shuffle.
    pub shuffle: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            browser: None,
            volume: 0.8,
            repeat: Repeat::Off,
            fast_way: true,
            autoplay: true,
            even_loudness: true,
            mini_guide: false,
            shuffle: false,
        }
    }
}

pub enum Auth {
    Choosing { browser: Browser },
    Working { browser: Browser, progress: String },
    SignedIn { name: String },
    Failed { browser: Browser, message: String },
}

// A page's header is a few hundred bytes, and at most 24 pages are kept:
// boxing it would save nothing worth the indirection.
#[allow(clippy::large_enum_variant)]
pub enum Loadable {
    Loading,
    Ready(Page),
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayState {
    Idle,
    /// Finding and downloading the song.
    Preparing,
    Playing,
    /// The queue ran out; waiting for more songs (radio).
    WaitingForMore,
    Failed(String),
}

/// The song being prepared or played.
pub struct Playback {
    pub entry: Option<Entry>,
    pub state: PlayState,
    pub format: String,
    pub report: Option<PlayReport>,
    /// What YouTube said about the song, for reporting a repeat.
    pub info: Option<PlayerInfo>,
    /// The video an Up next request was last sent for (asked once).
    pub asked_more_for: Option<String>,
    /// The stretches of the song listened to, from and to (in seconds),
    /// before the last jump; and where the one now playing began. They are
    /// what the listening report says (YouTube's player does the same).
    pub listened: Vec<(f64, f64)>,
    pub listening_from: f64,
}

/// What happens at the end of a song.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Repeat {
    /// The next song; more like it when the queue runs out.
    #[default]
    Off,
    /// The whole queue, again and again.
    All,
    /// This song, again and again.
    One,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            entry: None,
            state: PlayState::Idle,
            format: String::new(),
            report: None,
            info: None,
            asked_more_for: None,
            listened: Vec::new(),
            listening_from: 0.0,
        }
    }
}

/// What to do with a playlist's songs once they arrive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueMode {
    Play,
    Shuffle,
    Next,
    End,
}

/// A switch on the Settings page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    FastWay,
    Autoplay,
    EvenLoudness,
}

/// A small window asking one thing.
#[derive(Clone, Debug)]
pub enum Dialog {
    /// A new playlist's name; `song` is added to it when made.
    NewPlaylist {
        name: String,
        song: Option<String>,
    },
    Rename {
        playlist_id: String,
        name: String,
    },
    Delete {
        playlist_id: String,
        title: String,
    },
    /// "Save to playlist": the account's playlists, to add `video_id` to
    /// one, or a new playlist.
    SaveToPlaylist {
        video_id: String,
    },
    /// An album's or playlist's whole description.
    Description {
        title: String,
        text: String,
    },
}

/// The player page's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NpTab {
    UpNext,
    Lyrics,
    Related,
}

/// What the account thinks of a song.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LikeState {
    Liked,
    Disliked,
    Neutral,
}

/// Everything a view can ask for.
pub enum Action {
    Navigate(Route),
    Back,
    Forward,
    Search(String),
    Retry(Route),
    /// Play these songs, from `start`. `source` is the playlist they came
    /// from, for more songs when they run out.
    PlayTracks {
        tracks: Vec<Track>,
        start: usize,
        source: Option<String>,
    },
    /// Play these songs in a random order.
    Shuffle {
        tracks: Vec<Track>,
        source: Option<String>,
    },
    /// A card or header target: open a page, or play.
    Open(Target, Option<Track>),
    Play(Target, Option<Track>),
    TogglePause,
    Next,
    Previous,
    Seek(f64),
    SetVolume(f32),
    /// Silence, or back to the volume before it.
    ToggleMute,
    JumpTo(u64),
    /// Put a song right after the one playing now.
    PlayNext(Track),
    /// Put a song at the end of the queue.
    AddToQueue(Track),
    /// A short message above the player bar.
    Notify(String),
    /// Add a song to one of the account's playlists.
    AddToPlaylist {
        playlist_id: String,
        title: String,
        video_id: String,
    },
    /// Any other change to the account.
    Edit(Edit),
    /// Ask for search suggestions.
    Suggest(String),
    /// A playlist's or album's songs: play, shuffle, or queue them.
    QueuePlaylist(String, QueueMode),
    Toggle(Setting),
    OpenDialog(Dialog),
    RenamePlaylist {
        playlist_id: String,
        name: String,
    },
    DeletePlaylist(String),
    /// Take a row out of the playlist shown (by its own row ID).
    RemoveFromPlaylist {
        playlist_id: String,
        video_id: String,
        set_video_id: String,
    },
    /// Save an album or playlist to the library, or take it out.
    ToggleSave {
        playlist_id: String,
        save: bool,
    },
    ToggleSubscribe {
        channel_id: String,
        subscribe: bool,
    },
    OpenLogFolder,
    /// Open System Settings at Full Disk Access (on a Mac).
    OpenFullDiskAccess,
    /// Queue edits, by entry.
    RemoveFromQueue(u64),
    ShiftInQueue(u64, bool),
    MoveNextInQueue(u64),
    /// Open the menu on the left, or close it to its icons.
    ToggleGuide,
    /// Open or close the player page.
    ToggleNowPlaying,
    CloseNowPlaying,
    NowPlayingTab(NpTab),
    /// Like, dislike, or neither.
    Rate(String, LikeState),
    /// Shuffle on or off (a mode): on, the songs after the current one in
    /// a random order; off, back in the queue's own.
    ShuffleQueue,
    /// Repeat off, the queue, this song.
    CycleRepeat,
    ChooseBrowser(Browser),
    SignIn,
    SignOut,
}

pub struct App {
    pub backend: Backend,
    pub audio: Audio,
    pub images: RefCell<Images>,
    pub scrolling: fastframe_scroll::Scrolling,
    pub controls: Option<now_playing::NowPlaying>,
    pub demo: bool,
    pub settings: Settings,
    pub auth: Auth,
    pub route: Route,
    back: Vec<Route>,
    forward: Vec<Route>,
    pub pages: HashMap<Route, Loadable>,
    /// Pages in the order they were shown, the latest last.
    visited: Vec<Route>,
    pub queue: Queue,
    pub playback: Playback,
    /// Songs skipped in a row because they could not play (see
    /// [`App::song_failed`]).
    skipped_in_a_row: usize,
    /// The last playlist asked for to play: its songs play when they
    /// arrive, unless something else was chosen since.
    wanted_playlist: Option<u64>,
    /// Numbers the playlists asked for.
    tickets: u64,
    /// The audio thread's latest status, read once per frame.
    pub audio_status: Status,
    /// A short message shown above the player bar.
    pub notice: Option<(String, Instant)>,
    pub actions: RefCell<Vec<Action>>,
    /// Songs already asked for ahead of time.
    warmed: RefCell<std::collections::HashSet<String>>,
    /// The song row under the pointer (see [`App::resting_on`]).
    pointed: RefCell<Option<Pointed>>,
    /// The player page is open, and on which tab.
    pub now_playing: bool,
    pub np_tab: NpTab,
    /// Lyrics and related pages, by song.
    pub lyrics: HashMap<String, crate::lyrics::State>,
    pub related: HashMap<String, Loadable>,
    /// What the account thinks of songs, as known.
    pub likes: HashMap<String, LikeState>,
    /// Library saves and subscriptions changed in this run.
    pub saved: HashMap<String, bool>,
    pub subscribed: HashMap<String, bool>,
    pub dialog: RefCell<Option<Dialog>>,
    /// Search suggestions, for the text they were asked for.
    pub suggestions: (String, ytfast_core::read::Suggestions),
    /// The fonts for other scripts, added once some words need them.
    script_fonts: crate::theme::ScriptFonts,
    /// The volume before muting, which unmuting goes back to.
    loud_volume: f32,
    /// Changes to the account on their way to YouTube, by what they change
    /// (see [`App::edit_answered`]).
    edits_in_flight: HashMap<String, usize>,
    /// What a song's like was before the changes on their way, to go back
    /// to if YouTube refuses them.
    like_before: HashMap<String, Option<LikeState>>,
    /// The window's size has been looked at (see
    /// [`App::mend_window_size`]).
    window_checked: bool,
    /// The latest loading of each page: its answers carry that number
    /// ([`Request::Page`]), and an older loading's are dropped.
    page_loads: HashMap<Route, u64>,
    /// Numbers the page loadings.
    loads: u64,
    /// The text search suggestions were last asked for.
    suggesting: String,
    /// A jump asked for in the song playing (its entry, the second): the
    /// seek line shows it until the player gets there.
    pub seeking: Option<(u64, f64, Instant)>,
    /// The page shown was opened anew (not by Back or Forward): it starts
    /// at its top.
    pub fresh_page: std::cell::Cell<bool>,
    /// The page is scrolled down from its top (as the page saw it last
    /// frame): the top bar then turns solid, as YouTube Music's does.
    pub page_scrolled: std::cell::Cell<bool>,
    /// How far the page is scrolled (as the page saw it last frame): an
    /// album's background moves up with its songs.
    pub page_offset: std::cell::Cell<f32>,
    /// Where this frame's album background goes: kept under the bars and
    /// the menu (`backdrop::paint`).
    pub backdrop_slot: std::cell::Cell<Option<egui::layers::ShapeIdx>>,
    /// The cover behind an album's or playlist's page: its address, its
    /// middle band shrunk to a few pixels (drawn stretched, a blur), and
    /// when it was made (it fades in).
    pub page_cover: RefCell<Option<(String, egui::TextureHandle, f64)>>,
    /// "/" just opened the search box: its character, arriving in the next
    /// frame, is not typed there.
    drop_slash: bool,
}

/// The song row under the pointer, and since when.
struct Pointed {
    video_id: String,
    /// When it came under the pointer (egui's clock).
    since: f64,
    /// The frame it was last seen under the pointer in.
    frame: u64,
}

fn default_browser() -> Browser {
    if cfg!(windows) {
        Browser::Firefox
    } else {
        Browser::Chrome
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, demo: bool) -> Self {
        let script_fonts = crate::theme::install(&cc.egui_ctx);
        let settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, "ytfast"))
            .unwrap_or_default();

        let ctx = cc.egui_ctx.clone();
        let wake = move || ctx.request_repaint();
        let backend = Backend::start(wake.clone(), demo).expect("YtFast's folders can be made");
        let audio = Audio::start(wake.clone(), demo);

        let mut info = now_playing::App::new("ytfast", "YTFast");
        info.can_raise = true;
        let controls = Some(now_playing::NowPlaying::start(info, wake));
        Self::with(backend, audio, controls, script_fonts, settings, demo)
    }

    /// The app around a backend and a player already started (stand-ins,
    /// in the tests).
    fn with(
        backend: Backend,
        audio: Audio,
        controls: Option<now_playing::NowPlaying>,
        script_fonts: crate::theme::ScriptFonts,
        settings: Settings,
        demo: bool,
    ) -> Self {
        audio.send(Command::Volume(settings.volume));
        backend.send(Request::FastWay(settings.fast_way));

        let remembered = settings.browser.as_deref().and_then(Browser::parse);
        let auth = match (demo, remembered) {
            (true, _) => {
                backend.send(Request::SignIn(Browser::Firefox));
                Auth::Working {
                    browser: Browser::Firefox,
                    progress: "Opening the demo...".into(),
                }
            }
            (false, Some(browser)) if browser.problem_here().is_none() => {
                backend.send(Request::SignIn(browser));
                Auth::Working {
                    browser,
                    progress: "Signing in...".into(),
                }
            }
            _ => Auth::Choosing {
                browser: default_browser(),
            },
        };
        let loud_volume = if settings.volume > 0.001 {
            settings.volume
        } else {
            Settings::default().volume
        };

        Self {
            backend,
            audio,
            images: RefCell::new(Images::default()),
            scrolling: fastframe_scroll::Scrolling::default(),
            controls,
            demo,
            settings,
            auth,
            route: Route::Home,
            back: Vec::new(),
            forward: Vec::new(),
            pages: HashMap::new(),
            visited: Vec::new(),
            queue: Queue::default(),
            playback: Playback::default(),
            skipped_in_a_row: 0,
            wanted_playlist: None,
            tickets: 0,
            audio_status: Status::default(),
            notice: None,
            actions: RefCell::new(Vec::new()),
            warmed: RefCell::new(std::collections::HashSet::new()),
            pointed: RefCell::new(None),
            now_playing: false,
            np_tab: NpTab::UpNext,
            lyrics: HashMap::new(),
            related: HashMap::new(),
            likes: HashMap::new(),
            saved: HashMap::new(),
            subscribed: HashMap::new(),
            dialog: RefCell::new(None),
            suggestions: (String::new(), Default::default()),
            script_fonts,
            loud_volume,
            edits_in_flight: HashMap::new(),
            like_before: HashMap::new(),
            window_checked: false,
            page_loads: HashMap::new(),
            loads: 0,
            suggesting: String::new(),
            seeking: None,
            fresh_page: std::cell::Cell::new(false),
            page_scrolled: std::cell::Cell::new(false),
            page_offset: std::cell::Cell::new(0.0),
            backdrop_slot: std::cell::Cell::new(None),
            page_cover: RefCell::new(None),
            drop_slash: false,
        }
    }

    /// Queues an action for after this frame.
    pub fn act(&self, action: Action) {
        self.actions.borrow_mut().push(action);
    }

    /// A window closed while minimised opens again the size of a minimised
    /// one, in a corner (eframe keeps the size it had then, and the least
    /// size does not hold at opening): it goes back to its first size.
    /// Looked at once, when the window first has a size.
    fn mend_window_size(&mut self, ctx: &egui::Context) {
        if self.window_checked {
            return;
        }
        let Some(inner) = ctx.input(|i| i.viewport().inner_rect) else {
            return;
        };
        self.window_checked = true;
        let [least_width, least_height] = MIN_WINDOW_SIZE;
        if inner.width() < least_width || inner.height() < least_height {
            log::info!(
                "the window opened {:.0} by {:.0}; back to its first size",
                inner.width(),
                inner.height()
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(WINDOW_SIZE.into()));
        }
    }

    /// Does what the views and the media keys asked for.
    fn apply_actions(&mut self) {
        let actions = std::mem::take(self.actions.get_mut());
        for action in actions {
            self.apply(action);
        }
    }

    /// The account's playlists (ID without `VL`, title), from the
    /// Library, for "Add to playlist". The Library also holds playlists
    /// saved from other people, which YouTube does not let the account
    /// change: a playlist's line under its name begins with whose it is, so
    /// those naming the account are kept. If none does (a channel named
    /// unlike the account), all are.
    pub fn own_playlists(&self) -> Vec<(String, String)> {
        self.own_playlist_cards()
            .into_iter()
            .map(|(id, card)| (id, card.title))
            .collect()
    }

    /// The same playlists with their cards (cover, the line under the
    /// name), for "Save to playlist".
    pub fn own_playlist_cards(&self) -> Vec<(String, ytfast_core::read::Card)> {
        let Some(Loadable::Ready(page)) = self.pages.get(&Route::Library) else {
            return Vec::new();
        };
        let playlists: Vec<&ytfast_core::read::Card> = page
            .sections
            .iter()
            .flat_map(|s| &s.items)
            .filter_map(|item| match item {
                Item::Card(card) => match &card.open {
                    Some(Target::Browse { id, .. }) => {
                        let id = id.strip_prefix("VL")?;
                        // Liked Music and Episodes are not edited this way.
                        (id != "LM" && id != "SE").then_some(card)
                    }
                    _ => None,
                },
                Item::Track(_) => None,
            })
            .collect();
        let account = match &self.auth {
            Auth::SignedIn { name } if !name.is_empty() => Some(name.as_str()),
            _ => None,
        };
        let whose = |card: &&ytfast_core::read::Card| {
            account.is_some_and(|name| card.subtitle.split(" \u{2022} ").any(|part| part == name))
        };
        let own: Vec<&ytfast_core::read::Card> = playlists.iter().copied().filter(whose).collect();
        let chosen = if own.is_empty() { playlists } else { own };
        chosen
            .into_iter()
            .filter_map(|card| match &card.open {
                Some(Target::Browse { id, .. }) => {
                    Some((id.strip_prefix("VL")?.to_string(), card.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// The playlist shown, when the account can change it: its ID without
    /// `VL`.
    pub fn editable_playlist(&self) -> Option<String> {
        let Route::Browse { id, .. } = &self.route else {
            return None;
        };
        match self.pages.get(&self.route) {
            Some(Loadable::Ready(Page {
                header: Some(header),
                ..
            })) if header.editable => Some(id.trim_start_matches("VL").to_string()),
            _ => None,
        }
    }

    /// Loads a playlist again, as YouTube now has it.
    fn reload_playlist(&mut self, playlist_id: &str) {
        let route = Route::browse(format!("VL{playlist_id}"), None);
        self.pages.remove(&route);
        if self.route == route {
            self.show_current();
        }
    }

    /// Asks for what the player page shows of the playing song (lyrics,
    /// related), once per song, when the page is open.
    fn want_song_extras(&mut self) {
        if !self.now_playing {
            return;
        }
        let Some(entry) = &self.playback.entry else {
            return;
        };
        let track = &entry.track;
        match self.np_tab {
            NpTab::Lyrics if !self.lyrics.contains_key(&track.video_id) => {
                // The player's length is this song's only once it plays it
                // (until then it is the last song's).
                let playing_it = self.audio_status.entry == Some(entry.id);
                let duration = if playing_it && self.audio_status.length > 0.0 {
                    Some(self.audio_status.length)
                } else {
                    track.duration_seconds.map(f64::from)
                };
                // LRCLIB matches on the length: without one from YouTube,
                // wait for the song to start.
                let starting = matches!(
                    self.playback.state,
                    PlayState::Preparing | PlayState::Playing
                );
                if duration.is_none() && !playing_it && starting {
                    return;
                }
                if self.lyrics.len() > 30 {
                    self.lyrics.clear();
                }
                self.lyrics
                    .insert(track.video_id.clone(), crate::lyrics::State::Loading);
                self.backend.send(Request::Lyrics {
                    video_id: track.video_id.clone(),
                    title: track.title.clone(),
                    artist: track.artists.clone(),
                    album: track.album.clone(),
                    duration,
                });
            }
            NpTab::Related if !self.related.contains_key(&track.video_id) => {
                if self.related.len() > 10 {
                    self.related.clear();
                }
                self.related
                    .insert(track.video_id.clone(), Loadable::Loading);
                self.backend.send(Request::Related(track.video_id.clone()));
            }
            _ => {}
        }
    }

    /// How long, in seconds, the pointer has rested on the song row
    /// `video_id` (drawn this frame under the pointer): since the pointer
    /// last moved, and since the row came under it. A list scrolled with
    /// the wheel moves rows under a still pointer, and those only pass.
    pub fn resting_on(&self, ctx: &egui::Context, video_id: &str) -> f32 {
        let (now, still) = ctx.input(|i| (i.time, i.pointer.time_since_last_movement()));
        let frame = ctx.cumulative_frame_nr();
        let mut pointed = self.pointed.borrow_mut();
        let since = match pointed.as_mut() {
            // Under the pointer last frame too.
            Some(p) if p.video_id == video_id && p.frame + 1 >= frame => {
                p.frame = frame;
                p.since
            }
            _ => {
                *pointed = Some(Pointed {
                    video_id: video_id.to_string(),
                    since: now,
                    frame,
                });
                now
            }
        };
        still.min((now - since) as f32)
    }

    /// Finds a song's audio ahead of time, so it starts at once if played
    /// (it was pointed at, or is the top search result). Once per song.
    pub fn warm(&self, video_id: &str) {
        let mut warmed = self.warmed.borrow_mut();
        if warmed.len() > 300 {
            warmed.clear();
        }
        if warmed.insert(video_id.to_string()) {
            self.backend.send(Request::Warm(video_id.to_string()));
        }
    }

    /// Where the song playing is, for the seek line and the time: a jump
    /// asked for shows at once, before the player gets there.
    pub fn shown_position(&self) -> f64 {
        match self.seeking {
            Some((entry, to, _)) if self.audio_status.entry == Some(entry) => to,
            _ => self.audio_status.position,
        }
    }

    /// The cover texture for a picture address, when it has arrived.
    pub fn picture(&self, url: &str) -> Option<egui::TextureHandle> {
        self.images.borrow_mut().get(url, &self.backend)
    }

    /// The cover `url` behind an album's or playlist's page, once it has
    /// arrived: its middle band (the rows a wide window shows of a square
    /// cover, a third to two thirds down), shrunk to a few pixels, and when
    /// it was first shown.
    pub fn page_cover(&self, ctx: &egui::Context, url: &str) -> Option<(egui::TextureId, f64)> {
        let mut cover = self.page_cover.borrow_mut();
        if let Some((made_for, texture, since)) = cover.as_ref()
            && made_for == url
        {
            return Some((texture.id(), *since));
        }
        let summary = self.images.borrow_mut().summary(url, &self.backend)?;
        let tiny = &summary.tiny;
        let [width, height] = tiny.size;
        let band: Vec<egui::Color32> = (height / 3..height - height / 3)
            .flat_map(|row| tiny.pixels[row * width..(row + 1) * width].iter().copied())
            .collect();
        let rows = band.len() / width.max(1);
        let image = egui::ColorImage::new([width, rows], band);
        let texture = ctx.load_texture(
            format!("page cover {url}"),
            image,
            egui::TextureOptions::LINEAR,
        );
        let since = ctx.input(|i| i.time);
        let id = texture.id();
        *cover = Some((url.to_string(), texture, since));
        Some((id, since))
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    /// Sends a change to the account (already shown), counting it until
    /// YouTube answers.
    fn send_edit(&mut self, change: Edit) {
        *self
            .edits_in_flight
            .entry(change.item().to_string())
            .or_default() += 1;
        self.backend.send(Request::Edit(change));
    }

    /// YouTube answered a change to the account. True when it was the last
    /// change to that song, playlist or channel on its way: a refusal of
    /// an older one then undoes nothing, since the newer choice stands.
    fn edit_answered(&mut self, change: &Edit) -> bool {
        let item = change.item();
        match self.edits_in_flight.get_mut(item) {
            Some(count) if *count > 1 => {
                *count -= 1;
                false
            }
            _ => {
                self.edits_in_flight.remove(item);
                true
            }
        }
    }

    // ---- Events from the backend and the audio thread ----

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Some(event) = self.backend.try_recv() {
            for word in words(&event) {
                self.script_fonts.want_for(word);
            }
            match event {
                Event::Progress(line) => {
                    if let Auth::Working { progress, .. } = &mut self.auth {
                        *progress = line;
                    }
                }
                Event::SignedIn { name } => {
                    if let Auth::Working { browser, .. } = &self.auth
                        && !self.demo
                    {
                        self.settings.browser = Some(browser.label().to_string());
                    }
                    self.auth = Auth::SignedIn { name };
                    self.pages.clear();
                    self.visited.clear();
                    self.load(Route::Home);
                    self.load(Route::Library);
                    // The page that was open (after signing in again).
                    self.show_current();
                }
                Event::SignInFailed(message) => {
                    let browser = match &self.auth {
                        Auth::Working { browser, .. } => *browser,
                        _ => default_browser(),
                    };
                    self.auth = Auth::Failed { browser, message };
                }
                Event::SignedOut => {
                    if matches!(self.auth, Auth::SignedIn { .. }) {
                        let browser = self
                            .settings
                            .browser
                            .as_deref()
                            .and_then(Browser::parse)
                            .unwrap_or_else(default_browser);
                        self.auth = Auth::Failed {
                            browser,
                            message: "YouTube signed this session out. Check you are signed in to music.youtube.com in your browser, then sign in again.".into(),
                        };
                    }
                }
                Event::Page(route, load, result) => {
                    // An older loading of the page, or one let go while it
                    // loaded: not wanted any more.
                    if self.page_loads.get(&route) != Some(&load) {
                        continue;
                    }
                    // The top search result is the likeliest song to be
                    // played next.
                    if let (Route::Search(_), Ok(page)) = (&route, &result)
                        && let Some(top) = page.tracks().first()
                    {
                        self.warm(&top.video_id);
                    }
                    let loaded = match result {
                        Ok(page) => Loadable::Ready(page),
                        // A refresh that failed keeps what was shown.
                        Err(_) if matches!(self.pages.get(&route), Some(Loadable::Ready(_))) => {
                            continue;
                        }
                        Err(message) => Loadable::Failed(message),
                    };
                    if let Some(slot) = self.pages.get_mut(&route) {
                        *slot = loaded;
                    }
                }
                Event::Prepared { entry, result } => self.prepared(entry, result),
                Event::UpNext {
                    video_id,
                    playlist_id,
                    queue,
                    result,
                } => self.more_arrived(video_id, playlist_id, queue, result),
                Event::PlaylistQueue {
                    playlist_id,
                    mode,
                    ticket,
                    result,
                } => self.playlist_arrived(playlist_id, mode, ticket, result),
                // Only for what was last typed: an answer for older text,
                // arriving late, would replace the right one.
                Event::Suggestions(text, found) if text == self.suggesting => {
                    self.suggestions = (text, found);
                }
                Event::Suggestions(..) => {}
                Event::Lyrics(video_id, lyrics) => {
                    let state = match lyrics {
                        Some(lyrics) if !lyrics.lines.is_empty() => {
                            crate::lyrics::State::Ready(lyrics)
                        }
                        _ => crate::lyrics::State::Missing,
                    };
                    self.lyrics.insert(video_id, state);
                }
                Event::Related(video_id, result) => {
                    let loaded = match result {
                        Ok(page) => Loadable::Ready(page),
                        Err(message) => Loadable::Failed(message),
                    };
                    self.related.insert(video_id, loaded);
                }
                Event::Liked(video_id, like) => {
                    // What the user just chose wins over a late answer.
                    self.likes.entry(video_id).or_insert(like);
                }
                Event::EditFailed(change, message) => {
                    if !self.edit_answered(&change) {
                        // A newer change to the same thing is on its way.
                        log::warn!("a change to the account was refused: {message}");
                        continue;
                    }
                    match &change {
                        // Back to what it was before.
                        Edit::Rate { video_id, .. } => {
                            match self.like_before.remove(video_id).flatten() {
                                Some(before) => self.likes.insert(video_id.clone(), before),
                                None => self.likes.remove(video_id),
                            };
                        }
                        Edit::Save { playlist_id, .. } => {
                            self.saved.remove(playlist_id);
                        }
                        Edit::Subscribe { channel_id, .. } => {
                            self.subscribed.remove(channel_id);
                        }
                        // Show the playlist as it really is.
                        Edit::RemoveFromPlaylist { playlist_id, .. }
                        | Edit::RenamePlaylist { playlist_id, .. } => {
                            self.reload_playlist(playlist_id);
                        }
                        _ => {}
                    }
                    self.notify(format!("That did not work: {message}"));
                }
                Event::Edited(change) => {
                    // A newer change to the same thing is on its way: its
                    // answer is the one that counts.
                    if !self.edit_answered(&change) {
                        continue;
                    }
                    match change {
                        // The library and the changed playlist show the
                        // change.
                        Edit::CreatePlaylist { title, .. } => {
                            self.notify(format!("Made the playlist {title}"));
                            self.refresh(Route::Library);
                        }
                        Edit::RenamePlaylist { .. } | Edit::DeletePlaylist { .. } => {
                            self.refresh(Route::Library);
                        }
                        // An album saved shows in Library > Albums and its
                        // songs in Library > Songs too.
                        Edit::Save { .. } => {
                            for route in [Route::Library, Route::LibraryAlbums, Route::LibrarySongs]
                            {
                                if self.pages.contains_key(&route) {
                                    self.refresh(route);
                                }
                            }
                        }
                        Edit::AddToPlaylist { playlist_id, .. } => {
                            self.reload_playlist(&playlist_id);
                        }
                        // Taken: nothing to go back to any more. Liked
                        // Music shows the change the next time it opens
                        // (not under the user while they look at it).
                        Edit::Rate { video_id, .. } => {
                            self.like_before.remove(&video_id);
                            if self.route != Route::Liked {
                                self.pages.remove(&Route::Liked);
                            }
                        }
                        _ => {}
                    }
                }
                Event::MoreRows {
                    route,
                    load,
                    tracks,
                } => {
                    if self.page_loads.get(&route) != Some(&load) {
                        continue;
                    }
                    // A long list playing from its first songs gets the
                    // rest as they arrive.
                    if route.playlist().is_some() && route.playlist() == self.queue.source {
                        let playable: Vec<Track> =
                            tracks.iter().filter(|t| t.playable).cloned().collect();
                        let added = if self.queue.shuffled() {
                            self.queue.insert_shuffled(playable)
                        } else {
                            self.queue.append(playable)
                        };
                        if added > 0 {
                            self.queue_grew();
                        }
                    }
                    if let Some(Loadable::Ready(page)) = self.pages.get_mut(&route) {
                        add_rows(page, tracks);
                    }
                }
                Event::Image(url, picture) => self.images.borrow_mut().arrived(ctx, url, picture),
                Event::ImageSkipped(url) => self.images.borrow_mut().skipped(&url),
            }
        }

        self.audio_status = self.audio.status();
        // A jump shown until the player gets there (or a minute passes).
        if let Some((entry, to, asked)) = self.seeking
            && (self.audio_status.entry != Some(entry)
                || (self.audio_status.position - to).abs() < 1.0
                || asked.elapsed() > Duration::from_secs(60))
        {
            self.seeking = None;
        }
        if let Some(ended) = self.audio.take_ended()
            && self.playback.entry.as_ref().is_some_and(|e| e.id == ended)
        {
            if self.settings.repeat == Repeat::One {
                self.play_again();
            } else {
                self.next();
            }
        }
        if let Some(failure) = self.audio.take_failed()
            && let Some(entry) = self
                .playback
                .entry
                .clone()
                .filter(|e| e.id == failure.entry)
        {
            self.song_failed(&entry.track.title, failure.message, failure.song_only);
        }
    }

    fn prepared(
        &mut self,
        entry: u64,
        result: Result<crate::backend::Ready, crate::backend::Failure>,
    ) {
        // An answer about a song no longer wanted, or a second one about the
        // song playing (it was asked for twice): it must not start again.
        let preparing = self.playback.state == PlayState::Preparing;
        let Some(current) = self
            .playback
            .entry
            .clone()
            .filter(|e| e.id == entry && preparing)
        else {
            return;
        };
        match result {
            Ok(ready) => {
                let length = ready
                    .length
                    .or(current.track.duration_seconds.map(f64::from))
                    .unwrap_or(0.0);
                let gain = if self.settings.even_loudness {
                    ready.gain
                } else {
                    1.0
                };
                self.audio.send(Command::Play {
                    entry,
                    data: ready.data,
                    gain,
                    length,
                });
                self.backend
                    .send(Request::Details(current.track.video_id.clone()));
                // Lyrics or Related that did not load last time it played
                // (the network, a slow answer) are asked for again.
                let video_id = &current.track.video_id;
                if matches!(
                    self.lyrics.get(video_id),
                    Some(crate::lyrics::State::Missing)
                ) {
                    self.lyrics.remove(video_id);
                }
                if matches!(self.related.get(video_id), Some(Loadable::Failed(_))) {
                    self.related.remove(video_id);
                }
                let report = PlayReport::new(&ready.info);
                if let Some(url) = report.started(0.0) {
                    self.backend.send(Request::Report(url));
                }
                self.playback.report = Some(report);
                self.playback.info = Some(ready.info);
                let quality = if ready.premium {
                    format!("Premium audio: {}", ready.format)
                } else {
                    ready.format
                };
                let way = if ready.direct { "fast way" } else { "yt-dlp" };
                self.playback.format = format!(
                    "{quality}\nFound in {:.1} s ({way}), started {:.1} s later",
                    ready.find_time.as_secs_f64(),
                    ready.start_time.as_secs_f64()
                );
                log::info!("song ready: {}", self.playback.format.replace('\n', "; "));
                self.playback.state = PlayState::Playing;
                self.skipped_in_a_row = 0;
                // Get the next song ready while this one plays.
                self.prepare_next();
                // With the queue on repeat, it plays again instead.
                if self.queue.remaining() <= 1 && self.settings.repeat != Repeat::All {
                    self.ask_for_more();
                }
            }
            Err(failure) => {
                self.song_failed(&current.track.title, failure.message, failure.song_only);
            }
        }
    }

    /// A song could not be played. Only a problem with the song itself
    /// moves on to the next, and only a few songs in a row: something that
    /// hits every song (YouTube refusing, a broken yt-dlp, no network)
    /// stops and waits for Play, rather than running through the queue and
    /// asking YouTube for each song.
    fn song_failed(&mut self, title: &str, message: String, song_only: bool) {
        if song_only && self.skipped_in_a_row < MAX_SKIPS {
            self.skipped_in_a_row += 1;
            self.notify(format!("Skipped \"{title}\": {message}"));
            self.next();
            return;
        }
        let message = if song_only {
            format!(
                "{} songs in a row could not play. {message}",
                self.skipped_in_a_row + 1
            )
        } else {
            message
        };
        self.skipped_in_a_row = 0;
        self.finish_report();
        self.audio.send(Command::Stop);
        self.playback.state = PlayState::Failed(message);
    }

    fn more_arrived(
        &mut self,
        video_id: String,
        playlist_id: Option<String>,
        queue: u64,
        result: Result<(Vec<Track>, Option<String>), String>,
    ) {
        // Asked for a queue that has been replaced since.
        if queue != self.queue.generation() {
            return;
        }
        let tracks = match result {
            Ok((tracks, title)) => {
                // A radio is named by its first answer ("Yellow Mix").
                if self.queue.title.is_none() {
                    self.queue.title = title;
                }
                tracks
            }
            Err(e) => {
                log::warn!("Up next for {video_id} failed: {e}");
                // Asked again the next time the queue runs out.
                self.playback.asked_more_for = None;
                if self.playback.state == PlayState::WaitingForMore {
                    self.notify("Could not get more songs to play.");
                }
                return;
            }
        };
        if self.queue.append(tracks) > 0 {
            self.queue_grew();
        } else if playlist_id.is_some() && self.settings.autoplay {
            // The playlist or album has played to its end: carry on with
            // songs like its last one, as autoplay does.
            self.backend.send(Request::UpNext {
                video_id,
                playlist_id: None,
                queue,
            });
        }
    }

    /// A playlist's or album's songs arrived, to play or queue.
    fn playlist_arrived(
        &mut self,
        playlist_id: String,
        mode: QueueMode,
        ticket: u64,
        result: Result<Vec<Track>, String>,
    ) {
        // Something else was chosen to play while it loaded.
        if matches!(mode, QueueMode::Play | QueueMode::Shuffle)
            && self.wanted_playlist != Some(ticket)
        {
            return;
        }
        match result {
            Ok(tracks) if !tracks.is_empty() => match mode {
                QueueMode::Play => self.play_tracks(tracks, 0, Some(playlist_id)),
                QueueMode::Shuffle => self.apply(Action::Shuffle {
                    tracks,
                    source: Some(playlist_id),
                }),
                QueueMode::Next => {
                    let count = tracks.len();
                    self.queue.play_next_all(tracks);
                    self.notify(format!("{count} songs play next"));
                    self.queue_grew();
                }
                QueueMode::End => {
                    let count = tracks.len();
                    for track in tracks {
                        self.queue.add_to_end(track);
                    }
                    self.notify(format!("Added {count} songs to the queue"));
                    self.queue_grew();
                }
            },
            Ok(_) => self.notify("That playlist has no songs that can play."),
            Err(e) => self.notify(format!("Could not play that: {e}")),
        }
    }

    /// Asks for a playlist's or album's songs, to play or queue them.
    fn ask_for_playlist(&mut self, playlist_id: String, mode: QueueMode) {
        self.tickets += 1;
        if matches!(mode, QueueMode::Play | QueueMode::Shuffle) {
            self.wanted_playlist = Some(self.tickets);
        }
        self.backend.send(Request::PlaylistQueue {
            playlist_id,
            mode,
            ticket: self.tickets,
        });
    }

    // ---- Playing ----

    fn play_tracks(&mut self, tracks: Vec<Track>, start: usize, source: Option<String>) {
        // Songs YouTube no longer offers stay on their page but do not play.
        if tracks.get(start).is_some_and(|t| !t.playable) {
            self.notify(UNAVAILABLE);
            return;
        }
        let start = tracks.iter().take(start).filter(|t| t.playable).count();
        let tracks: Vec<Track> = tracks.into_iter().filter(|t| t.playable).collect();
        // What the user chose last plays: not a playlist still loading.
        self.wanted_playlist = None;
        let title = self.list_title(source.as_deref());
        if let Some(entry) = self.queue.replace(tracks, start, source).cloned() {
            self.queue.title = title;
            // Shuffle stays on from one queue to the next.
            if self.settings.shuffle {
                self.queue.shuffle_on();
            }
            self.playback.asked_more_for = None;
            self.start(entry);
        }
    }

    /// What a queue started now plays from, for Up next's "Playing from":
    /// the playlist `source`'s page when one is kept, else the open page's
    /// own title (an album's), unless the player page is in front of it.
    fn list_title(&self, source: Option<&str>) -> Option<String> {
        let title = |page: &Page| {
            page.header
                .as_ref()
                .map(|h| h.title.clone())
                .filter(|t| !t.is_empty())
        };
        if let Some(source) = source {
            return self.pages.iter().find_map(|(route, page)| match page {
                Loadable::Ready(page)
                    if route.playlist().as_deref() == Some(source)
                        || page.header.as_ref().and_then(|h| h.library_id.as_deref())
                            == Some(source) =>
                {
                    title(page)
                }
                _ => None,
            });
        }
        match self.pages.get(&self.route) {
            Some(Loadable::Ready(page)) if !self.now_playing => title(page),
            _ => None,
        }
    }

    /// Starts preparing `entry`; it plays when ready.
    fn start(&mut self, entry: Entry) {
        self.finish_report();
        self.audio.send(Command::Stop);
        self.backend.send(Request::Prepare {
            entry: entry.id,
            video_id: entry.track.video_id.clone(),
            play: true,
        });
        self.playback = Playback {
            entry: Some(entry),
            state: PlayState::Preparing,
            asked_more_for: self.playback.asked_more_for.take(),
            ..Playback::default()
        };
    }

    /// Reports how long the current song was listened to.
    fn finish_report(&mut self) {
        if let Some(url) = self.listened_report() {
            self.backend.send(Request::Report(url));
        }
    }

    /// The report of how long the current song was listened to (its
    /// stretches, see [`Playback::listened`]), once.
    fn listened_report(&mut self) -> Option<String> {
        let report = self.playback.report.take()?;
        let mut stretches = std::mem::take(&mut self.playback.listened);
        let at = self.audio_status.position;
        if at > self.playback.listening_from || stretches.is_empty() {
            stretches.push((self.playback.listening_from, at));
        }
        report.listened_to(&stretches)
    }

    /// Jumps to `to` seconds in the song playing: the stretch listened to
    /// so far ends here, and the seek line shows `to` until the player gets
    /// there.
    fn jump(&mut self, to: f64) {
        let at = self.audio_status.position;
        if at > self.playback.listening_from {
            let from = self.playback.listening_from;
            self.playback.listened.push((from, at));
        }
        self.playback.listening_from = to;
        if let Some(entry) = &self.playback.entry {
            self.seeking = Some((entry.id, to, Instant::now()));
        }
        self.audio.send(Command::Seek(to));
    }

    fn next(&mut self) {
        match self.queue.advance().cloned() {
            Some(entry) => self.start(entry),
            // A queue of one song, on repeat, that played: no need to fetch
            // it again. (One that could not play is fetched again below.)
            None if self.settings.repeat == Repeat::All
                && self.queue.entries().len() == 1
                && self.playback.state == PlayState::Playing =>
            {
                self.play_again();
            }
            None if self.settings.repeat == Repeat::All => {
                let first = self.queue.entries().first().map(|e| e.id);
                if let Some(entry) = first.and_then(|id| self.queue.jump(id)).cloned() {
                    self.start(entry);
                }
            }
            None => {
                self.finish_report();
                self.audio.send(Command::Stop);
                self.playback.state = PlayState::WaitingForMore;
                self.ask_for_more();
            }
        }
    }

    /// Plays the current song again from the start, as a new play.
    fn play_again(&mut self) {
        self.finish_report();
        self.playback.listened.clear();
        self.playback.listening_from = 0.0;
        if let Some(info) = &self.playback.info {
            let report = PlayReport::new(info);
            if let Some(url) = report.started(0.0) {
                self.backend.send(Request::Report(url));
            }
            self.playback.report = Some(report);
        }
        self.audio.send(Command::Seek(0.0));
    }

    /// Gets the next song ready ahead, after the queue changed.
    fn prepare_next(&self) {
        if self.playback.state == PlayState::Playing
            && let Some(next) = self.queue.peek_next()
        {
            self.backend.send(Request::Prepare {
                entry: next.id,
                video_id: next.track.video_id.clone(),
                play: false,
            });
        }
    }

    fn previous(&mut self) {
        if self.audio_status.position > 3.0 {
            self.jump(0.0);
            return;
        }
        let current = self.queue.current().map(|e| e.id);
        match self.queue.back().cloned() {
            // The first song: back to its start (not fetched again).
            Some(entry) if Some(entry.id) == current => self.jump(0.0),
            Some(entry) => self.start(entry),
            None => {}
        }
    }

    /// Asks for what plays after the last song in the queue, once per song.
    /// Without autoplay, only a playlist's or album's own songs follow.
    fn ask_for_more(&mut self) {
        if !self.settings.autoplay && self.queue.source.is_none() {
            return;
        }
        let Some(last) = self.queue.entries().last() else {
            return;
        };
        let video_id = last.track.video_id.clone();
        if self.playback.asked_more_for.as_deref() == Some(video_id.as_str()) {
            return;
        }
        self.playback.asked_more_for = Some(video_id.clone());
        self.backend.send(Request::UpNext {
            video_id,
            playlist_id: self.queue.source.clone(),
            queue: self.queue.generation(),
        });
    }

    /// After songs were put in the queue: play them if nothing is playing,
    /// and get the next one ready.
    fn queue_grew(&mut self) {
        match self.playback.state {
            PlayState::Idle => {
                if let Some(entry) = self.queue.current().cloned() {
                    self.start(entry);
                }
            }
            PlayState::WaitingForMore => self.next(),
            _ => self.prepare_next(),
        }
    }

    fn toggle_pause(&mut self) {
        match self.playback.state {
            PlayState::Playing if self.audio_status.paused => self.audio.send(Command::Resume),
            PlayState::Playing => self.audio.send(Command::Pause),
            PlayState::Failed(_) => {
                if let Some(entry) = self.playback.entry.clone() {
                    self.start(entry);
                }
            }
            // The queue ran out and nothing more came: Play starts its last
            // song again.
            PlayState::WaitingForMore => {
                if let Some(entry) = self.queue.current().cloned() {
                    self.start(entry);
                }
            }
            _ => {}
        }
    }

    // ---- Pages ----

    pub fn load(&mut self, route: Route) {
        self.touch(&route);
        self.pages.insert(route.clone(), Loadable::Loading);
        self.ask_for_page(route);
        self.trim_pages();
    }

    /// Loads a page again, as YouTube now has it, while what it showed
    /// stays on screen until the new one arrives (no spinner under the
    /// user, and menus built from it keep working).
    fn refresh(&mut self, route: Route) {
        if matches!(self.pages.get(&route), Some(Loadable::Ready(_))) {
            self.ask_for_page(route);
        } else {
            self.load(route);
        }
    }

    /// Asks for a page, as a new loading of it: answers for older ones are
    /// dropped.
    fn ask_for_page(&mut self, route: Route) {
        self.loads += 1;
        self.page_loads.insert(route.clone(), self.loads);
        self.backend.send(Request::Page {
            route,
            load: self.loads,
        });
    }

    /// Marks a page as just shown, for [`App::trim_pages`].
    fn touch(&mut self, route: &Route) {
        self.visited.retain(|r| r != route);
        self.visited.push(route.clone());
    }

    /// Lets go of the pages shown longest ago, beyond [`MAX_PAGES`]. The
    /// page on screen and the sidebar's pages stay.
    fn trim_pages(&mut self) {
        while self.pages.len() > MAX_PAGES {
            let current = &self.route;
            let keep = |r: &Route| {
                r == current || matches!(r, Route::Home | Route::Library | Route::Liked)
            };
            let Some(index) = self.visited.iter().position(|r| !keep(r)) else {
                break;
            };
            let old = self.visited.remove(index);
            self.pages.remove(&old);
        }
    }

    /// Shows the page for the current route, loading it when it is not
    /// here (never loaded, let go, or failed). History grows as songs play:
    /// it is loaded again each time it is shown.
    fn show_current(&mut self) {
        let route = self.route.clone();
        if route.is_local() {
            return;
        }
        match self.pages.get(&route) {
            None | Some(Loadable::Failed(_)) => self.load(route),
            Some(Loadable::Ready(_)) if route == Route::History => {
                self.touch(&route);
                self.refresh(route);
            }
            Some(_) => self.touch(&route),
        }
    }

    fn navigate(&mut self, route: Route) {
        // Going to a page shows it: the player page steps aside.
        self.now_playing = false;
        if route == self.route {
            return;
        }
        let old = std::mem::replace(&mut self.route, route);
        self.back.push(old);
        self.forward.clear();
        // A page opened anew starts at its top (Back and Forward return to
        // where it was left).
        self.fresh_page.set(true);
        self.show_current();
    }

    pub fn can_go_back(&self) -> bool {
        self.now_playing || !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Opening a card: a page, or playing it.
    fn open(&mut self, target: Target, track: Option<Track>) {
        match target {
            Target::Browse { id, params, .. } => self.navigate(Route::browse(id, params)),
            watch @ Target::Watch { .. } => self.play(watch, track),
            Target::Search { query, params } => self.navigate(match params {
                Some(params) => Route::SearchOnly(query, params),
                None => Route::Search(query),
            }),
        }
    }

    fn play(&mut self, target: Target, track: Option<Track>) {
        match target {
            Target::Watch {
                video_id: Some(video_id),
                playlist_id,
            } => {
                let track = track.unwrap_or_else(|| Track {
                    video_id: video_id.clone(),
                    set_video_id: None,
                    title: "Song".into(),
                    artists: String::new(),
                    album: None,
                    duration_seconds: None,
                    kind: TrackKind::Unknown,
                    thumbnail: None,
                    ..Track::default()
                });
                self.play_tracks(vec![track], 0, playlist_id);
                self.ask_for_more();
            }
            Target::Watch {
                video_id: None,
                playlist_id: Some(playlist_id),
            } => {
                self.notify("Getting the songs...");
                self.ask_for_playlist(playlist_id, QueueMode::Play);
            }
            Target::Watch { .. } => {}
            other @ (Target::Browse { .. } | Target::Search { .. }) => self.open(other, None),
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Navigate(route) => self.navigate(route),
            Action::Back => {
                // With the player page open, Back closes it.
                if self.now_playing {
                    self.now_playing = false;
                } else if let Some(route) = self.back.pop() {
                    let old = std::mem::replace(&mut self.route, route);
                    self.forward.push(old);
                    self.show_current();
                }
            }
            Action::Forward => {
                self.now_playing = false;
                if let Some(route) = self.forward.pop() {
                    let old = std::mem::replace(&mut self.route, route);
                    self.back.push(old);
                    self.show_current();
                }
            }
            Action::Search(query) => {
                self.script_fonts.want_for(&query);
                let query = query.trim().to_string();
                if !query.is_empty() {
                    let route = Route::Search(query);
                    self.navigate(route.clone());
                    if !matches!(self.pages.get(&route), Some(Loadable::Loading)) {
                        self.load(route);
                    }
                }
            }
            Action::Retry(route) => self.load(route),
            Action::PlayTracks {
                tracks,
                start,
                source,
            } => {
                let single = tracks.len() == 1;
                self.play_tracks(tracks, start, source);
                if single {
                    self.ask_for_more();
                }
            }
            Action::Shuffle { mut tracks, source } => {
                use rand::seq::SliceRandom;
                tracks.retain(|t| t.playable);
                tracks.shuffle(&mut rand::rng());
                self.play_tracks(tracks, 0, source);
                self.queue.set_shuffled();
            }
            Action::Open(target, track) => self.open(target, track),
            Action::Play(target, track) => self.play(target, track),
            Action::TogglePause => self.toggle_pause(),
            Action::Next => self.next(),
            Action::Previous => self.previous(),
            Action::Seek(to) => {
                self.jump(to);
                if let Some(controls) = &self.controls {
                    controls.seeked(Duration::from_secs_f64(to.max(0.0)));
                }
            }
            Action::SetVolume(volume) => {
                self.settings.volume = volume;
                if volume > 0.001 {
                    self.loud_volume = volume;
                }
                self.audio.send(Command::Volume(volume));
            }
            Action::ToggleMute => {
                let to = if self.settings.volume <= 0.001 {
                    self.loud_volume
                } else {
                    0.0
                };
                self.apply(Action::SetVolume(to));
            }
            // The song playing, chosen in Up next: pause or play it, rather
            // than start it over (and count another play).
            Action::JumpTo(id) if self.playback.entry.as_ref().is_some_and(|e| e.id == id) => {
                self.toggle_pause();
            }
            Action::JumpTo(id) => {
                if let Some(entry) = self.queue.jump(id).cloned() {
                    self.start(entry);
                }
            }
            Action::PlayNext(track) | Action::AddToQueue(track) if !track.playable => {
                self.notify(UNAVAILABLE);
            }
            Action::PlayNext(track) => {
                self.notify(format!("\u{201c}{}\u{201d} plays next", track.title));
                self.queue.play_next(track);
                self.queue_grew();
            }
            Action::AddToQueue(track) => {
                self.notify(format!(
                    "Added \u{201c}{}\u{201d} to the queue",
                    track.title
                ));
                self.queue.add_to_end(track);
                self.queue_grew();
            }
            Action::Notify(text) => self.notify(text),
            Action::RemoveFromQueue(id) => {
                self.queue.remove(id);
                self.prepare_next();
            }
            Action::ShiftInQueue(id, up) => {
                self.queue.shift(id, up);
                self.prepare_next();
            }
            Action::MoveNextInQueue(id) => {
                self.queue.move_next(id);
                self.prepare_next();
            }
            Action::ToggleGuide => self.settings.mini_guide = !self.settings.mini_guide,
            Action::ToggleNowPlaying => self.now_playing = !self.now_playing,
            Action::CloseNowPlaying => self.now_playing = false,
            Action::NowPlayingTab(tab) => self.np_tab = tab,
            Action::Rate(video_id, like) => {
                // Shown at once; YouTube is told in the background. What
                // it was before the first change on its way, should
                // YouTube refuse.
                if !self.edits_in_flight.contains_key(&video_id) {
                    let before = self.likes.get(&video_id).copied();
                    self.like_before.insert(video_id.clone(), before);
                }
                self.likes.insert(video_id.clone(), like);
                self.send_edit(Edit::Rate { video_id, like });
                if like == LikeState::Liked {
                    self.notify("Added to Liked Music");
                }
            }
            Action::AddToPlaylist {
                playlist_id,
                title,
                video_id,
            } => {
                self.notify(format!("Added to {title}"));
                self.send_edit(Edit::AddToPlaylist {
                    playlist_id,
                    video_id,
                });
            }
            Action::Edit(change) => self.send_edit(change),
            Action::Suggest(text) => {
                // Words being typed show in their own script at once.
                self.script_fonts.want_for(&text);
                self.suggesting.clone_from(&text);
                self.backend.send(Request::Suggest(text));
            }
            Action::QueuePlaylist(playlist_id, mode) => self.ask_for_playlist(playlist_id, mode),
            Action::Toggle(which) => {
                let s = &mut self.settings;
                match which {
                    Setting::FastWay => {
                        s.fast_way = !s.fast_way;
                        self.backend.send(Request::FastWay(s.fast_way));
                    }
                    Setting::Autoplay => s.autoplay = !s.autoplay,
                    Setting::EvenLoudness => s.even_loudness = !s.even_loudness,
                }
            }
            Action::OpenDialog(dialog) => *self.dialog.get_mut() = Some(dialog),
            Action::RenamePlaylist { playlist_id, name } => {
                self.notify(format!("Renamed to {name}"));
                let route = Route::browse(format!("VL{playlist_id}"), None);
                if let Some(Loadable::Ready(Page {
                    header: Some(header),
                    ..
                })) = self.pages.get_mut(&route)
                {
                    header.title = name.clone();
                }
                self.send_edit(Edit::RenamePlaylist { playlist_id, name });
            }
            Action::DeletePlaylist(playlist_id) => {
                self.notify("Deleted the playlist");
                // Away from its page.
                if matches!(&self.route, Route::Browse { id, .. } if id.trim_start_matches("VL") == playlist_id)
                {
                    self.route = Route::Library;
                    self.show_current();
                }
                self.send_edit(Edit::DeletePlaylist { playlist_id });
            }
            Action::RemoveFromPlaylist {
                playlist_id,
                video_id,
                set_video_id,
            } => {
                let route = Route::browse(format!("VL{playlist_id}"), None);
                if let Some(Loadable::Ready(page)) = self.pages.get_mut(&route) {
                    for section in &mut page.sections {
                        section.items.retain(|item| {
                            !matches!(item, Item::Track(t)
                                if t.set_video_id.as_deref() == Some(set_video_id.as_str()))
                        });
                    }
                }
                self.notify("Removed from the playlist");
                self.send_edit(Edit::RemoveFromPlaylist {
                    playlist_id,
                    video_id,
                    set_video_id,
                });
            }
            Action::ToggleSave { playlist_id, save } => {
                self.saved.insert(playlist_id.clone(), save);
                self.notify(if save {
                    "Saved to your library"
                } else {
                    "Removed from your library"
                });
                self.send_edit(Edit::Save { playlist_id, save });
            }
            Action::ToggleSubscribe {
                channel_id,
                subscribe,
            } => {
                self.subscribed.insert(channel_id.clone(), subscribe);
                self.send_edit(Edit::Subscribe {
                    channel_id,
                    subscribe,
                });
            }
            Action::OpenLogFolder => {
                if let Some(dirs) = directories::ProjectDirs::from("", "", "YtFast") {
                    open_folder(dirs.cache_dir());
                }
            }
            Action::OpenFullDiskAccess => open_full_disk_access(),
            Action::ShuffleQueue => {
                self.settings.shuffle = !self.settings.shuffle;
                if self.settings.shuffle {
                    self.queue.shuffle_on();
                } else {
                    self.queue.shuffle_off();
                }
                self.prepare_next();
            }
            Action::CycleRepeat => {
                self.settings.repeat = match self.settings.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
            }
            Action::ChooseBrowser(browser) => match &mut self.auth {
                Auth::Choosing { browser: chosen } => *chosen = browser,
                // Another browser after a failed sign-in: the old error was
                // that browser's, and goes.
                Auth::Failed {
                    browser: chosen, ..
                } if *chosen != browser => self.auth = Auth::Choosing { browser },
                _ => {}
            },
            Action::SignIn => {
                let browser = match &self.auth {
                    Auth::Choosing { browser } | Auth::Failed { browser, .. } => *browser,
                    _ => return,
                };
                self.backend.send(Request::SignIn(browser));
                self.auth = Auth::Working {
                    browser,
                    progress: "Signing in...".into(),
                };
            }
            Action::SignOut => {
                // The last song's listening time goes with the sign-out, so
                // it is sent before the sign-in is let go.
                let last_report = self.listened_report();
                self.audio.send(Command::Stop);
                self.queue.clear();
                self.playback = Playback::default();
                self.pages.clear();
                self.visited.clear();
                // The next account's are its own.
                self.likes.clear();
                self.saved.clear();
                self.subscribed.clear();
                self.edits_in_flight.clear();
                self.like_before.clear();
                self.settings.browser = None;
                self.backend.send(Request::SignOut { last_report });
                self.auth = Auth::Choosing {
                    browser: default_browser(),
                };
            }
        }
    }

    /// Space plays and pauses; "/" and Ctrl+F (Cmd+F on a Mac) go to the
    /// search box. Taken before the views run, so a focused button does
    /// not also take the Space.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if !matches!(self.auth, Auth::SignedIn { .. }) {
            return;
        }
        // Typing in a text box (the search box, a dialog's name): its keys
        // are its own. (egui's `egui_wants_keyboard_input` is true for any
        // widget with the focus, a button too.)
        let typing = ctx.text_edit_focused();
        // Escape closes a dialog (or leaves the search box) first.
        if self.now_playing
            && !typing
            && self.dialog.get_mut().is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.act(Action::CloseNowPlaying);
        }
        // Back and Forward as in a browser: the mouse's side buttons, and
        // Alt+Left/Right (Cmd+[ and Cmd+] on a Mac).
        let (back, forward) = ctx.input_mut(|i| {
            use egui::{Key, Modifiers, PointerButton};
            let mac = cfg!(target_os = "macos");
            let back = i.pointer.button_pressed(PointerButton::Extra1)
                || (!mac && i.consume_key(Modifiers::ALT, Key::ArrowLeft))
                || (mac && i.consume_key(Modifiers::COMMAND, Key::OpenBracket));
            let forward = i.pointer.button_pressed(PointerButton::Extra2)
                || (!mac && i.consume_key(Modifiers::ALT, Key::ArrowRight))
                || (mac && i.consume_key(Modifiers::COMMAND, Key::CloseBracket));
            (back, forward)
        });
        if back && self.can_go_back() {
            self.act(Action::Back);
        }
        if forward && self.can_go_forward() {
            self.act(Action::Forward);
        }
        // A button the keyboard has moved to takes Space (to press it) and
        // the arrow keys (to move on), as in a browser.
        let focused = ctx.memory(|m| m.focused().is_some());
        let (play, slash, search) = ctx.input_mut(|i| {
            let play =
                !typing && !focused && i.consume_key(egui::Modifiers::NONE, egui::Key::Space);
            let slash = !typing && i.consume_key(egui::Modifiers::NONE, egui::Key::Slash);
            if slash {
                // The key's "/" would land in the search box it opens.
                i.events
                    .retain(|e| !matches!(e, egui::Event::Text(text) if text == "/"));
            }
            let search = slash || i.consume_key(egui::Modifiers::COMMAND, egui::Key::F);
            (play, slash, search)
        });
        // The "/" may come a frame after its key, too.
        self.drop_slash = slash;
        if play {
            self.act(Action::TogglePause);
        }
        if !typing {
            use egui::{Key, Modifiers};
            let pressed = |key, modifiers| ctx.input_mut(|i| i.consume_key(modifiers, key));
            let position = self.shown_position();
            let volume = self.settings.volume;
            if !focused {
                if pressed(Key::ArrowLeft, Modifiers::NONE) {
                    self.act(Action::Seek((position - 10.0).max(0.0)));
                }
                if pressed(Key::ArrowRight, Modifiers::NONE) {
                    self.act(Action::Seek(position + 10.0));
                }
                if pressed(Key::ArrowUp, Modifiers::NONE) {
                    self.act(Action::SetVolume((volume + 0.1).min(1.0)));
                }
                if pressed(Key::ArrowDown, Modifiers::NONE) {
                    self.act(Action::SetVolume((volume - 0.1).max(0.0)));
                }
            }
            if pressed(Key::N, Modifiers::SHIFT) {
                self.act(Action::Next);
            }
            if pressed(Key::P, Modifiers::SHIFT) {
                self.act(Action::Previous);
            }
            if pressed(Key::M, Modifiers::NONE) {
                self.act(Action::ToggleMute);
            }
            if pressed(Key::L, Modifiers::NONE)
                && let Some(entry) = &self.playback.entry
            {
                let id = entry.track.video_id.clone();
                let liked = self.likes.get(&id) == Some(&LikeState::Liked);
                let next = if liked {
                    LikeState::Neutral
                } else {
                    LikeState::Liked
                };
                self.act(Action::Rate(id, next));
            }
        }
        if search {
            ctx.memory_mut(|m| m.request_focus(views::SEARCH_BOX.into()));
        }
    }

    /// The desktop's media keys and Now Playing panel.
    fn media_controls(&mut self) {
        let Some(commands) = self.controls.as_ref().map(|c| c.commands()) else {
            return;
        };
        let sounding = self.playback.state == PlayState::Playing && !self.audio_status.paused;
        let current = self.playback.entry.as_ref().map(|e| e.id.to_string());
        for command in commands {
            let action = match command {
                // "Play" while playing, or "Pause" while paused, changes
                // nothing.
                now_playing::Command::Play => (!sounding).then_some(Action::TogglePause),
                now_playing::Command::Pause | now_playing::Command::Stop => {
                    sounding.then_some(Action::TogglePause)
                }
                now_playing::Command::PlayPause => Some(Action::TogglePause),
                now_playing::Command::Next => Some(Action::Next),
                now_playing::Command::Previous => Some(Action::Previous),
                now_playing::Command::SeekBy(ms) => {
                    Some(Action::Seek(self.shown_position() + ms as f64 / 1000.0))
                }
                now_playing::Command::SetPosition { track_id, position } => {
                    // Only for the song playing now.
                    let here = current.as_deref() == Some(track_id.as_str());
                    here.then_some(Action::Seek(position.as_secs_f64()))
                }
                now_playing::Command::SetVolume(v) => Some(Action::SetVolume(v as f32)),
                _ => None,
            };
            if let Some(action) = action {
                self.act(action);
            }
        }
        let playing = self.playback.state == PlayState::Playing;
        let track = self.playback.entry.as_ref().map(|e| now_playing::Track {
            id: format!("{}", e.id),
            title: e.track.title.clone(),
            artists: vec![e.track.artists.clone()],
            album: e.track.album.clone().unwrap_or_default(),
            duration: Some(Duration::from_secs_f64(self.audio_status.length.max(0.0))),
            art_url: e.track.thumbnail.as_ref().map(|t| t.sized(544)),
            ..now_playing::Track::default()
        });
        let state = now_playing::State {
            playback: match (playing, self.audio_status.paused) {
                (true, false) => now_playing::Playback::Playing,
                (true, true) => now_playing::Playback::Paused,
                _ => now_playing::Playback::Stopped,
            },
            track,
            position: Duration::from_secs_f64(self.shown_position().max(0.0)),
            volume: Some(f64::from(self.settings.volume)),
            ..now_playing::State::default()
        };
        if let Some(controls) = &mut self.controls {
            controls.update(state);
        }
    }
}

/// The words an event brings to the screen (titles, names, lyrics), to
/// know which fonts they need.
fn words(event: &Event) -> Vec<&str> {
    fn track<'a>(track: &'a Track, words: &mut Vec<&'a str>) {
        words.extend([track.title.as_str(), track.artists.as_str()]);
        words.extend(track.album.as_deref());
    }
    fn page<'a>(page: &'a Page, words: &mut Vec<&'a str>) {
        if let Some(header) = &page.header {
            words.extend([
                header.title.as_str(),
                header.subtitle.as_str(),
                header.owner.as_str(),
            ]);
        }
        for section in &page.sections {
            words.push(&section.title);
            for item in &section.items {
                match item {
                    Item::Track(t) => track(t, words),
                    Item::Card(card) => words.extend([card.title.as_str(), card.subtitle.as_str()]),
                }
            }
        }
    }
    let mut words = Vec::new();
    match event {
        Event::SignedIn { name } => words.push(name.as_str()),
        Event::Page(_, _, Ok(found)) | Event::Related(_, Ok(found)) => page(found, &mut words),
        Event::MoreRows { tracks, .. }
        | Event::UpNext {
            result: Ok((tracks, _)),
            ..
        }
        | Event::PlaylistQueue {
            result: Ok(tracks), ..
        } => {
            for t in tracks {
                track(t, &mut words);
            }
        }
        Event::Lyrics(_, Some(lyrics)) => {
            words.extend(lyrics.lines.iter().map(|line| line.text.as_str()));
        }
        Event::Suggestions(_, found) => {
            words.extend(found.words.iter().map(String::as_str));
            for item in &found.items {
                match item {
                    Item::Track(t) => track(t, &mut words),
                    Item::Card(c) => words.extend([c.title.as_str(), c.subtitle.as_str()]),
                }
            }
        }
        _ => {}
    }
    words
}

/// Adds a long list's later songs after its first ones.
fn add_rows(page: &mut Page, tracks: Vec<Track>) {
    // The list is the untitled section of songs (others have titles).
    let has_songs = |s: &Section| s.items.iter().any(|i| matches!(i, Item::Track(_)));
    let index = page
        .sections
        .iter()
        .position(|s| s.title.is_empty() && has_songs(s))
        .or_else(|| page.sections.iter().position(has_songs));
    if let Some(section) = index.map(|i| &mut page.sections[i]) {
        section.items.extend(tracks.into_iter().map(Item::Track));
    }
}

/// The most pages kept in memory; one shown again after being let go
/// loads again.
const MAX_PAGES: usize = 24;

/// The most songs skipped in a row for problems of their own before the
/// queue stops: a few removed songs together are skipped, but a problem
/// every song has cannot run through the queue.
const MAX_SKIPS: usize = 5;

/// How long a notice shows: 3 s, as YouTube Music's, and 0.3 s to go.
const NOTICE_TIME: Duration = Duration::from_millis(3300);

/// The widest texture egui makes (see `raw_input_hook`). The covers are
/// far smaller.
const MAX_TEXTURE_SIDE: usize = 4096;

/// "YTFast 0.4.0 (abc1234)": the version, and the commit it was built from
/// ("+" when built with changes not yet committed).
pub const VERSION: &str = concat!(
    "YTFast ",
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("YTFAST_COMMIT"),
    ")"
);

/// What choosing a song YouTube no longer offers says.
const UNAVAILABLE: &str = "This song is not available.";

/// The window's first size, and the least it can be made, in points.
pub const WINDOW_SIZE: [f32; 2] = [1280.0, 820.0];
pub const MIN_WINDOW_SIZE: [f32; 2] = [960.0, 600.0];

impl eframe::App for App {
    /// Runs before every frame, and also while the window is minimised or
    /// hidden, when eframe draws nothing: what keeps the music going (the
    /// next song, the media keys) must not wait for the window to show.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.script_fonts.add_when_read();
        self.handle_events(ctx);
        self.media_controls();
        // A media key pressed while the window is hidden acts at once.
        self.apply_actions();
        // Keep the progress bar moving, and hear when the song ends, while
        // a song plays.
        if self.playback.state == PlayState::Playing && !self.audio_status.paused {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.scrolling.apply(&ctx);
        self.mend_window_size(&ctx);
        self.script_fonts.check();
        self.images.get_mut().begin_frame();
        self.shortcuts(&ctx);
        self.want_song_extras();

        views::show(self, ui);

        self.apply_actions();
        // A notice shows for a few seconds.
        if let Some((_, at)) = &self.notice {
            let left = NOTICE_TIME.saturating_sub(at.elapsed());
            if left.is_zero() {
                self.notice = None;
            } else {
                ctx.request_repaint_after(left);
            }
        }
    }

    /// egui sizes its text atlas (in memory and on the graphics card) as
    /// wide as the card allows, 16384 pixels on most: 4096 is plenty for
    /// one window's text and keeps it small.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.max_texture_side = raw_input
            .max_texture_side
            .map(|side| side.min(MAX_TEXTURE_SIDE));
        if std::mem::take(&mut self.drop_slash) {
            raw_input
                .events
                .retain(|e| !matches!(e, egui::Event::Text(text) if text == "/"));
        }
    }

    /// Where pages were scrolled to is not kept for the next run: a page
    /// opens at its top, as on YouTube Music.
    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // What is changed in the demo is not kept: it is for trying things.
        if !self.demo {
            eframe::set_value(storage, "ytfast", &self.settings);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Sent before the backend stops with the window.
        if let Some(url) = self.listened_report() {
            self.backend.report_before_closing(url);
        }
        self.backend.forget_sign_in();
    }
}

/// Opens a folder in the system's file browser.
fn open_folder(path: &std::path::Path) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::fs::create_dir_all(path);
    if let Err(e) = std::process::Command::new(program).arg(path).spawn() {
        log::warn!("could not open the folder: {e}");
    }
}

/// Opens System Settings at Full Disk Access. A Mac has no question an app
/// can ask for this permission; showing its switch is the nearest thing.
fn open_full_disk_access() {
    let page = "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";
    if let Err(e) = std::process::Command::new("open").arg(page).spawn() {
        log::warn!("could not open System Settings: {e}");
    }
}

#[cfg(test)]
mod tests {
    //! The app's own rules, with stand-ins for the backend and the player:
    //! what it asks for is read back, and answers are handed to it as the
    //! next frame would take them in.

    use super::*;
    use crate::backend::{Failure, Ready};

    struct Harness {
        app: App,
        ctx: egui::Context,
        requests: tokio::sync::mpsc::UnboundedReceiver<Request>,
        events: std::sync::mpsc::Sender<Event>,
        commands: std::sync::mpsc::Receiver<Command>,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            let (backend, requests, events) = Backend::for_tests();
            let (audio, commands) = Audio::for_tests();
            let fonts = crate::theme::install(&ctx);
            let app = App::with(backend, audio, None, fonts, Settings::default(), false);
            let mut harness = Self {
                app,
                ctx,
                requests,
                events,
                commands,
            };
            // What starting up sent.
            harness.requests();
            harness.commands();
            harness
        }

        /// Hands the app an answer, and lets it take it in.
        fn answer(&mut self, event: Event) {
            self.events.send(event).expect("the app is listening");
            self.frame();
        }

        /// The start of a frame: answers and the player's news are taken in.
        fn frame(&mut self) {
            self.app.handle_events(&self.ctx);
        }

        fn act(&mut self, action: Action) {
            self.app.apply(action);
        }

        /// What the app has asked the backend for since the last look.
        fn requests(&mut self) -> Vec<Request> {
            std::iter::from_fn(|| self.requests.try_recv().ok()).collect()
        }

        /// What the app has told the player since the last look.
        fn commands(&mut self) -> Vec<Command> {
            self.commands.try_iter().collect()
        }

        /// The queue entry playing (or being got ready), and its song.
        fn entry(&self) -> (u64, &str) {
            let entry = self.app.playback.entry.as_ref().expect("a song");
            (entry.id, entry.track.video_id.as_str())
        }

        /// Plays `songs` from the first, and lets it get ready.
        fn play(&mut self, songs: &[&str]) {
            self.act(Action::PlayTracks {
                tracks: songs.iter().map(|id| song(id)).collect(),
                start: 0,
                source: None,
            });
            self.ready();
        }

        /// The song being got ready is ready.
        fn ready(&mut self) {
            let (entry, _) = self.entry();
            self.answer(Event::Prepared {
                entry,
                result: Ok(ready()),
            });
        }
    }

    fn song(id: &str) -> Track {
        Track {
            video_id: id.to_string(),
            title: id.to_uppercase(),
            duration_seconds: Some(200),
            ..Track::default()
        }
    }

    fn ready() -> Ready {
        Ready {
            data: ytfast_core::stream::SongData::complete(Vec::new()),
            gain: 1.0,
            info: PlayerInfo::default(),
            format: "test".into(),
            premium: true,
            length: Some(200.0),
            find_time: Duration::ZERO,
            start_time: Duration::ZERO,
            direct: true,
        }
    }

    fn failed(song_only: bool) -> Result<Ready, Failure> {
        Err(Failure {
            message: "no".into(),
            song_only,
        })
    }

    fn plays(commands: &[Command]) -> usize {
        commands
            .iter()
            .filter(|c| matches!(c, Command::Play { .. }))
            .count()
    }

    #[test]
    fn an_answer_about_a_song_no_longer_wanted_changes_nothing() {
        let mut h = Harness::new();
        h.act(Action::PlayTracks {
            tracks: vec![song("a"), song("b")],
            start: 0,
            source: None,
        });
        let (first, _) = h.entry();
        h.act(Action::Next);
        h.commands();
        h.answer(Event::Prepared {
            entry: first,
            result: Ok(ready()),
        });
        assert_eq!(h.entry().1, "b");
        assert_eq!(h.app.playback.state, PlayState::Preparing);
        assert_eq!(plays(&h.commands()), 0);
    }

    #[test]
    fn a_second_answer_about_the_song_playing_does_not_start_it_again() {
        let mut h = Harness::new();
        h.play(&["a"]);
        assert_eq!(plays(&h.commands()), 1);
        h.ready();
        assert_eq!(plays(&h.commands()), 0);
    }

    #[test]
    fn a_song_that_cannot_play_is_skipped() {
        let mut h = Harness::new();
        h.act(Action::PlayTracks {
            tracks: vec![song("a"), song("b")],
            start: 0,
            source: None,
        });
        let (entry, _) = h.entry();
        h.answer(Event::Prepared {
            entry,
            result: failed(true),
        });
        assert_eq!(h.entry().1, "b");
        assert_eq!(h.app.playback.state, PlayState::Preparing);
    }

    #[test]
    fn a_failure_not_of_the_song_stops_and_keeps_the_queue() {
        let mut h = Harness::new();
        h.act(Action::PlayTracks {
            tracks: vec![song("a"), song("b")],
            start: 0,
            source: None,
        });
        let (entry, _) = h.entry();
        h.answer(Event::Prepared {
            entry,
            result: failed(false),
        });
        assert_eq!(h.entry().1, "a");
        assert!(matches!(h.app.playback.state, PlayState::Failed(_)));
        assert_eq!(h.app.queue.entries().len(), 2);
        // Play tries the same song again.
        h.requests();
        h.act(Action::TogglePause);
        assert_eq!(h.entry().1, "a");
        assert!(
            h.requests()
                .iter()
                .any(|r| matches!(r, Request::Prepare { play: true, .. }))
        );
    }

    #[test]
    fn songs_that_cannot_play_stop_being_skipped_after_a_few() {
        let mut h = Harness::new();
        let songs: Vec<String> = (0..10).map(|n| format!("s{n}")).collect();
        h.act(Action::PlayTracks {
            tracks: songs.iter().map(|id| song(id)).collect(),
            start: 0,
            source: None,
        });
        for _ in 0..=MAX_SKIPS {
            let (entry, _) = h.entry();
            h.answer(Event::Prepared {
                entry,
                result: failed(true),
            });
        }
        assert!(matches!(h.app.playback.state, PlayState::Failed(_)));
        assert_eq!(h.entry().1, format!("s{MAX_SKIPS}"));
    }

    #[test]
    fn repeat_all_goes_back_to_the_first_song() {
        let mut h = Harness::new();
        h.app.settings.repeat = Repeat::All;
        h.play(&["a", "b"]);
        let (first, _) = h.entry();
        h.act(Action::Next);
        h.ready();
        let (last, _) = h.entry();
        h.app.audio.set_status(|s| s.ended = Some(last));
        h.frame();
        assert_eq!(h.entry(), (first, "a"));
        assert_eq!(h.app.playback.state, PlayState::Preparing);
    }

    #[test]
    fn repeat_one_plays_the_song_again_from_its_start() {
        let mut h = Harness::new();
        h.app.settings.repeat = Repeat::One;
        h.play(&["a", "b"]);
        let (entry, _) = h.entry();
        h.commands();
        h.app.audio.set_status(|s| s.ended = Some(entry));
        h.frame();
        assert_eq!(h.entry(), (entry, "a"));
        let commands = h.commands();
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, Command::Seek(to) if *to == 0.0))
        );
        assert_eq!(plays(&commands), 0);
    }

    #[test]
    fn more_songs_are_asked_for_once_per_last_song() {
        let mut h = Harness::new();
        h.play(&["a"]);
        h.ready();
        let asked = h
            .requests()
            .iter()
            .filter(|r| matches!(r, Request::UpNext { video_id, .. } if video_id == "a"))
            .count();
        assert_eq!(asked, 1);
    }

    #[test]
    fn a_late_answer_does_not_undo_the_users_like() {
        let mut h = Harness::new();
        h.act(Action::Rate("a".into(), LikeState::Liked));
        h.answer(Event::Liked("a".into(), LikeState::Neutral));
        assert_eq!(h.app.likes.get("a"), Some(&LikeState::Liked));
    }

    #[test]
    fn a_refused_like_is_taken_back_unless_a_newer_choice_stands() {
        let mut h = Harness::new();
        let rate = |like| Edit::Rate {
            video_id: "a".into(),
            like,
        };
        h.act(Action::Rate("a".into(), LikeState::Liked));
        h.act(Action::Rate("a".into(), LikeState::Disliked));
        // The first change is refused: the second still stands.
        h.answer(Event::EditFailed(rate(LikeState::Liked), "no".into()));
        assert_eq!(h.app.likes.get("a"), Some(&LikeState::Disliked));
        // The second is refused too: back to as it was before both.
        h.answer(Event::EditFailed(rate(LikeState::Disliked), "no".into()));
        assert_eq!(h.app.likes.get("a"), None);
    }

    #[test]
    fn suggestions_for_older_text_are_not_shown() {
        let mut h = Harness::new();
        h.act(Action::Suggest("fad".into()));
        h.act(Action::Suggest("faded".into()));
        let words = |w: &str| ytfast_core::read::Suggestions {
            words: vec![w.into()],
            items: Vec::new(),
        };
        h.answer(Event::Suggestions("faded".into(), words("faded")));
        h.answer(Event::Suggestions("fad".into(), words("fade")));
        assert_eq!(h.app.suggestions.0, "faded");
        assert_eq!(h.app.suggestions.1.words, ["faded"]);
    }

    #[test]
    fn an_older_loading_of_a_page_is_dropped() {
        let mut h = Harness::new();
        h.app.load(Route::Home);
        h.app.load(Route::Home);
        let loads: Vec<u64> = h
            .requests()
            .into_iter()
            .filter_map(|r| match r {
                Request::Page { load, .. } => Some(load),
                _ => None,
            })
            .collect();
        let [older, newer] = loads[..] else {
            panic!("two loadings: {loads:?}");
        };
        h.answer(Event::Page(Route::Home, older, Err("old".into())));
        assert!(matches!(
            h.app.pages.get(&Route::Home),
            Some(Loadable::Loading)
        ));
        h.answer(Event::Page(Route::Home, newer, Err("new".into())));
        assert!(matches!(
            h.app.pages.get(&Route::Home),
            Some(Loadable::Failed(message)) if message == "new"
        ));
    }

    #[test]
    fn pages_kept_are_limited_but_the_sidebars_stay() {
        let mut h = Harness::new();
        for route in [Route::Home, Route::Library, Route::Liked] {
            h.app.load(route);
        }
        for n in 0..(MAX_PAGES + 10) {
            h.app.load(Route::browse(format!("VLtest{n}"), None));
        }
        assert!(h.app.pages.len() <= MAX_PAGES);
        for route in [Route::Home, Route::Library, Route::Liked] {
            assert!(h.app.pages.contains_key(&route), "{route:?} kept");
        }
    }
}
