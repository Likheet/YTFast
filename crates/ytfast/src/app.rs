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
    /// ([`Dialog::ConfirmClose`]). Only with `keep_running` off: on, closing
    /// stops nothing.
    pub confirm_close: bool,
    /// Lyrics show each line in English too, and in Latin letters when
    /// written in another script (the Lyrics tab's Translate).
    pub translate_lyrics: bool,
    /// Closing the window leaves YTFast running, the music playing, its
    /// icon by the clock (`tray`), whose menu quits. Off: closing quits.
    pub keep_running: bool,
    /// YTFast opens when the user signs in to the computer, without its
    /// window (`login`).
    pub open_at_login: bool,
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
            translate_lyrics: false,
            keep_running: true,
            open_at_login: false,
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
    /// The song playing when YTFast was last closed ([`LeftOff`]), kept
    /// at its second (`Playback::start_at`): nothing is asked of YouTube
    /// until Play starts it there.
    LeftOff,
}

/// The queue as YTFast was last closed (the owner's word, 11 October
/// 2026): its songs around the one playing (at most [`LEFT_OFF_BEFORE`]
/// before it and [`LEFT_OFF_AFTER`] after), which of them played, and the
/// second it was at. Saved by eframe beside the settings, under
/// [`LEFT_OFF_KEY`] (not in the demo), and taken up at the first sign-in,
/// paused there ([`PlayState::LeftOff`]).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LeftOff {
    pub tracks: Vec<Track>,
    /// Which of `tracks` played.
    pub current: usize,
    /// Where in it, in seconds of the song (a video's moment is saved as
    /// its song's).
    pub at: f64,
    /// What Up next says it plays from, and where more songs come from.
    pub title: Option<String>,
    pub source: Option<String>,
}

/// The key the queue is saved under, beside the settings' "ytfast".
const LEFT_OFF_KEY: &str = "ytfast-queue";
/// The most songs kept before the one playing, and after it.
const LEFT_OFF_BEFORE: usize = 20;
const LEFT_OFF_AFTER: usize = 200;

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
    /// It starts paused there: the other version of a song that was paused
    /// when the video mode was turned on or off (the owner found the switch
    /// playing it).
    pub start_paused: bool,
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
            start_paused: false,
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
    Autoplay,
    EvenLoudness,
    MovingBackground,
    AutoUpdate,
    SkipDisliked,
    ConfirmClose,
    TranslateLyrics,
    KeepRunning,
    OpenAtLogin,
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
    /// A Mac's YTFast opened from its disk image offers to move itself to
    /// Applications (`update_mac::move_to_applications`).
    MoveToApplications,
    /// "Replace your queue?": `then` would remove the `lined_up` songs the
    /// user put in the queue by hand ([`asks_first`]).
    ReplaceQueue {
        lined_up: usize,
        then: Box<Action>,
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
#[derive(Clone, Debug)]
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
    /// A Mac's YTFast moves itself from its disk image to Applications,
    /// closes, and opens there.
    MoveToApplications,
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
    /// An action the user was asked about and said yes to: it replaces
    /// songs they lined up.
    Confirmed(Box<Action>),
}

/// From how many lined-up songs an action that would remove them asks
/// first (the owner's choice: two Play nexts mean a queue being set up).
const ASK_FROM: usize = 2;

