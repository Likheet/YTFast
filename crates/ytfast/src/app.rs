//! The app's state, and what every click does. Views (in `views/`) draw
//! from a shared borrow of [`App`] and push [`Action`]s; the actions are
//! applied after drawing, so nothing changes under a view mid-frame.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use fastframe_now_playing as now_playing;
use serde::{Deserialize, Serialize};
use ytfast_core::library::Privacy;
use ytfast_core::playreport::PlayReport;
use ytfast_core::read::{
    Item, Page, PlayerInfo, Section, SortOrder, Target, Track, TrackKind, VideoPair,
};
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
    /// The order chosen for each of the Library's tabs: its page ID, and
    /// its sort button's `params` for that order.
    pub library_order: BTreeMap<String, String>,
    /// The look the window wears (Settings, Theme).
    #[serde(serialize_with = "crate::dynamic::save_theme")]
    pub theme: crate::theme::Theme,
    /// The Dynamic Background theme's colours move while a song plays
    /// (off: they stay still).
    pub moving_background: bool,
    /// The Dynamic Background theme is worn. It is saved here, `theme`
    /// saying YouTube Music meanwhile, so that a build without that theme
    /// still reads these settings (it skips what it does not know) instead
    /// of forgetting them all. Set when saved; read by [`Settings::loaded`].
    pub dynamic_background: bool,
    /// Download new versions in the background and install them on a
    /// restart (`update`).
    pub auto_update: bool,
    /// Disliking the song playing moves on to the next, as YouTube Music
    /// does.
    pub skip_disliked: bool,
    /// Closing the window while a song plays asks first
    /// ([`Dialog::ConfirmClose`]).
    pub confirm_close: bool,
}

impl Settings {
    /// The settings as read from disk, with the Dynamic Background theme
    /// back in `theme` when it was the one worn.
    pub fn loaded(mut self) -> Self {
        if self.dynamic_background {
            self.theme = crate::theme::Theme::DynamicBackground;
        }
        self
    }

    /// Ready to save: `dynamic_background` says whether that theme is worn.
    fn for_saving(&mut self) -> &Self {
        self.dynamic_background = self.theme == crate::theme::Theme::DynamicBackground;
        self
    }
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
            library_order: BTreeMap::new(),
            theme: crate::theme::Theme::default(),
            moving_background: true,
            dynamic_background: false,
            auto_update: true,
            skip_disliked: true,
            confirm_close: true,
        }
    }
}

/// What the updater left for this run: why an update was undone, and the
/// receipt a freshly installed version gives back once its window is up.
pub struct Startup {
    pub update_error: Option<String>,
    pub receipt: Option<fastframe_update::Receipt>,
}

pub enum Auth {
    Choosing {
        browser: Browser,
    },
    Working {
        browser: Browser,
        progress: String,
    },
    SignedIn {
        name: String,
        /// "@handle", and the account's photo's address.
        handle: Option<String>,
        photo: Option<String>,
    },
    Failed {
        browser: Browser,
        message: String,
    },
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
    /// The ID being got ready or played: the song's own, or in the video
    /// mode its video's (see [`App::version_of`]); `None` while the video
    /// mode waits to hear whether it has one.
    pub version: Option<String>,
    /// Where it plays from, in seconds: its start, or where the other
    /// version was when the video mode was turned on or off.
    pub start_at: f64,
}

/// The video mode's picture for the song playing (a queue entry).
pub struct VideoShow {
    pub entry: u64,
    pub state: VideoState,
}

/// Where the video mode's picture is.
pub enum VideoState {
    /// Waiting to hear whether the song has a video (its details).
    Finding,
    /// The video's sound is getting ready; its picture is asked for once
    /// it plays.
    Coming,
    /// The picture is on its way.
    Loading,
    /// Showing: the pictures, and the one last put on screen (its time,
    /// and the texture holding it).
    Playing {
        video: Box<ytfast_core::video::Video>,
        shown: RefCell<Option<(f64, egui::TextureHandle)>>,
    },
    /// The demo's stand-in for a video.
    Demo,
    /// The song has no video: its cover shows.
    Missing,
    Failed(String),
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
            version: None,
            start_at: 0.0,
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
    MovingBackground,
    AutoUpdate,
    SkipDisliked,
    ConfirmClose,
}

/// A small window asking one thing.
#[derive(Clone, Debug)]
pub enum Dialog {
    /// A new playlist's name, description and privacy; `songs` are added
    /// to it when made.
    NewPlaylist {
        name: String,
        description: String,
        privacy: Privacy,
        songs: Vec<String>,
    },
    /// Editing one of the account's own playlists (YouTube Music's Edit
    /// playlist).
    Rename {
        playlist_id: String,
        name: String,
        description: String,
        privacy: Privacy,
    },
    Delete {
        playlist_id: String,
        title: String,
    },
    /// "Save to playlist": the account's playlists, to add songs to one,
    /// or a new playlist.
    SaveToPlaylist {
        video_ids: Vec<String>,
    },
    /// An album's or playlist's whole description.
    Description {
        title: String,
        text: String,
    },
    /// The keyboard's shortcuts ("?").
    Shortcuts,
    /// The update window: the new version, its download, and Restart to
    /// update (as Spotifast's).
    Update,
    /// "Do you really want to close? There's a song playing.", with "Do
    /// not ask again" (ticked as it opens).
    ConfirmClose {
        dont_ask: bool,
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
    /// Tick a song's box on a page, or untick it.
    ToggleSelected(Track),
    /// Untick every song.
    ClearSelected,
    /// The songs ticked, after the current one, in their order.
    PlayNextAll(Vec<Track>),
    /// The songs ticked, at the end of the queue.
    AddToQueueAll(Vec<Track>),
    /// A playlist's songs in another order (its Sort menu); `None` is
    /// its own order, as YouTube keeps it.
    SortPlaylist(Route, Option<PlaylistSort>),
    /// Add songs to one of the account's playlists.
    AddToPlaylist {
        playlist_id: String,
        title: String,
        video_ids: Vec<String>,
    },
    /// Any other change to the account.
    Edit(Edit),
    /// Ask for search suggestions (with nothing typed, the past searches).
    Suggest(String),
    /// The end of a search of one kind is in view: its next results.
    MoreResults(Route),
    /// Show a Library tab in another order (one its sort button offers).
    SortLibrary(SortOrder),
    /// Remove a past search from the account's search history: its words,
    /// and the token its suggestion carried.
    ForgetSearch {
        words: String,
        token: String,
    },
    /// A playlist's or album's songs: play, shuffle, or queue them.
    QueuePlaylist(String, QueueMode),
    Toggle(Setting),
    /// Wear another look (Settings, Theme).
    SetTheme(crate::theme::Theme),
    /// Look for a new version now (Settings).
    CheckForUpdates,
    /// Download the new version found (when installs are not automatic).
    InstallUpdate,
    /// Install the downloaded version: the window closes and the new
    /// version opens.
    RestartToUpdate,
    /// Open the update window (the top bar's update badge).
    ShowUpdate,
    /// Close the window after all (the close question's Yes); with
    /// `dont_ask`, never ask again (Settings can ask again).
    ConfirmClose {
        dont_ask: bool,
    },
    OpenDialog(Dialog),
    RenamePlaylist {
        playlist_id: String,
        name: String,
        description: String,
        privacy: Privacy,
    },
    DeletePlaylist(String),
    /// Take a row out of the playlist shown (by its own row ID).
    /// Take songs out of a playlist: each its song ID and its row's own
    /// (`set_video_id`).
    RemoveFromPlaylist {
        playlist_id: String,
        songs: Vec<(String, String)>,
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
    /// Open the newest release's page in the browser, where a Mac copy
    /// gets a new version (it cannot install one itself).
    OpenDownloadPage,
    /// Queue edits, by entry. `MoveInQueue` puts it at a place (a row of
    /// Up next dragged there).
    RemoveFromQueue(u64),
    MoveInQueue(u64, usize),
    MoveNextInQueue(u64),
    /// Open the menu on the left, or close it to its icons.
    ToggleGuide,
    /// Open or close the player page.
    ToggleNowPlaying,
    /// The window to the whole screen with the player page, larger (F),
    /// or back.
    ToggleFullscreen,
    CloseNowPlaying,
    NowPlayingTab(NpTab),
    /// Like, dislike, or neither.
    Rate(String, LikeState),
    /// Shuffle on or off (a mode): on, the songs after the current one in
    /// a random order; off, back in the queue's own.
    ShuffleQueue,
    /// Repeat off, the queue, this song.
    CycleRepeat,
    /// The player page's Song and Video switch: the video mode on (the
    /// queue's songs play as their music videos) or off.
    VideoMode(bool),
    ChooseBrowser(Browser),
    SignIn,
    SignOut,
}

/// The orders a playlist's Sort menu offers besides its own. (YouTube
/// Music's also offers newest and oldest added first, which YTFast cannot
/// tell.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaylistSort {
    Title,
    Artist,
    Album,
}

impl PlaylistSort {
    pub const ALL: [Self; 3] = [Self::Title, Self::Artist, Self::Album];

    pub fn words(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Artist => "Artist",
            Self::Album => "Album",
        }
    }

    /// Puts `page`'s songs in this order (A to Z, as YouTube Music's).
    fn apply(self, page: &mut Page) {
        let key = |item: &Item| match item {
            Item::Track(t) => match self {
                Self::Title => t.title.to_lowercase(),
                Self::Artist => t.artists.to_lowercase(),
                Self::Album => t.album.clone().unwrap_or_default().to_lowercase(),
            },
            Item::Card(_) => String::new(),
        };
        for section in &mut page.sections {
            if section.items.iter().any(|i| matches!(i, Item::Track(_))) {
                section.items.sort_by_cached_key(key);
            }
        }
    }
}

pub struct App {
    pub backend: Backend,
    pub audio: Audio,
    /// The theme egui's own colours were last set for.
    styled: Option<crate::theme::Theme>,
    pub images: RefCell<Images>,
    pub scrolling: fastframe_scroll::Scrolling,
    pub controls: Option<now_playing::NowPlaying>,
    pub demo: bool,
    /// The updater (none in the demo).
    pub updates: Option<crate::update::Updates>,
    /// The newer version known this run (the top bar's update badge shows
    /// while there is one), and what the updater said last.
    pub update_found: Option<String>,
    update_seen: crate::update::State,
    /// "Check for updates" was pressed: its answer is said, even "up to
    /// date".
    update_manual: bool,
    /// A freshly installed version's receipt, given back after the first
    /// frame (else the helper puts the old version back).
    receipt: Option<fastframe_update::Receipt>,
    /// The window was asked to close for an update.
    restarting: bool,
    /// The close question was answered Yes: the window closes (once
    /// `close_now` has asked it to), without asking again.
    closing: bool,
    close_now: bool,
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
    warmed: RefCell<HashSet<String>>,
    /// The song row under the pointer (see [`App::resting_on`]).
    pointed: RefCell<Option<Pointed>>,
    /// The player page is open, and on which tab.
    pub now_playing: bool,
    pub np_tab: NpTab,
    /// The video mode: the queue's songs play as their music videos, shown
    /// on the player page. A song started anywhere but the queue turns it
    /// off (see [`App::play_tracks`]).
    pub video_mode: bool,
    /// The picture for the song playing, in the video mode.
    pub video: Option<VideoShow>,
    /// Songs' videos as their details said ([`Event::SongVideo`]).
    song_videos: HashMap<String, Option<VideoPair>>,
    /// The player page drew the video this frame; otherwise its decoding
    /// rests.
    pub video_drawn: std::cell::Cell<bool>,
    /// The window fills the screen with the player page (F): no top bar or
    /// menu, the cover or video larger. Leaving the player page leaves it.
    pub fullscreen: bool,
    /// Full screen to ask the window for, and when it last was (egui's
    /// clock): the window's own word on it is believed only a while after.
    fullscreen_wanted: Option<bool>,
    fullscreen_asked: f64,
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
    /// The songs ticked on the page shown, in the order they were ticked.
    pub selected: Vec<Track>,
    /// The order chosen for playlists in this run (their Sort menu).
    pub playlist_sort: HashMap<Route, PlaylistSort>,
    /// Past searches removed in this run (lowercase), kept out of answers
    /// asked for before the removal reached YouTube. One per click on a
    /// suggestion's bin, so it stays small.
    forgotten_searches: HashSet<String>,
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
    /// Pages whose next part (a search of one kind's results, Home's
    /// shelves) is on its way (true) or has all arrived (false); one entry
    /// per such page shown.
    pub more_results: HashMap<Route, bool>,
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
    /// A dialog just opened: its first field takes the keyboard.
    pub dialog_fresh: std::cell::Cell<bool>,
    /// Where this frame's album background goes: kept under the bars and
    /// the menu (`backdrop::paint`).
    pub backdrop_slot: std::cell::Cell<Option<egui::layers::ShapeIdx>>,
    /// The page drawn this frame has a background of its own (an album's
    /// cover, an artist's picture), so Premium's ambient colours stay off.
    pub page_backdrop: std::cell::Cell<bool>,
    /// The page drawn is the one just left, standing in while the page
    /// wanted loads (`page::stand_in`): its mood buttons light the one
    /// wanted, not their own.
    pub standing_in: std::cell::Cell<bool>,
    /// The cover behind an album's or playlist's page: its address, its
    /// middle band shrunk to a few pixels (drawn stretched, a blur), and
    /// when it was made (it fades in).
    pub page_cover: RefCell<Option<(String, egui::TextureHandle, f64)>>,
    /// Premium's wash of the playing song's cover (`listening_wash`).
    listening_wash: RefCell<Option<(String, egui::TextureHandle)>>,
    /// "/" just opened the search box: its character, arriving in the next
    /// frame, is not typed there.
    drop_slash: bool,
    /// When "g" was pressed (egui's clock): the next key goes to a page
    /// (h Home, e Explore, l Library, "," Settings), as YouTube Music's.
    go_to: Option<f64>,
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
    pub fn new(cc: &eframe::CreationContext<'_>, demo: bool, startup: Startup) -> Self {
        let script_fonts = crate::theme::install(&cc.egui_ctx);
        // The demo laid out as on another screen (`YTFAST_DEMO_ZOOM`): 0.67
        // shows a laptop's window as a 2560 wide screen's, smaller, for
        // pictures of large screens on a small one. Set every time, as the
        // demo's saved window state would otherwise keep the last one.
        if demo {
            let zoom = std::env::var("YTFAST_DEMO_ZOOM")
                .ok()
                .and_then(|z| z.parse::<f32>().ok())
                .filter(|z| (0.25..=4.0).contains(z))
                .unwrap_or(1.0);
            cc.egui_ctx.set_zoom_factor(zoom);
        }
        let settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value::<Settings>(s, "ytfast"))
            .map(Settings::loaded)
            .unwrap_or_default();

