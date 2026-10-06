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
        }
    }
}

pub enum Auth {
    Choosing { browser: Browser },
    Working { browser: Browser, progress: String },
    SignedIn { name: String },
    Failed { browser: Browser, message: String },
}

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
    /// Queue edits, by entry.
    RemoveFromQueue(u64),
    ShiftInQueue(u64, bool),
    MoveNextInQueue(u64),
    ToggleQueue,
    /// Open or close the player page.
    ToggleNowPlaying,
    CloseNowPlaying,
    NowPlayingTab(NpTab),
    /// Like, dislike, or neither.
    Rate(String, LikeState),
    /// Put the songs after the current one in a random order.
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
    /// The audio thread's latest status, read once per frame.
    pub audio_status: Status,
    pub show_queue: bool,
    /// A short message shown above the player bar.
    pub notice: Option<(String, Instant)>,
    pub actions: RefCell<Vec<Action>>,
    /// Songs already asked for ahead of time.
    warmed: RefCell<std::collections::HashSet<String>>,
    /// The playing song's cover colours (backdrop, accent).
    pub colors: Option<crate::colors::Summary>,
    pub backdrop: RefCell<Backdrop>,
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
    pub suggestions: (String, Vec<String>),
}

/// The backdrop's textures: the current song's, and the one fading out.
#[derive(Default)]
pub struct Backdrop {
    pub current: Option<(String, egui::TextureHandle)>,
    pub previous: Option<(String, egui::TextureHandle)>,
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
        crate::theme::install(&cc.egui_ctx);
        let settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, "ytfast"))
            .unwrap_or_default();

        let ctx = cc.egui_ctx.clone();
        let wake = move || ctx.request_repaint();
        let backend = Backend::start(wake.clone(), demo).expect("YtFast's folders can be made");
        let audio = Audio::start(wake.clone(), demo);
        audio.send(Command::Volume(settings.volume));
        backend.send(Request::FastWay(settings.fast_way));

        let mut info = now_playing::App::new("ytfast", "YTFast");
        info.can_raise = true;
        let controls = Some(now_playing::NowPlaying::start(info, wake));

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
            audio_status: Status::default(),
            show_queue: false,
            notice: None,
            actions: RefCell::new(Vec::new()),
            warmed: RefCell::new(std::collections::HashSet::new()),
            colors: None,
            backdrop: RefCell::new(Backdrop::default()),
            now_playing: false,
            np_tab: NpTab::Lyrics,
            lyrics: HashMap::new(),
            related: HashMap::new(),
            likes: HashMap::new(),
            saved: HashMap::new(),
            subscribed: HashMap::new(),
            dialog: RefCell::new(None),
            suggestions: (String::new(), Vec::new()),
        }
    }

    /// Queues an action for after this frame.
    pub fn act(&self, action: Action) {
        self.actions.borrow_mut().push(action);
    }

    /// The account's playlists (ID without `VL`, title), from the
    /// Library, for "Add to playlist".
    pub fn own_playlists(&self) -> Vec<(String, String)> {
        let Some(Loadable::Ready(page)) = self.pages.get(&Route::Library) else {
            return Vec::new();
        };
        page.sections
            .iter()
            .flat_map(|s| &s.items)
            .filter_map(|item| match item {
                ytfast_core::read::Item::Card(card) => match &card.open {
                    Some(Target::Browse { id, .. }) => {
                        let id = id.strip_prefix("VL")?;
                        // Liked Music and Episodes are not edited this way.
                        (id != "LM" && id != "SE").then(|| (id.to_string(), card.title.clone()))
                    }
                    _ => None,
                },
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

    /// The accent colour: from the playing song's cover, or the default.
    pub fn accent(&self) -> egui::Color32 {
        self.colors
            .as_ref()
            .map_or(crate::theme::PALETTE.accent, |c| c.accent)
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
                    duration: (self.audio_status.length > 0.0)
                        .then_some(self.audio_status.length)
                        .or(track.duration_seconds.map(f64::from)),
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

    /// Reads the playing song's cover colours, and makes its backdrop.
    fn update_colors(&mut self, ctx: &egui::Context) {
        let cover = self
            .playback
            .entry
            .as_ref()
            .and_then(|e| e.track.thumbnail.as_ref())
            .map(|t| t.sized(120));
        let Some(cover) = cover else { return };
        let Some(colors) = self.images.get_mut().summary(&cover, &self.backend) else {
            return;
        };
        let backdrop = self.backdrop.get_mut();
        if backdrop.current.as_ref().map(|c| &c.0) != Some(&cover) {
            let texture = ctx.load_texture(
                format!("backdrop {cover}"),
                colors.tiny.clone(),
                egui::TextureOptions::LINEAR,
            );
            backdrop.previous = backdrop.current.take();
            backdrop.current = Some((cover, texture));
        }
        self.colors = Some(colors);
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

    /// The cover texture for a picture address, when it has arrived.
    pub fn picture(&self, url: &str) -> Option<egui::TextureHandle> {
        self.images.borrow_mut().get(url, &self.backend)
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    // ---- Events from the backend and the audio thread ----

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Some(event) = self.backend.try_recv() {
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
                Event::Page(route, result) => {
                    // The top search result is the likeliest song to be
                    // played next.
                    if let (Route::Search(_), Ok(page)) = (&route, &result)
                        && let Some(top) = page.tracks().first()
                    {
                        self.warm(&top.video_id);
                    }
                    let loaded = match result {
                        Ok(page) => Loadable::Ready(page),
                        Err(message) => Loadable::Failed(message),
                    };
                    // A page let go while it loaded is not wanted any more.
                    if let Some(slot) = self.pages.get_mut(&route) {
                        *slot = loaded;
                    }
                }
                Event::Prepared { entry, result } => self.prepared(entry, result),
                Event::UpNext { video_id, result } => self.more_arrived(&video_id, result),
                Event::PlaylistQueue {
                    playlist_id,
                    mode,
                    result,
                } => match result {
                    Ok(tracks) if !tracks.is_empty() => match mode {
                        QueueMode::Play => self.play_tracks(tracks, 0, Some(playlist_id)),
                        QueueMode::Shuffle => self.apply(Action::Shuffle {
                            tracks,
                            source: Some(playlist_id),
                        }),
                        QueueMode::Next => {
                            let count = tracks.len();
                            for track in tracks.into_iter().rev() {
                                self.queue.play_next(track);
                            }
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
                },
                Event::Suggestions(text, found) => self.suggestions = (text, found),
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
                    match &change {
                        Edit::Rate { video_id, .. } => {
                            self.likes.remove(video_id);
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
                Event::Edited(change) => match change {
                    // The library and the changed playlist show the change.
                    Edit::CreatePlaylist { title, .. } => {
                        self.notify(format!("Made the playlist {title}"));
                        self.load(Route::Library);
                    }
                    Edit::RenamePlaylist { .. }
                    | Edit::DeletePlaylist { .. }
                    | Edit::Save { .. } => self.load(Route::Library),
                    Edit::AddToPlaylist { playlist_id, .. } => {
                        self.reload_playlist(&playlist_id);
                    }
                    _ => {}
                },
                Event::MoreRows { route, tracks } => {
                    if let Some(Loadable::Ready(page)) = self.pages.get_mut(&route) {
                        add_rows(page, tracks);
                    }
                }
                Event::Image(url, picture) => self.images.borrow_mut().arrived(ctx, url, picture),
            }
        }

        self.audio_status = self.audio.status();
        if let Some(ended) = self.audio.take_ended()
            && self.playback.entry.as_ref().is_some_and(|e| e.id == ended)
        {
            if self.settings.repeat == Repeat::One {
                self.play_again();
            } else {
                self.next();
            }
        }
    }

    fn prepared(
        &mut self,
        entry: u64,
        result: Result<crate::backend::Ready, crate::backend::Failure>,
    ) {
        let Some(current) = self.playback.entry.clone().filter(|e| e.id == entry) else {
            // An answer about a song no longer wanted.
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
                // Get the next song ready while this one plays.
                self.prepare_next();
                // With the queue on repeat, it plays again instead.
                if self.queue.remaining() <= 1 && self.settings.repeat != Repeat::All {
                    self.ask_for_more();
                }
            }
            Err(failure) if failure.song_only => {
                self.notify(format!(
                    "Skipped \"{}\": {}",
                    current.track.title, failure.message
                ));
                self.next();
            }
            Err(failure) => {
                self.playback.state = PlayState::Failed(failure.message);
            }
        }
    }

    fn more_arrived(&mut self, video_id: &str, result: Result<Vec<Track>, String>) {
        let tracks = match result {
            Ok(tracks) => tracks,
            Err(e) => {
                log::warn!("Up next for {video_id} failed: {e}");
                return;
            }
        };
        let added = self.queue.append(tracks);
        if added > 0 && self.playback.state == PlayState::WaitingForMore {
            self.next();
        }
    }

    // ---- Playing ----

    fn play_tracks(&mut self, tracks: Vec<Track>, start: usize, source: Option<String>) {
        if let Some(entry) = self.queue.replace(tracks, start, source).cloned() {
            self.playback.asked_more_for = None;
            self.start(entry);
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
        if let Some(report) = self.playback.report.take()
            && let Some(url) = report.listened(0.0, self.audio_status.position)
        {
            self.backend.send(Request::Report(url));
        }
    }

    fn next(&mut self) {
        match self.queue.advance().cloned() {
            Some(entry) => self.start(entry),
            // A queue of one song, on repeat: no need to fetch it again.
            None if self.settings.repeat == Repeat::All && self.queue.entries().len() == 1 => {
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
            self.audio.send(Command::Seek(0.0));
        } else if let Some(entry) = self.queue.back().cloned() {
            self.start(entry);
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
            _ => {}
        }
    }

    // ---- Pages ----

    pub fn load(&mut self, route: Route) {
        self.touch(&route);
        self.pages.insert(route.clone(), Loadable::Loading);
        self.backend.send(Request::Page(route));
        self.trim_pages();
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
    /// here (never loaded, let go, or failed).
    fn show_current(&mut self) {
        let route = self.route.clone();
        if route.is_local() {
            return;
        }
        match self.pages.get(&route) {
            None | Some(Loadable::Failed(_)) => self.load(route),
            Some(_) => self.touch(&route),
        }
    }

    fn navigate(&mut self, route: Route) {
        if route == self.route {
            return;
        }
        let old = std::mem::replace(&mut self.route, route);
        self.back.push(old);
        self.forward.clear();
        self.show_current();
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Opening a card: a page, or playing it.
    fn open(&mut self, target: Target, track: Option<Track>) {
        match target {
            Target::Browse { id, params, .. } => self.navigate(Route::browse(id, params)),
            watch @ Target::Watch { .. } => self.play(watch, track),
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
                self.backend.send(Request::PlaylistQueue {
                    playlist_id,
                    mode: QueueMode::Play,
                });
            }
            Target::Watch { .. } => {}
            Target::Browse { id, params, .. } => self.navigate(Route::browse(id, params)),
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Navigate(route) => self.navigate(route),
            Action::Back => {
                if let Some(route) = self.back.pop() {
                    let old = std::mem::replace(&mut self.route, route);
                    self.forward.push(old);
                    self.show_current();
                }
            }
            Action::Forward => {
                if let Some(route) = self.forward.pop() {
                    let old = std::mem::replace(&mut self.route, route);
                    self.back.push(old);
                    self.show_current();
                }
            }
            Action::Search(query) => {
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
                tracks.shuffle(&mut rand::rng());
                self.play_tracks(tracks, 0, source);
            }
            Action::Open(target, track) => self.open(target, track),
            Action::Play(target, track) => self.play(target, track),
            Action::TogglePause => self.toggle_pause(),
            Action::Next => self.next(),
            Action::Previous => self.previous(),
            Action::Seek(to) => {
                self.audio.send(Command::Seek(to));
                if let Some(controls) = &self.controls {
                    controls.seeked(Duration::from_secs_f64(to.max(0.0)));
                }
            }
            Action::SetVolume(volume) => {
                self.settings.volume = volume;
                self.audio.send(Command::Volume(volume));
            }
            Action::JumpTo(id) => {
                if let Some(entry) = self.queue.jump(id).cloned() {
                    self.start(entry);
                }
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
            Action::ToggleQueue => self.show_queue = !self.show_queue,
            Action::ToggleNowPlaying => self.now_playing = !self.now_playing,
            Action::CloseNowPlaying => self.now_playing = false,
            Action::NowPlayingTab(tab) => self.np_tab = tab,
            Action::Rate(video_id, like) => {
                // Shown at once; YouTube is told in the background.
                self.likes.insert(video_id.clone(), like);
                self.backend
                    .send(Request::Edit(Edit::Rate { video_id, like }));
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
                self.backend.send(Request::Edit(Edit::AddToPlaylist {
                    playlist_id,
                    video_id,
                }));
            }
            Action::Edit(change) => self.backend.send(Request::Edit(change)),
            Action::Suggest(text) => self.backend.send(Request::Suggest(text)),
            Action::QueuePlaylist(playlist_id, mode) => {
                self.backend
                    .send(Request::PlaylistQueue { playlist_id, mode });
            }
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
                self.backend
                    .send(Request::Edit(Edit::RenamePlaylist { playlist_id, name }));
            }
            Action::DeletePlaylist(playlist_id) => {
                self.notify("Deleted the playlist");
                // Away from its page.
                if matches!(&self.route, Route::Browse { id, .. } if id.trim_start_matches("VL") == playlist_id)
                {
                    self.route = Route::Library;
                    self.show_current();
                }
                self.backend
                    .send(Request::Edit(Edit::DeletePlaylist { playlist_id }));
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
                self.backend.send(Request::Edit(Edit::RemoveFromPlaylist {
                    playlist_id,
                    video_id,
                    set_video_id,
                }));
            }
            Action::ToggleSave { playlist_id, save } => {
                self.saved.insert(playlist_id.clone(), save);
                self.notify(if save {
                    "Saved to your library"
                } else {
                    "Removed from your library"
                });
                self.backend
                    .send(Request::Edit(Edit::Save { playlist_id, save }));
            }
            Action::ToggleSubscribe {
                channel_id,
                subscribe,
            } => {
                self.subscribed.insert(channel_id.clone(), subscribe);
                self.backend.send(Request::Edit(Edit::Subscribe {
                    channel_id,
                    subscribe,
                }));
            }
            Action::OpenLogFolder => {
                if let Some(dirs) = directories::ProjectDirs::from("", "", "YtFast") {
                    open_folder(dirs.cache_dir());
                }
            }
            Action::ShuffleQueue => {
                self.queue.shuffle_upcoming();
                self.prepare_next();
                self.notify("Shuffled the songs coming up");
            }
            Action::CycleRepeat => {
                self.settings.repeat = match self.settings.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
            }
            Action::ChooseBrowser(browser) => {
                if let Auth::Choosing { browser: chosen }
                | Auth::Failed {
                    browser: chosen, ..
                } = &mut self.auth
                {
                    *chosen = browser;
                }
            }
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
                self.finish_report();
                self.audio.send(Command::Stop);
                self.queue.clear();
                self.playback = Playback::default();
                self.pages.clear();
                self.visited.clear();
                self.settings.browser = None;
                self.backend.send(Request::SignOut);
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
        let typing = ctx.egui_wants_keyboard_input();
        if self.now_playing
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.act(Action::CloseNowPlaying);
        }
        let (play, search) = ctx.input_mut(|i| {
            let play = !typing && i.consume_key(egui::Modifiers::NONE, egui::Key::Space);
            let search = i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)
                || (!typing && i.consume_key(egui::Modifiers::NONE, egui::Key::Slash));
            (play, search)
        });
        if play {
            self.act(Action::TogglePause);
        }
        if !typing {
            use egui::{Key, Modifiers};
            let pressed = |key, modifiers| ctx.input_mut(|i| i.consume_key(modifiers, key));
            let position = self.audio_status.position;
            let volume = self.settings.volume;
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
            if pressed(Key::N, Modifiers::SHIFT) {
                self.act(Action::Next);
            }
            if pressed(Key::P, Modifiers::SHIFT) {
                self.act(Action::Previous);
            }
            if pressed(Key::M, Modifiers::NONE) {
                self.act(Action::SetVolume(if volume > 0.0 { 0.0 } else { 0.8 }));
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
                now_playing::Command::SeekBy(ms) => Some(Action::Seek(
                    self.audio_status.position + ms as f64 / 1000.0,
                )),
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
            position: Duration::from_secs_f64(self.audio_status.position.max(0.0)),
            volume: Some(f64::from(self.settings.volume)),
            ..now_playing::State::default()
        };
        if let Some(controls) = &mut self.controls {
            controls.update(state);
        }
    }
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

/// How long a notice shows.
const NOTICE_TIME: Duration = Duration::from_secs(4);

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.scrolling.apply(&ctx);
        self.images.get_mut().begin_frame();
        self.handle_events(&ctx);
        self.media_controls();
        self.shortcuts(&ctx);
        self.update_colors(&ctx);
        self.want_song_extras();

        views::show(self, ui);

        let actions = std::mem::take(self.actions.get_mut());
        for action in actions {
            self.apply(action);
        }
        // A notice shows for a few seconds.
        if let Some((_, at)) = &self.notice {
            let left = NOTICE_TIME.saturating_sub(at.elapsed());
            if left.is_zero() {
                self.notice = None;
            } else {
                ctx.request_repaint_after(left);
            }
        }
        // Keep the progress bar moving while a song plays.
        if self.playback.state == PlayState::Playing && !self.audio_status.paused {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "ytfast", &self.settings);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.finish_report();
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