/// Whether `action` asks before it runs: it puts a new queue in place of
/// one with `lined_up` songs the user put in by hand, which a stray click
/// would lose.
fn asks_first(action: &Action, lined_up: usize) -> bool {
    lined_up >= ASK_FROM
        && matches!(
            action,
            Action::PlayTracks { .. }
                | Action::Shuffle { .. }
                | Action::Open(Target::Watch { .. }, _)
                | Action::Play(Target::Watch { .. }, _)
                | Action::QueuePlaylist(_, QueueMode::Play | QueueMode::Shuffle)
        )
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
    /// A sideways swipe going Back or Forward (`swipe.rs`).
    pub swipe: RefCell<crate::swipe::Swipe>,
    /// The queue saved when YTFast was last closed, until it is taken up
    /// at the first sign-in ([`LeftOff`]).
    pub left_off: Option<LeftOff>,
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
    /// Draws whichever window is open again, or wakes YTFast running
    /// without one (fastframe-shell): what the threads and the icon hold.
    pub waker: fastframe_shell::Waker,
    /// The icon by the clock (`tray`); none in the tests, or where it
    /// cannot be made.
    tray: Option<fastframe_tray::Tray>,
    /// What the icon's menu and tooltip say now.
    tray_shown: crate::tray::Shown,
    /// The icon is shown, so a window closed can leave YTFast running.
    pub has_tray: bool,
    /// A window is open; none while YTFast runs on, closed (or started
    /// with the computer).
    pub window_open: bool,
    /// The window is closing and YTFast runs on without it.
    hide_intent: bool,
    /// A window is wanted (the icon, the Dock, another launch).
    wants_show: bool,
    /// YTFast is quitting: the icon's Quit, the close question's Yes, an
    /// update, or with "Keep playing when closed" off, the window's close.
    quit_requested: bool,
    /// What other launches asked for (`instance`).
    asked: std::sync::Arc<crate::instance::Asked>,
    /// Where the settings and the queue are saved while no window is open
    /// (eframe saves them while one is); none in the tests.
    store: Option<crate::store::Store>,
    /// What was saved while no window was open, last, and when.
    saved_headless: Option<(Instant, Vec<(&'static str, String)>)>,
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
    /// The window was maximized when full screen began, and is maximized
    /// again when it ends (Windows).
    maximized_before_fullscreen: bool,
    /// Lyrics and related pages, by song.
    pub lyrics: HashMap<String, crate::lyrics::State>,
    /// Lyrics translated, by song.
    pub translations: HashMap<String, crate::lyrics::Translation>,
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
    /// The app, before any window: it may run without one from the start
    /// (with the computer). Its settings and queue come from `store`, as
    /// eframe kept them; `waker` wakes it, with or without a window.
    pub fn new(
        waker: &fastframe_shell::Waker,
        demo: bool,
        startup: Startup,
        store: Option<crate::store::Store>,
        asked: std::sync::Arc<crate::instance::Asked>,
    ) -> Self {
        let settings: Settings = store
            .as_ref()
            .and_then(|s| s.get::<Settings>("ytfast"))
            .map(Settings::loaded)
            .unwrap_or_default();

        let wake = {
            let waker = waker.clone();
            move || waker.wake()
        };
        let backend = Backend::start(wake.clone(), demo).expect("YtFast's folders can be made");
        let audio = Audio::start(wake.clone(), demo);

        let mut info = now_playing::App::new("ytfast", "YTFast");
        info.can_raise = true;
        let controls = Some(now_playing::NowPlaying::start(info, wake.clone()));
        let tray = crate::tray::spawn(demo, wake.clone());
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
            Some(crate::update::Updates::start(settings.auto_update, wake))
        };
        // This copy starts with the computer while the setting is on.
        if !demo {
            crate::login::set(settings.open_at_login);
        }
        let fonts = crate::theme::ScriptFonts::new();
        let mut app = Self::with(backend, audio, controls, fonts, settings, demo);
        // Where the music was when YTFast was last closed: taken up once
        // signed in.
        if !demo {
            app.left_off = store
                .as_ref()
                .and_then(|s| s.get::<Option<LeftOff>>(LEFT_OFF_KEY))
                .flatten();
        }
        // A Mac's YTFast opened from its disk image offers to move itself.
        if !demo && crate::update_mac::from_disk_image() {
            *app.dialog.get_mut() = Some(Dialog::MoveToApplications);
        }
        app.waker = waker.clone();
        app.has_tray = tray.as_ref().is_some_and(fastframe_tray::Tray::is_shown);
        app.tray = tray;
        app.asked = asked;
        app.store = store;
        app.updates = updates;
        app.receipt = startup.receipt;
        if let Some(error) = startup.update_error {
            app.notify(error);
        }
        app
    }

    /// A window opened (the first, or one asked for after YTFast ran on
    /// without one): what egui needs in it, set up again, as the window
    /// before took all it held.
    pub fn attach(&mut self, ctx: &egui::Context) {
        crate::theme::install_in(ctx, &mut self.script_fonts);
        // The demo laid out as on another screen (`YTFAST_DEMO_ZOOM`): 0.67
        // shows a laptop's window as a 2560 wide screen's, smaller, for
        // pictures of large screens on a small one. Set every time, as the
        // demo's saved window state would otherwise keep the last one.
        if self.demo {
            let zoom = std::env::var("YTFAST_DEMO_ZOOM")
                .ok()
                .and_then(|z| z.parse::<f32>().ok())
                .filter(|z| (0.25..=4.0).contains(z))
                .unwrap_or(1.0);
            ctx.set_zoom_factor(zoom);
        }
        // egui's colours and the look's fonts, set for this window.
        self.styled = None;
        self.window_checked = false;
        self.window_open = true;
        self.hide_intent = false;
        self.wants_show = false;
        // A Mac's menu bar item is made with the first window, and each
        // window brings YTFast forward.
        if let Some(tray) = &mut self.tray {
            tray.attach();
        }
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
            swipe: RefCell::default(),
            left_off: None,
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
            waker: fastframe_shell::Waker::default(),
            tray: None,
            tray_shown: crate::tray::Shown::default(),
            has_tray: false,
            // The tests' window; from the first window on, the shell's.
            window_open: true,
            hide_intent: false,
            wants_show: false,
            quit_requested: false,
            asked: std::sync::Arc::default(),
            store: None,
            saved_headless: None,
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
            maximized_before_fullscreen: false,
            lyrics: HashMap::new(),
            translations: HashMap::new(),
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
            self.apply_or_ask(action);
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
    ///
    /// On Windows a maximized window leaves maximized first, and is
    /// maximized again after: the window library keeps a maximized window
    /// of YTFast's own frame to the screen less the taskbar, in full screen
    /// too, and the taskbar's strip showed black under the player bar (the
    /// owner's picture, 11 October 2026).
    fn sync_fullscreen(&mut self, ctx: &egui::Context) {
        use egui::ViewportCommand;
        let now = ctx.input(|i| i.time);
        if let Some(on) = self.fullscreen_wanted.take() {
            if on && cfg!(windows) {
                self.maximized_before_fullscreen =
                    ctx.input(|i| i.viewport().maximized == Some(true));
                if self.maximized_before_fullscreen {
                    ctx.send_viewport_cmd(ViewportCommand::Maximized(false));
                }
            }
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(on));
            if !on && std::mem::take(&mut self.maximized_before_fullscreen) {
                ctx.send_viewport_cmd(ViewportCommand::Maximized(true));
            }
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
    /// related), once per song, when the page is open. Its lyrics are
    /// asked for whichever tab shows, so Lyrics opens at once. While
    /// Lyrics shows, the next song's lyrics are asked for too, so they
    /// show as it starts, and with Translate on, both are translated.
    fn want_song_extras(&mut self) {
        if !self.now_playing {
            return;
        }
        let Some(entry) = &self.playback.entry else {
            return;
        };
        let track = &entry.track;
        let next = self.queue.peek_next().map(|next| &next.track);
        let lyrics_shown = self.np_tab == NpTab::Lyrics;
        let mut wanted = Vec::new();
        if !self.lyrics.contains_key(&track.video_id) {
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
            // LRCLIB and Musixmatch match on the length: without one from
            // YouTube, wait for the song to start.
            let starting = matches!(
                self.playback.state,
                PlayState::Preparing | PlayState::Playing
            );
            if duration.is_some() || playing_it || !starting {
                wanted.push(lyrics_request(track, duration));
            }
        } else if lyrics_shown
            && !matches!(
                self.lyrics.get(&track.video_id),
                Some(crate::lyrics::State::Loading)
            )
            && let Some(next) = next
            && !self.lyrics.contains_key(&next.video_id)
            && let Some(seconds) = next.duration_seconds
        {
            wanted.push(lyrics_request(next, Some(f64::from(seconds))));
        }
        if lyrics_shown && self.settings.translate_lyrics {
            let songs = std::iter::once(&track.video_id).chain(next.map(|n| &n.video_id));
            for video_id in songs {
                if let Some(crate::lyrics::State::Ready(lyrics)) = self.lyrics.get(video_id)
                    && !self.translations.contains_key(video_id)
                {
                    wanted.push(Request::Translate {
                        video_id: video_id.clone(),
                        lines: lyrics.texts(),
                        musixmatch: lyrics.musixmatch.clone(),
                    });
                }
            }
        }
        if self.np_tab == NpTab::Related && !self.related.contains_key(&track.video_id) {
            wanted.push(Request::Related(track.video_id.clone()));
        }
        // What the page shows stays when a store is full.
        let keep: Vec<String> = std::iter::once(track.video_id.clone())
            .chain(next.map(|n| n.video_id.clone()))
            .collect();
        for request in wanted {
            match &request {
                Request::Lyrics { video_id, .. } => {
                    keep_only(&mut self.lyrics, MAX_LYRICS, &keep);
                    self.lyrics
                        .insert(video_id.clone(), crate::lyrics::State::Loading);
                }
                Request::Translate { video_id, .. } => {
                    keep_only(&mut self.translations, MAX_LYRICS, &keep);
                    self.translations
                        .insert(video_id.clone(), crate::lyrics::Translation::Loading);
                }
                Request::Related(video_id) => {
                    keep_only(&mut self.related, MAX_RELATED, &keep);
                    self.related.insert(video_id.clone(), Loadable::Loading);
                }
                _ => {}
            }
            self.backend.send(request);
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
        if self.playback.state == PlayState::LeftOff {
            return self.playback.start_at;
        }
        match self.seeking {
            Some((entry, to, _)) if self.audio_status.entry == Some(entry) => to,
            _ => self.audio_status.position,
        }
    }

    /// The song's length for the player bar: the player's, or before it
    /// plays (where YTFast was last closed) its row's.
    pub fn shown_length(&self) -> f64 {
        match (&self.playback.state, &self.playback.entry) {
            (PlayState::LeftOff, Some(entry)) => {
                entry.track.duration_seconds.map_or(0.0, f64::from)
            }
            _ => self.audio_status.length,
        }
    }

    /// What to save of the queue ([`LeftOff`]): the songs around the one
    /// playing, and where it is, a video's moment as its song's. `None`
    /// with nothing in the queue.
    pub fn left_off(&self) -> Option<LeftOff> {
        let entries = self.queue.entries();
        let current = self.queue.current_index()?;
        let at = match self.playback.state {
            PlayState::Playing => self.shown_position(),
            PlayState::Idle | PlayState::WaitingForMore => 0.0,
            _ => self.playback.start_at,
        };
        let at = match &self.playback.entry {
            Some(entry) if self.playing_video_map().is_some() => {
                self.moment_in(entry, at, &entry.track.video_id)
            }
            _ => at,
        };
        let from = current.saturating_sub(LEFT_OFF_BEFORE);
        let to = (current + LEFT_OFF_AFTER + 1).min(entries.len());
        // Each with what the account thinks of it now (this run's choices
        // too), so it shows the same next time.
        let kept = |entry: &Entry| Track {
            like: self.known_like(&entry.track).map(crate::backend::rating),
            ..entry.track.clone()
        };
        Some(LeftOff {
            tracks: entries[from..to].iter().map(kept).collect(),
            current: current - from,
            at: at.max(0.0),
            title: self.queue.title.clone(),
            source: self.queue.source.clone(),
        })
    }

    /// Takes up the queue saved when YTFast was last closed ([`LeftOff`]),
    /// once, while nothing plays: paused at its second, nothing asked of
    /// YouTube until Play.
    fn pick_up(&mut self) {
        let Some(left) = self.left_off.take() else {
            return;
        };
        if left.tracks.is_empty() || self.playback.entry.is_some() || self.queue.current().is_some()
        {
            return;
        }
        let current = left.current.min(left.tracks.len() - 1);
        let Some(entry) = self
            .queue
            .replace(left.tracks, current, left.source)
            .cloned()
        else {
            return;
        };
        self.queue.title = left.title;
        let at = left.at.max(0.0);
        // A Mac's media keys reach only an app that has played since it
        // started: this one claims them for the song kept (nothing
        // elsewhere).
        if let Some(controls) = &mut self.controls {
            controls.claim(&entry.id.to_string(), Duration::from_secs_f64(at));
        }
        self.playback = Playback {
            entry: Some(entry),
            state: PlayState::LeftOff,
            start_at: at,
            listening_from: at,
            ..Playback::default()
        };
    }

    /// The cover texture for a picture address, when it has arrived.
    pub fn picture(&self, url: &str) -> Option<egui::TextureHandle> {
        self.images.borrow_mut().get(url, &self.backend)
    }

    /// Whether the picture at `url` did not load.
    pub fn picture_failed(&self, url: &str) -> bool {
        self.images.borrow_mut().failed(url)
    }

    /// What the account thinks of `track`, when known: as chosen in this
    /// run, or as YouTube said of the song playing ([`Event::Liked`]), on
    /// top of what its row said (`Track::like`).
    pub fn known_like(&self, track: &Track) -> Option<LikeState> {
        self.likes
            .get(&track.video_id)
            .copied()
            .or_else(|| track.like.map(crate::backend::like_state))
    }

    /// [`App::known_like`], neither liked nor disliked when not known.
    pub fn like_of(&self, track: &Track) -> LikeState {
        self.known_like(track).unwrap_or(LikeState::Neutral)
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

    fn handle_events(&mut self) {
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
                    // Where the music was when YTFast was last closed.
                    self.pick_up();
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
                Event::Translated(video_id, result) => {
                    let state = match result {
                        Ok(translated) => crate::lyrics::Translation::Ready(translated),
                        Err(_) => crate::lyrics::Translation::Missing,
                    };
                    self.translations.insert(video_id, state);
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
                Event::Image(url, picture) => self.images.borrow_mut().arrived(url, picture),
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
            let (from, paused) = (self.playback.start_at, self.playback.start_paused);
            self.start_from(current, from);
            self.playback.start_paused = paused;
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
                    paused: self.playback.start_paused,
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
                if matches!(
                    self.translations.get(video_id),
                    Some(crate::lyrics::Translation::Missing)
                ) {
                    self.translations.remove(video_id);
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
        // Carried on between the player's reports (ten a second), so words
        // light smoothly.
        let position = self.clock();
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
                // Paused, the other version waits there, paused too.
                let paused = self.audio_status.paused;
                self.start_from(entry, at);
                self.playback.start_paused = paused;
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
                let (from, paused) = (self.playback.start_at, self.playback.start_paused);
                self.start_from(entry, from);
                self.playback.start_paused = paused;
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
            // Getting ready to start paused (the switch moved while
            // paused): Play starts it playing.
            PlayState::Preparing if self.playback.start_paused => {
                self.playback.start_paused = false;
            }
            PlayState::Failed(_) => {
                if let Some(entry) = self.playback.entry.clone() {
                    self.start(entry);
                }
            }
            // The song from where YTFast was last closed: it starts there.
            PlayState::LeftOff => {
                if let Some(entry) = self.playback.entry.clone() {
                    let at = self.playback.start_at;
                    self.start_from(entry, at);
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

    /// Applies an action from a view. One that would remove songs the user
    /// lined up asks first ([`Dialog::ReplaceQueue`]).
    fn apply_or_ask(&mut self, action: Action) {
        let lined_up = self.queue.lined_up();
        if asks_first(&action, lined_up) {
            self.apply(Action::OpenDialog(Dialog::ReplaceQueue {
                lined_up,
                then: Box::new(action),
            }));
        } else {
            self.apply(action);
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
            Action::Seek(to) if self.playback.state == PlayState::LeftOff => {
                self.playback.start_at = to.max(0.0);
            }
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
                    Setting::Autoplay => s.autoplay = !s.autoplay,
                    Setting::EvenLoudness => s.even_loudness = !s.even_loudness,
                    Setting::MovingBackground => s.moving_background = !s.moving_background,
                    Setting::SkipDisliked => s.skip_disliked = !s.skip_disliked,
                    Setting::ConfirmClose => s.confirm_close = !s.confirm_close,
                    Setting::TranslateLyrics => s.translate_lyrics = !s.translate_lyrics,
                    Setting::KeepRunning => s.keep_running = !s.keep_running,
                    Setting::OpenAtLogin => {
                        s.open_at_login = !s.open_at_login;
                        if !self.demo {
                            crate::login::set(s.open_at_login);
                        }
                    }
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
                self.quit_requested = true;
            }
            Action::MoveToApplications => match crate::update_mac::move_to_applications() {
                // The helper waits for YTFast to end, then opens the copy.
                Ok(()) => {
                    self.quit_requested = true;
                    self.close_now = true;
                }
                Err(e) => self.notify(e),
            },
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
            Action::Confirmed(action) => self.apply(*action),
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
            let next = if self.like_of(&entry.track) == wanted {
                LikeState::Neutral
            } else {
                wanted
            };
            self.act(Action::Rate(id, next));
        }
    }

    /// The window asked to close (its ×, Alt+F4, the taskbar, a Mac's red
    /// button). With "Keep playing when closed" on, it closes and YTFast
    /// runs on without it, its icon by the clock (`hide_intent`, which the
    /// shell reads). Otherwise, while a song plays, it stays and asks first
    /// ([`Dialog::ConfirmClose`]), unless Settings says not to. Quitting
    /// (the icon's Quit, the question answered Yes, an update) closes it
    /// for good. Read in `logic`, which eframe also runs for a minimized
    /// window (closed from the taskbar).
    fn ask_before_closing(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.close_now) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.quit_requested || !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.keeps_running() {
            self.hide_intent = true;
            return;
        }
        let sounding = self.playback.state == PlayState::Playing && !self.audio_status.paused;
        let ask = self.settings.confirm_close && sounding && !self.closing && !self.restarting;
        if !ask {
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

    /// Closing the window leaves YTFast running: the setting is on, and
    /// the icon is there to bring the window back and to quit.
    pub fn keeps_running(&self) -> bool {
        self.settings.keep_running && self.has_tray
    }

    /// What keeps YTFast going, with a window or none: the backend's and
    /// the player's news, the updater, the media keys, the icon by the
    /// clock and other launches, and what they asked for.
    fn background(&mut self, ctx: &egui::Context) {
        self.script_fonts.add_when_read();
        self.handle_events();
        self.watch_updates();
        self.media_controls();
        self.tray_and_launches(ctx);
        // A media key pressed while the window is hidden acts at once.
        self.apply_actions();
        self.show_in_tray();
    }

    /// The icon's clicks (`tray`), and what other launches asked for
    /// (`instance`).
    fn tray_and_launches(&mut self, ctx: &egui::Context) {
        let events = self
            .tray
            .as_ref()
            .map(fastframe_tray::Tray::events)
            .unwrap_or_default();
        for event in events {
            self.tray_event(event, ctx);
        }
        if self.asked.take_show() {
            self.show_window(ctx);
        }
        if self.asked.take_quit() {
            self.quit(ctx);
        }
        if let Some(tray) = &self.tray {
            self.has_tray = tray.is_shown();
        }
    }

    /// A click on the icon, or one of its menu's entries.
    fn tray_event(&mut self, event: fastframe_tray::Event, ctx: &egui::Context) {
        use fastframe_tray::Event;
        match event {
            // Windows' click and a Mac's Dock: the window; a Mac's click
            // on its menu bar item too.
            Event::Show | Event::Toggle | Event::Menu(crate::tray::SHOW) => self.show_window(ctx),
            Event::Menu(crate::tray::PLAY_PAUSE) => self.act(Action::TogglePause),
            Event::Menu(crate::tray::NEXT) => self.act(Action::Next),
            Event::Menu(crate::tray::PREVIOUS) => self.act(Action::Previous),
            Event::Menu(crate::tray::QUIT) => self.quit(ctx),
            Event::Menu(_) => {}
        }
    }

    /// Brings the window back: a new one when none is open (the shell
    /// makes it), else the one open, from the taskbar or the Dock too.
    fn show_window(&mut self, ctx: &egui::Context) {
        if self.window_open {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        } else {
            self.wants_show = true;
        }
    }

    /// Quits YTFast (the icon's Quit, `--quit`), without asking: the
    /// window closes for good, or YTFast ends without one.
    fn quit(&mut self, ctx: &egui::Context) {
        self.quit_requested = true;
        if self.window_open {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// The icon's menu (Play or Pause, what can be chosen) and its tooltip
    /// (the song), told what changed.
    fn show_in_tray(&mut self) {
        let Some(tray) = &mut self.tray else {
            return;
        };
        let entry = self.playback.entry.as_ref();
        let shown = crate::tray::Shown {
            sounding: self.playback.state == PlayState::Playing && !self.audio_status.paused,
            song: entry.is_some(),
            tooltip: crate::tray::tooltip(
                crate::tray::name(self.demo),
                entry.map(|e| (e.track.title.as_str(), e.track.artists.as_str())),
            ),
        };
        if shown != self.tray_shown {
            shown.apply(&self.tray_shown, tray);
            self.tray_shown = shown;
        }
    }

    /// The window closed and YTFast runs on: what the window held (its
    /// pictures on the graphics card, its fonts, a question it asked) went
    /// with it, and the next window starts afresh. Covers come back from
    /// the backend's memory, not from YouTube.
    fn window_gone(&mut self) {
        self.window_open = false;
        self.hide_intent = false;
        self.wants_show = false;
        *self.images.get_mut() = Images::default();
        *self.page_cover.get_mut() = None;
        *self.listening_wash.get_mut() = None;
        if let Some(VideoShow {
            state: VideoState::Playing { video, shown },
            ..
        }) = &mut self.video
        {
            *shown.get_mut() = None;
            video.rest();
        }
        self.video_drawn.set(false);
        self.script_fonts.detach();
        self.fullscreen = false;
        self.fullscreen_wanted = None;
        *self.dialog.get_mut() = None;
        *self.pointed.get_mut() = None;
        // eframe saved everything as the window closed.
        self.saved_headless = Some((Instant::now(), Vec::new()));
    }

    /// With no window open, eframe saves nothing: the settings and the
    /// queue are saved here when they changed, at most every
    /// [`HEADLESS_SAVE`] (eframe's own pace), and at once as YTFast quits
    /// (`now`).
    fn save_headless(&mut self, now: bool) {
        if self.store.is_none() || self.demo {
            return;
        }
        let recent = self
            .saved_headless
            .as_ref()
            .is_some_and(|(at, _)| at.elapsed() < HEADLESS_SAVE);
        if recent && !now {
            return;
        }
        let left_off = self.left_off.clone().or_else(|| self.left_off());
        let values: Vec<(&'static str, String)> = [
            ("ytfast", crate::store::text(self.settings.for_saving())),
            (LEFT_OFF_KEY, crate::store::text(&left_off)),
        ]
        .into_iter()
        .filter_map(|(key, value)| Some((key, value?)))
        .collect();
        if self
            .saved_headless
            .as_ref()
            .is_some_and(|(_, saved)| *saved == values)
        {
            return;
        }
        if let Some(store) = &self.store {
            store.set(&values);
        }
        self.saved_headless = Some((Instant::now(), values));
    }

    /// A freshly installed version is up: its receipt tells the helper to
    /// keep it (else it puts the old version back).
    fn acknowledge_update(&mut self) {
        if let Some(receipt) = self.receipt.take() {
            std::thread::spawn(move || {
                if let Err(e) = receipt.acknowledge() {
                    log::warn!("updates: could not confirm the new version: {e:#}");
                }
            });
        }
    }

    /// YTFast ends: the last song's report goes to YouTube, and the
    /// sign-in's copy goes. Quit with no window open, the settings and the
    /// queue are saved here (eframe saved them as a window closed).
    fn shutdown(&mut self) {
        if !self.window_open {
            self.save_headless(true);
        }
        if let Some(url) = self.listened_report() {
            self.backend.report_before_closing(url);
        }
        self.backend.forget_sign_in();
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
        Event::Translated(_, Ok(translated)) => {
            words.extend(
                translated
                    .lines
                    .iter()
                    .chain(&translated.latin)
                    .flatten()
                    .map(String::as_str),
            );
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

/// The most songs whose lyrics (and their translations) are kept.
const MAX_LYRICS: usize = 30;
/// The most songs whose related pages are kept.
const MAX_RELATED: usize = 10;

/// A song's lyrics, asked for: its length, as the player or YouTube says,
/// tells it from other versions.
fn lyrics_request(track: &Track, duration: Option<f64>) -> Request {
    Request::Lyrics {
        video_id: track.video_id.clone(),
        title: track.title.clone(),
        artist: track.artists.clone(),
        album: track.album.clone(),
        duration,
    }
}

/// Makes room in a store of songs' things that holds `most`: when full,
/// it keeps only those of `keep` (what the page shows).
fn keep_only<V>(store: &mut HashMap<String, V>, most: usize, keep: &[String]) {
    if store.len() >= most {
        store.retain(|video_id, _| keep.contains(video_id));
    }
}

/// The most songs whose videos are remembered (see [`App::video_of`]).
const MAX_SONG_VIDEOS: usize = 500;

/// The most songs skipped in a row for problems of their own before the
/// queue stops: a few removed songs together are skipped, but a problem
/// every song has cannot run through the queue.
const MAX_SKIPS: usize = 5;

/// How long a notice shows: 3 s, as YouTube Music's, and 0.3 s to go.
const NOTICE_TIME: Duration = Duration::from_millis(3300);

/// The most often the settings and the queue are saved while no window is
/// open: as often as eframe saves them with one (every 30 s).
const HEADLESS_SAVE: Duration = Duration::from_secs(30);

/// How often YTFast, running without a window, looks at the song playing
/// (for the media overlay's time, and the queue to save). The player says
/// itself when a song ends, and media keys and the icon wake it at once.
const HEADLESS_PLAYING: Duration = Duration::from_secs(1);

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
        self.background(ctx);
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
        // A freshly installed version is up, its window drawn.
        self.acknowledge_update();
        // The helper waits for YTFast to end to install the update.
        if !self.restarting
            && self.updates.as_ref().map(crate::update::Updates::state)
                == Some(crate::update::State::Restarting)
        {
            self.restarting = true;
            self.quit_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.scrolling.apply(&ctx);
        self.mend_window_size(&ctx);
        self.script_fonts.check();
        self.images.get_mut().begin_frame(&ctx);
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
            // Where the music is, for the next run; one not yet taken up
            // (before signing in) is kept as it was.
            let left_off = self.left_off.clone().or_else(|| self.left_off());
            eframe::set_value(storage, LEFT_OFF_KEY, &left_off);
        }
    }
}

/// What fastframe-shell does with the app between windows: a window
/// closed with "Keep playing when closed" on leaves YTFast running without
/// one, the music playing, until the icon, the Dock or another launch asks
/// for a window again, or its Quit ends it.
impl fastframe_shell::Resident for App {
    fn closed(&self) -> fastframe_shell::Closed {
        if self.hide_intent && !self.quit_requested {
            fastframe_shell::Closed::Hide
        } else {
            fastframe_shell::Closed::Quit
        }
    }

    fn window_gone(&mut self) {
        App::window_gone(self);
    }

    /// What a frame does to keep the music going, without drawing; then
    /// asleep until the player, the backend, a media key, the icon or
    /// another launch wakes it (the song playing looked at each second).
    fn headless_frame(&mut self, ctx: &egui::Context) -> fastframe_shell::Headless {
        use fastframe_shell::Headless;
        self.background(ctx);
        self.save_headless(false);
        if self.quit_requested {
            Headless::Quit
        } else if self.wants_show {
            Headless::Show
        } else if self.playback.state == PlayState::Playing && !self.audio_status.paused {
            Headless::WaitFor(HEADLESS_PLAYING)
        } else {
            Headless::WaitFor(Duration::MAX)
        }
    }

    /// Started with the computer: no window, only the icon, when there is
    /// one to bring the window back with.
    fn start_hidden(&mut self) -> bool {
        if !self.has_tray {
            return false;
        }
        // A Mac's menu bar item, made without bringing YTFast forward.
        if let Some(tray) = &mut self.tray {
            tray.create_item();
        }
        // A version installed just now is up: no window to wait for.
        self.acknowledge_update();
        true
    }

    fn shutdown(&mut self) {
        App::shutdown(self);
    }
}

/// A window around the app (fastframe-shell): eframe drops it with the
/// window, handing the app back to the shell, which runs it on without a
/// window or ends it.
pub struct AppWindow {
    app: fastframe_shell::Held<App>,
}

impl AppWindow {
    pub fn new(app: fastframe_shell::Held<App>) -> Self {
        Self { app }
    }
}

impl eframe::App for AppWindow {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        eframe::App::logic(&mut *self.app, ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut *self.app, ui, frame);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        eframe::App::raw_input_hook(&mut *self.app, ctx, raw_input);
    }

    fn persist_egui_memory(&self) -> bool {
        eframe::App::persist_egui_memory(&*self.app)
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::App::save(&mut *self.app, storage);
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
            self.app.handle_events();
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
        /// Words on screen (egui's labels, which screen readers get as a
        /// value, not a name).
        texts: Vec<String>,
        /// What the frames asked of the window (close it, move it...).
        commands: Vec<egui::ViewportCommand>,
        /// The pointer's look after the last frame.
        cursor: egui::CursorIcon,
        /// The window says it is maximized.
        maximized: bool,
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
                texts: Vec::new(),
                commands: Vec::new(),
                cursor: egui::CursorIcon::Default,
                maximized: false,
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
            let update = output.platform_output.accesskit_update;
            self.texts = update
                .iter()
                .flat_map(|update| &update.nodes)
                .filter_map(|(_, node)| node.value().map(str::to_string))
                .collect();
            self.named = update
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
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 820.0),
                )),
                time: Some(self.time),
                events: vec![down(true), down(false)],
                ..Default::default()
            };
            input
                .viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .maximized = Some(self.maximized);
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

        /// Whether these words show (as words, not a control's name).
        fn says(&self, words: &str) -> bool {
            self.texts.iter().any(|t| t == words)
        }

        /// A frame with the wheel turned (points, as a touchpad's) at `at`.
        fn wheel(&mut self, at: egui::Pos2, dy: f32) {
            self.wheel_by(at, egui::vec2(0.0, dy));
        }

        /// A frame with a touchpad's two fingers moved by `delta` (points;
        /// sideways too) at `at`.
        fn wheel_by(&mut self, at: egui::Pos2, delta: egui::Vec2) {
            self.frame(vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta,
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
    }

    /// Home twenty shelves long, shown in a test window, and its loading.
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
        let shelves = (0..20)
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

    /// A long Home at the owner's window (1707 by 1019 at 150%), its covers
    /// as large as YouTube's there (360 by 360): scrolled down to its end,
    /// back to its top and down again, it asks for each cover once, as the
    /// window keeps them all. With room for only 32 MB of covers it let go
    /// of most and asked again on every pass; downloaded again, they came
    /// in late, and the owner found such scrolls lagging.
    #[test]
    fn a_long_home_keeps_its_covers_when_scrolled_back_and_forth() {
        use ytfast_core::read::{Card, CardLook, Shape, Thumb};
        let mut h = Harness::new();
        h.app.auth = Auth::SignedIn {
            name: "Listener".into(),
            handle: None,
            photo: None,
        };
        h.ctx.set_pixels_per_point(1.5);
        h.act(Action::Navigate(Route::Home));
        h.app.load(Route::Home);
        let load = h
            .requests()
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
        let card = |n: usize, i: usize| {
            Item::Card(Card {
                title: format!("Album {n}-{i}"),
                subtitle: "Album • Artist".into(),
                thumbnail: Some(Thumb {
                    url: format!("https://lh3.googleusercontent.com/c{n}x{i}=w226-h226-l90-rj"),
                    width: 226,
                    height: 226,
                }),
                round: false,
                open: None,
                play: None,
                podcast: None,
                look: CardLook::default(),
            })
        };
        let sections = (0..15)
            .map(|n| Section {
                title: format!("Shelf {n}"),
                shape: Shape::Carousel,
                items: (0..12).map(|i| card(n, i)).collect(),
                ..Section::default()
            })
            .collect();
        h.answer(Event::Page(
            Route::Home,
            load,
            Ok(Page {
                sections,
                ..Page::default()
            }),
        ));
        let mut asked: HashMap<String, usize> = HashMap::new();
        let mut time = 0.0;
        // Frames as the window draws them, the wheel turned `dy` in each,
        // until the page stops moving; covers answered as YouTube's.
        let mut scroll = |h: &mut Harness, asked: &mut HashMap<String, usize>, dy: f32| {
            let (mut still, mut last) = (0, f32::NAN);
            for _ in 0..2000 {
                time += 1.0 / 60.0;
                h.frame();
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1707.0, 1019.0),
                    )),
                    time: Some(time),
                    events: vec![
                        egui::Event::PointerMoved(egui::pos2(900.0, 500.0)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, dy),
                            phase: egui::TouchPhase::Move,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                };
                let app = &h.app;
                let mut output = h.ctx.run_ui(input, |ui| {
                    crate::theme::set(app.settings.theme);
                    app.images.borrow_mut().begin_frame(ui.ctx());
                    views::show(app, ui);
                });
                output.textures_delta.clear();
                h.app.apply_actions();
                for request in h.requests() {
                    match request {
                        Request::Image(url) => {
                            *asked.entry(url.clone()).or_default() += 1;
                            let cover = egui::ColorImage::new(
                                [360, 360],
                                vec![egui::Color32::GRAY; 360 * 360],
                            );
                            let picture = crate::backend::Picture::new(cover);
                            h.events.send(Event::Image(url, Some(picture))).unwrap();
                        }
                        Request::MoreResults { route, load } => {
                            let none = Event::MoreSections {
                                route,
                                load,
                                sections: Vec::new(),
                                done: true,
                            };
                            h.events.send(none).unwrap();
                        }
                        _ => {}
                    }
                }
                let now = h.app.page_offset.get();
                still = if now == last { still + 1 } else { 0 };
                last = now;
                if still >= 30 {
                    break;
                }
            }
        };
        scroll(&mut h, &mut asked, -60.0);
        let end = h.app.page_offset.get();
        scroll(&mut h, &mut asked, 600.0);
        assert!(h.app.page_offset.get() < 1.0, "back at the top");
        scroll(&mut h, &mut asked, -300.0);
        assert!(
            (h.app.page_offset.get() - end).abs() < 1.0,
            "at the end again"
        );
        // More covers were shown than 32 MB holds...
        let covers = asked.len();
        assert!(covers * 360 * 360 * 4 > 32 * 1024 * 1024, "{covers} covers");
        // ...and none was asked for twice.
        let again = asked.values().filter(|n| **n > 1).count();
        assert_eq!(again, 0, "{again} of {covers} covers asked for again");
    }

    /// A sideways swipe of two fingers on a touchpad goes Back (swiped
    /// right) and Forward (swiped left), as in a browser, once let go
    /// full, an arrow showing how far meanwhile; a wheel turned sideways by
    /// Shift goes nowhere.
    #[test]
    fn a_sideways_swipe_goes_back_and_forward() {
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        long_home(&mut w);
        w.settle();
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
        w.h.answer(Event::Page(route.clone(), load, Ok(page)));
        w.settle();
        let at = egui::pos2(700.0, 400.0);
        for _ in 0..6 {
            w.wheel_by(at, egui::vec2(60.0, 0.0));
        }
        // Not while the fingers are on the pad: the arrow shows how far.
        assert_eq!(w.h.app.route, route);
        assert!(w.shows("Swiping back"));
        // Let go: Back.
        w.settle();
        assert_eq!(w.h.app.route, Route::Home);
        assert!(!w.shows("Swiping back"));
        // Swiped left: Forward again.
        for _ in 0..6 {
            w.wheel_by(at, egui::vec2(-60.0, 0.0));
        }
        w.settle();
        assert_eq!(w.h.app.route, route);
        // The wheel turned sideways by Shift: nowhere.
        for _ in 0..6 {
            w.frame(vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 60.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::SHIFT,
                },
            ]);
        }
        w.settle();
        assert_eq!(w.h.app.route, route);
    }

    /// A swipe that starts over a shelf that can scroll that way is the
    /// shelf's: it scrolls, and the page stays. At the shelf's end, a new
    /// swipe goes on to the page, as in a browser.
    #[test]
    fn a_swipe_over_a_shelf_scrolls_the_shelf() {
        use ytfast_core::read::{Card, CardLook, Shape, Thumb};
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        long_home(&mut w);
        w.settle();
        w.h.act(Action::Navigate(Route::Explore));
        let load =
            w.h.requests()
                .into_iter()
                .find_map(|r| match r {
                    Request::Page { load, .. } => Some(load),
                    _ => None,
                })
                .expect("Explore is asked for");
        let cards = (0..12)
            .map(|i| {
                Item::Card(Card {
                    title: format!("Card {i}"),
                    subtitle: "Album".into(),
                    // Albums' covers (without, they are Explore's buttons).
                    thumbnail: Some(Thumb {
                        url: format!("https://example.com/c{i}.jpg"),
                        width: 226,
                        height: 226,
                    }),
                    round: false,
                    open: None,
                    play: None,
                    podcast: None,
                    look: CardLook::default(),
                })
            })
            .collect();
        let page = Page {
            sections: vec![Section {
                title: "A shelf".into(),
                shape: Shape::Carousel,
                items: cards,
                ..Section::default()
            }],
            ..Page::default()
        };
        w.h.answer(Event::Page(Route::Explore, load, Ok(page)));
        w.settle();
        let card = w.find("Card 3", 0);
        let at = card.center();
        // Swiped left over the shelf: it scrolls on.
        for _ in 0..6 {
            w.wheel_by(at, egui::vec2(-60.0, 0.0));
        }
        w.settle();
        assert!(w.find("Card 3", 0).left() < card.left() - 200.0);
        // Swiped right over it: it scrolls back, and the page stays.
        for _ in 0..6 {
            w.wheel_by(at, egui::vec2(60.0, 0.0));
        }
        w.settle();
        assert_eq!(w.h.app.route, Route::Explore);
        assert!((w.find("Card 3", 0).left() - card.left()).abs() < 1.0);
        // At its start now, a new swipe right goes Back.
        for _ in 0..6 {
            w.wheel_by(at, egui::vec2(60.0, 0.0));
        }
        w.settle();
        assert_eq!(w.h.app.route, Route::Home);
    }

    /// Home's next shelves are asked for three screens before its end
    /// comes into view, not at its top.
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

    /// Home scrolled to its end, and on; then three more shelves arrive.
    /// Returns where the end was.
    fn home_at_its_end_then_more(w: &mut Window) -> f32 {
        let load = long_home(w);
        w.settle();
        let at = egui::pos2(700.0, 400.0);
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
        w.h.answer(Event::MoreSections {
            route: Route::Home,
            load,
            sections: (20..23)
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
        end
    }

    /// A fast flick that reached Home's end before its next shelves came
    /// glides on into them once they arrive, as a long document scrolls on
    /// (the owner found it pausing at each batch, then going on).
    #[test]
    fn a_glide_goes_on_into_shelves_that_arrived() {
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        let end = home_at_its_end_then_more(&mut w);
        let at = egui::pos2(700.0, 400.0);
        // A glide dying away: each movement smaller than the one before.
        for dy in [-90.0, -70.0, -50.0, -35.0, -20.0, -12.0, -6.0] {
            w.wheel(at, dy);
        }
        assert!(
            w.h.app.page_offset.get() > end + 100.0,
            "glided on from {end} to {}",
            w.h.app.page_offset.get()
        );
    }

    /// Scrolling on without a pause (the wheel turned on, fingers moving on
    /// the touchpad) goes on into the shelves that arrived, as a browser's:
    /// the owner found the page stuck at the end of each batch, as if it
    /// slowed there, until they stopped scrolling.
    #[test]
    fn scrolling_on_goes_into_shelves_that_arrived() {
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        let end = home_at_its_end_then_more(&mut w);
        let at = egui::pos2(700.0, 400.0);
        for _ in 0..3 {
            w.wheel(at, -120.0);
        }
        assert!(
            w.h.app.page_offset.get() > end + 100.0,
            "scrolled on from {end} to {}",
            w.h.app.page_offset.get()
        );
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

    /// In every look, a song dragged in Up next lands where it is let go;
    /// the rows are YouTube Music's (56 high, 1 between), Premium's 64 high
    /// and 2 apart.
    #[test]
    fn a_song_dragged_in_up_next_lands_where_it_is_let_go() {
        use crate::theme::Theme;
        for (theme, pitch) in [
            (Theme::YouTubeMusic, 57.0),
            (Theme::Premium, 66.0),
            (Theme::DynamicBackground, 57.0),
        ] {
            let mut w = Window::new(theme);
            w.h.play(&["a", "b", "c", "d", "e"]);
            w.h.app.now_playing = true;
            w.h.app.np_tab = NpTab::UpNext;
            w.settle();
            let b = w.find("B", 0);
            let d = w.find("D", 0);
            assert!(
                (d.top() - b.top() - 2.0 * pitch).abs() < 0.5,
                "{theme:?} {b:?} {d:?}"
            );
            // B picked up and carried down to D's upper half: it goes
            // before D.
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
            assert_eq!(order, ["a", "c", "b", "d", "e"], "{theme:?}");
        }
        crate::theme::set(Theme::YouTubeMusic);
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

    /// Songs YouTube says are liked keep their filled thumb shown
    /// ("Liked") in their rows without the pointer, on a playlist's page
    /// and in Up next, in every look; what is chosen in this run wins over
    /// what the row said; the menu and the like button start from it.
    #[test]
    fn liked_songs_show_their_thumb_in_their_rows() {
        use crate::theme::Theme;
        use ytfast_core::read::Rating;
        let rated = |id: &str, like: Option<Rating>| Track { like, ..song(id) };
        let tracks = vec![
            rated("a", Some(Rating::Like)),
            rated("b", Some(Rating::Indifferent)),
            rated("c", None),
            rated("d", Some(Rating::Like)),
        ];
        // The rows named `row` hold a "Liked" thumb.
        let marked = |w: &Window| -> Vec<String> {
            let mut rows: Vec<String> = w
                .named
                .iter()
                .filter(|(name, rect)| {
                    name.len() == 1
                        && w.named
                            .iter()
                            .any(|(n, mark)| n == "Liked" && rect.contains_rect(*mark))
                })
                .map(|(name, _)| name.clone())
                .collect();
            rows.sort();
            rows.dedup();
            rows
        };
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
                sections: vec![Section {
                    items: tracks.iter().cloned().map(Item::Track).collect(),
                    ..Section::default()
                }],
                ..Page::default()
            };
            w.h.answer(Event::Page(route, load, Ok(page)));
            w.point(egui::pos2(5.0, 5.0));
            w.settle();
            assert_eq!(marked(&w), ["A", "D"], "{theme:?}");
            // Chosen in this run: A no longer, B now.
            w.h.act(Action::Rate("a".into(), LikeState::Neutral));
            w.h.act(Action::Rate("b".into(), LikeState::Liked));
            w.settle();
            assert_eq!(marked(&w), ["B", "D"], "{theme:?}");
            // D's menu offers to take it out of Liked Music...
            let d = w.find("D", 0);
            w.right_click(d.center());
            assert!(w.shows("Remove from liked songs"), "{theme:?}");
            // Closed by a click on the empty page under the rows.
            w.click(egui::pos2(700.0, 640.0));
            // ...and its like button, under the pointer, takes the like
            // back.
            w.point(d.center());
            w.frame(Vec::new());
            let like = w.find("Like", 0);
            assert!(d.contains_rect(like), "{theme:?}: {d:?} {like:?}");
            w.click(like.center());
            assert_eq!(w.h.app.likes.get("d"), Some(&LikeState::Neutral));

            // Up next, from the rows as they came.
            let mut w = Window::new(theme);
            w.h.act(Action::PlayTracks {
                tracks: tracks.clone(),
                start: 1,
                source: None,
            });
            w.h.ready();
            w.h.app.now_playing = true;
            w.h.app.np_tab = NpTab::UpNext;
            w.point(egui::pos2(5.0, 5.0));
            w.settle();
            assert_eq!(marked(&w), ["A", "D"], "{theme:?}: Up next");
            crate::theme::set(Theme::YouTubeMusic);
        }
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
                        height: 544,
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
                    height: 226,
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

    /// The queue is kept for the next run (the owner's word, 11 October
    /// 2026): its songs around the one playing, and where it was. Taken up
    /// at the first sign-in, paused there, without asking YouTube
    /// anything; a jump before Play moves where it starts; Play starts it
    /// there.
    #[test]
    fn the_queue_is_kept_for_the_next_run() {
        let mut h = Harness::new();
        h.act(Action::PlayTracks {
            tracks: ["a", "b", "c"].map(song).to_vec(),
            start: 1,
            source: Some("PLx".into()),
        });
        h.ready();
        h.app.audio_status.entry = Some(h.entry().0);
        h.app.audio_status.position = 83.0;
        // C liked in this run: kept so.
        h.act(Action::Rate("c".into(), LikeState::Liked));
        let saved = h.app.left_off().expect("a queue");
        let ids: Vec<&str> = saved.tracks.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
        assert_eq!(saved.tracks[2].like, Some(ytfast_core::read::Rating::Like));
        assert_eq!((saved.current, saved.at), (1, 83.0));
        assert_eq!(saved.source.as_deref(), Some("PLx"));

        // The next run: nothing until signed in, then B paused at 83 s.
        let mut h = Harness::new();
        h.app.left_off = Some(saved);
        assert!(h.app.playback.entry.is_none());
        h.answer(Event::SignedIn {
            name: "Listener".into(),
            handle: None,
            photo: None,
        });
        assert_eq!(h.entry().1, "b");
        assert_eq!(h.app.playback.state, PlayState::LeftOff);
        assert_eq!(
            (h.app.shown_position(), h.app.shown_length()),
            (83.0, 200.0)
        );
        let asked = h.requests();
        assert!(
            !asked.iter().any(|r| matches!(
                r,
                Request::Prepare { .. } | Request::Warm(_) | Request::Details(_)
            )),
            "nothing is asked for the song"
        );
        assert_eq!(plays(&h.commands()), 0);
        // A jump before Play moves where it starts.
        h.act(Action::Seek(90.0));
        assert_eq!(h.app.shown_position(), 90.0);
        // Play: B is got ready, and plays from there.
        h.act(Action::TogglePause);
        let asked = h.requests();
        assert!(
            asked.iter().any(
                |r| matches!(r, Request::Prepare { video_id, play: true, .. } if video_id == "b")
            ),
            "B is got ready"
        );
        h.ready();
        let started = h.commands().into_iter().find_map(|c| match c {
            Command::Play { from, paused, .. } => Some((from, paused)),
            _ => None,
        });
        assert_eq!(started, Some((90.0, false)));
    }

    /// Of a long queue, the songs around the one playing are kept; in the
    /// video mode, the video's moment is kept as its song's.
    #[test]
    fn what_is_kept_of_the_queue() {
        use ytfast_core::read::{Segment, TrackMore};
        let mut h = Harness::new();
        let ids: Vec<String> = (0..300).map(|n| format!("s{n}")).collect();
        h.act(Action::PlayTracks {
            tracks: ids.iter().map(|id| song(id)).collect(),
            start: 50,
            source: None,
        });
        let saved = h.app.left_off().expect("a queue");
        assert_eq!(saved.tracks.len(), LEFT_OFF_BEFORE + 1 + LEFT_OFF_AFTER);
        assert_eq!(saved.current, LEFT_OFF_BEFORE);
        assert_eq!(saved.tracks[saved.current].video_id, "s50");

        // Its music video starts the song 21 s in: 51 s into the video is
        // 30 s into the song.
        let mut h = Harness::new();
        let mut track = song("a");
        track.more = Some(Box::new(TrackMore {
            counterpart: Some("avideo00001".into()),
            segments: vec![Segment {
                song_ms: 0,
                video_ms: 21_000,
                length_ms: 180_000,
            }],
            ..TrackMore::default()
        }));
        h.act(Action::PlayTracks {
            tracks: vec![track],
            start: 0,
            source: None,
        });
        h.ready();
        h.app.playback.version = Some("avideo00001".into());
        h.app.audio_status.entry = Some(h.entry().0);
        h.app.audio_status.position = 51.0;
        assert_eq!(h.app.left_off().map(|l| l.at), Some(30.0));
    }

    /// eframe's storage, in memory.
    #[derive(Default)]
    struct Stored(HashMap<String, String>);

    impl eframe::Storage for Stored {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_string(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    /// The queue is saved beside the settings and read back as it was; one
    /// saved but not yet taken up (before signing in) is saved again as it
    /// was; the demo saves none.
    #[test]
    fn the_queue_is_saved_beside_the_settings() {
        use eframe::App as _;
        let mut h = Harness::new();
        h.play(&["a", "b"]);
        let mut stored = Stored::default();
        h.app.save(&mut stored);
        let read = eframe::get_value::<Option<LeftOff>>(&stored, LEFT_OFF_KEY).flatten();
        assert_eq!(read, h.app.left_off());
        assert_eq!(read.as_ref().map(|l| l.tracks.len()), Some(2));
        // Not yet taken up: saved again as it was.
        let mut later = Harness::new();
        later.app.left_off = read.clone();
        let mut again = Stored::default();
        later.app.save(&mut again);
        assert_eq!(
            eframe::get_value::<Option<LeftOff>>(&again, LEFT_OFF_KEY).flatten(),
            read
        );
        // The demo keeps nothing.
        h.app.demo = true;
        let mut demo = Stored::default();
        h.app.save(&mut demo);
        assert!(demo.0.is_empty());
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
                height: 60,
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

    /// Paused, the switch changes the version and keeps it paused, at the
    /// same moment (the owner found it playing); Play then plays it.
    #[test]
    fn the_switch_keeps_a_paused_song_paused() {
        let mut h = Harness::new();
        h.act(Action::PlayTracks {
            tracks: vec![song_with_video("a", "va")],
            start: 0,
            source: None,
        });
        h.ready();
        at(&mut h, 42.0);
        h.app.audio.set_status(|s| s.paused = true);
        h.frame();
        h.requests();
        h.commands();
        let paused_play = |commands: &[Command]| {
            commands.iter().find_map(|c| match c {
                Command::Play { from, paused, .. } => Some((*from, *paused)),
                _ => None,
            })
        };
        h.act(Action::VideoMode(true));
        assert_eq!(prepares(&h.requests(), true), ["va"]);
        h.ready();
        assert_eq!(paused_play(&h.commands()), Some((42.0, true)));
        // And back, still paused.
        at(&mut h, 42.0);
        h.app.audio.set_status(|s| s.paused = true);
        h.frame();
        h.act(Action::VideoMode(false));
        h.ready();
        assert_eq!(paused_play(&h.commands()), Some((42.0, true)));
        // Playing, the switch plays the other version on.
        at(&mut h, 50.0);
        h.app.audio.set_status(|s| s.paused = false);
        h.frame();
        h.act(Action::VideoMode(true));
        h.ready();
        assert_eq!(paused_play(&h.commands()), Some((50.0, false)));
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

    /// Which actions ask before they run: those that put a new queue in
    /// place of one with two or more songs lined up by hand. Lining up
    /// more, opening a page, or moving within the queue never asks.
    #[test]
    fn asks_before_replacing_two_or_more_lined_up_songs() {
        let watch = |video_id: Option<&str>, playlist_id: Option<&str>| Target::Watch {
            video_id: video_id.map(str::to_string),
            playlist_id: playlist_id.map(str::to_string),
        };
        let album = Target::Browse {
            id: "MPREb_a".into(),
            kind: ytfast_core::read::PageKind::Album,
            params: None,
        };
        let play = || Action::PlayTracks {
            tracks: vec![song("a")],
            start: 0,
            source: None,
        };
        let cases = [
            // A song clicked on a page, an album's Play and Shuffle, a
            // song's tile, Start mix, a playlist's play button.
            (play(), true),
            (
                Action::Shuffle {
                    tracks: vec![song("a")],
                    source: None,
                },
                true,
            ),
            (Action::Open(watch(Some("a"), None), Some(song("a"))), true),
            (Action::Play(watch(Some("a"), None), Some(song("a"))), true),
            (Action::Play(watch(None, Some("PL1")), None), true),
            (Action::QueuePlaylist("PL1".into(), QueueMode::Play), true),
            (
                Action::QueuePlaylist("PL1".into(), QueueMode::Shuffle),
                true,
            ),
            // These keep the queue.
            (Action::Open(album.clone(), None), false),
            (Action::Play(album, None), false),
            (Action::QueuePlaylist("PL1".into(), QueueMode::Next), false),
            (Action::QueuePlaylist("PL1".into(), QueueMode::End), false),
            (Action::PlayNext(song("a")), false),
            (Action::AddToQueue(song("a")), false),
            (Action::PlayNextAll(vec![song("a")]), false),
            (Action::AddToQueueAll(vec![song("a")]), false),
            (Action::JumpTo(1), false),
            (Action::MoveNextInQueue(1), false),
            (Action::Next, false),
            // The user was asked already.
            (Action::Confirmed(Box::new(play())), false),
        ];
        for (action, asks) in cases {
            assert_eq!(asks_first(&action, 2), asks, "{action:?}");
            assert!(!asks_first(&action, 1), "one song lined up: {action:?}");
        }
    }

    /// A song's menu offers Play next first, and Start mix after Add to
    /// queue. With two or more songs lined up by hand, a click that would
    /// put a new queue in their place asks first: Cancel (or Esc) keeps
    /// them, Play anyway plays. With one, or in Up next, a click plays at
    /// once.
    #[test]
    fn a_click_that_would_replace_lined_up_songs_asks_first() {
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
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
                items: ["a", "b", "c", "d"]
                    .into_iter()
                    .map(|id| Item::Track(song(id)))
                    .collect(),
                ..Section::default()
            }],
            ..Page::default()
        };
        w.h.answer(Event::Page(route, load, Ok(page)));
        w.point(egui::pos2(5.0, 5.0));
        w.settle();
        let a = w.find("A", 0);
        w.click(a.center());
        assert_eq!(w.h.entry().1, "a");

        // The menu: Play next, Add to queue, then Start mix.
        let b = w.find("B", 0);
        w.right_click(b.center());
        let next = w.find("Play next", 0);
        let add = w.find("Add to queue", 0);
        let mix = w.find("Start mix", 0);
        assert!(next.top() < add.top() && add.top() < mix.top());
        w.click(next.center());
        // One song lined up: a click plays at once.
        let c = w.find("C", 0);
        w.click(c.center());
        assert!(w.h.app.dialog.borrow().is_none());
        assert_eq!(w.h.entry().1, "c");

        // Two: it asks, and nothing changes meanwhile.
        for (title, item) in [("B", "Play next"), ("D", "Add to queue")] {
            let row = w.find(title, 0);
            w.right_click(row.center());
            let item = w.find(item, 0);
            w.click(item.center());
        }
        let a = w.find("A", 0);
        w.click(a.center());
        w.settle();
        assert!(matches!(
            *w.h.app.dialog.borrow(),
            Some(Dialog::ReplaceQueue { lined_up: 2, .. })
        ));
        assert!(w.says("Replace your queue?"));
        assert!(w.says(
            "You've lined up 2 songs that haven't played yet. Playing this will remove them."
        ));
        assert_eq!(w.h.entry().1, "c");
        // Cancel keeps them.
        let cancel = w.find("Cancel", 0);
        w.click(cancel.center());
        w.settle();
        assert!(w.h.app.dialog.borrow().is_none());
        assert_eq!(w.h.entry().1, "c");
        assert_eq!(w.h.app.queue.lined_up(), 2);
        // Play anyway plays.
        let a = w.find("A", 0);
        w.click(a.center());
        w.settle();
        let anyway = w.find("Play anyway", 0);
        w.click(anyway.center());
        w.settle();
        assert!(w.h.app.dialog.borrow().is_none());
        assert_eq!(w.h.entry().1, "a");
        assert_eq!(w.h.app.queue.lined_up(), 0);

        // In Up next a click moves within the queue: it never asks.
        w.h.act(Action::PlayNext(song("b")));
        w.h.act(Action::PlayNext(song("c")));
        w.h.app.now_playing = true;
        w.h.app.np_tab = NpTab::UpNext;
        w.settle();
        let d = w.find("D", 0);
        w.click(d.center());
        assert!(w.h.app.dialog.borrow().is_none());
        assert_eq!(w.h.entry().1, "d");
        // Esc closes the question, not the player page behind it.
        w.h.act(Action::PlayNext(song("b")));
        w.h.act(Action::PlayNext(song("c")));
        w.h.app.act(Action::PlayTracks {
            tracks: vec![song("a")],
            start: 0,
            source: None,
        });
        w.settle();
        assert!(w.h.app.dialog.borrow().is_some());
        w.press(egui::Key::Escape);
        w.settle();
        assert!(w.h.app.dialog.borrow().is_none());
        assert!(w.h.app.now_playing);
        assert_eq!(w.h.entry().1, "d");
    }

    /// With "Keep playing when closed" on and the icon by the clock, the
    /// window closes and YTFast runs on: nothing asks, nothing stops, and
    /// the shell keeps the app. Without the icon (no way back), or with the
    /// switch off, closing quits, asking first while a song plays, as
    /// before; the icon's Quit quits without asking.
    #[test]
    fn closing_leaves_ytfast_running_by_the_clock() {
        use fastframe_shell::{Closed, Resident};
        let cancels = |commands: &[egui::ViewportCommand]| {
            commands.contains(&egui::ViewportCommand::CancelClose)
        };
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        w.h.app.has_tray = true;
        w.h.play(&["a", "b"]);
        w.h.commands();
        assert!(!cancels(&w.close_request()));
        assert!(w.h.app.dialog.borrow().is_none(), "nothing asks");
        assert_eq!(Resident::closed(&w.h.app), Closed::Hide);
        assert!(w.h.commands().is_empty(), "the music goes on");

        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        w.h.play(&["a"]);
        assert!(cancels(&w.close_request()), "no icon: it asks");
        assert_eq!(Resident::closed(&w.h.app), Closed::Quit);

        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        w.h.app.has_tray = true;
        w.h.app.settings.keep_running = false;
        w.h.play(&["a"]);
        assert!(cancels(&w.close_request()), "switched off: it asks");
        assert_eq!(Resident::closed(&w.h.app), Closed::Quit);

        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        w.h.app.has_tray = true;
        w.h.play(&["a"]);
        let ctx = w.h.ctx.clone();
        w.h.app
            .tray_event(fastframe_tray::Event::Menu(crate::tray::QUIT), &ctx);
        assert!(!cancels(&w.close_request()), "Quit asks nothing");
        assert_eq!(Resident::closed(&w.h.app), Closed::Quit);
    }

    /// The window gone, YTFast plays on without one: a song that ends
    /// moves on to the next, the icon's Previous and Pause act, YTFast
    /// sleeps while nothing plays, a window comes back when another launch
    /// (or the icon) asks for one, and the icon's Quit ends YTFast.
    #[test]
    fn without_a_window_the_music_goes_on() {
        use fastframe_shell::{Headless, Resident};
        use fastframe_tray::Event as Click;
        let mut h = Harness::new();
        h.app.has_tray = true;
        h.play(&["a", "b"]);
        Resident::window_gone(&mut h.app);
        assert!(!h.app.window_open);
        let headless = egui::Context::default();
        assert_eq!(
            Resident::headless_frame(&mut h.app, &headless),
            Headless::WaitFor(HEADLESS_PLAYING),
            "playing: looked at each second"
        );

        let (first, _) = h.entry();
        h.app.audio.set_status(|s| s.ended = Some(first));
        Resident::headless_frame(&mut h.app, &headless);
        assert_eq!(h.entry().1, "b", "the next song, without a window");
        h.ready();

        h.app
            .tray_event(Click::Menu(crate::tray::PREVIOUS), &headless);
        Resident::headless_frame(&mut h.app, &headless);
        assert_eq!(h.entry().1, "a");
        h.ready();
        h.commands();
        h.app
            .tray_event(Click::Menu(crate::tray::PLAY_PAUSE), &headless);
        Resident::headless_frame(&mut h.app, &headless);
        assert!(h.commands().iter().any(|c| matches!(c, Command::Pause)));
        h.app.audio.set_status(|s| s.paused = true);
        assert_eq!(
            Resident::headless_frame(&mut h.app, &headless),
            Headless::WaitFor(Duration::MAX),
            "paused: asleep until woken"
        );

        h.app.asked.handle(crate::instance::request::SHOW, &|| {});
        assert_eq!(
            Resident::headless_frame(&mut h.app, &headless),
            Headless::Show
        );
        let ctx = h.ctx.clone();
        h.app.attach(&ctx);
        assert!(h.app.window_open);

        Resident::window_gone(&mut h.app);
        h.app.tray_event(Click::Show, &headless);
        assert_eq!(
            Resident::headless_frame(&mut h.app, &headless),
            Headless::Show,
            "the icon's click"
        );
        h.app.attach(&ctx);
        Resident::window_gone(&mut h.app);
        h.app.tray_event(Click::Menu(crate::tray::QUIT), &headless);
        assert_eq!(
            Resident::headless_frame(&mut h.app, &headless),
            Headless::Quit
        );
    }

    /// Started with the computer, YTFast opens no window only when the
    /// icon is there to bring one back.
    #[test]
    fn starting_hidden_needs_the_icon() {
        use fastframe_shell::Resident;
        let mut h = Harness::new();
        assert!(!Resident::start_hidden(&mut h.app));
        h.app.has_tray = true;
        assert!(Resident::start_hidden(&mut h.app));
    }

    /// With no window open, eframe saves nothing: YTFast saves the settings
    /// and the queue itself when they changed (not again just after the
    /// window saved them), and as it quits.
    #[test]
    fn without_a_window_the_queue_is_kept_for_the_next_run() {
        use fastframe_shell::Resident;
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("app.ron");
        let mut h = Harness::new();
        h.app.has_tray = true;
        h.app.store = Some(crate::store::Store::at(path.clone()));
        h.play(&["a", "b"]);
        Resident::window_gone(&mut h.app);
        h.app.save_headless(false);
        assert!(!path.exists(), "the window saved it as it closed");
        Resident::shutdown(&mut h.app);
        let store = crate::store::Store::at(path);
        let kept = store.get::<Option<LeftOff>>(LEFT_OFF_KEY).flatten();
        assert_eq!(kept.map(|k| k.tracks.len()), Some(2));
        assert!(store.get::<Settings>("ytfast").is_some());
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

    /// A Mac's Move to Applications question shows at once, before
    /// signing in, in each look; Cancel leaves YTFast where it is.
    #[test]
    fn the_move_question_shows_before_signing_in() {
        use crate::theme::Theme;
        for theme in [
            Theme::YouTubeMusic,
            Theme::Premium,
            Theme::DynamicBackground,
        ] {
            let mut w = Window::new(theme);
            w.h.app.auth = Auth::Choosing {
                browser: Browser::Firefox,
            };
            *w.h.app.dialog.borrow_mut() = Some(Dialog::MoveToApplications);
            w.settle();
            for name in ["Move", "Cancel"] {
                assert!(w.shows(name), "{theme:?}: {name}");
            }
            let cancel = w.find("Cancel", 0);
            w.click(cancel.center());
            w.settle();
            assert!(w.h.app.dialog.borrow().is_none(), "{theme:?}");
            assert!(!w.h.app.quit_requested, "{theme:?}");
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

    /// On Windows a maximized window leaves maximized just before full
    /// screen, so full screen covers the taskbar's strip too (it showed
    /// black under the player bar), and is maximized again after it.
    #[test]
    fn a_maximized_window_fills_the_whole_screen_in_full_screen() {
        let mut w = Window::new(crate::theme::Theme::YouTubeMusic);
        w.h.play(&["a"]);
        w.h.app.now_playing = true;
        w.settle();
        w.maximized = true;
        let told = w.press(egui::Key::F);
        let at = |told: &[egui::ViewportCommand], command| told.iter().position(|t| *t == command);
        let full = at(&told, egui::ViewportCommand::Fullscreen(true));
        let restored = at(&told, egui::ViewportCommand::Maximized(false));
        assert!(full.is_some(), "{told:?}");
        if cfg!(windows) {
            assert!(restored.is_some_and(|r| Some(r) < full), "{told:?}");
        } else {
            assert_eq!(restored, None, "{told:?}");
        }
        let told = w.press(egui::Key::Escape);
        let windowed = at(&told, egui::ViewportCommand::Fullscreen(false));
        let maximized = at(&told, egui::ViewportCommand::Maximized(true));
        assert!(windowed.is_some(), "{told:?}");
        if cfg!(windows) {
            assert!(maximized.is_some_and(|m| Some(m) > windowed), "{told:?}");
        }
        // A window not maximized is left as it is.
        w.maximized = false;
        let told = w.press(egui::Key::F);
        assert_eq!(at(&told, egui::ViewportCommand::Maximized(false)), None);
        let told = w.press(egui::Key::Escape);
        assert_eq!(at(&told, egui::ViewportCommand::Maximized(true)), None);
    }

    /// Made-up lyrics timed by the line, as a source sends them: their
    /// words' times estimated.
    fn timed_lyrics(lines: &[(u64, &str)]) -> crate::lyrics::Lyrics {
        use ytfast_core::lyrics as core;
        let mut found: Vec<core::LyricLine> = lines
            .iter()
            .map(|&(start_ms, text)| core::LyricLine {
                start_ms: Some(start_ms),
                text: text.into(),
                ..Default::default()
            })
            .collect();
        for i in 1..found.len() {
            found[i - 1].end_ms = found[i].start_ms;
        }
        crate::lyrics::Lyrics::from(core::Lyrics {
            lines: found,
            synced: true,
            source: "Test".into(),
            ..Default::default()
        })
    }

    /// The player page asks for the playing song's lyrics whatever its tab,
    /// once. On Lyrics it asks for the next song's too, once the playing
    /// one's are in, by its length; with Translate on, both are
    /// translated. A translation that did not come is asked for again when
    /// the song plays again.
    #[test]
    fn lyrics_are_asked_for_ahead_and_translated_when_wanted() {
        use crate::lyrics::Translation;
        let lyrics_asked = |requests: &[Request]| -> Vec<String> {
            requests
                .iter()
                .filter_map(|r| match r {
                    Request::Lyrics { video_id, .. } => Some(video_id.clone()),
                    _ => None,
                })
                .collect()
        };
        let translations_asked = |requests: &[Request]| -> Vec<String> {
            requests
                .iter()
                .filter_map(|r| match r {
                    Request::Translate {
                        video_id, lines, ..
                    } => {
                        assert_eq!(lines.len(), 2);
                        Some(video_id.clone())
                    }
                    _ => None,
                })
                .collect()
        };
        let mut h = Harness::new();
        h.play(&["a", "b", "c"]);
        h.requests();
        // The player page closed: nothing.
        h.app.want_song_extras();
        assert!(lyrics_asked(&h.requests()).is_empty());
        // Open on Up next: the playing song's lyrics, once.
        h.app.now_playing = true;
        h.app.np_tab = NpTab::UpNext;
        h.app.want_song_extras();
        assert_eq!(lyrics_asked(&h.requests()), ["a"]);
        h.app.want_song_extras();
        assert!(h.requests().is_empty());
        // On Lyrics: the next song's once the playing one's are in.
        h.app.np_tab = NpTab::Lyrics;
        h.app.want_song_extras();
        assert!(h.requests().is_empty());
        let made_up = || timed_lyrics(&[(1_000, "one two"), (3_000, "three")]);
        h.answer(Event::Lyrics("a".into(), Some(made_up())));
        h.app.want_song_extras();
        let asked = h.requests();
        assert_eq!(lyrics_asked(&asked), ["b"]);
        assert!(asked.iter().any(
            |r| matches!(r, Request::Lyrics { video_id, duration: Some(d), .. } if video_id == "b" && *d == 200.0)
        ));
        assert!(translations_asked(&asked).is_empty());
        // Translate on: the playing song's, then the next song's once in.
        h.act(Action::Toggle(Setting::TranslateLyrics));
        h.app.want_song_extras();
        assert_eq!(translations_asked(&h.requests()), ["a"]);
        h.answer(Event::Lyrics("b".into(), Some(made_up())));
        h.app.want_song_extras();
        assert_eq!(translations_asked(&h.requests()), ["b"]);
        h.app.want_song_extras();
        assert!(h.requests().is_empty());
        // Not translated: asked again only when the song plays again.
        h.answer(Event::Translated("a".into(), Err("no".into())));
        assert!(matches!(
            h.app.translations.get("a"),
            Some(Translation::Missing)
        ));
        h.app.want_song_extras();
        assert!(translations_asked(&h.requests()).is_empty());
        h.play(&["a", "b"]);
        assert!(!h.app.translations.contains_key("a"));
        h.app.want_song_extras();
        assert_eq!(translations_asked(&h.requests()), ["a"]);
    }

    /// A full store makes room by keeping only what the page shows.
    #[test]
    fn a_full_store_keeps_what_shows() {
        let mut store: HashMap<String, u8> =
            (0..MAX_LYRICS).map(|i| (format!("s{i}"), 0)).collect();
        keep_only(
            &mut store,
            MAX_LYRICS,
            &["s3".to_string(), "new".to_string()],
        );
        assert_eq!(store.keys().collect::<Vec<_>>(), ["s3"]);
        let mut small: HashMap<String, u8> = HashMap::from([("x".to_string(), 0)]);
        keep_only(&mut small, MAX_LYRICS, &[]);
        assert_eq!(small.len(), 1);
    }

    /// In each look: the line being sung lights word by word (drawn without
    /// fail, partway through a word); there is no Translate button over the
    /// lyrics (the owner wants it in Settings only), and with Settings'
    /// "Translate lyrics" on, each line shows its translation under it (the
    /// line grows).
    #[test]
    fn lyrics_light_word_by_word_and_show_their_translation_in_each_look() {
        use crate::lyrics::{State, Translation};
        use crate::theme::Theme;
        for theme in [
            Theme::YouTubeMusic,
            Theme::Premium,
            Theme::DynamicBackground,
        ] {
            let mut w = Window::new(theme);
            w.h.play(&["a", "b"]);
            w.h.app.now_playing = true;
            w.h.app.np_tab = NpTab::Lyrics;
            let lyrics = timed_lyrics(&[(1_000, "Paper planes tonight"), (5_000, "Over the town")]);
            let words = lyrics.lines[0].words.clone();
            w.h.app.lyrics.insert("a".into(), State::Ready(lyrics));
            // Partway through the line's second word.
            w.h.app.audio_status.entry = Some(w.h.entry().0);
            w.h.app.audio_status.position = (words[1].start + words[1].end) / 2.0;
            assert_eq!(w.h.app.lyrics_clock(), Some(w.h.app.audio_status.position));
            w.settle();
            assert!(w.shows("Paper planes tonight"), "{theme:?}");
            let before = w.largest("Paper planes tonight");
            assert!(!w.h.app.settings.translate_lyrics);
            assert!(!w.shows("Translate"), "{theme:?}: no button");
            w.h.act(Action::Toggle(Setting::TranslateLyrics));
            w.settle();
            assert!(w.h.app.settings.translate_lyrics, "{theme:?}");
            w.h.app.translations.insert(
                "a".into(),
                Translation::Ready(ytfast_core::translate::Translated {
                    language: Some("es".into()),
                    lines: vec![Some("Aviones de papel esta noche".into()), None],
                    latin: vec![None, None],
                    source: "Test".into(),
                }),
            );
            w.settle();
            let after = w.largest("Paper planes tonight");
            assert!(
                after.height() > before.height() + 10.0,
                "{theme:?}: {before:?} then {after:?}"
            );
        }
        crate::theme::set(Theme::YouTubeMusic);
    }

    /// In each look: lyrics scrolled by hand, then a line clicked further
    /// down. The song jumps to it, and the lyrics glide from where they
    /// were to put it a third of the way down, never first back to the
    /// line that was being sung (which they once did, then slid to the
    /// line clicked).
    #[test]
    fn a_lyric_clicked_after_scrolling_glides_there_from_where_it_was() {
        use crate::lyrics::State;
        use crate::theme::Theme;
        let texts: Vec<String> = (0..40).map(|n| format!("Line number {n}")).collect();
        let timed: Vec<(u64, &str)> = texts
            .iter()
            .enumerate()
            .map(|(n, text)| (n as u64 * 4_000 + 1_000, text.as_str()))
            .collect();
        for theme in [
            Theme::YouTubeMusic,
            Theme::Premium,
            Theme::DynamicBackground,
        ] {
            let mut w = Window::new(theme);
            w.h.play(&["a", "b"]);
            w.h.app.now_playing = true;
            w.h.app.np_tab = NpTab::Lyrics;
            w.h.app
                .lyrics
                .insert("a".into(), State::Ready(timed_lyrics(&timed)));
            // The sixth line being sung (a third of the way down), paused.
            w.h.app.audio_status.entry = Some(w.h.entry().0);
            w.h.app.audio_status.position = 21.5;
            // The player page risen, and at rest.
            for _ in 0..4 {
                w.settle();
            }
            let sung = w.find("Line number 5", 0);
            // Scrolled by hand, a long way down.
            for _ in 0..5 {
                w.wheel(sung.center(), -200.0);
            }
            w.frame(Vec::new());
            w.frame(Vec::new());
            // A line further down, on screen.
            let (name, before) = (8..40)
                .map(|n| format!("Line number {n}"))
                .map(|name| {
                    let rect = w.find(&name, 0);
                    (name, rect)
                })
                .find(|(_, rect)| rect.center().y > sung.center().y + 40.0 && rect.bottom() < 760.0)
                .unwrap_or_else(|| panic!("{theme:?}: no line further down on screen"));
            w.click(before.center());
            assert!(
                w.h.commands()
                    .iter()
                    .any(|c| matches!(c, Command::Seek(to) if *to > 21.5)),
                "{theme:?}: the song jumps to the line"
            );
            // The frame after the click, then until the lyrics rest.
            let mut path = vec![w.find(&name, 0).center().y];
            for _ in 0..40 {
                w.frame(Vec::new());
                path.push(w.find(&name, 0).center().y);
            }
            let rest = *path.last().unwrap();
            assert!(
                (rest - sung.center().y).abs() < 2.0,
                "{theme:?}: the line clicked rests where the sung one was: {rest} and {}",
                sung.center().y
            );
            assert!(
                path[0] <= before.center().y + 1.0,
                "{theme:?}: no jump back first: {} then {path:?}",
                before.center().y
            );
            assert!(
                path.windows(2)
                    .all(|w| w[1] <= w[0] + 0.5 && w[1] >= rest - 0.5),
                "{theme:?}: only toward its place: {path:?}"
            );
        }
        crate::theme::set(Theme::YouTubeMusic);
    }

    /// The demo's answer to what the app asks for, as the backend gives
    /// it in `--demo`: pages, Up next, lyrics, Related, a song's details.
    /// Pictures stay unloaded.
    fn demo_answer(request: Request) -> Option<Event> {
        Some(match request {
            Request::Page { route, load, order } => {
                let page = crate::demo::page(&route, order.as_deref());
                Event::Page(route, load, Ok(page))
            }
            Request::Image(url) => Event::Image(url, None),
            Request::Related(id) => Event::Related(id.clone(), Ok(crate::demo::related(&id))),
            Request::Lyrics { video_id, .. } => {
                Event::Lyrics(video_id.clone(), Some(crate::demo::lyrics(&video_id)))
            }
            Request::UpNext {
                video_id,
                playlist_id,
                queue,
            } => Event::UpNext {
                result: Ok(crate::demo::up_next(&video_id)),
                video_id,
                playlist_id,
                queue,
            },
            Request::Details(id) => Event::SongVideo(id, Ok(None)),
            _ => return None,
        })
    }

    /// Every named part of the window on `screen` in `theme`, at `size`
    /// (role, name, place), after the demo has answered and the window
    /// settled; a song from a demo album playing. In Roboto, or with
    /// `own_font` in the look's own (Dynamic Background's Inter).
    fn parts_of(
        theme: crate::theme::Theme,
        screen: &str,
        size: egui::Vec2,
        own_font: bool,
    ) -> Vec<(String, String, egui::Rect)> {
        parts_at(theme, screen, size, own_font, 1.0)
    }

    /// [`parts_of`] on a screen of `pixels_per_point` (1.5 on the owner's
    /// Windows laptop, 2 on their Mac), where words may break at other
    /// places.
    fn parts_at(
        theme: crate::theme::Theme,
        screen: &str,
        size: egui::Vec2,
        own_font: bool,
        pixels_per_point: f32,
    ) -> Vec<(String, String, egui::Rect)> {
        let mut h = Harness::new();
        h.ctx.set_pixels_per_point(pixels_per_point);
        h.ctx.enable_accesskit();
        h.app.settings.theme = theme;
        h.app
            .script_fonts
            .set_inter_first(own_font && theme == crate::theme::Theme::DynamicBackground);
        h.app.auth = Auth::SignedIn {
            name: "Listener".into(),
            handle: None,
            photo: None,
        };
        let album = Route::Browse {
            id: "demo-album-Postcards".into(),
            params: None,
        };
        let tracks: Vec<Track> = crate::demo::page(&album, None)
            .sections
            .iter()
            .flat_map(|s| &s.items)
            .filter_map(|i| match i {
                Item::Track(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        h.act(Action::PlayTracks {
            tracks,
            start: 0,
            source: None,
        });
        h.ready();
        let route = match screen {
            "album" => album,
            "playlist" => Route::Browse {
                id: "demo-playlist-Road trip".into(),
                params: None,
            },
            "artist" => Route::Browse {
                id: "demo-artist-Mara Sol".into(),
                params: None,
            },
            "search" => Route::Search("mara".into()),
            "library" => Route::Library,
            "settings" => Route::Settings,
            _ => Route::Home,
        };
        h.act(Action::Navigate(route));
        if let Some(tab) = screen.strip_prefix("player-") {
            h.app.now_playing = true;
            h.app.np_tab = match tab {
                "lyrics" => NpTab::Lyrics,
                "related" => NpTab::Related,
                _ => NpTab::UpNext,
            };
        }
        let mut parts = Vec::new();
        for frame in 0..14 {
            for request in h.requests() {
                if let Some(event) = demo_answer(request) {
                    h.events.send(event).unwrap();
                }
            }
            h.frame();
            h.app.want_song_extras();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(frame as f64 * 0.1),
                ..Default::default()
            };
            let app = &h.app;
            let mut output = h.ctx.run_ui(input, |ui| {
                crate::theme::set(app.settings.theme);
                if crate::theme::dynamic() {
                    crate::dynamic::paint(app, ui);
                }
                views::show(app, ui);
            });
            output.textures_delta.clear();
            if let Some(update) = output.platform_output.accesskit_update {
                parts = update
                    .nodes
                    .into_iter()
                    .filter_map(|(_, node)| {
                        let b = node.bounds()?;
                        let name = node
                            .label()
                            .map(str::to_string)
                            .or_else(|| node.value().map(str::to_string))?;
                        let rect = egui::Rect::from_min_max(
                            egui::pos2(b.x0 as f32, b.y0 as f32),
                            egui::pos2(b.x1 as f32, b.y1 as f32),
                        );
                        Some((format!("{:?}", node.role()), name, rect))
                    })
                    .collect();
            }
            h.app.apply_actions();
        }
        crate::theme::set(crate::theme::Theme::YouTubeMusic);
        parts
    }

    /// Whether a part of Settings is the line under the looks, which says
    /// what the look worn is, or is Dynamic Background's own switch: it
    /// differs by design between the looks (and is as tall in each).
    fn says_which_look(name: &str) -> bool {
        use crate::theme::Theme;
        Theme::ALL
            .iter()
            .any(|look| name == format!("{}.", look.words().1))
            || name == "Moving background"
            || name == "The song's colours drift slowly while it plays."
    }

    /// The parts by role, name and how many of the same came before
    /// (from the top left).
    fn keyed(
        mut parts: Vec<(String, String, egui::Rect)>,
    ) -> HashMap<(String, String, usize), egui::Rect> {
        parts.sort_by(|a, b| {
            (a.2.top(), a.2.left())
                .partial_cmp(&(b.2.top(), b.2.left()))
                .unwrap()
        });
        let mut seen: HashMap<(String, String), usize> = HashMap::new();
        parts
            .into_iter()
            .map(|(role, name, rect)| {
                let n = seen.entry((role.clone(), name.clone())).or_default();
                *n += 1;
                ((role, name, *n - 1), rect)
            })
            .collect()
    }

    /// Switching between YouTube Music's look and Dynamic Background
    /// changes colours, corners, glass and fonts, never where things are
    /// (the owner found things moving around, 11 October 2026; Premium
    /// keeps a layout of its own, at their word). Every named part YouTube
    /// Music's look draws on these screens, at three window sizes, is in
    /// Dynamic Background too, at the same place and size. Both are drawn
    /// here in Roboto: Dynamic Background's Inter, on Roboto's lines, sets
    /// words a little wider (up to 12 along a row of chips), so a card's
    /// title may take a second line there (as on YouTube Music, a card is
    /// as tall as its words). The lyrics are left out: each look sets them
    /// in its own size.
    #[test]
    fn dynamic_background_puts_things_where_youtube_music_does() {
        use crate::theme::Theme;
        let screens = [
            "home",
            "album",
            "playlist",
            "artist",
            "search",
            "library",
            "settings",
            "player-upnext",
            "player-related",
        ];
        let mut wrong = Vec::new();
        for size in [
            egui::vec2(1280.0, 820.0),
            egui::vec2(1707.0, 1019.0),
            egui::vec2(1000.0, 700.0),
        ] {
            for screen in screens {
                let without_look_line = |parts: Vec<(String, String, egui::Rect)>| {
                    parts
                        .into_iter()
                        .filter(|part| !says_which_look(&part.1))
                        .collect()
                };
                let reference = keyed(without_look_line(parts_of(
                    Theme::YouTubeMusic,
                    screen,
                    size,
                    false,
                )));
                for theme in [Theme::DynamicBackground] {
                    let other = keyed(without_look_line(parts_of(theme, screen, size, false)));
                    for (key, a) in &reference {
                        let Some(b) = other.get(key) else {
                            wrong.push(format!("{size:?} {screen} {theme:?}: no {key:?}"));
                            continue;
                        };
                        let fits = (a.top() - b.top()).abs() <= 1.0
                            && (a.height() - b.height()).abs() <= 1.0
                            && (a.left() - b.left()).abs() <= 1.0
                            && (a.width() - b.width()).abs() <= 1.0;
                        if !fits {
                            wrong.push(format!(
                                "{size:?} {screen} {theme:?}: {key:?} at {a:?} in YouTube Music, {b:?} here"
                            ));
                        }
                    }
                    for key in other.keys() {
                        if !reference.contains_key(key) {
                            wrong.push(format!("{size:?} {screen} {theme:?}: only here: {key:?}"));
                        }
                    }
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} parts differ:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    /// Choosing Dynamic Background in Settings moves no setting, in its own
    /// font: Inter, on Roboto's lines, keeps every row as tall and where it
    /// was (its words are a little wider, so a paragraph may break at
    /// another word, but onto no more lines), on screens of every scale the
    /// owner uses. (A Mac's own words are checked in `views::settings`.)
    #[test]
    fn choosing_dynamic_background_moves_no_setting() {
        use crate::theme::Theme;
        for scale in [1.0, 1.5, 2.0] {
            for size in [
                egui::vec2(1280.0, 820.0),
                egui::vec2(1707.0, 1019.0),
                egui::vec2(1000.0, 700.0),
            ] {
                let at = |theme| {
                    let parts = parts_at(theme, "settings", size, true, scale);
                    keyed(
                        parts
                            .into_iter()
                            .filter(|part| part.0 != "TextRun" && !says_which_look(&part.1))
                            .collect(),
                    )
                };
                let (youtube, dynamic) = (at(Theme::YouTubeMusic), at(Theme::DynamicBackground));
                assert_eq!(youtube.len(), dynamic.len(), "{scale} {size:?}");
                for (key, a) in &youtube {
                    let b = dynamic
                        .get(key)
                        .unwrap_or_else(|| panic!("{scale} {size:?}: no {key:?}"));
                    assert!(
                        (a.top() - b.top()).abs() <= 1.0 && (a.height() - b.height()).abs() <= 1.0,
                        "{scale} {size:?} {key:?}: {a:?} in YouTube Music, {b:?} in Dynamic Background"
                    );
                }
            }
        }
    }

    /// Settings, touched up (the owner's word, 11 October 2026): the looks
    /// as three buttons, with what the look worn is under them, or Dynamic
    /// Background's own switch; no switch for the fast way (always tried
    /// first) or for autoplay (Up next has it).
    #[test]
    fn settings_shows_the_looks_as_buttons_and_fewer_switches() {
        use crate::theme::Theme;
        let mut w = Window::new(Theme::YouTubeMusic);
        w.h.act(Action::Navigate(Route::Settings));
        w.settle();
        for name in ["YouTube Music", "Premium", "Dynamic Background"] {
            assert!(w.shows(name), "{name}");
        }
        assert!(w.says("YouTube Music's own look."));
        assert!(w.shows("Even out loudness"));
        assert!(!w.shows("Start songs the fast way"));
        assert!(!w.shows("Keep playing when the queue ends"));
        assert!(!w.shows("Moving background"));
        let dynamic = w.find("Dynamic Background", 0);
        w.click(dynamic.center());
        w.settle();
        assert_eq!(w.h.app.settings.theme, Theme::DynamicBackground);
        assert!(w.shows("Moving background"));
        crate::theme::set(Theme::YouTubeMusic);
    }

    /// A fan's video has only its still: the player page asks for its
    /// whole frame (`hq720.jpg`, the row's still being often its middle,
    /// zoomed) and, when the video has none, takes the still its row gave.
    #[test]
    fn the_player_page_shows_a_videos_whole_frame_or_its_rows_still() {
        let mut h = Harness::new();
        h.app.auth = Auth::SignedIn {
            name: "Listener".into(),
            handle: None,
            photo: None,
        };
        let still = "https://i.ytimg.com/vi/fanvideo01/sddefault.jpg?sqp=zoomed";
        let whole = "https://i.ytimg.com/vi/fanvideo01/hq720.jpg";
        let mut track = song("fanvideo01");
        track.thumbnail = Some(ytfast_core::read::Thumb {
            url: still.into(),
            width: 400,
            height: 225,
        });
        h.act(Action::PlayTracks {
            tracks: vec![track],
            start: 0,
            source: None,
        });
        h.ready();
        h.app.now_playing = true;
        let asked = |h: &mut Harness| -> Vec<String> {
            let app = &h.app;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 820.0),
                )),
                ..Default::default()
            };
            let mut output = h.ctx.run_ui(input, |ui| views::show(app, ui));
            output.textures_delta.clear();
            h.requests()
                .into_iter()
                .filter_map(|request| match request {
                    Request::Image(url) => Some(url),
                    _ => None,
                })
                .collect()
        };
        let first = asked(&mut h);
        assert!(first.iter().any(|url| url == whole), "{first:?}");
        assert!(!first.iter().any(|url| url == still), "{first:?}");
        // This video has no such frame.
        h.answer(Event::Image(whole.into(), None));
        let then = asked(&mut h);
        assert!(then.iter().any(|url| url == still), "{then:?}");
        assert!(!then.iter().any(|url| url == whole), "{then:?}");
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