        let ctx = cc.egui_ctx.clone();
        let wake = move || ctx.request_repaint();
        let backend = Backend::start(wake.clone(), demo).expect("YtFast's folders can be made");
        let audio = Audio::start(wake.clone(), demo);

        let mut info = now_playing::App::new("ytfast", "YTFast");
        info.can_raise = true;
        let controls = Some(now_playing::NowPlaying::start(info, wake));
        let updates = if demo {
            // The demo looks for nothing; `YTFAST_DEMO_UPDATE` (downloading
            // or ready) shows the update badge and window as they would be.
            std::env::var("YTFAST_DEMO_UPDATE").ok().map(|kind| {
                let version = "0.6.1".to_string();
                crate::update::Updates::sample(if kind == "downloading" {
                    crate::update::State::Downloading {
                        version,
                        received: 9_400_000,
                        total: 20_100_000,
                    }
                } else {
                    crate::update::State::Ready { version }
                })
            })
        } else {
            let ctx = cc.egui_ctx.clone();
            Some(crate::update::Updates::start(
                settings.auto_update,
                move || ctx.request_repaint(),
            ))
        };
        let mut app = Self::with(backend, audio, controls, script_fonts, settings, demo);
        app.updates = updates;
        app.receipt = startup.receipt;
        if let Some(error) = startup.update_error {
            app.notify(error);
        }
        app
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
            updates: None,
            update_found: None,
            update_seen: crate::update::State::Idle,
            update_manual: false,
            receipt: None,
            restarting: false,
            closing: false,
            close_now: false,
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
            warmed: RefCell::new(HashSet::new()),
            pointed: RefCell::new(None),
            now_playing: false,
            np_tab: NpTab::UpNext,
            video_mode: false,
            video: None,
            song_videos: HashMap::new(),
            video_drawn: std::cell::Cell::new(false),
            fullscreen: false,
            fullscreen_wanted: None,
            fullscreen_asked: f64::NEG_INFINITY,
            lyrics: HashMap::new(),
            related: HashMap::new(),
            likes: HashMap::new(),
            saved: HashMap::new(),
            subscribed: HashMap::new(),
            dialog: RefCell::new(None),
            suggestions: (String::new(), Default::default()),
            selected: Vec::new(),
            playlist_sort: HashMap::new(),
            forgotten_searches: HashSet::new(),
            script_fonts,
            styled: None,
            loud_volume,
            edits_in_flight: HashMap::new(),
            like_before: HashMap::new(),
            window_checked: false,
            page_loads: HashMap::new(),
            more_results: HashMap::new(),
            loads: 0,
            suggesting: String::new(),
            seeking: None,
            fresh_page: std::cell::Cell::new(false),
            page_scrolled: std::cell::Cell::new(false),
            page_offset: std::cell::Cell::new(0.0),
            dialog_fresh: std::cell::Cell::new(false),
            backdrop_slot: std::cell::Cell::new(None),
            page_backdrop: std::cell::Cell::new(false),
            standing_in: std::cell::Cell::new(false),
            page_cover: RefCell::new(None),
            listening_wash: RefCell::new(None),
            drop_slash: false,
            go_to: None,
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
        // Full screen is the player page's: leaving it leaves full screen.
        if self.fullscreen && !self.now_playing {
            self.set_fullscreen(false);
        }
    }

    /// Full screen on (the player page with it) or off.
    fn set_fullscreen(&mut self, on: bool) {
        if on == self.fullscreen {
            return;
        }
        self.fullscreen = on;
        self.fullscreen_wanted = Some(on);
        if on {
            self.now_playing = true;
            self.notify("Full screen: press F or Esc to leave");
        }
    }

    /// Asks the window for full screen when it was chosen; and when the
    /// window left it by itself (a Mac's own Esc or green button), the
    /// page is laid out as before.
    fn sync_fullscreen(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if let Some(on) = self.fullscreen_wanted.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
            self.fullscreen_asked = now;
            return;
        }
        let actual = ctx.input(|i| i.viewport().fullscreen);
        if self.fullscreen && actual == Some(false) && now - self.fullscreen_asked > 1.5 {
            self.fullscreen = false;
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
            Auth::SignedIn { name, .. } if !name.is_empty() => Some(name.as_str()),
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
                // (until then it is the last song's), as itself (not its
                // video, in the video mode).
                let playing_it = self.audio_status.entry == Some(entry.id)
                    && self.playback.version.as_deref() == Some(track.video_id.as_str());
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

    /// The playing song's cover as Premium's player page wears it behind
    /// the whole window (`colors::wash`), once the cover has arrived. One
    /// is kept, for the song playing.
    pub fn listening_wash(&self, ctx: &egui::Context) -> Option<egui::TextureId> {
        let thumb = self.playback.entry.as_ref()?.track.thumbnail.as_ref()?;
        let url = thumb.sized(120);
        let mut wash = self.listening_wash.borrow_mut();
        if let Some((made_for, texture)) = wash.as_ref()
            && *made_for == url
        {
            return Some(texture.id());
        }
        let summary = self.images.borrow_mut().summary(&url, &self.backend)?;
        let texture = ctx.load_texture(
            format!("listening wash {url}"),
            crate::colors::wash(&summary.small),
            egui::TextureOptions::LINEAR,
        );
        let id = texture.id();
        *wash = Some((url, texture));
        Some(id)
    }

    /// Says what the updater found, as Spotifast does: "YTFast 0.6.1 is
    /// available" once per version (the badge then stays in the top bar),
    /// and after "Check for updates", that YTFast is up to date or could
    /// not look.
    fn watch_updates(&mut self) {
        use crate::update::State;
        let Some(state) = self.updates.as_ref().map(crate::update::Updates::state) else {
            return;
        };
        if state == self.update_seen {
            return;
        }
        self.update_seen = state.clone();
        match state {
            State::Available { version, .. }
            | State::Downloading { version, .. }
            | State::Ready { version } => {
                if self.update_found.as_deref() != Some(version.as_str()) {
                    self.notify(format!("YTFast {version} is available"));
                    self.update_found = Some(version);
                }
            }
            State::UpToDate => {
                self.update_found = None;
                if std::mem::take(&mut self.update_manual) {
                    self.notify("YTFast is up to date");
                }
            }
            State::Failed(message) => {
                if std::mem::take(&mut self.update_manual) {
                    self.notify(message);
                }
            }
            State::Idle | State::Checking | State::Restarting => {}
        }
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
                Event::SignedIn {
                    name,
                    handle,
                    photo,
                } => {
                    if let Auth::Working { browser, .. } = &self.auth
                        && !self.demo
                    {
                        self.settings.browser = Some(browser.label().to_string());
                    }
                    self.auth = Auth::SignedIn {
                        name,
                        handle,
                        photo,
                    };
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
                        Ok(mut page) => {
                            // In the order chosen for it.
                            if let Some(order) = self.playlist_sort.get(&route) {
                                order.apply(&mut page);
                            }
                            Loadable::Ready(page)
                        }
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
                Event::Suggestions(text, mut found) if text == self.suggesting => {
                    let forgotten = &self.forgotten_searches;
                    found.words.retain(|w| {
                        w.forget.is_none() || !forgotten.contains(&w.text.to_lowercase())
                    });
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
                        // It shows again the next time it is suggested.
                        Edit::ForgetSearch { words, .. } => {
                            self.forgotten_searches.remove(&words.to_lowercase());
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
                            self.refresh_library();
                        }
                        Edit::RenamePlaylist { .. } | Edit::DeletePlaylist { .. } => {
                            self.refresh_library();
                        }
                        // An album saved shows in Library > Albums and its
                        // songs in Library > Songs too.
                        Edit::Save { .. } => {
                            for route in [
                                Route::Library,
                                Route::LibraryRecent,
                                Route::LibraryAlbums,
                                Route::LibrarySongs,
                            ] {
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
                Event::MoreResults {
                    route,
                    load,
                    items,
                    done,
                } => {
                    if self.page_loads.get(&route) != Some(&load) {
                        continue;
                    }
                    if let Some(Loadable::Ready(page)) = self.pages.get_mut(&route) {
                        page.extend_list(items);
                    }
                    // Asked for again when the list's new end comes into
                    // view, unless that was all.
                    if done {
                        self.more_results.insert(route, false);
                    } else {
                        self.more_results.remove(&route);
                    }
                }
                Event::MoreSections {
                    route,
                    load,
                    sections,
                    done,
                } => {
                    if self.page_loads.get(&route) != Some(&load) {
                        continue;
                    }
                    if let Some(Loadable::Ready(page)) = self.pages.get_mut(&route) {
                        page.sections.extend(sections);
                    }
                    if done {
                        self.more_results.insert(route, false);
                    } else {
                        self.more_results.remove(&route);
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
                        if let Some(order) = self.playlist_sort.get(&route) {
                            order.apply(page);
                        }
                    }
                }
                Event::SongVideo(song, found) => self.song_video(song, found),
                Event::Video { entry, result } => {
                    let Some(show) = self
                        .video
                        .as_mut()
                        .filter(|v| v.entry == entry && matches!(v.state, VideoState::Loading))
                    else {
                        continue;
                    };
                    show.state = match result {
                        Ok(Some(data)) => VideoState::Playing {
                            video: Box::new(ytfast_core::video::Video::start(data)),
                            shown: RefCell::new(None),
                        },
                        Ok(None) => VideoState::Demo,
                        Err(message) => {
                            log::warn!("the video did not load: {message}");
                            VideoState::Failed(message)
                        }
                    };
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
        // The video mode was turned on or off while it got ready, or its
        // video became known: the version now wanted instead. (One still
        // unknown takes what is ready; it changes once known.)
        if let Some(wanted) = self.version_of(&current.track)
            && self.playback.version.as_ref() != Some(&wanted)
        {
            let from = self.playback.start_at;
            self.start_from(current, from);
            return;
        }
        let own = self.playback.version.as_deref() == Some(current.track.video_id.as_str());
        match result {
            Ok(ready) => {
                // The player's answer names the song, for one started by
                // its ID alone (its cover comes with Up next).
                let info = &ready.info;
                let heard = Track {
                    video_id: current.track.video_id.clone(),
                    title: info.title.clone().unwrap_or_default(),
                    artists: info
                        .author
                        .as_deref()
                        .map(|a| a.trim_end_matches(" - Topic").to_string())
                        .unwrap_or_default(),
                    duration_seconds: info.length_seconds,
                    kind: info.kind.clone(),
                    ..Track::default()
                };
                // (Its video's answer names the video.)
                if own {
                    self.fill_in(&heard);
                }
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
                    from: self.playback.start_at,
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
                if let Some(url) = report.started(self.playback.start_at) {
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
                // In the video mode, its picture.
                self.ask_for_picture();
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
        self.video = None;
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
                // Up next lists the song playing too, in full: a song
                // started by its ID alone gets its name and cover here.
                for track in &tracks {
                    self.fill_in(track);
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
            // Chosen anywhere but the queue: a song again, not its video.
            self.video_mode = false;
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

    /// Fills in what the queue's songs, and the one playing, lack from
    /// another answer about the same song (see [`crate::queue::fill_in`]).
    fn fill_in(&mut self, track: &Track) {
        let mut changed = self.queue.fill_in(track);
        if let Some(entry) = &mut self.playback.entry {
            changed |= crate::queue::fill_in(&mut entry.track, track);
        }
        if changed {
            self.script_fonts.want_for(&track.title);
            self.script_fonts.want_for(&track.artists);
        }
    }

    /// Starts preparing `entry`; it plays when ready.
    fn start(&mut self, entry: Entry) {
        self.start_from(entry, 0.0);
    }

    /// Starts preparing `entry` to play from `from` seconds, as the
    /// version the video mode wants; it plays when ready. In the video
    /// mode a song whose video is not known yet waits for its details.
    fn start_from(&mut self, entry: Entry, from: f64) {
        self.finish_report();
        self.audio.send(Command::Stop);
        let version = self.version_of(&entry.track);
        match &version {
            Some(version) => self.backend.send(Request::Prepare {
                entry: entry.id,
                video_id: version.clone(),
                play: true,
            }),
            None => self
                .backend
                .send(Request::Details(entry.track.video_id.clone())),
        }
        self.video = self.video_mode.then(|| VideoShow {
            entry: entry.id,
            state: self.video_state(&entry.track),
        });
        self.playback = Playback {
            entry: Some(entry),
            state: PlayState::Preparing,
            asked_more_for: self.playback.asked_more_for.take(),
            version,
            start_at: from,
            listening_from: from,
            ..Playback::default()
        };
    }

    /// The video `track` shows in the video mode: itself for a video, else
    /// its music video, as Up next or its details said, with YouTube's map
    /// of the song in it. `Some(None)` when it has none, `None` while that
    /// is not known.
    pub fn pair_of(&self, track: &Track) -> Option<Option<VideoPair>> {
        let told = self.song_videos.get(&track.video_id);
        if track.is_video() {
            let video = track.video_id.clone();
            let segments = told
                .and_then(Option::as_ref)
                .filter(|pair| pair.video == video)
                .map(|pair| pair.segments.clone())
                .unwrap_or_default();
            return Some(Some(VideoPair { video, segments }));
        }
        if let Some(more) = &track.more
            && let Some(video) = &more.counterpart
        {
            return Some(Some(VideoPair {
                video: video.clone(),
                segments: more.segments.clone(),
            }));
        }
        told.cloned()
    }

    /// The ID of the video `track` shows in the video mode (see
    /// [`App::pair_of`]).
    fn video_of(&self, track: &Track) -> Option<Option<String>> {
        self.pair_of(track).map(|pair| pair.map(|pair| pair.video))
    }

    /// Where the song playing should start as `to` (the other version),
    /// from `at` in the version playing: the same music, by YouTube's map
    /// of the song in its video (a video with an intro of its own starts
    /// the song later in it).
    fn moment_in(&self, entry: &Entry, at: f64, to: &str) -> f64 {
        let Some(Some(pair)) = self.pair_of(&entry.track) else {
            return at;
        };
        let song = entry.track.video_id.as_str();
        let from = self.playback.version.as_deref().unwrap_or(song);
        if from == song && to == pair.video {
            ytfast_core::read::song_to_video(&pair.segments, at)
        } else if from == pair.video && to == song {
            ytfast_core::read::video_to_song_or_next(&pair.segments, at)
        } else {
            at
        }
    }

    /// The song playing's map of its music in the version playing, when
    /// that is its music video (not the song itself).
    fn playing_video_map(&self) -> Option<Vec<ytfast_core::read::Segment>> {
        let entry = self.playback.entry.as_ref()?;
        let version = self.playback.version.as_deref()?;
        let pair = self.pair_of(&entry.track)??;
        (version != entry.track.video_id && version == pair.video).then_some(pair.segments)
    }

    /// Where the song playing is for its lyrics, which are timed to the
    /// song: while its music video plays, the video's moment as the song's
    /// (by YouTube's map); `None` while the video shows what the song does
    /// not have (its own intro, a scene between).
    pub fn lyrics_clock(&self) -> Option<f64> {
        let position = self.audio_status.position;
        match self.playing_video_map() {
            Some(map) => ytfast_core::read::video_to_song(&map, position),
            None => Some(position),
        }
    }

    /// The moment in what plays for `song` seconds of the song (a line of
    /// its lyrics chosen).
    pub fn lyrics_moment(&self, song: f64) -> f64 {
        match self.playing_video_map() {
            Some(map) => ytfast_core::read::song_to_video(&map, song),
            None => song,
        }
    }

    /// Whether the song playing has a video: `None` while that is not
    /// known (the Song and Video switch then offers it).
    pub fn has_video(&self) -> Option<bool> {
        let entry = self.playback.entry.as_ref()?;
        self.pair_of(&entry.track).map(|pair| pair.is_some())
    }

    /// What `track` plays as: itself, or in the video mode its video (a
    /// song without one, itself). `None` while that is not known.
    fn version_of(&self, track: &Track) -> Option<String> {
        if !self.video_mode {
            return Some(track.video_id.clone());
        }
        match self.video_of(track) {
            Some(Some(video)) => Some(video),
            Some(None) => Some(track.video_id.clone()),
            None => None,
        }
    }

    /// What the player page shows for `track` in the video mode while it
    /// gets ready.
    fn video_state(&self, track: &Track) -> VideoState {
        match self.video_of(track) {
            Some(Some(_)) => VideoState::Coming,
            Some(None) => VideoState::Missing,
            None => VideoState::Finding,
        }
    }

    /// In the video mode, once the song plays as its video: the picture is
    /// asked for (a song without one keeps its cover).
    fn ask_for_picture(&mut self) {
        let Some(entry) = &self.playback.entry else {
            return;
        };
        let id = entry.id;
        let video = self.video_of(&entry.track);
        let playing = self.playback.state == PlayState::Playing;
        let version = self.playback.version.clone();
        let Some(show) = self.video.as_mut().filter(|v| v.entry == id) else {
            return;
        };
        if !matches!(show.state, VideoState::Finding | VideoState::Coming) {
            return;
        }
        match video {
            Some(Some(video)) if playing && version.as_ref() == Some(&video) => {
                show.state = VideoState::Loading;
                self.backend.send(Request::Video {
                    entry: id,
                    video_id: video,
                });
            }
            Some(Some(_)) => show.state = VideoState::Coming,
            Some(None) => show.state = VideoState::Missing,
            None => {}
        }
    }

    /// A song's details said what its video is (or did not load).
    fn song_video(&mut self, song: String, found: Result<Option<VideoPair>, String>) {
        let known = match found {
            Ok(video) => {
                if self.song_videos.len() >= MAX_SONG_VIDEOS {
                    self.song_videos.clear();
                }
                self.song_videos.insert(song.clone(), video);
                true
            }
            Err(e) => {
                log::info!("whether a song has a video is not known: {e}");
                false
            }
        };
        if !self.video_mode {
            return;
        }
        // The next song gets ready as its video.
        if known
            && self
                .queue
                .peek_next()
                .is_some_and(|next| next.track.video_id == song)
        {
            self.prepare_next();
        }
        let Some(entry) = self
            .playback
            .entry
            .clone()
            .filter(|e| e.track.video_id == song)
        else {
            return;
        };
        let waiting = self.playback.version.is_none();
        match (&self.playback.state, self.version_of(&entry.track)) {
            // It waited to know: it plays now, as its video if it has one.
            (PlayState::Preparing, Some(_)) if waiting => {
                let from = self.playback.start_at;
                self.start_from(entry, from);
            }
            // Its details did not load: it plays as itself.
            (PlayState::Preparing, None) if waiting => {
                let version = entry.track.video_id.clone();
                self.backend.send(Request::Prepare {
                    entry: entry.id,
                    video_id: version.clone(),
                    play: true,
                });
                self.playback.version = Some(version);
                if let Some(show) = &mut self.video {
                    show.state = VideoState::Failed("This song's video could not be found.".into());
                }
            }
            // Playing as itself, and it has a video: the video plays on
            // from here.
            (PlayState::Playing, Some(wanted))
                if self.playback.version.as_ref() != Some(&wanted) =>
            {
                let at = self.moment_in(&entry, self.clock(), &wanted);
                self.start_from(entry, at);
            }
            // (While it gets ready, `prepared` changes the version.)
            _ => self.ask_for_picture(),
        }
    }

    /// The Song and Video switch.
    fn set_video_mode(&mut self, on: bool) {
        if on == self.video_mode {
            return;
        }
        self.video_mode = on;
        let Some(entry) = self.playback.entry.clone() else {
            return;
        };
        let wanted = self.version_of(&entry.track);
        match self.playback.state {
            // The other version plays on from the same music.
            PlayState::Playing
                if wanted
                    .as_ref()
                    .is_some_and(|w| self.playback.version.as_ref() != Some(w)) =>
            {
                let to = wanted.as_deref().unwrap_or_default();
                let at = self.moment_in(&entry, self.clock(), to);
                self.start_from(entry, at);
            }
            // The same sound (a video plays its own): only the picture
            // comes or goes. Whether a song has a video is asked when it
            // is not known (its details are kept, so this costs nothing
            // when they have loaded).
            PlayState::Playing => {
                self.video = on.then(|| VideoShow {
                    entry: entry.id,
                    state: self.video_state(&entry.track),
                });
                if wanted.is_none() {
                    self.backend
                        .send(Request::Details(entry.track.video_id.clone()));
                }
                self.ask_for_picture();
            }
            // Waiting to hear about its video: as itself, at once.
            PlayState::Preparing if self.playback.version.is_none() => {
                let from = self.playback.start_at;
                self.start_from(entry, from);
            }
            // Getting ready: `prepared` changes the version once it is.
            PlayState::Preparing => {
                self.video = on.then(|| VideoShow {
                    entry: entry.id,
                    state: self.video_state(&entry.track),
                });
            }
            _ => self.video = None,
        }
        self.prepare_next();
    }

    /// Where the song playing is, in seconds, finer than the player's
    /// reports: for the video's pictures, and where the other version
    /// plays from when the video mode changes.
    pub fn clock(&self) -> f64 {
        let Some(entry) = &self.playback.entry else {
            return 0.0;
        };
        match self.seeking {
            Some((seeking, to, _)) if seeking == entry.id => to,
            _ if self.audio_status.entry == Some(entry.id) => self.audio_status.clock(),
            _ => self.playback.start_at,
        }
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
                self.video = None;
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

    /// Gets the next song ready ahead, after the queue changed: in the
    /// video mode as its video, once its details say what that is.
    fn prepare_next(&self) {
        if self.playback.state == PlayState::Playing
            && let Some(next) = self.queue.peek_next()
        {
            match self.version_of(&next.track) {
                Some(version) => self.backend.send(Request::Prepare {
                    entry: next.id,
                    video_id: version,
                    play: false,
                }),
                // `App::song_video` gets it ready once they have come.
                None => self
                    .backend
                    .send(Request::Details(next.track.video_id.clone())),
            }
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
        // A fresh loading has its own next results.
        self.more_results.remove(&route);
        // A Library tab in the order chosen for it.
        let order = route
            .library_tab()
            .and_then(|tab| self.settings.library_order.get(tab.browse_id()).cloned());
        self.backend.send(Request::Page {
            route,
            load: self.loads,
            order,
        });
    }

    /// The Library's playlists again (the menu's list comes from them),
    /// and its front page if it was loaded.
    fn refresh_library(&mut self) {
        self.refresh(Route::Library);
        if self.pages.contains_key(&Route::LibraryRecent) {
            self.refresh(Route::LibraryRecent);
        }
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
        // Songs ticked belong to the page they were ticked on.
        self.selected.clear();
        // A page opened anew starts at its top (Back and Forward return to
        // where it was left).
        self.fresh_page.set(true);
        self.show_current();
    }

    /// The page shown before this one, if any.
    pub fn came_from(&self) -> Option<&Route> {
        self.back.last()
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
                // Only the song's ID is known (an artist's Shuffle or Mix,
                // Start mix): its name, artists and cover are filled in from
                // the player's answer and Up next's (`App::fill_in`).
                let track = track.unwrap_or_else(|| Track {
                    video_id: video_id.clone(),
                    set_video_id: None,
                    title: crate::queue::UNNAMED.into(),
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
                    self.selected.clear();
                    self.show_current();
                }
            }
            Action::Forward => {
                self.now_playing = false;
                if let Some(route) = self.forward.pop() {
                    let old = std::mem::replace(&mut self.route, route);
                    self.back.push(old);
                    self.selected.clear();
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
            Action::ToggleFullscreen => self.set_fullscreen(!self.fullscreen),
            Action::VideoMode(on) => self.set_video_mode(on),
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
            Action::ToggleSelected(track) => {
                let same = |t: &Track| {
                    t.video_id == track.video_id && t.set_video_id == track.set_video_id
                };
                if self.selected.iter().any(same) {
                    self.selected.retain(|t| !same(t));
                } else {
                    self.selected.push(track);
                }
            }
            Action::ClearSelected => self.selected.clear(),
            Action::PlayNextAll(tracks) => {
                let count = tracks.len();
                // Each put right after the current one: the last first.
                for track in tracks.into_iter().rev() {
                    self.queue.play_next(track);
                }
                self.notify(format!("{count} songs play next"));
                self.queue_grew();
                self.selected.clear();
            }
            Action::AddToQueueAll(tracks) => {
                let count = tracks.len();
                for track in tracks {
                    self.queue.add_to_end(track);
                }
                self.notify(format!("{count} songs added to the queue"));
                self.queue_grew();
                self.selected.clear();
            }
            Action::SortPlaylist(route, order) => match order {
                Some(order) => {
                    self.playlist_sort.insert(route.clone(), order);
                    if let Some(Loadable::Ready(page)) = self.pages.get_mut(&route) {
                        order.apply(page);
                    }
                }
                // Its own order again, as YouTube has it.
                None => {
                    if self.playlist_sort.remove(&route).is_some() {
                        self.refresh(route);
                    }
                }
            },
            Action::RemoveFromQueue(id) => {
                self.queue.remove(id);
                self.prepare_next();
            }
            Action::MoveInQueue(id, to) => {
                self.queue.move_to(id, to);
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
                // The song playing, disliked: the next one, as YouTube
                // Music does (Settings can keep it playing).
                let skip = like == LikeState::Disliked
                    && self.settings.skip_disliked
                    && self
                        .playback
                        .entry
                        .as_ref()
                        .is_some_and(|e| e.track.video_id == video_id);
                self.send_edit(Edit::Rate { video_id, like });
                if like == LikeState::Liked {
                    self.notify("Added to Liked Music");
                }
                if skip {
                    self.next();
                }
            }
            Action::AddToPlaylist {
                playlist_id,
                title,
                video_ids,
            } => {
                self.notify(format!("Added to {title}"));
                self.send_edit(Edit::AddToPlaylist {
                    playlist_id,
                    video_ids,
                });
            }
            Action::Edit(change) => self.send_edit(change),
            Action::Suggest(text) => {
                // Words being typed show in their own script at once.
                self.script_fonts.want_for(&text);
                self.suggesting.clone_from(&text);
                self.backend.send(Request::Suggest(text));
            }
            Action::SortLibrary(order) => {
                // The tab loads again in that order; what it showed stays
                // until then, under the new order's name.
                let tab = Route::LIBRARY.into_iter().find(|r| {
                    r.library_tab()
                        .is_some_and(|t| t.browse_id() == order.browse_id)
                });
                self.settings
                    .library_order
                    .insert(order.browse_id, order.params);
                if let Some(route) = tab {
                    self.refresh(route);
                }
            }
            Action::MoreResults(route) => {
                // One request at a time, and none once all have come.
                if !self.more_results.contains_key(&route)
                    && let Some(load) = self.page_loads.get(&route).copied()
                {
                    let pages = &self.pages;
                    self.more_results.retain(|r, _| pages.contains_key(r));
                    self.more_results.insert(route.clone(), true);
                    self.backend.send(Request::MoreResults { route, load });
                }
            }
            Action::ForgetSearch { words, token } => {
                // Gone at once, as on YouTube Music, with its words.
                self.suggestions
                    .1
                    .words
                    .retain(|w| w.forget.as_deref() != Some(token.as_str()));
                self.forgotten_searches.insert(words.to_lowercase());
                self.notify("This item has been removed from your history.");
                self.send_edit(Edit::ForgetSearch { words, token });
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
                    Setting::MovingBackground => s.moving_background = !s.moving_background,
                    Setting::SkipDisliked => s.skip_disliked = !s.skip_disliked,
                    Setting::ConfirmClose => s.confirm_close = !s.confirm_close,
                    Setting::AutoUpdate => {
                        s.auto_update = !s.auto_update;
                        if let Some(updates) = &self.updates {
                            updates.set_automatic(s.auto_update);
                        }
                    }
                }
            }
            Action::CheckForUpdates => {
                if let Some(updates) = &self.updates {
                    self.update_manual = true;
                    updates.check();
                }
            }
            Action::InstallUpdate => {
                if let Some(updates) = &self.updates {
                    updates.install();
                }
            }
            Action::RestartToUpdate => {
                if let Some(updates) = &self.updates {
                    updates.restart();
                }
            }
            Action::ShowUpdate => {
                *self.dialog.get_mut() = Some(Dialog::Update);
                self.dialog_fresh.set(true);
            }
            Action::SetTheme(theme) => self.settings.theme = theme,
            Action::OpenDialog(dialog) => {
                *self.dialog.get_mut() = Some(dialog);
                self.dialog_fresh.set(true);
            }
            Action::ConfirmClose { dont_ask } => {
                if dont_ask {
                    self.settings.confirm_close = false;
                }
                self.closing = true;
                self.close_now = true;
            }
            Action::RenamePlaylist {
                playlist_id,
                name,
                description,
                privacy,
            } => {
                self.notify("Saved the playlist");
                let route = Route::browse(format!("VL{playlist_id}"), None);
                if let Some(Loadable::Ready(Page {
                    header: Some(header),
                    ..
                })) = self.pages.get_mut(&route)
                {
                    header.title = name.clone();
                    header.description = description.clone();
                    // "Playlist • Public • 2024": its privacy changed in place.
                    if let Some(old) = Privacy::from_subtitle(&header.subtitle) {
                        header.subtitle = header.subtitle.replace(old.words().0, privacy.words().0);
                    }
                }
                self.send_edit(Edit::RenamePlaylist {
                    playlist_id,
                    name,
                    description,
                    privacy,
                });
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
            Action::RemoveFromPlaylist { playlist_id, songs } => {
                let route = Route::browse(format!("VL{playlist_id}"), None);
                let gone = |t: &Track| {
                    songs
                        .iter()
                        .any(|(_, row)| t.set_video_id.as_deref() == Some(row.as_str()))
                };
                if let Some(Loadable::Ready(page)) = self.pages.get_mut(&route) {
                    for section in &mut page.sections {
                        section
                            .items
                            .retain(|item| !matches!(item, Item::Track(t) if gone(t)));
                    }
                }
                self.notify(if songs.len() == 1 {
                    "Removed from the playlist".to_string()
                } else {
                    format!("{} songs removed from the playlist", songs.len())
                });
                self.send_edit(Edit::RemoveFromPlaylist { playlist_id, songs });
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
            Action::OpenDownloadPage => open_link(crate::update::DOWNLOAD_PAGE),
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
        // Escape closes a dialog (or leaves the search box) first, then
        // full screen, then the player page.
        if self.fullscreen
            && !typing
            && self.dialog.get_mut().is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.act(Action::ToggleFullscreen);
        }
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
        if !typing && self.dialog.get_mut().is_none() {
            self.youtube_keys(ctx, focused);
        }
        if search {
            ctx.memory_mut(|m| m.request_focus(views::SEARCH_BOX.into()));
        }
    }

    /// YouTube Music's own keys (its list opens with "?"), and YTFast's
    /// arrows: ←/→ 10 s, ↑/↓ the volume, unless a button has the keyboard.
    /// egui ignores an extra Shift when matching, so the keys with Shift
    /// are asked about first.
    fn youtube_keys(&mut self, ctx: &egui::Context, focused: bool) {
        use egui::{Key, Modifiers};
        let pressed = |key, modifiers| ctx.input_mut(|i| i.consume_key(modifiers, key));
        // A character with no key of its own in egui ("_").
        let typed = |text: &str| {
            ctx.input_mut(|i| {
                let found = i
                    .events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Text(t) if t == text));
                if found {
                    i.events
                        .retain(|e| !matches!(e, egui::Event::Text(t) if t == text));
                }
                found
            })
        };
        let position = self.shown_position();
        let volume = self.settings.volume;
        let now = ctx.input(|i| i.time);

        // "g", then where to go.
        if let Some(since) = self.go_to {
            let target = if now - since > 1.5 {
                None
            } else if pressed(Key::H, Modifiers::NONE) {
                Some(Route::Home)
            } else if pressed(Key::E, Modifiers::NONE) {
                Some(Route::Explore)
            } else if pressed(Key::L, Modifiers::NONE) {
                Some(Route::LibraryRecent)
            } else if pressed(Key::Comma, Modifiers::NONE) {
                Some(Route::Settings)
            } else {
                None
            };
            if let Some(route) = target {
                self.go_to = None;
                self.now_playing = false;
                self.act(Action::Navigate(route));
                return;
            }
            if now - since > 1.5 {
                self.go_to = None;
            }
        }
        if pressed(Key::G, Modifiers::NONE) {
            self.go_to = Some(now);
            return;
        }
        // YTFast's own: full screen, with the player page.
        if pressed(Key::F, Modifiers::NONE) {
            self.act(Action::ToggleFullscreen);
        }

        let seek = |by: f64| Action::Seek((position + by).max(0.0));
        // One second: Shift+L / Shift+H, Ctrl+Shift+→ / ←.
        if pressed(Key::L, Modifiers::SHIFT)
            || pressed(Key::ArrowRight, Modifiers::COMMAND | Modifiers::SHIFT)
        {
            self.act(seek(1.0));
        }
        if pressed(Key::H, Modifiers::SHIFT)
            || pressed(Key::ArrowLeft, Modifiers::COMMAND | Modifiers::SHIFT)
        {
            self.act(seek(-1.0));
        }
        if pressed(Key::N, Modifiers::SHIFT) || pressed(Key::J, Modifiers::NONE) {
            self.act(Action::Next);
        }
        if pressed(Key::P, Modifiers::SHIFT) || pressed(Key::K, Modifiers::NONE) {
            self.act(Action::Previous);
        }
        // Ten seconds: l / h, Shift+→ / ← (and the arrows alone, YTFast's).
        let arrows = !focused;
        if pressed(Key::L, Modifiers::NONE) || (arrows && pressed(Key::ArrowRight, Modifiers::NONE))
        {
            self.act(seek(10.0));
        }
        if pressed(Key::H, Modifiers::NONE) || (arrows && pressed(Key::ArrowLeft, Modifiers::NONE))
        {
            self.act(seek(-10.0));
        }
        if pressed(Key::Semicolon, Modifiers::NONE) {
            self.act(Action::TogglePause);
        }
        if pressed(Key::Equals, Modifiers::NONE)
            || (arrows && pressed(Key::ArrowUp, Modifiers::NONE))
        {
            self.act(Action::SetVolume((volume + 0.1).min(1.0)));
        }
        if pressed(Key::Minus, Modifiers::NONE)
            || (arrows && pressed(Key::ArrowDown, Modifiers::NONE))
        {
            self.act(Action::SetVolume((volume - 0.1).max(0.0)));
        }
        if pressed(Key::M, Modifiers::NONE) {
            self.act(Action::ToggleMute);
        }
        if pressed(Key::S, Modifiers::NONE) {
            self.act(Action::ShuffleQueue);
        }
        if pressed(Key::R, Modifiers::NONE) {
            self.act(Action::CycleRepeat);
        }
        if pressed(Key::Q, Modifiers::NONE) {
            self.act(Action::ToggleNowPlaying);
        }
        if pressed(Key::Questionmark, Modifiers::NONE) {
            self.act(Action::OpenDialog(Dialog::Shortcuts));
        }
        // "+" likes and "_" dislikes the song playing (again: neither).
        let rate = if pressed(Key::Plus, Modifiers::NONE) {
            Some(LikeState::Liked)
        } else if typed("_") {
            Some(LikeState::Disliked)
        } else {
            None
        };
        if let (Some(wanted), Some(entry)) = (rate, &self.playback.entry) {
            let id = entry.track.video_id.clone();
            let next = if self.likes.get(&id) == Some(&wanted) {
                LikeState::Neutral
            } else {
                wanted
            };
            self.act(Action::Rate(id, next));
        }
    }

    /// The window asked to close (its ×, Alt+F4, the taskbar, a Mac's
    /// red button) while a song plays: it stays, and asks first
    /// ([`Dialog::ConfirmClose`]), unless Settings says not to, an update
    /// is closing YTFast, or the question was answered Yes.
    fn ask_before_closing(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.close_now) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let sounding = self.playback.state == PlayState::Playing && !self.audio_status.paused;
        let ask = self.settings.confirm_close && sounding && !self.closing && !self.restarting;
        if !ask || !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        // Closed from the taskbar while minimised: the question shows.
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        if !matches!(*self.dialog.get_mut(), Some(Dialog::ConfirmClose { .. })) {
            self.apply(Action::OpenDialog(Dialog::ConfirmClose { dont_ask: true }));
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
        Event::SignedIn { name, .. } => words.push(name.as_str()),
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
        Event::MoreResults { items, .. } => {
            for item in items {
                match item {
                    Item::Track(t) => track(t, &mut words),
                    Item::Card(c) => words.extend([c.title.as_str(), c.subtitle.as_str()]),
                }
            }
        }
        Event::MoreSections { sections, .. } => {
            for section in sections {
                words.push(section.title.as_str());
                for item in &section.items {
                    match item {
                        Item::Track(t) => track(t, &mut words),
                        Item::Card(c) => words.extend([c.title.as_str(), c.subtitle.as_str()]),
                    }
                }
            }
        }
        Event::Suggestions(_, found) => {
            words.extend(found.words.iter().map(|w| w.text.as_str()));
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

/// The most songs whose videos are remembered (see [`App::video_of`]).
const MAX_SONG_VIDEOS: usize = 500;

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
        self.watch_updates();
        self.media_controls();
        // A media key pressed while the window is hidden acts at once.
        self.apply_actions();
        self.ask_before_closing(ctx);
        // Keep the progress bar moving, and hear when the song ends, while
        // a song plays.
        if self.playback.state == PlayState::Playing && !self.audio_status.paused {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // The look chosen in Settings: egui's own colours follow it.
        crate::theme::set(self.settings.theme);
        if self.styled != Some(self.settings.theme) {
            crate::theme::restyle(&ctx);
            self.styled = Some(self.settings.theme);
            // Dynamic Background sets everything in Inter.
            self.script_fonts.set_inter_first(crate::theme::dynamic());
        }
        // A freshly installed version is up: its receipt tells the helper
        // to keep it.
        if let Some(receipt) = self.receipt.take() {
            std::thread::spawn(move || {
                if let Err(e) = receipt.acknowledge() {
                    log::warn!("updates: could not confirm the new version: {e:#}");
                }
            });
        }
        // The helper waits for this window to close to install the update.
        if !self.restarting
            && self.updates.as_ref().map(crate::update::Updates::state)
                == Some(crate::update::State::Restarting)
        {
            self.restarting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.scrolling.apply(&ctx);
        self.mend_window_size(&ctx);
        self.script_fonts.check();
        self.images.get_mut().begin_frame();
        self.shortcuts(&ctx);
        self.want_song_extras();
        // Dynamic Background: the song's cover behind everything.
        if crate::theme::dynamic() {
            crate::dynamic::paint(self, ui);
        }

        views::show(self, ui);
        self.apply_actions();
        self.sync_fullscreen(&ctx);
        // The player page is closed, or shows no video: its decoding rests.
        if !self.video_drawn.replace(false)
            && let Some(VideoShow {
                state: VideoState::Playing { video, .. },
                ..
            }) = &self.video
        {
            video.rest();
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
            eframe::set_value(storage, "ytfast", self.settings.for_saving());
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

/// Opens a web page in the default browser.
fn open_link(url: &str) {
    let mut command = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        std::process::Command::new("explorer")
    } else {
        std::process::Command::new("xdg-open")
    };
    if let Err(e) = command.arg(url).spawn() {
        log::warn!("could not open the page: {e}");
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
    use ytfast_core::read::SuggestedWords;

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

    /// The whole window drawn in the Dynamic Background theme, without a
    /// real window: the sign-in screen; signed in with a song playing,
    /// the pointer resting all over the window (what shows only under
    /// it); Settings; the player page's three tabs; a dialog and a toast.
    /// Nothing may fail, and every frame draws something.
    #[test]
    fn the_dynamic_background_theme_draws_every_part_of_the_window() {
        use crate::theme::Theme;
        let mut h = Harness::new();
        h.app.settings.theme = Theme::DynamicBackground;
        let size = egui::vec2(1280.0, 820.0);
        let draw = |h: &mut Harness, pointer: Option<egui::Pos2>| {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            };
            if let Some(at) = pointer {
                input.events.push(egui::Event::PointerMoved(at));
            }
            let app = &h.app;
            let mut output = h.ctx.run_ui(input, |ui| {
                crate::theme::set(app.settings.theme);
                crate::dynamic::paint(app, ui);
                views::show(app, ui);
            });
            assert!(!output.shapes.is_empty());
            // No window takes the frame's new textures here.
            output.textures_delta.clear();
        };

        // Signed out: the sign-in screen.
        assert!(!matches!(h.app.auth, Auth::SignedIn { .. }));
        draw(&mut h, None);
        draw(&mut h, Some(egui::pos2(640.0, 400.0)));

        // Signed in, a song playing, the pointer resting everywhere.
        h.app.auth = Auth::SignedIn {
            name: "Listener".into(),
            handle: None,
            photo: None,
        };
        h.play(&["a", "b", "c"]);
        for y in (8..820).step_by(48) {
            for x in (8..1280).step_by(96) {
                draw(&mut h, Some(egui::pos2(x as f32, y as f32)));
            }
        }
        h.act(Action::Navigate(Route::Settings));
        draw(&mut h, None);

        // The player page, each tab, as it rises.
        h.app.now_playing = true;
        for tab in [NpTab::UpNext, NpTab::Lyrics, NpTab::Related] {
            h.app.np_tab = tab;
            for x in [700.0, 1000.0, 1200.0] {
                draw(&mut h, Some(egui::pos2(x, 300.0)));
            }
        }

        // A dialog, and a toast.
        h.app.now_playing = false;
        h.act(Action::OpenDialog(Dialog::NewPlaylist {
            name: String::new(),
            description: String::new(),
            privacy: ytfast_core::library::Privacy::Private,
            songs: Vec::new(),
        }));
        h.app.notice = Some(("Added to the queue".into(), Instant::now()));
        draw(&mut h, None);
        draw(&mut h, Some(egui::pos2(640.0, 400.0)));
        crate::theme::set(Theme::YouTubeMusic);
    }

    /// A window of the app with the pointer worked by hand: each frame is
    /// drawn as the window would (1280 by 820, a tenth of a second apart),
    /// its actions applied, and what screen readers see kept, so a test
    /// can find a control by its name and press or drag it.
    struct Window {
        h: Harness,
        time: f64,
        named: Vec<(String, egui::Rect)>,
        /// What the frames asked of the window (close it, move it...).
        commands: Vec<egui::ViewportCommand>,
        /// The pointer's look after the last frame.
        cursor: egui::CursorIcon,
    }

    impl Window {
        fn new(theme: crate::theme::Theme) -> Self {
            let mut h = Harness::new();
            h.ctx.enable_accesskit();
            h.app.settings.theme = theme;
            h.app.auth = Auth::SignedIn {
                name: "Listener".into(),
                handle: None,
                photo: None,
            };
            Self {
                h,
                time: 0.0,
                named: Vec::new(),
                commands: Vec::new(),
                cursor: egui::CursorIcon::Default,
            }
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 0.1;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 820.0),
                )),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let app = &self.h.app;
            let mut output = self.h.ctx.run_ui(input, |ui| {
                crate::theme::set(app.settings.theme);
                if crate::theme::dynamic() {
                    crate::dynamic::paint(app, ui);
                }
                views::show(app, ui);
            });
            output.textures_delta.clear();
            self.cursor = output.platform_output.cursor_icon;
            self.commands.extend(
                output
                    .viewport_output
                    .values()
                    .flat_map(|viewport| viewport.commands.iter().cloned()),
            );
            self.named = output
                .platform_output
                .accesskit_update
                .map(|update| {
                    update
                        .nodes
                        .into_iter()
                        .filter_map(|(_, node)| {
                            let b = node.bounds()?;
                            let rect = egui::Rect::from_min_max(
                                egui::pos2(b.x0 as f32, b.y0 as f32),
                                egui::pos2(b.x1 as f32, b.y1 as f32),
                            );
                            Some((node.label()?.to_string(), rect))
                        })
                        .collect()
                })
                .unwrap_or_default();
            self.h.app.apply_actions();
        }

        /// A frame in which the window is asked to close (its ×, Alt+F4),
        /// as `App::logic` takes it: what the app told the window.
        fn close_request(&mut self) -> Vec<egui::ViewportCommand> {
            self.time += 0.1;
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 820.0),
                )),
                time: Some(self.time),
                ..Default::default()
            };
            input.viewports.insert(
                egui::ViewportId::ROOT,
                egui::ViewportInfo {
                    events: vec![egui::ViewportEvent::Close],
                    ..Default::default()
                },
            );
            let app = &mut self.h.app;
            let mut output = self
                .h
                .ctx
                .run_ui(input, |ui| app.ask_before_closing(ui.ctx()));
            output.textures_delta.clear();
            output
                .viewport_output
                .values()
                .flat_map(|viewport| viewport.commands.iter().cloned())
                .collect()
        }

        /// `key` pressed, in a frame run as the window runs one: the
        /// shortcuts, the views, the actions, full screen. What the app
        /// told the window.
        fn press(&mut self, key: egui::Key) -> Vec<egui::ViewportCommand> {
            self.time += 0.1;
            let down = |pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 820.0),
                )),
                time: Some(self.time),
                events: vec![down(true), down(false)],
                ..Default::default()
            };
            let app = &mut self.h.app;
            let mut output = self.h.ctx.run_ui(input, |ui| {
                app.shortcuts(ui.ctx());
                crate::theme::set(app.settings.theme);
                views::show(app, ui);
                app.apply_actions();
                app.sync_fullscreen(ui.ctx());
            });
            output.textures_delta.clear();
            output
                .viewport_output
                .values()
                .flat_map(|viewport| viewport.commands.iter().cloned())
                .collect()
        }

        /// The largest control named `name` (the cover, beside the
        /// player bar's button of the same name).
        fn largest(&self, name: &str) -> egui::Rect {
            self.named
                .iter()
                .filter(|(n, _)| n == name)
                .map(|(_, rect)| *rect)
                .max_by(|a, b| a.area().total_cmp(&b.area()))
                .unwrap_or_else(|| panic!("no {name}"))
        }

        /// Frames enough for anything sliding or fading to settle.
        fn settle(&mut self) {
            for _ in 0..6 {
                self.frame(Vec::new());
            }
        }

        /// Where the control named `name` is, the `nth` one so named (from
        /// the top).
        fn find(&self, name: &str, nth: usize) -> egui::Rect {
            let mut found: Vec<egui::Rect> = self
                .named
                .iter()
                .filter(|(n, _)| n == name)
                .map(|(_, r)| *r)
                .collect();
            found.sort_by(|a, b| a.top().total_cmp(&b.top()));
            *found
                .get(nth)
                .unwrap_or_else(|| panic!("nothing named {name} on screen"))
        }

        fn point(&mut self, at: egui::Pos2) {
            self.frame(vec![egui::Event::PointerMoved(at)]);
        }

        fn button(&mut self, at: egui::Pos2, pressed: bool) {
            self.frame(vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }]);
        }

        fn click(&mut self, at: egui::Pos2) {
            self.point(at);
            self.button(at, true);
            self.button(at, false);
            self.frame(Vec::new());
        }

        /// A right-click at `at`.
        fn right_click(&mut self, at: egui::Pos2) {
            self.point(at);
            for pressed in [true, false] {
                self.frame(vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }]);
            }
            self.frame(Vec::new());
        }

        fn shows(&self, name: &str) -> bool {
            self.named.iter().any(|(n, _)| n == name)
        }

        /// A frame with the wheel turned (points, as a touchpad's) at `at`.
        fn wheel(&mut self, at: egui::Pos2, dy: f32) {
            self.frame(vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, dy),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
    }

    /// Home ten shelves long, shown in a test window, and its loading.
    fn long_home(w: &mut Window) -> u64 {
        w.h.act(Action::Navigate(Route::Home));
        w.h.app.load(Route::Home);
        let load =
            w.h.requests()
                .into_iter()
                .filter_map(|r| match r {
                    Request::Page {
                        route: Route::Home,
                        load,
                        ..
                    } => Some(load),
                    _ => None,
                })
                .last()
                .expect("Home is asked for");
        let shelves = (0..10)
            .map(|n| Section {
                title: format!("Shelf {n}"),
                items: (0..3)
                    .map(|i| Item::Track(song(&format!("s{n}-{i}"))))
                    .collect(),
                ..Section::default()
            })
            .collect();
        w.h.answer(Event::Page(
            Route::Home,
            load,
            Ok(Page {
                sections: shelves,
                ..Page::default()
            }),
        ));
        load
    }

    fn asked_for_more(w: &mut Window) -> usize {
        w.h.requests()
            .into_iter()
            .filter(|r| {
                matches!(
                    r,
                    Request::MoreResults {
                        route: Route::Home,
                        ..
                    }
                )
            })
            .count()
    }

    /// Home's next shelves are asked for a screen and a half before its
    /// end comes into view, not at its top.
    #[test]
    fn home_asks_for_more_before_its_end_shows() {
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        long_home(&mut w);
        w.settle();
        assert_eq!(asked_for_more(&mut w), 0, "at the top");
        // Scrolled on until the end is near, not yet in view.
        let at = egui::pos2(700.0, 400.0);
        let mut asked = 0;
        for _ in 0..40 {
            w.wheel(at, -60.0);
            asked += asked_for_more(&mut w);
            if asked > 0 {
                break;
            }
        }
        assert_eq!(asked, 1);
        // The end was not yet near the view: well over a third of a
        // screen of the page was still to come.
        let asked_at = w.h.app.page_offset.get();
        for _ in 0..40 {
            w.wheel(at, -60.0);
        }
        assert!(
            w.h.app.page_offset.get() - asked_at > 300.0,
            "asked at {asked_at}, the end at {}",
            w.h.app.page_offset.get()
        );
    }

    /// A scroll that reaches Home's end stops there: shelves that arrive
    /// meanwhile show below, and the page scrolls on into them only once
    /// the scrolling has paused.
    #[test]
    fn a_scroll_stops_at_homes_end_until_it_pauses() {
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        let load = long_home(&mut w);
        w.settle();
        let at = egui::pos2(700.0, 400.0);
        // Scrolled to the end, and on.
        let mut last = -1.0;
        for _ in 0..80 {
            w.wheel(at, -120.0);
            let now = w.h.app.page_offset.get();
            if now == last {
                break;
            }
            last = now;
        }
        let end = w.h.app.page_offset.get();
        w.h.requests();
        // More shelves arrive while the scroll goes on: it stays put.
        w.h.answer(Event::MoreSections {
            route: Route::Home,
            load,
            sections: (10..13)
                .map(|n| Section {
                    title: format!("Shelf {n}"),
                    items: (0..3)
                        .map(|i| Item::Track(song(&format!("s{n}-{i}"))))
                        .collect(),
                    ..Section::default()
                })
                .collect(),
            done: true,
        });
        for _ in 0..5 {
            w.wheel(at, -120.0);
        }
        assert!(
            (w.h.app.page_offset.get() - end).abs() < 1.0,
            "held at the end"
        );
        // A pause, then scrolling on: into the new shelves.
        w.frame(Vec::new());
        w.frame(Vec::new());
        for _ in 0..3 {
            w.wheel(at, -120.0);
        }
        assert!(w.h.app.page_offset.get() > end + 100.0, "scrolled on");
    }

    /// In every theme, a right-click on the player bar (not on its
    /// buttons) opens the playing song's menu, as its ⋮ does.
    #[test]
    fn a_right_click_on_the_player_bar_opens_the_songs_menu() {
        use crate::theme::Theme;
        for theme in [
            Theme::YouTubeMusic,
            Theme::Premium,
            Theme::DynamicBackground,
        ] {
            let mut w = Window::new(theme);
            w.h.play(&["a", "b", "c"]);
            w.settle();
            assert!(!w.shows("Start mix"), "{theme:?}");
            // Just after Next, on the bar itself.
            let next = w.find("Next", 0);
            w.right_click(egui::pos2(next.right() + 16.0, next.center().y));
            assert!(w.shows("Start mix"), "{theme:?}");
            // A left-click there still opens the player page.
            assert!(!w.h.app.now_playing, "{theme:?}");
        }
        crate::theme::set(Theme::YouTubeMusic);
    }

    /// In Up next, a row shows the open hand (it is dragged to a new
    /// place), its cover too; its ⋮ shows the pointing hand.
    #[test]
    fn up_next_shows_a_hand_over_its_menu_button() {
        use crate::theme::Theme;
        for theme in [
            Theme::YouTubeMusic,
            Theme::Premium,
            Theme::DynamicBackground,
        ] {
            let mut w = Window::new(theme);
            w.h.play(&["a", "b", "c"]);
            w.h.app.now_playing = true;
            w.h.app.np_tab = NpTab::UpNext;
            w.settle();
            let b = w.find("B", 0);
            w.point(b.center());
            w.frame(Vec::new());
            assert_eq!(w.cursor, egui::CursorIcon::Grab, "{theme:?}");
            // Its cover, at its left.
            w.point(egui::pos2(b.left() + 30.0, b.center().y));
            w.frame(Vec::new());
            assert_eq!(w.cursor, egui::CursorIcon::Grab, "{theme:?}");
            let more = w.find("More actions", 0);
            assert!(b.contains_rect(more), "{theme:?} {b:?} {more:?}");
            w.point(more.center());
            w.frame(Vec::new());
            assert_eq!(w.cursor, egui::CursorIcon::PointingHand, "{theme:?}");
        }
        crate::theme::set(Theme::YouTubeMusic);
    }

    /// In Dynamic Background, whose rows in Up next are taller and apart,
    /// a song dragged in Up next lands where it is let go.
    #[test]
    fn a_song_dragged_in_up_next_lands_where_it_is_let_go() {
        let mut w = Window::new(crate::theme::Theme::DynamicBackground);
        w.h.play(&["a", "b", "c", "d", "e"]);
        w.h.app.now_playing = true;
        w.h.app.np_tab = NpTab::UpNext;
        w.settle();
        let b = w.find("B", 0);
        let d = w.find("D", 0);
        // Rows the theme's height apart (58, and 8 between).
        assert!((d.top() - b.top() - 2.0 * 66.0).abs() < 0.5, "{b:?} {d:?}");
        // B picked up and carried down to D's upper half: it goes before D.
        let from = b.center();
        w.point(from);
        w.button(from, true);
        for step in 1..=8 {
            let y = from.y + (d.top() + 12.0 - from.y) * step as f32 / 8.0;
            w.point(egui::pos2(from.x, y));
        }
        let to = egui::pos2(from.x, d.top() + 12.0);
        w.button(to, false);
        w.frame(Vec::new());
        let order: Vec<&str> =
            w.h.app
                .queue
                .entries()
                .iter()
                .map(|e| e.track.video_id.as_str())
                .collect();
        assert_eq!(order, ["a", "c", "b", "d", "e"]);
        crate::theme::set(crate::theme::Theme::YouTubeMusic);
    }

    /// In Dynamic Background, a playlist's song is ticked by its tick box
    /// (shown under the pointer), which brings up the bar of what can be
    /// done with ticked songs; ticked again, it is let go.
    #[test]
    fn a_song_is_ticked_by_its_tick_box() {
        let mut w = Window::new(crate::theme::Theme::DynamicBackground);
        let route = Route::browse("VLPLx".into(), None);
        w.h.act(Action::Navigate(route.clone()));
        let load =
            w.h.requests()
                .into_iter()
                .find_map(|r| match r {
                    Request::Page { load, .. } => Some(load),
                    _ => None,
                })
                .expect("the page is asked for");
        let page = Page {
            sections: vec![Section {
                items: ["a", "b", "c"].map(|v| Item::Track(song(v))).to_vec(),
                ..Section::default()
            }],
            ..Page::default()
        };
        w.h.answer(Event::Page(route, load, Ok(page)));
        w.settle();
        // Under the pointer, B's row shows its tick box.
        let row = w.find("B", 0);
        w.point(row.center());
        w.frame(Vec::new());
        let tick = w.find("Tick B", 0);
        assert!(row.contains_rect(tick), "{row:?} {tick:?}");
        w.click(tick.center());
        let ticked: Vec<&str> =
            w.h.app
                .selected
                .iter()
                .map(|t| t.video_id.as_str())
                .collect();
        assert_eq!(ticked, ["b"]);
        // The bar for ticked songs is drawn, glass and all.
        w.settle();
        w.find("Clear the ticks", 0);
        // Ticked again: let go.
        let tick = w.find("Tick B", 0);
        w.click(tick.center());
        assert!(w.h.app.selected.is_empty());
        crate::theme::set(crate::theme::Theme::YouTubeMusic);
    }

    /// In every theme, a song the pointer rests on (0.35 s) is found ahead
    /// of time, so a click starts it at once; one the pointer only passes
    /// over is not.
    #[test]
    fn a_song_the_pointer_rests_on_is_found_ahead() {
        use crate::theme::Theme;
        for theme in [
            Theme::YouTubeMusic,
            Theme::Premium,
            Theme::DynamicBackground,
        ] {
            let mut w = Window::new(theme);
            let route = Route::browse("VLPLx".into(), None);
            w.h.act(Action::Navigate(route.clone()));
            let load =
                w.h.requests()
                    .into_iter()
                    .find_map(|r| match r {
                        Request::Page { load, .. } => Some(load),
                        _ => None,
                    })
                    .expect("the page is asked for");
            let page = Page {
                header: Some(ytfast_core::read::Header {
                    title: "A playlist".into(),
                    thumbnail: Some(ytfast_core::read::Thumb {
                        url: "https://example.com/cover.jpg".into(),
                        width: 544,
                    }),
                    ..Default::default()
                }),
                sections: vec![Section {
                    items: ["a", "b", "c"].map(|v| Item::Track(song(v))).to_vec(),
                    ..Section::default()
                }],
                ..Page::default()
            };
            w.h.answer(Event::Page(route, load, Ok(page)));
            // The Play button is named only in the frames it changes in.
            let mut play = None;
            for _ in 0..6 {
                w.frame(Vec::new());
                if let Some((_, rect)) = w.named.iter().find(|(n, _)| n == "Play") {
                    play = Some(*rect);
                }
            }
            let play = play.expect("the page's Play button");
            w.h.requests();
            let warmed = |w: &mut Window| -> Vec<String> {
                w.h.requests()
                    .into_iter()
                    .filter_map(|r| match r {
                        Request::Warm(id) => Some(id),
                        _ => None,
                    })
                    .collect()
            };
            // Passing over A on the way to B: A is not found. B, rested on
            // for 0.2 s (two frames), is.
            let a = w.find("A", 0);
            w.point(a.center());
            let b = w.find("B", 0);
            w.point(b.center());
            for _ in 0..2 {
                w.frame(Vec::new());
            }
            assert_eq!(warmed(&mut w), ["b"], "{theme:?}");
            // The page's Play button: the song it starts.
            w.point(play.center());
            for _ in 0..6 {
                w.frame(Vec::new());
            }
            assert_eq!(warmed(&mut w), ["a"], "{theme:?}");

            // A song's tile, as on Home.
            w.h.act(Action::Navigate(Route::Home));
            let load =
                w.h.requests()
                    .into_iter()
                    .find_map(|r| match r {
                        Request::Page { load, .. } => Some(load),
                        _ => None,
                    })
                    .expect("Home is asked for");
            let tile = ytfast_core::read::Card {
                title: "Tile song".into(),
                subtitle: String::new(),
                thumbnail: Some(ytfast_core::read::Thumb {
                    url: "https://example.com/tile.jpg".into(),
                    width: 226,
                }),
                round: false,
                open: None,
                play: Some(Target::Watch {
                    video_id: Some("t".into()),
                    playlist_id: None,
                }),
                podcast: None,
                look: Default::default(),
            };
            let page = Page {
                sections: vec![Section {
                    title: "Quick picks".into(),
                    items: vec![Item::Card(tile)],
                    ..Section::default()
                }],
                ..Page::default()
            };
            w.h.answer(Event::Page(Route::Home, load, Ok(page)));
            w.settle();
            w.h.requests();
            let tile = w.find("Tile song", 0);
            w.point(tile.center());
            for _ in 0..6 {
                w.frame(Vec::new());
            }
            assert_eq!(warmed(&mut w), ["t"], "{theme:?}");
        }
        crate::theme::set(Theme::YouTubeMusic);
    }

    /// On Windows, in every theme, the window's close button reaches the
    /// top right corner: the pointer thrown into the corner, as far as it
    /// goes, and pressed there, closes the window. (The Mac keeps its own
    /// title bar.)
    #[test]
    fn the_windows_top_right_corner_closes_it() {
        use crate::theme::Theme;
        if !cfg!(windows) {
            return;
        }
        for theme in [
            Theme::YouTubeMusic,
            Theme::Premium,
            Theme::DynamicBackground,
        ] {
            let mut w = Window::new(theme);
            w.settle();
            for corner in [egui::pos2(1279.9, 0.1), egui::pos2(1279.9, 40.0)] {
                w.commands.clear();
                w.click(corner);
                assert!(
                    w.commands
                        .iter()
                        .any(|c| matches!(c, egui::ViewportCommand::Close)),
                    "{theme:?} {corner:?}: {:?}",
                    w.commands
                );
            }
        }
        crate::theme::set(Theme::YouTubeMusic);
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
            words: vec![SuggestedWords {
                text: w.into(),
                forget: None,
            }],
            items: Vec::new(),
        };
        h.answer(Event::Suggestions("faded".into(), words("faded")));
        h.answer(Event::Suggestions("fad".into(), words("fade")));
        assert_eq!(h.app.suggestions.0, "faded");
        assert_eq!(h.app.suggestions.1.words[0].text, "faded");
    }

    #[test]
    fn ticked_songs_play_next_in_order_and_playlists_sort() {
        let mut h = Harness::new();
        h.play(&["a"]);
        h.act(Action::ToggleSelected(song("c")));
        h.act(Action::ToggleSelected(song("b")));
        h.act(Action::ToggleSelected(song("d")));
        h.act(Action::ToggleSelected(song("d")));
        assert_eq!(h.app.selected.len(), 2);
        h.act(Action::PlayNextAll(h.app.selected.clone()));
        let order: Vec<&str> = h
            .app
            .queue
            .entries()
            .iter()
            .map(|e| e.track.video_id.as_str())
            .collect();
        assert_eq!(order, ["a", "c", "b"]);
        assert!(h.app.selected.is_empty());

        // A playlist sorted by title, and back to its own order (asked for
        // again).
        let route = Route::browse("VLPLx".into(), None);
        h.app.load(route.clone());
        let load = h
            .requests()
            .into_iter()
            .find_map(|r| match r {
                Request::Page { load, .. } => Some(load),
                _ => None,
            })
            .expect("the page is asked for");
        let page = Page {
            sections: vec![Section {
                items: ["b", "c", "a"].map(|v| Item::Track(song(v))).to_vec(),
                ..Section::default()
            }],
            ..Page::default()
        };
        h.answer(Event::Page(route.clone(), load, Ok(page)));
        h.act(Action::SortPlaylist(
            route.clone(),
            Some(PlaylistSort::Title),
        ));
        let titles = |h: &Harness| match h.app.pages.get(&route) {
            Some(Loadable::Ready(page)) => page.tracks().into_iter().map(|t| t.title).collect(),
            _ => Vec::new(),
        };
        assert_eq!(titles(&h), ["A", "B", "C"]);
        h.act(Action::SortPlaylist(route.clone(), None));
        assert!(
            h.requests()
                .iter()
                .any(|r| matches!(r, Request::Page { route: r, .. } if *r == route))
        );
    }

    /// Home shows its first shelves, and its next ones come as its end
    /// comes into view, one batch at a time, until YouTube has no more;
    /// shelves for an older loading of Home are dropped.
    #[test]
    fn home_loads_its_next_shelves_as_its_end_comes_into_view() {
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        w.h.act(Action::Navigate(Route::Home));
        w.h.app.load(Route::Home);
        let load =
            w.h.requests()
                .into_iter()
                .filter_map(|r| match r {
                    Request::Page {
                        route: Route::Home,
                        load,
                        ..
                    } => Some(load),
                    _ => None,
                })
                .last()
                .expect("Home is asked for");
        let shelf = |title: &str, ids: [&str; 2]| Section {
            title: title.into(),
            items: ids.map(|v| Item::Track(song(v))).to_vec(),
            ..Section::default()
        };
        let page = Page {
            sections: vec![shelf("Quick picks", ["a", "b"])],
            ..Page::default()
        };
        w.h.answer(Event::Page(Route::Home, load, Ok(page)));
        let asked = |w: &mut Window| {
            w.h.requests()
                .into_iter()
                .filter(
                    |r| matches!(r, Request::MoreResults { route, .. } if *route == Route::Home),
                )
                .count()
        };
        // Its end is in view (one short shelf): asked once, however many
        // frames pass before the answer.
        w.settle();
        assert_eq!(asked(&mut w), 1);
        w.h.answer(Event::MoreSections {
            route: Route::Home,
            load,
            sections: vec![shelf("Mixed for you", ["c", "d"])],
            done: false,
        });
        let titles = |w: &Window| match w.h.app.pages.get(&Route::Home) {
            Some(Loadable::Ready(page)) => page
                .sections
                .iter()
                .map(|s| s.title.clone())
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        assert_eq!(titles(&w), ["Quick picks", "Mixed for you"]);
        // Shelves for an older loading change nothing.
        w.h.answer(Event::MoreSections {
            route: Route::Home,
            load: load - 1,
            sections: vec![shelf("Stale", ["e", "f"])],
            done: false,
        });
        assert_eq!(titles(&w).len(), 2);
        // The new end in view: the next batch, which is the last.
        w.settle();
        assert_eq!(asked(&mut w), 1);
        w.h.answer(Event::MoreSections {
            route: Route::Home,
            load,
            sections: vec![shelf("Forgotten favourites", ["g", "h"])],
            done: true,
        });
        assert_eq!(titles(&w).len(), 3);
        w.settle();
        assert_eq!(asked(&mut w), 0);
    }

    #[test]
    fn a_search_of_one_kind_asks_for_more_one_batch_at_a_time() {
        let mut h = Harness::new();
        let route = Route::SearchOnly("glass".into(), "songs".into());
        h.app.load(route.clone());
        let load = h
            .requests()
            .into_iter()
            .find_map(|r| match r {
                Request::Page { load, .. } => Some(load),
                _ => None,
            })
            .expect("the page is asked for");
        let page = Page {
            sections: vec![Section {
                title: "Songs".into(),
                items: vec![Item::Track(song("a"))],
                ..Section::default()
            }],
            ..Page::default()
        };
        h.answer(Event::Page(route.clone(), load, Ok(page)));
        let asked = |h: &mut Harness| {
            h.requests()
                .iter()
                .filter(|r| matches!(r, Request::MoreResults { .. }))
                .count()
        };
        // The end in view frame after frame: one request until it answers.
        h.act(Action::MoreResults(route.clone()));
        h.act(Action::MoreResults(route.clone()));
        assert_eq!(asked(&mut h), 1);
        h.answer(Event::MoreResults {
            route: route.clone(),
            load,
            items: vec![Item::Track(song("b"))],
            done: false,
        });
        let shown = |h: &Harness| match h.app.pages.get(&route) {
            Some(Loadable::Ready(page)) => page.tracks().len(),
            _ => 0,
        };
        assert_eq!(shown(&h), 2);
        // The new end in view: the next batch, which is the last.
        h.act(Action::MoreResults(route.clone()));
        assert_eq!(asked(&mut h), 1);
        h.answer(Event::MoreResults {
            route: route.clone(),
            load,
            items: vec![Item::Track(song("c"))],
            done: true,
        });
        assert_eq!(shown(&h), 3);
        h.act(Action::MoreResults(route.clone()));
        assert_eq!(asked(&mut h), 0);
    }

    #[test]
    fn a_song_started_by_its_id_alone_gets_its_name() {
        let mut h = Harness::new();
        // An artist's Mix names only the song it starts with.
        h.act(Action::Play(
            Target::Watch {
                video_id: Some("a".into()),
                playlist_id: Some("RDa".into()),
            },
            None,
        ));
        assert_eq!(h.entry().1, "a");
        assert_eq!(h.app.playback.entry.as_ref().unwrap().track.title, "Song");
        // The player's answer names it.
        let (entry, _) = h.entry();
        let mut ready = ready();
        ready.info.title = Some("Yellow".into());
        ready.info.author = Some("Coldplay - Topic".into());
        h.answer(Event::Prepared {
            entry,
            result: Ok(ready),
        });
        let playing = &h.app.playback.entry.as_ref().unwrap().track;
        assert_eq!(
            (playing.title.as_str(), playing.artists.as_str()),
            ("Yellow", "Coldplay")
        );
        // Up next brings its cover and album, and leaves the name alone.
        let full = Track {
            video_id: "a".into(),
            title: "Yellow (Remastered)".into(),
            artists: "Coldplay".into(),
            album: Some("Parachutes".into()),
            thumbnail: Some(ytfast_core::read::Thumb {
                url: "https://example.com/cover.jpg".into(),
                width: 60,
            }),
            ..Track::default()
        };
        h.answer(Event::UpNext {
            video_id: "a".into(),
            playlist_id: Some("RDa".into()),
            queue: h.app.queue.generation(),
            result: Ok((vec![full, song("b")], None)),
        });
        let playing = &h.app.playback.entry.as_ref().unwrap().track;
        assert_eq!(playing.title, "Yellow");
        assert_eq!(playing.album.as_deref(), Some("Parachutes"));
        assert!(playing.thumbnail.is_some());
        assert_eq!(
            h.app.queue.current().unwrap().track.album.as_deref(),
            Some("Parachutes")
        );
    }

    #[test]
    fn a_library_tab_loads_again_in_the_order_chosen() {
        let mut h = Harness::new();
        h.act(Action::SortLibrary(SortOrder {
            title: "A to Z".into(),
            browse_id: "FEmusic_liked_videos".into(),
            params: "az".into(),
        }));
        let asked = |requests: Vec<Request>| {
            requests.into_iter().find_map(|r| match r {
                Request::Page {
                    route: Route::LibrarySongs,
                    order,
                    ..
                } => Some(order),
                _ => None,
            })
        };
        assert_eq!(asked(h.requests()), Some(Some("az".to_string())));
        // Later loadings keep it; other tabs keep their own.
        h.app.load(Route::LibrarySongs);
        assert_eq!(asked(h.requests()), Some(Some("az".to_string())));
        h.app.load(Route::LibraryAlbums);
        assert!(h.requests().iter().any(|r| matches!(
            r,
            Request::Page {
                route: Route::LibraryAlbums,
                order: None,
                ..
            }
        )));
    }

    #[test]
    fn a_removed_past_search_stays_gone() {
        let mut h = Harness::new();
        let past = |text: &str, token: &str| SuggestedWords {
            text: text.into(),
            forget: Some(token.into()),
        };
        let found = ytfast_core::read::Suggestions {
            words: vec![past("faded", "t1"), past("yellow", "t2")],
            items: Vec::new(),
        };
        h.act(Action::Suggest(String::new()));
        h.answer(Event::Suggestions(String::new(), found.clone()));
        h.requests();
        h.act(Action::ForgetSearch {
            words: "faded".into(),
            token: "t1".into(),
        });
        // Gone from the list at once, and YouTube is asked to forget it.
        let shown: Vec<&str> = h
            .app
            .suggestions
            .1
            .words
            .iter()
            .map(|w| w.text.as_str())
            .collect();
        assert_eq!(shown, ["yellow"]);
        assert!(h.requests().iter().any(|r| matches!(
            r,
            Request::Edit(Edit::ForgetSearch { token, .. }) if token == "t1"
        )));
        // An answer asked for before YouTube forgot it leaves it out too.
        h.act(Action::Suggest(String::new()));
        h.answer(Event::Suggestions(String::new(), found.clone()));
        assert_eq!(h.app.suggestions.1.words.len(), 1);
        // Refused: it may show again.
        h.answer(Event::EditFailed(
            Edit::ForgetSearch {
                words: "faded".into(),
                token: "t1".into(),
            },
            "no".into(),
        ));
        h.answer(Event::Suggestions(String::new(), found));
        assert_eq!(h.app.suggestions.1.words.len(), 2);
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

    /// A song with a music video (as Up next pairs them).
    fn song_with_video(id: &str, video: &str) -> Track {
        Track {
            more: Some(Box::new(ytfast_core::read::TrackMore {
                counterpart: Some(video.to_string()),
                ..Default::default()
            })),
            ..song(id)
        }
    }

    /// A video with no map of its song in it.
    fn pair(video: &str) -> VideoPair {
        VideoPair {
            video: video.into(),
            segments: Vec::new(),
        }
    }

    /// The song's music 21 s into its video, as Despacito's.
    fn late_start() -> Vec<ytfast_core::read::Segment> {
        vec![ytfast_core::read::Segment {
            song_ms: 0,
            video_ms: 21_000,
            length_ms: 228_000,
        }]
    }

    /// What was asked to be got ready (to play, or ahead), by ID.
    fn prepares(requests: &[Request], play: bool) -> Vec<String> {
        requests
            .iter()
            .filter_map(|r| match r {
                Request::Prepare {
                    video_id, play: p, ..
                } if *p == play => Some(video_id.clone()),
                _ => None,
            })
            .collect()
    }

    fn videos_asked(requests: &[Request]) -> Vec<String> {
        requests
            .iter()
            .filter_map(|r| match r {
                Request::Video { video_id, .. } => Some(video_id.clone()),
                _ => None,
            })
            .collect()
    }

    /// Where the player was told to start the song.
    fn started_from(commands: &[Command]) -> Option<f64> {
        commands.iter().find_map(|c| match c {
            Command::Play { from, .. } => Some(*from),
            _ => None,
        })
    }

    /// The player says the song playing is at `position`.
    fn at(h: &mut Harness, position: f64) {
        let (entry, _) = h.entry();
        h.app.audio.set_status(|s| {
            s.entry = Some(entry);
            s.position = position;
            s.length = 200.0;
        });
        h.frame();
    }

    /// The video mode plays the queue's songs as their videos (Next, and
    /// the song after, got ready ahead as its video); a song chosen
    /// anywhere else plays as a song, and the mode is off.
    #[test]
    fn the_video_mode_keeps_to_the_queue() {
        let mut h = Harness::new();
        let mut b = song("b");
        b.kind = TrackKind::MusicVideo;
        h.act(Action::PlayTracks {
            tracks: vec![song_with_video("a", "va"), b, song("c")],
            start: 0,
            source: None,
        });
        h.ready();
        at(&mut h, 42.0);
        h.requests();
        h.commands();

        // Turned on halfway through: its video plays on from there.
        h.act(Action::VideoMode(true));
        assert_eq!(prepares(&h.requests(), true), ["va"]);
        h.ready();
        assert_eq!(started_from(&h.commands()), Some(42.0));
        let asked = h.requests();
        assert_eq!(videos_asked(&asked), ["va"]);
        // The next is a video itself: got ready ahead as it is.
        assert_eq!(prepares(&asked, false), ["b"]);

        // Next: the video plays from its start, and its picture comes.
        h.act(Action::Next);
        assert_eq!(prepares(&h.requests(), true), ["b"]);
        h.ready();
        assert_eq!(started_from(&h.commands()), Some(0.0));
        let asked = h.requests();
        assert_eq!(videos_asked(&asked), ["b"]);
        // Whether the song after has a video is asked first, then it is got
        // ready as its video.
        assert!(prepares(&asked, false).is_empty());
        assert!(
            asked
                .iter()
                .any(|r| matches!(r, Request::Details(id) if id == "c"))
        );
        h.answer(Event::SongVideo("c".into(), Ok(Some(pair("vc")))));
        assert_eq!(prepares(&h.requests(), false), ["vc"]);
        h.act(Action::Next);
        assert_eq!(prepares(&h.requests(), true), ["vc"]);
        assert!(h.app.video_mode);

        // A song chosen on a page: a song again.
        h.act(Action::PlayTracks {
            tracks: vec![song_with_video("d", "vd")],
            start: 0,
            source: None,
        });
        assert!(!h.app.video_mode);
        assert!(h.app.video.is_none());
        assert_eq!(prepares(&h.requests(), true), ["d"]);
    }

    /// Turned off, the song plays on from where its video was; a video
    /// itself keeps playing, only its picture goes.
    #[test]
    fn turning_the_video_mode_off_plays_the_song_from_there() {
        let mut h = Harness::new();
        h.act(Action::VideoMode(true));
        h.act(Action::PlayNext(song_with_video("a", "va")));
        h.requests();
        h.ready();
        assert_eq!(h.app.playback.version.as_deref(), Some("va"));
        at(&mut h, 30.0);
        h.requests();
        h.commands();
        h.act(Action::VideoMode(false));
        assert_eq!(prepares(&h.requests(), true), ["a"]);
        assert!(h.app.video.is_none());
        h.ready();
        assert_eq!(started_from(&h.commands()), Some(30.0));

        // A video itself: the same sound, so nothing is fetched again.
        let mut h = Harness::new();
        let mut v = song("v");
        v.kind = TrackKind::MusicVideo;
        h.act(Action::PlayNext(v));
        h.ready();
        h.requests();
        h.act(Action::VideoMode(true));
        let asked = h.requests();
        assert!(prepares(&asked, true).is_empty());
        assert_eq!(videos_asked(&asked), ["v"]);
        h.act(Action::VideoMode(false));
        assert!(prepares(&h.requests(), true).is_empty());
        assert!(h.app.video.is_none());
    }

    /// A song whose video is not known waits for its details in the video
    /// mode, then plays as its video; one without a video, or whose
    /// details did not load, plays as itself.
    #[test]
    fn the_video_mode_waits_to_hear_of_a_songs_video() {
        let mut h = Harness::new();
        h.app.video_mode = true;
        h.act(Action::PlayNext(song("a")));
        let asked = h.requests();
        assert!(prepares(&asked, true).is_empty());
        assert!(
            asked
                .iter()
                .any(|r| matches!(r, Request::Details(id) if id == "a"))
        );
        assert!(matches!(
            h.app.video.as_ref().map(|v| &v.state),
            Some(VideoState::Finding)
        ));
        h.answer(Event::SongVideo("a".into(), Ok(Some(pair("va")))));
        assert_eq!(prepares(&h.requests(), true), ["va"]);

        // None: its cover, and it plays as itself.
        h.act(Action::PlayNext(song("b")));
        h.act(Action::Next);
        h.requests();
        h.answer(Event::SongVideo("b".into(), Ok(None)));
        assert_eq!(prepares(&h.requests(), true), ["b"]);
        assert!(matches!(
            h.app.video.as_ref().map(|v| &v.state),
            Some(VideoState::Missing)
        ));

        // Its details did not load: as itself, and says so.
        h.act(Action::PlayNext(song("c")));
        h.act(Action::Next);
        h.requests();
        h.answer(Event::SongVideo("c".into(), Err("no network".into())));
        assert_eq!(prepares(&h.requests(), true), ["c"]);
        h.ready();
        assert_eq!(h.app.playback.state, PlayState::Playing);
        assert!(matches!(
            h.app.video.as_ref().map(|v| &v.state),
            Some(VideoState::Failed(_))
        ));
    }

    /// Turned on while the song gets ready: what is ready does not play;
    /// its video is got ready instead, from the start.
    #[test]
    fn the_video_mode_turned_on_while_a_song_gets_ready() {
        let mut h = Harness::new();
        h.act(Action::PlayNext(song_with_video("a", "va")));
        h.requests();
        h.act(Action::VideoMode(true));
        // Nothing asked twice while the song's own is on its way.
        assert!(prepares(&h.requests(), true).is_empty());
        h.ready();
        assert_eq!(plays(&h.commands()), 0);
        assert_eq!(prepares(&h.requests(), true), ["va"]);
        h.ready();
        assert_eq!(started_from(&h.commands()), Some(0.0));
        assert_eq!(videos_asked(&h.requests()), ["va"]);
        // The picture's answer is taken for this song only.
        let (entry, _) = h.entry();
        h.answer(Event::Video {
            entry: entry + 1,
            result: Ok(None),
        });
        assert!(matches!(
            h.app.video.as_ref().map(|v| &v.state),
            Some(VideoState::Loading)
        ));
        h.answer(Event::Video {
            entry,
            result: Ok(None),
        });
        assert!(matches!(
            h.app.video.as_ref().map(|v| &v.state),
            Some(VideoState::Demo)
        ));
    }

    /// The player page in each look, with the video mode on: the switch's
    /// two buttons, Video pressed and Song pressed.
    #[test]
    fn the_song_and_video_switch_on_the_player_page() {
        for theme in [
            crate::theme::Theme::YouTubeMusic,
            crate::theme::Theme::Premium,
            crate::theme::Theme::DynamicBackground,
        ] {
            let mut w = Window::new(theme);
            w.h.play(&["a", "b"]);
            w.h.app.now_playing = true;
            w.settle();
            let video = w.find("Video", 0);
            w.click(video.center());
            w.settle();
            assert!(w.h.app.video_mode, "{theme:?}");
            // Its demo stand-in shows in the cover's place.
            w.h.answer(Event::SongVideo("a".into(), Ok(Some(pair("a")))));
            let (entry, _) = w.h.entry();
            w.h.answer(Event::Video {
                entry,
                result: Ok(None),
            });
            w.settle();
            assert!(w.shows("Song"), "{theme:?}");
            let song = w.find("Song", 0);
            w.click(song.center());
            w.settle();
            assert!(!w.h.app.video_mode, "{theme:?}");
        }
        crate::theme::set(crate::theme::Theme::YouTubeMusic);
    }

    /// Switching between a song and its music video plays the same music:
    /// a video whose song starts 21 s in is joined 21 s later, and left
    /// 21 s earlier; its own intro leaves the song at its start.
    #[test]
    fn switching_lands_on_the_same_music() {
        let mut h = Harness::new();
        let mut a = song_with_video("a", "va");
        a.more.as_mut().unwrap().segments = late_start();
        h.act(Action::PlayNext(a));
        h.ready();
        at(&mut h, 42.0);
        h.requests();
        h.commands();
        h.act(Action::VideoMode(true));
        h.ready();
        assert_eq!(started_from(&h.commands()), Some(63.0));
        // Back to the song from 70 s into the video: the song's 49 s.
        at(&mut h, 70.0);
        h.act(Action::VideoMode(false));
        h.ready();
        assert_eq!(started_from(&h.commands()), Some(49.0));
        // From the video's own intro: the song's start.
        h.act(Action::VideoMode(true));
        h.ready();
        at(&mut h, 5.0);
        h.commands();
        h.act(Action::VideoMode(false));
        h.ready();
        assert_eq!(started_from(&h.commands()), Some(0.0));
    }

    /// The lyrics are timed to the song: while its video plays they follow
    /// the song's moment (none lit in the video's own intro), and a line
    /// chosen jumps to its place in the video.
    #[test]
    fn lyrics_follow_the_song_while_its_video_plays() {
        let mut h = Harness::new();
        let mut a = song_with_video("a", "va");
        a.more.as_mut().unwrap().segments = late_start();
        h.app.video_mode = true;
        h.act(Action::PlayNext(a));
        h.ready();
        assert_eq!(h.app.playback.version.as_deref(), Some("va"));
        at(&mut h, 81.0);
        assert_eq!(h.app.lyrics_clock(), Some(60.0));
        assert_eq!(h.app.lyrics_moment(60.0), 81.0);
        at(&mut h, 10.0);
        assert_eq!(h.app.lyrics_clock(), None);
        // As the song itself, its own times.
        h.act(Action::VideoMode(false));
        h.ready();
        at(&mut h, 30.0);
        assert_eq!(h.app.lyrics_clock(), Some(30.0));
        assert_eq!(h.app.lyrics_moment(30.0), 30.0);
    }

    /// A song without a video has Video greyed out: pressing it does
    /// nothing. The mode stays as it was for the songs to come.
    #[test]
    fn video_is_greyed_out_for_a_song_without_one() {
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        w.h.play(&["a", "b"]);
        w.h.app.now_playing = true;
        w.h.answer(Event::SongVideo("a".into(), Ok(None)));
        w.settle();
        assert_eq!(w.h.app.has_video(), Some(false));
        let video = w.find("Video", 0);
        w.click(video.center());
        w.settle();
        assert!(!w.h.app.video_mode);
        assert_eq!(w.cursor, egui::CursorIcon::NotAllowed);
        // A song with one: Video can be pressed.
        w.h.answer(Event::SongVideo("a".into(), Ok(Some(pair("va")))));
        w.settle();
        let video = w.find("Video", 0);
        w.click(video.center());
        w.settle();
        assert!(w.h.app.video_mode);
    }

    /// A song without a video between two with one: the video mode stays
    /// on, and the next song plays as its video again.
    #[test]
    fn the_video_mode_outlasts_a_song_without_a_video() {
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        w.h.play(&["g", "m", "l"]);
        w.h.app.now_playing = true;
        w.h.answer(Event::SongVideo("g".into(), Ok(Some(pair("g")))));
        w.settle();
        let video = w.find("Video", 0);
        w.click(video.center());
        w.settle();
        assert!(w.h.app.video_mode, "on");
        w.h.answer(Event::SongVideo("m".into(), Ok(None)));
        let next = w.find("Next", 0);
        w.click(next.center());
        w.settle();
        w.h.ready();
        w.settle();
        assert_eq!(w.h.entry().1, "m");
        assert!(w.h.app.video_mode, "on through a song without a video");
        w.h.answer(Event::SongVideo("l".into(), Ok(Some(pair("l")))));
        w.settle();
        let next = w.find("Next", 0);
        w.click(next.center());
        w.settle();
        w.h.ready();
        w.settle();
        assert_eq!(w.h.entry().1, "l");
        assert!(w.h.app.video_mode, "still on");
    }

    /// Disliking the song playing moves on to the next, as YouTube Music
    /// does; another song's dislike, or with the setting off, does not.
    #[test]
    fn disliking_the_song_playing_skips_it() {
        let mut h = Harness::new();
        h.play(&["a", "b", "c"]);
        h.act(Action::Rate("b".into(), LikeState::Disliked));
        assert_eq!(h.entry().1, "a");
        h.act(Action::Rate("a".into(), LikeState::Disliked));
        assert_eq!(h.entry().1, "b");
        assert_eq!(h.app.likes.get("a"), Some(&LikeState::Disliked));
        h.ready();
        h.app.settings.skip_disliked = false;
        h.act(Action::Rate("b".into(), LikeState::Disliked));
        assert_eq!(h.entry().1, "b");
        // Liking or taking the dislike back never skips.
        h.app.settings.skip_disliked = true;
        h.act(Action::Rate("b".into(), LikeState::Neutral));
        h.act(Action::Rate("b".into(), LikeState::Liked));
        assert_eq!(h.entry().1, "b");
    }

    /// The window asked to close while a song plays: it stays and asks.
    /// Yes closes it (and, with "Do not ask again" ticked, it never asks
    /// again); No keeps it. Paused, or for an update, it closes at once.
    #[test]
    fn closing_while_a_song_plays_asks_first() {
        let cancels = |commands: &[egui::ViewportCommand]| {
            commands.contains(&egui::ViewportCommand::CancelClose)
        };
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        // Nothing playing: it closes.
        assert!(!cancels(&w.close_request()));
        w.h.play(&["a", "b"]);
        // Paused: it closes.
        w.h.app.audio.set_status(|s| s.paused = true);
        w.h.frame();
        assert!(!cancels(&w.close_request()));
        w.h.app.audio.set_status(|s| s.paused = false);
        w.h.frame();
        // Playing: it stays, and asks, "Do not ask again" ticked.
        assert!(cancels(&w.close_request()));
        assert!(matches!(
            *w.h.app.dialog.borrow(),
            Some(Dialog::ConfirmClose { dont_ask: true })
        ));
        w.settle();
        assert!(w.shows("Do not ask again"));
        // No: it stays, and asks next time.
        let no = w.find("No", 0);
        w.click(no.center());
        w.settle();
        assert!(w.h.app.dialog.borrow().is_none());
        assert!(w.h.app.settings.confirm_close);
        assert!(cancels(&w.close_request()));
        w.settle();
        // Unticked, then Yes: it closes, and asks again another time.
        let tick = w.find("Do not ask again", 0);
        w.click(tick.center());
        w.settle();
        let yes = w.find("Yes", 0);
        w.click(yes.center());
        w.settle();
        assert!(w.h.app.settings.confirm_close);
        assert!(w.close_request().contains(&egui::ViewportCommand::Close));
        assert!(!cancels(&w.close_request()));

        // Ticked (as it opens), Yes: never asks again.
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        w.h.play(&["a"]);
        assert!(cancels(&w.close_request()));
        w.settle();
        let yes = w.find("Yes", 0);
        w.click(yes.center());
        w.settle();
        assert!(!w.h.app.settings.confirm_close);
        assert!(w.close_request().contains(&egui::ViewportCommand::Close));
        assert!(!cancels(&w.close_request()));

        // An update closing YTFast never asks.
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        w.h.play(&["a"]);
        w.h.app.restarting = true;
        assert!(!cancels(&w.close_request()));
    }

    /// The close question in each look: drawn, and answered.
    #[test]
    fn the_close_question_in_each_look() {
        use crate::theme::Theme;
        for theme in [
            Theme::YouTubeMusic,
            Theme::Premium,
            Theme::DynamicBackground,
        ] {
            let mut w = Window::new(theme);
            w.h.play(&["a"]);
            w.close_request();
            w.settle();
            for name in ["Yes", "No", "Do not ask again"] {
                assert!(w.shows(name), "{theme:?}: {name}");
            }
            let yes = w.find("Yes", 0);
            w.click(yes.center());
            w.settle();
            assert!(!w.h.app.settings.confirm_close, "{theme:?}");
        }
        crate::theme::set(Theme::YouTubeMusic);
    }

    /// F: the window to the whole screen with the player page, its top
    /// bar and menu gone and the cover larger, in each look; Esc leaves
    /// full screen (the player page stays), and so does leaving the
    /// player page.
    #[test]
    fn f_puts_the_player_page_in_full_screen() {
        use crate::theme::Theme;
        for theme in [
            Theme::YouTubeMusic,
            Theme::Premium,
            Theme::DynamicBackground,
        ] {
            let mut w = Window::new(theme);
            w.h.play(&["a"]);
            w.h.app.now_playing = true;
            w.settle();
            let windowed = w.largest("Pause");
            assert!(w.shows("Home"), "{theme:?}");
            let told = w.press(egui::Key::F);
            assert!(w.h.app.fullscreen && w.h.app.now_playing, "{theme:?}");
            assert!(
                told.contains(&egui::ViewportCommand::Fullscreen(true)),
                "{theme:?} {told:?}"
            );
            w.settle();
            assert!(!w.shows("Home"), "{theme:?}: the menu is gone");
            let full = w.largest("Pause");
            assert!(
                full.width() > windowed.width() * 1.05,
                "{theme:?}: {windowed:?} then {full:?}"
            );
            for name in ["UP NEXT", "Song", "Video"] {
                assert!(w.shows(name), "{theme:?}: {name}");
            }
            // Esc: out of full screen, still on the player page.
            let told = w.press(egui::Key::Escape);
            assert!(!w.h.app.fullscreen && w.h.app.now_playing, "{theme:?}");
            assert!(told.contains(&egui::ViewportCommand::Fullscreen(false)));
            w.settle();
            assert!(w.shows("Home"), "{theme:?}");
            // Leaving the player page leaves full screen.
            w.press(egui::Key::F);
            assert!(w.h.app.fullscreen);
            w.press(egui::Key::Q);
            assert!(!w.h.app.fullscreen && !w.h.app.now_playing, "{theme:?}");
        }
        crate::theme::set(Theme::YouTubeMusic);
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
