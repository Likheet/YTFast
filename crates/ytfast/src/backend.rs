//! Everything that waits on the network or on yt-dlp runs here, on a tokio
//! runtime of its own, so the window never waits. The window sends
//! [`Request`]s and reads [`Event`]s each frame.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;

use tokio::sync::{Mutex, OnceCell, RwLock, Semaphore};
use ytfast_core::cookies::CookieJar;
use ytfast_core::direct::Direct;
use ytfast_core::helpers::{self, Progress};
use ytfast_core::innertube::{ApiError, Renewer, Renewing, Session};
use ytfast_core::library::{LibraryTab, Privacy};
use ytfast_core::net;
use ytfast_core::prepare::{PrepareError, Prepared, Preparer};
use ytfast_core::read::{
    Continuation, Item, Page, PlayerInfo, Rating, Section, Shape, SongDetails, Track,
};
use ytfast_core::solver::{self, Solver};
use ytfast_core::stream::SongData;
use ytfast_core::ytdlp::{Browser, YtDlp};

use crate::{colors, demo};

/// A page the window can show.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Route {
    Home,
    Explore,
    /// Saved playlists (the Library's Playlists).
    Library,
    /// The Library's front page: everything, most recently used first.
    LibraryRecent,
    Liked,
    /// Any other page: an album, playlist, artist, mood.
    Browse {
        id: String,
        params: Option<String>,
    },
    Search(String),
    /// Search results of one kind only: the query, and what YouTube's
    /// filter button for that kind sends (its `params`).
    SearchOnly(String, String),
    /// The library's other tabs.
    LibrarySongs,
    LibraryAlbums,
    LibraryArtists,
    LibraryProfiles,
    LibraryPodcasts,
    History,
    /// YTFast's own settings (not loaded from YouTube).
    Settings,
}

impl Route {
    /// Pages that are not loaded from YouTube.
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Settings)
    }

    /// A page of shelves that YouTube Music loads more of as its end
    /// comes into view (Home, Explore, a mood's Home): its browse ID and
    /// `params`.
    pub fn shelves(&self) -> Option<(&str, Option<&str>)> {
        match self {
            Self::Home => Some(("FEmusic_home", None)),
            Self::Explore => Some(("FEmusic_explore", None)),
            Self::Browse { id, params } if id.starts_with("FEmusic") => {
                Some((id.as_str(), params.as_deref()))
            }
            _ => None,
        }
    }

    /// A page whose next part is loaded as its end comes into view
    /// ([`Request::MoreResults`]): a page of shelves, or a search of one
    /// kind.
    pub fn loads_more(&self) -> bool {
        matches!(self, Self::SearchOnly(..)) || self.shelves().is_some()
    }

    /// The playlist a page lists (its ID without `VL`; Liked Music's is
    /// `LM`), for more songs when its songs run out.
    pub fn playlist(&self) -> Option<String> {
        match self {
            Self::Liked => Some("LM".into()),
            Self::Browse { id, .. } => id.strip_prefix("VL").map(str::to_string),
            _ => None,
        }
    }

    /// The route for a page ID. Liked Music has a route of its own, so it
    /// is one page however it is reached.
    pub fn browse(id: String, params: Option<String>) -> Self {
        if id == "VLLM" && params.is_none() {
            Self::Liked
        } else {
            Self::Browse { id, params }
        }
    }

    /// The Library's pages, the front page first.
    pub const LIBRARY: [Route; 7] = [
        Route::LibraryRecent,
        Route::Library,
        Route::LibrarySongs,
        Route::LibraryAlbums,
        Route::LibraryArtists,
        Route::LibraryProfiles,
        Route::LibraryPodcasts,
    ];

    /// The Library's tab this page is, if it is one (each has a sort
    /// button of its own).
    pub fn library_tab(&self) -> Option<LibraryTab> {
        match self {
            Self::Library => Some(LibraryTab::Playlists),
            Self::LibraryRecent => Some(LibraryTab::Recent),
            Self::LibrarySongs => Some(LibraryTab::Songs),
            Self::LibraryAlbums => Some(LibraryTab::Albums),
            Self::LibraryArtists => Some(LibraryTab::Artists),
            Self::LibraryProfiles => Some(LibraryTab::Profiles),
            Self::LibraryPodcasts => Some(LibraryTab::Podcasts),
            _ => None,
        }
    }
}

pub enum Request {
    SignIn(Browser),
    /// Forget the sign-in (the browser keeps its own), after sending
    /// `last_report` (how long the song playing was listened to) with it.
    SignOut {
        last_report: Option<String>,
    },
    /// A page; `load` numbers this loading of it, given back with the
    /// answer and the rest of a long list, so an older one is known. A
    /// Library tab comes in `order` (its sort button's `params`) when one
    /// was chosen.
    Page {
        route: Route,
        load: u64,
        order: Option<String>,
    },
    /// Get a song ready. With `play`, the window gets [`Event::Prepared`];
    /// without, the song is only made ready ahead of time.
    Prepare {
        entry: u64,
        video_id: String,
        play: bool,
    },
    /// What plays after a song (its playlist, or a radio). `queue` is the
    /// queue it is asked for ([`crate::queue::Queue::generation`]), given
    /// back with the answer.
    UpNext {
        video_id: String,
        playlist_id: Option<String>,
        queue: u64,
    },
    /// The songs of a playlist or album, to play it (or queue it).
    /// `ticket` is given back with the answer, so a late one is known.
    PlaylistQueue {
        playlist_id: String,
        mode: crate::app::QueueMode,
        ticket: u64,
    },
    /// Suggestions for what is being typed in the search box.
    Suggest(String),
    /// The next part of a page whose end has come into view
    /// ([`Route::loads_more`]), for the `load` that showed it: a search of
    /// one kind's next results ([`Event::MoreResults`]), or a page of
    /// shelves' next shelves ([`Event::MoreSections`]).
    MoreResults {
        route: Route,
        load: u64,
    },
    /// Find a song's audio ahead of time (it was pointed at, or is the
    /// top search result), so it starts at once if played.
    Warm(String),
    /// A song's lyrics (time-synced when they can be found).
    Lyrics {
        video_id: String,
        title: String,
        artist: String,
        album: Option<String>,
        duration: Option<f64>,
    },
    /// Songs and artists related to a song.
    Related(String),
    /// A song started playing: read what the account thinks of it.
    Details(String),
    /// A change to the account: likes, playlists, the library.
    Edit(Edit),
    /// Whether to start songs the website's way (else always yt-dlp).
    FastWay(bool),
    /// A History report (see `playreport`).
    Report(String),
    /// The last History report as the window closes: told on `done` once
    /// it has gone.
    LastReport(String, mpsc::Sender<()>),
    /// A picture, by address.
    Image(String),
}

/// A cover, decoded, with the colours the look takes from it.
pub struct Picture {
    pub image: egui::ColorImage,
    pub summary: colors::Summary,
    /// The cover at 24 × 24 (about 2 KB), from which the Dynamic Background
    /// theme makes the background behind the whole window
    /// (`dynamic::wash`).
    pub soft: egui::ColorImage,
}

impl Picture {
    fn new(image: egui::ColorImage) -> Self {
        Self {
            summary: colors::summarize(&image),
            soft: colors::shrink(&image, crate::dynamic::SOFT),
            image,
        }
    }
}

impl Edit {
    /// What the change is to: a song, a playlist or a channel.
    pub fn item(&self) -> &str {
        match self {
            Self::Rate { video_id, .. } => video_id,
            Self::AddToPlaylist { playlist_id, .. }
            | Self::RemoveFromPlaylist { playlist_id, .. }
            | Self::RenamePlaylist { playlist_id, .. }
            | Self::DeletePlaylist { playlist_id }
            | Self::Save { playlist_id, .. } => playlist_id,
            Self::CreatePlaylist { title, .. } => title,
            Self::Subscribe { channel_id, .. } => channel_id,
            Self::ForgetSearch { token, .. } => token,
        }
    }
}

/// A change to the account.
#[derive(Clone, Debug)]
pub enum Edit {
    Rate {
        video_id: String,
        like: crate::app::LikeState,
    },
    AddToPlaylist {
        playlist_id: String,
        video_ids: Vec<String>,
    },
    /// Songs taken out of a playlist: each its song ID and its row's own
    /// (`set_video_id`).
    RemoveFromPlaylist {
        playlist_id: String,
        songs: Vec<(String, String)>,
    },
    CreatePlaylist {
        title: String,
        description: String,
        privacy: Privacy,
        video_ids: Vec<String>,
    },
    RenamePlaylist {
        playlist_id: String,
        name: String,
        description: String,
        privacy: Privacy,
    },
    DeletePlaylist {
        playlist_id: String,
    },
    /// Save an album or playlist to the library, or remove it.
    Save {
        playlist_id: String,
        save: bool,
    },
    Subscribe {
        channel_id: String,
        subscribe: bool,
    },
    /// Remove a past search from the account's search history.
    ForgetSearch {
        words: String,
        token: String,
    },
}

/// A song ready to play.
pub struct Ready {
    /// The audio, still arriving while it plays.
    pub data: Arc<SongData>,
    pub gain: f32,
    pub info: PlayerInfo,
    pub format: String,
    pub premium: bool,
    pub length: Option<f64>,
    /// How long finding the audio took, and its first part then.
    pub find_time: std::time::Duration,
    pub start_time: std::time::Duration,
    /// Found the website's way (not through yt-dlp).
    pub direct: bool,
}

#[derive(Clone, Debug)]
pub struct Failure {
    pub message: String,
    /// Only this song is affected (removed, not available here); the
    /// window can move on to the next one. Otherwise (signed out, no
    /// network) moving on would fail the same way, so it stops.
    pub song_only: bool,
}

pub enum Event {
    /// A line about what signing in is doing.
    Progress(String),
    SignedIn {
        name: String,
        /// "@handle", and the account's photo's address.
        handle: Option<String>,
        photo: Option<String>,
    },
    SignInFailed(String),
    /// YouTube treated a request as signed out.
    SignedOut,
    /// A page, for the `load` that asked for it ([`Request::Page`]).
    Page(Route, u64, Result<Page, String>),
    Prepared {
        entry: u64,
        result: Result<Ready, Failure>,
    },
    UpNext {
        video_id: String,
        playlist_id: Option<String>,
        queue: u64,
        /// The songs, and what they play from ("Playing from").
        result: Result<(Vec<Track>, Option<String>), String>,
    },
    PlaylistQueue {
        playlist_id: String,
        mode: crate::app::QueueMode,
        ticket: u64,
        result: Result<Vec<Track>, String>,
    },
    Suggestions(String, ytfast_core::read::Suggestions),
    /// More songs of a long list already shown (a playlist, Liked
    /// Music), loaded after its first ones.
    MoreRows {
        route: Route,
        load: u64,
        tracks: Vec<Track>,
    },
    /// The next results of a search of one kind ([`Request::MoreResults`]);
    /// `done` when there are no more after them.
    MoreResults {
        route: Route,
        load: u64,
        items: Vec<Item>,
        done: bool,
    },
    /// The next shelves of a page of shelves (Home's, [`Request::MoreResults`]);
    /// `done` when there are no more after them.
    MoreSections {
        route: Route,
        load: u64,
        sections: Vec<Section>,
        done: bool,
    },
    Image(String, Option<Picture>),
    /// A picture not fetched, because it was no longer wanted (it scrolled
    /// past): it is asked for again when it next shows.
    ImageSkipped(String),
    Lyrics(String, Option<crate::lyrics::Lyrics>),
    Related(String, Result<Page, String>),
    /// What the account thinks of a song, as YouTube says.
    Liked(String, crate::app::LikeState),
    /// A change to the account failed; the app undoes what it showed.
    EditFailed(Edit, String),
    /// A change to the account went through.
    Edited(Edit),
}

/// The window's end of the backend.
pub struct Backend {
    requests: tokio::sync::mpsc::UnboundedSender<Request>,
    events: mpsc::Receiver<Event>,
    /// The private folder holding this run's copy of the YouTube cookies.
    session_dir: PathBuf,
    /// Held while this run is open, so another run leaves the folder be
    /// (see [`still_open`]).
    session_lock: Option<std::fs::File>,
}

impl Backend {
    /// Starts the backend thread. `wake` asks the window to redraw.
    pub fn start(wake: impl Fn() + Send + Sync + 'static, demo: bool) -> std::io::Result<Self> {
        let folders = Folders::new(demo)?;
        let session_dir = folders.session.clone();
        let session_lock = lock_session(&session_dir);
        let (requests, receiver) = tokio::sync::mpsc::unbounded_channel();
        let (sender, events) = mpsc::channel();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        std::thread::Builder::new()
            .name("ytfast-backend".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("the backend runtime starts");
                runtime.block_on(serve(receiver, sender, wake, folders, demo));
            })?;
        Ok(Self {
            requests,
            events,
            session_dir,
            session_lock,
        })
    }

    pub fn send(&self, request: Request) {
        let _ = self.requests.send(request);
    }

    pub fn try_recv(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    /// Sends the last listening report and waits until it has gone (two
    /// seconds at most): the backend stops with the window.
    pub fn report_before_closing(&self, url: String) {
        let (done, gone) = mpsc::channel();
        self.send(Request::LastReport(url, done));
        let _ = gone.recv_timeout(std::time::Duration::from_secs(2));
    }

    /// A backend with nothing behind it, for the app's tests: what the app
    /// asks for comes out of the receiver, and events go in by the sender.
    #[cfg(test)]
    pub fn for_tests() -> (
        Self,
        tokio::sync::mpsc::UnboundedReceiver<Request>,
        mpsc::Sender<Event>,
    ) {
        let (requests, receiver) = tokio::sync::mpsc::unbounded_channel();
        let (sender, events) = mpsc::channel();
        let backend = Self {
            requests,
            events,
            session_dir: PathBuf::new(),
            session_lock: None,
        };
        (backend, receiver, sender)
    }

    /// Removes this run's sign-in copy. Called when the window closes.
    pub fn forget_sign_in(&mut self) {
        // Let go of the folder's lock first: Windows keeps a file in use.
        drop(self.session_lock.take());
        let _ = std::fs::remove_dir_all(&self.session_dir);
    }
}

/// The file each run keeps locked in its sign-in folder while it is open.
const SESSION_LOCK: &str = "open";

/// Locks this run's sign-in folder while the run is open. The lock goes
/// when the run ends, however it ends (a crash too).
fn lock_session(folder: &Path) -> Option<std::fs::File> {
    let file = std::fs::File::create(folder.join(SESSION_LOCK)).ok()?;
    file.try_lock().ok()?;
    Some(file)
}

/// Whether the run that made this sign-in folder is still open (another
/// YTFast window): it holds the folder's lock.
fn still_open(folder: &Path) -> bool {
    std::fs::File::open(folder.join(SESSION_LOCK))
        .is_ok_and(|file| matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock)))
}

/// Where YtFast keeps its helpers, caches and this run's sign-in copy.
struct Folders {
    helpers: PathBuf,
    yt_dlp_cache: PathBuf,
    session: PathBuf,
    /// YouTube's player code, for finding songs the fast way.
    player: PathBuf,
    /// The solver program.
    solver: PathBuf,
}

impl Folders {
    /// YtFast's own folders, or, when they cannot be made (no home folder,
    /// a full disk, security software), a temporary one, so the window
    /// still opens.
    fn new(demo: bool) -> std::io::Result<Self> {
        let own = directories::ProjectDirs::from("", "", "YtFast")
            .ok_or_else(|| std::io::Error::other("no home folder"))
            .and_then(|dirs| Self::under(dirs.cache_dir(), dirs.data_local_dir(), demo));
        own.or_else(|e| {
            log::warn!("YtFast's folders could not be made ({e}); using a temporary folder");
            let base = std::env::temp_dir().join("YtFast");
            Self::under(&base.join("cache"), &base.join("data"), demo)
        })
    }

    fn under(cache: &Path, data: &Path, demo: bool) -> std::io::Result<Self> {
        // Left over from a run that did not close properly. A run still
        // open (YTFast opened twice) keeps its folder: its sign-in is in
        // use. The demo leaves them all be: it may be open beside YTFast in
        // real use, whose sign-in is in one of them.
        let sessions = cache.join("app-sessions");
        if !demo {
            if let Ok(entries) = std::fs::read_dir(&sessions) {
                for entry in entries.flatten() {
                    if !still_open(&entry.path()) {
                        let _ = std::fs::remove_dir_all(entry.path());
                    }
                }
            }
            forget_old_check_sessions(&cache.join("sessions"));
        }
        let session = sessions.join(std::process::id().to_string());
        std::fs::create_dir_all(&session)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&session, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            helpers: data.join("helpers"),
            yt_dlp_cache: cache.join("yt-dlp"),
            player: cache.join("player"),
            solver: cache.join("solver"),
            session,
        })
    }
}

/// The check program (ytfast-check) keeps its sign-in copy in `sessions`,
/// and a check closed part way can leave it behind. Those not touched for
/// half a day are surely not in use: they go.
fn forget_old_check_sessions(sessions: &Path) {
    const OLD: std::time::Duration = std::time::Duration::from_secs(12 * 60 * 60);
    let Ok(entries) = std::fs::read_dir(sessions) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age > OLD);
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Writes a file only this user can read.
fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(contents.as_bytes())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, contents)
    }
}

/// Songs made ready, by video ID. A cell is shared, so a song asked for
/// twice (made ready ahead, then played) is only fetched once.
type PreparedCell = Arc<OnceCell<Result<Arc<Prepared>, Failure>>>;

/// The most songs kept ready ahead (each is a few MB).
const READY_AHEAD: usize = 3;

struct Shared {
    events: mpsc::Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
    folders: Folders,
    demo: bool,
    signed_in: RwLock<Option<Preparer>>,
    prepared: Mutex<(HashMap<String, PreparedCell>, VecDeque<String>)>,
    images: Semaphore,
    download: reqwest::Client,
    /// Start songs the website's way (see [`Request::FastWay`]).
    fast_way: std::sync::atomic::AtomicBool,
    /// What YouTube said about songs played (likes, lyrics, related),
    /// asked once per song.
    details: Mutex<HashMap<String, Arc<OnceCell<SongDetails>>>>,
    /// The latest loading of each page ([`Request::Page`]): an older one
    /// stops fetching the rest of its long list.
    loads: std::sync::Mutex<HashMap<Route, u64>>,
    /// Where the next results of the last few searches of one kind come
    /// from, by page and loading ([`Request::MoreResults`]).
    more_results: std::sync::Mutex<VecDeque<(Route, u64, Continuation)>>,
    /// Counts the pictures asked for, so one asked for long ago (it
    /// scrolled past while others waited) is skipped.
    pictures_asked: std::sync::atomic::AtomicU64,
    /// Counts sign-ins and sign-outs: a session's renewer reads the
    /// browser again only while its sign-in is the current one.
    sign_ins: Arc<std::sync::atomic::AtomicU64>,
}

impl Shared {
    fn send(&self, event: Event) {
        let _ = self.events.send(event);
        (self.wake)();
    }

    async fn preparer(&self) -> Option<Preparer> {
        let mut preparer = self.signed_in.read().await.clone()?;
        if !self.fast_way.load(std::sync::atomic::Ordering::Relaxed) {
            preparer.direct = None;
        }
        Some(preparer)
    }
}

async fn serve(
    mut requests: tokio::sync::mpsc::UnboundedReceiver<Request>,
    events: mpsc::Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
    folders: Folders,
    demo: bool,
) {
    let shared = Arc::new(Shared {
        events,
        wake,
        folders,
        demo,
        signed_in: RwLock::new(None),
        prepared: Mutex::new((HashMap::new(), VecDeque::new())),
        images: Semaphore::new(6),
        download: net::download_client(),
        fast_way: std::sync::atomic::AtomicBool::new(true),
        details: Mutex::new(HashMap::new()),
        loads: std::sync::Mutex::new(HashMap::new()),
        more_results: std::sync::Mutex::new(VecDeque::new()),
        pictures_asked: std::sync::atomic::AtomicU64::new(0),
        sign_ins: Arc::new(std::sync::atomic::AtomicU64::new(0)),
    });
    // Rest the solver when YtFast is not being used.
    {
        let shared = Arc::clone(&shared);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let direct = shared
                    .signed_in
                    .read()
                    .await
                    .as_ref()
                    .and_then(|p| p.direct.clone());
                if let Some(direct) = direct {
                    direct.rest_if_idle(SOLVER_IDLE).await;
                }
            }
        });
    }
    // Changes to the account go to YouTube one at a time, in the order they
    // were made: a quick like and unlike must end unliked.
    let (edits, mut queued_edits) = tokio::sync::mpsc::unbounded_channel::<Edit>();
    {
        let shared = Arc::clone(&shared);
        tokio::spawn(async move {
            while let Some(change) = queued_edits.recv().await {
                // One that stops part way does not stop the ones after it.
                let on_failure = failure_for(&Request::Edit(change.clone()));
                let worker = Arc::clone(&shared);
                if let Err(e) = tokio::spawn(async move { edit(&worker, change).await }).await
                    && e.is_panic()
                {
                    log::error!("a change to the account stopped part way: {e}");
                    if let Some(event) = on_failure {
                        shared.send(event);
                    }
                }
            }
        });
    }
    while let Some(request) = requests.recv().await {
        if let Request::Edit(change) = request {
            let _ = edits.send(change);
            continue;
        }
        // Should the work stop part way (a panic), the window is still
        // told, so no screen waits for ever.
        let on_failure = failure_for(&request);
        let watcher = Arc::clone(&shared);
        let shared = Arc::clone(&shared);
        let work = tokio::spawn(async move {
            match request {
                Request::SignIn(browser) => sign_in(&shared, browser).await,
                Request::SignOut { last_report } => {
                    // The last song's listening time goes with the old
                    // sign-in, before it is let go.
                    if let Some(url) = last_report {
                        report(&shared, url).await;
                    }
                    // The old session must not read the browser again
                    // (and write its sign-in back) from work still going.
                    shared
                        .sign_ins
                        .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                    *shared.signed_in.write().await = None;
                    let _ =
                        std::fs::remove_file(shared.folders.session.join("youtube-cookies.txt"));
                    {
                        let mut guard = shared.prepared.lock().await;
                        guard.0.clear();
                        guard.1.clear();
                    }
                    // What the old account thought of songs (its likes).
                    shared.details.lock().await.clear();
                }
                Request::Page { route, load, order } => page(&shared, route, load, order).await,
                Request::Prepare {
                    entry,
                    video_id,
                    play,
                } => prepare(&shared, entry, video_id, play).await,
                Request::UpNext {
                    video_id,
                    playlist_id,
                    queue,
                } => up_next(&shared, video_id, playlist_id, queue).await,
                Request::PlaylistQueue {
                    playlist_id,
                    mode,
                    ticket,
                } => playlist_queue(&shared, playlist_id, mode, ticket).await,
                Request::Suggest(text) => suggest(&shared, text).await,
                Request::MoreResults { route, load } => more_results(&shared, route, load).await,
                Request::Warm(video_id) => {
                    if !shared.demo
                        && let Some(preparer) = shared.preparer().await
                    {
                        preparer.warm(&video_id).await;
                    }
                }
                Request::Lyrics {
                    video_id,
                    title,
                    artist,
                    album,
                    duration,
                } => lyrics(&shared, video_id, title, artist, album, duration).await,
                Request::Related(video_id) => related(&shared, video_id).await,
                Request::Details(video_id) => song_started(&shared, video_id).await,
                // Queued above, in order.
                Request::Edit(_) => {}
                Request::FastWay(on) => shared
                    .fast_way
                    .store(on, std::sync::atomic::Ordering::Relaxed),
                Request::Report(url) => report(&shared, url).await,
                Request::LastReport(url, done) => {
                    report(&shared, url).await;
                    let _ = done.send(());
                }
                Request::Image(url) => image(&shared, url).await,
            }
        });
        tokio::spawn(async move {
            if let Err(e) = work.await
                && e.is_panic()
            {
                log::error!("work for the window stopped part way: {e}");
                if let Some(event) = on_failure {
                    watcher.send(event);
                }
            }
        });
    }
}

/// What the window waits for after `request`, as a failure: sent when the
/// work for it stops part way.
fn failure_for(request: &Request) -> Option<Event> {
    let failed = || {
        "Something went wrong in YTFast. Try again; if it keeps happening, send ytfast.log."
            .to_string()
    };
    Some(match request {
        Request::SignIn(_) => Event::SignInFailed(failed()),
        Request::Page { route, load, .. } => Event::Page(route.clone(), *load, Err(failed())),
        Request::Prepare {
            entry, play: true, ..
        } => Event::Prepared {
            entry: *entry,
            result: Err(Failure {
                message: failed(),
                song_only: false,
            }),
        },
        Request::UpNext {
            video_id,
            playlist_id,
            queue,
        } => Event::UpNext {
            video_id: video_id.clone(),
            playlist_id: playlist_id.clone(),
            queue: *queue,
            result: Err(failed()),
        },
        Request::PlaylistQueue {
            playlist_id,
            mode,
            ticket,
        } => Event::PlaylistQueue {
            playlist_id: playlist_id.clone(),
            mode: *mode,
            ticket: *ticket,
            result: Err(failed()),
        },
        Request::Lyrics { video_id, .. } => Event::Lyrics(video_id.clone(), None),
        Request::Related(video_id) => Event::Related(video_id.clone(), Err(failed())),
        Request::Image(url) => Event::Image(url.clone(), None),
        Request::Edit(change) => Event::EditFailed(change.clone(), failed()),
        // No more, rather than asking again while the end is in view.
        Request::MoreResults { route, load } if route.shelves().is_some() => Event::MoreSections {
            route: route.clone(),
            load: *load,
            sections: Vec::new(),
            done: true,
        },
        Request::MoreResults { route, load } => Event::MoreResults {
            route: route.clone(),
            load: *load,
            items: Vec::new(),
            done: true,
        },
        _ => return None,
    })
}

/// How many pages' next parts are remembered (searches of one kind, pages
/// of shelves).
const MORE_RESULTS_KEPT: usize = 8;

/// Remembers where `route`'s next part comes from, for its `load`.
fn keep_more_results(shared: &Shared, route: &Route, load: u64, from: Continuation) {
    if let Ok(mut kept) = shared.more_results.lock() {
        kept.retain(|(r, ..)| r != route);
        kept.push_back((route.clone(), load, from));
        while kept.len() > MORE_RESULTS_KEPT {
            kept.pop_front();
        }
    }
}

/// The next part of a page, as its end comes into view: one request each
/// time, as YouTube Music asks.
async fn more_results(shared: &Shared, route: Route, load: u64) {
    let from = shared.more_results.lock().ok().and_then(|mut kept| {
        let at = kept
            .iter()
            .position(|(r, l, _)| *r == route && *l == load)?;
        kept.remove(at).map(|(.., from)| from)
    });
    if route.shelves().is_some() {
        return more_shelves(shared, route, load, from).await;
    }
    let answer = |items: Vec<Item>, done: bool| Event::MoreResults {
        route: route.clone(),
        load,
        items,
        done,
    };
    let (Some(from), Route::SearchOnly(query, params)) = (from, &route) else {
        shared.send(answer(Vec::new(), true));
        return;
    };
    if shared.demo {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        shared.send(answer(demo::more_results(query, params), true));
        return;
    }
    let Some(preparer) = shared.preparer().await else {
        shared.send(answer(Vec::new(), true));
        return;
    };
    match preparer
        .session
        .more_search_results(query, params, &from)
        .await
    {
        Ok((items, next)) => {
            let done = next.is_none() || items.is_empty();
            if let Some(next) = next.filter(|_| !done) {
                keep_more_results(shared, &route, load, next);
            }
            shared.send(answer(items, done));
        }
        Err(e) => {
            log::warn!("more search results did not load");
            api_error(shared, e);
            shared.send(answer(Vec::new(), true));
        }
    }
}

/// A page of shelves (`Route::shelves`); where its next shelves come from
/// is kept for when its end comes into view.
async fn shelves_page(
    shared: &Shared,
    session: &Session,
    route: &Route,
    load: u64,
) -> Result<(Page, Option<Continuation>), ApiError> {
    let (id, params) = route.shelves().unwrap_or_default();
    let (page, more) = session.shelves_page(id, params).await?;
    if let Some(more) = more {
        keep_more_results(shared, route, load, more);
    }
    Ok((page, None))
}

/// The next shelves of a page of shelves (Home's), from `from`.
async fn more_shelves(shared: &Shared, route: Route, load: u64, from: Option<Continuation>) {
    let answer = |sections: Vec<Section>, done: bool| Event::MoreSections {
        route: route.clone(),
        load,
        sections,
        done,
    };
    let (Some(from), Some((id, params))) = (from, route.shelves()) else {
        shared.send(answer(Vec::new(), true));
        return;
    };
    if shared.demo {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        shared.send(answer(demo::more_shelves(&route), true));
        return;
    }
    let Some(preparer) = shared.preparer().await else {
        shared.send(answer(Vec::new(), true));
        return;
    };
    match preparer.session.more_shelves(id, params, &from).await {
        Ok((sections, next)) => {
            let done = next.is_none() || sections.is_empty();
            if let Some(next) = next.filter(|_| !done) {
                keep_more_results(shared, &route, load, next);
            }
            shared.send(answer(sections, done));
        }
        Err(e) => {
            log::warn!("a page's next shelves did not load");
            api_error(shared, e);
            shared.send(answer(Vec::new(), true));
        }
    }
}

async fn sign_in(shared: &Shared, browser: Browser) {
    if shared.demo {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        shared.send(Event::SignedIn {
            name: "Demo listener".into(),
            handle: Some("@demolistener".into()),
            photo: None,
        });
        return;
    }
    match try_sign_in(shared, browser).await {
        Ok(account) => shared.send(Event::SignedIn {
            name: account.name,
            handle: account.handle,
            photo: account.photo.map(|p| p.url),
        }),
        Err(message) => shared.send(Event::SignInFailed(message)),
    }
}

async fn try_sign_in(
    shared: &Shared,
    browser: Browser,
) -> Result<ytfast_core::read::Account, String> {
    let progress = |p: Progress| {
        let line = match p {
            Progress::Checking => "Checking the helper programs...".to_string(),
            Progress::Downloading { what, done, total } => match total {
                Some(total) => format!(
                    "Downloading {what} (first time only): {:.0} of {:.0} MB",
                    done as f64 / 1_048_576.0,
                    total as f64 / 1_048_576.0
                ),
                None => format!("Downloading {what} (first time only)..."),
            },
            Progress::Unpacking(what) => format!("Unpacking {what}..."),
            Progress::UsingInstalled { what, version } => format!("{what} {version} is ready."),
        };
        shared.send(Event::Progress(line));
    };
    let helpers = helpers::ensure(&shared.download, &shared.folders.helpers, &progress)
        .await
        .map_err(|e| {
            format!(
                "Could not get the helper programs: {}",
                ytfast_core::redact::urls(&format!("{e:#}"))
            )
        })?;
    let yt_dlp = YtDlp::new(&helpers, shared.folders.yt_dlp_cache.clone());

    shared.send(Event::Progress(format!(
        "Reading your sign-in from {}...",
        browser.label()
    )));
    let jar: CookieJar = yt_dlp
        .read_browser_sign_in(browser, None, &shared.folders.session)
        .await
        .map_err(|e| sign_in_problem(&e, browser, ytfast_core::cookies::full_disk_access()))?;
    if !jar.looks_signed_in() {
        return Err(format!(
            "{} has YouTube data but is not signed in. Open music.youtube.com there, sign in, then try again.",
            browser.label()
        ));
    }
    shared.send(Event::Progress("Opening YouTube Music...".into()));
    let session = Session::start(net::api_client(), &jar)
        .await
        .map_err(|e| e.to_string())?;
    let signed_out =
        || "YouTube Music treated the sign-in as signed out. Sign in again in your browser.";
    let (account, flags) = match session.account().await {
        Ok(found) => found,
        Err(ApiError::SignedOut) => return Err(signed_out().into()),
        Err(e) => return Err(e.to_string()),
    };
    if flags.logged_in == Some(false) {
        return Err(signed_out().into());
    }
    let account = account.unwrap_or_else(|| ytfast_core::read::Account {
        name: "your account".into(),
        handle: None,
        photo: None,
    });
    // yt-dlp's copy, kept only once the sign-in is known to work (a failed
    // sign-in leaves none behind).
    let cookies_file = shared.folders.session.join("youtube-cookies.txt");
    write_private(&cookies_file, &jar.to_netscape())
        .map_err(|e| format!("Could not keep the sign-in: {e}"))?;
    let session = Arc::new(session);
    session.renew_with(renewer(Renewal {
        yt_dlp: yt_dlp.clone(),
        browser,
        scratch: shared.folders.session.clone(),
        cookies_file: cookies_file.clone(),
        account: session.config().user_session_id.clone(),
        sign_ins: Arc::clone(&shared.sign_ins),
        sign_in: shared.sign_ins.load(std::sync::atomic::Ordering::Acquire),
    }));
    let direct = fast_way(shared, &helpers, &session);
    *shared.signed_in.write().await = Some(Preparer {
        session,
        yt_dlp,
        download: shared.download.clone(),
        cookies_file,
        direct,
    });
    Ok(account)
}

/// Why a browser's sign-in could not be read, in words for the sign-in
/// screen. On a Mac without Full Disk Access (`full_disk_access`,
/// [`ytfast_core::cookies::full_disk_access`]) the system hides other apps'
/// data, which yt-dlp takes for missing: whichever the browser, that is
/// then what is said, and the screen shows the button that opens the
/// permission's switch (`views::signin`).
fn sign_in_problem(
    error: &ytfast_core::ytdlp::YtDlpError,
    browser: Browser,
    full_disk_access: Option<bool>,
) -> String {
    if error.browser_data_unreadable() && full_disk_access == Some(false) {
        return format!(
            "Full Disk Access is not given, so YTFast cannot read {}'s sign-in. In System \
             Settings, Privacy & Security, Full Disk Access, turn on YTFast (if it is not \
             in the list, click + and choose it in Applications), then let your Mac reopen it.",
            browser.label()
        );
    }
    error.to_string()
}

/// What a session needs to read the sign-in again (see [`renewer`]).
#[derive(Clone)]
struct Renewal {
    yt_dlp: YtDlp,
    browser: Browser,
    scratch: PathBuf,
    cookies_file: PathBuf,
    /// The account signed in (the page's user session ID), when known.
    account: Option<String>,
    /// [`Shared::sign_ins`], and its value at this sign-in.
    sign_ins: Arc<std::sync::atomic::AtomicU64>,
    sign_in: u64,
}

impl Renewal {
    /// This sign-in is still the current one (not signed out since).
    fn current(&self) -> bool {
        self.sign_ins.load(std::sync::atomic::Ordering::Acquire) == self.sign_in
    }
}

/// How the session reads the sign-in again when YouTube stops accepting
/// its copy. A browser renews its sign-in as it goes, which ends a copy
/// taken earlier (within the hour, with YouTube open in the browser);
/// reading the browser's again mends it, without asking the user. Only
/// the same account is taken, and nothing once signed out.
fn renewer(renewal: Renewal) -> Renewer {
    Arc::new(move || -> Renewing {
        let renewal = renewal.clone();
        Box::pin(async move {
            let browser = renewal.browser;
            if !renewal.current() {
                return None;
            }
            let read = renewal
                .yt_dlp
                .read_browser_sign_in(browser, None, &renewal.scratch)
                .await;
            let jar = match read {
                Ok(jar) => jar,
                Err(e) => {
                    log::warn!("the sign-in could not be read again: {e}");
                    return None;
                }
            };
            if !jar.looks_signed_in() {
                log::warn!("{} is no longer signed in to YouTube", browser.label());
                return None;
            }
            // The browser may now be signed in to another Google account:
            // that one is not taken over without asking.
            if let Some(account) = &renewal.account {
                match Session::start(net::api_client(), &jar).await {
                    Ok(check) => {
                        let now = check.config().user_session_id.as_ref();
                        if now.is_some_and(|now| now != account) {
                            log::warn!(
                                "{} is now signed in to another account; not taking it",
                                browser.label()
                            );
                            return None;
                        }
                    }
                    Err(e) => log::info!("could not check the account read again: {e}"),
                }
            }
            if !renewal.current() {
                return None;
            }
            // yt-dlp's copy, too.
            if let Err(e) = write_private(&renewal.cookies_file, &jar.to_netscape()) {
                log::warn!("the sign-in read again could not be kept for yt-dlp: {e}");
            }
            // Not a fault, but worth knowing how often it happens.
            log::warn!(
                "YouTube stopped accepting the sign-in; read it from {} again",
                browser.label()
            );
            Some(jar)
        })
    })
}

/// Sets up finding songs the website's way, and gets it ready in the
/// background so the first song is quick too. `None` when it cannot be set
/// up; songs then come through yt-dlp, more slowly.
fn fast_way(
    shared: &Shared,
    helpers: &helpers::Helpers,
    session: &Arc<Session>,
) -> Option<Arc<Direct>> {
    let Some(ejs) = solver::find_ejs(&helpers.yt_dlp) else {
        log::warn!("yt-dlp's solver scripts were not found; songs will start more slowly");
        return None;
    };
    let solver = match Solver::new(helpers.deno.clone(), &ejs, &shared.folders.solver) {
        Ok(solver) => solver,
        Err(e) => {
            log::warn!("the solver could not be set up: {e}");
            return None;
        }
    };
    let direct = Arc::new(Direct::new(
        Arc::clone(session),
        solver,
        shared.folders.player.clone(),
    ));
    let warming = Arc::clone(&direct);
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        match warming.warm_up().await {
            Ok(()) => log::info!(
                "the fast way is ready ({:.1} s)",
                started.elapsed().as_secs_f64()
            ),
            Err(e) => log::warn!("the fast way could not get ready: {e}"),
        }
    });
    Some(direct)
}

/// How long the solver may sit unused before it is stopped to free its
/// memory. It starts again in about a second when needed.
const SOLVER_IDLE: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// A failure to talk to YouTube, in words for the window; the details go
/// to the log. A refused sign-in also shows the sign-in screen.
fn api_error(shared: &Shared, error: ApiError) -> String {
    log::warn!("YouTube Music: {error}");
    if matches!(error, ApiError::SignedOut) {
        shared.send(Event::SignedOut);
    }
    plain(&error).to_string()
}

fn plain(error: &ApiError) -> &'static str {
    match error {
        ApiError::SignedOut => "YouTube no longer accepts the sign-in.",
        ApiError::Network(_) => "YouTube could not be reached. Check the internet connection.",
        ApiError::Http { .. } | ApiError::Unexpected(_) => {
            "YouTube Music did not answer as expected. Try again in a moment."
        }
    }
}

async fn page(shared: &Shared, route: Route, load: u64, order: Option<String>) {
    if let Ok(mut loads) = shared.loads.lock() {
        loads.insert(route.clone(), load);
    }
    if shared.demo {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let page = demo::page(&route, order.as_deref());
        // A search of one kind, and Home, have one more batch of made-up
        // results.
        if matches!(route, Route::SearchOnly(..) | Route::Home) {
            keep_more_results(shared, &route, load, Continuation::Body("demo".into()));
        }
        shared.send(Event::Page(route.clone(), load, Ok(page)));
        return;
    }
    let Some(preparer) = shared.preparer().await else {
        shared.send(Event::Page(route, load, Err("Not signed in.".into())));
        return;
    };
    let session = &preparer.session;
    let result = match &route {
        // Pages of shelves: their next shelves wait until the page's end
        // comes into view.
        Route::Home | Route::Explore => shelves_page(shared, session, &route, load).await,
        Route::Browse { id, .. } if id.starts_with("FEmusic") => {
            shelves_page(shared, session, &route, load).await
        }
        Route::Library => session
            .library_playlists(order.as_deref())
            .await
            .map(|p| (p, None)),
        Route::LibraryRecent => session
            .library(LibraryTab::Recent, order.as_deref())
            .await
            .map(|p| (p, None)),
        Route::Liked => session.long_page("VLLM", None).await,
        Route::Browse { id, params } => session.long_page(id, params.as_deref()).await,
        Route::Search(query) => session.search(query).await.map(|p| (p, None)),
        // Its next results wait until the list's end comes into view.
        Route::SearchOnly(query, params) => {
            session
                .search_filtered(query, params)
                .await
                .map(|(page, more)| {
                    if let Some(more) = more {
                        keep_more_results(shared, &route, load, more);
                    }
                    (page, None)
                })
        }
        Route::LibrarySongs => {
            session
                .long_page(LibraryTab::Songs.browse_id(), order.as_deref())
                .await
        }
        Route::LibraryAlbums => session
            .library(LibraryTab::Albums, order.as_deref())
            .await
            .map(|p| (p, None)),
        Route::LibraryProfiles => session
            .library(LibraryTab::Profiles, order.as_deref())
            .await
            .map(|p| (p, None)),
        Route::LibraryPodcasts => session
            .library(LibraryTab::Podcasts, order.as_deref())
            .await
            .map(|p| (p, None)),
        Route::LibraryArtists => session
            .library(LibraryTab::Artists, order.as_deref())
            .await
            .map(|p| (p, None)),
        Route::History => session.history_page().await.map(|p| (p, None)),
        Route::Settings => return,
    };
    let more = match result {
        Ok((mut page, more)) => {
            // The Library's tabs draw their own chips.
            let tab = route.library_tab();
            if tab.is_some_and(|t| t != LibraryTab::Recent) || route == Route::History {
                drop_chips(&mut page);
            }
            shared.send(Event::Page(route.clone(), load, Ok(page)));
            more
        }
        Err(e) => {
            let message = api_error(shared, e);
            shared.send(Event::Page(route, load, Err(message)));
            return;
        }
    };
    // The rest of a long list, a hundred or so songs at a time, while this
    // is still the page's latest loading (a reload takes over).
    let latest = || {
        shared
            .loads
            .lock()
            .map_or(true, |loads| loads.get(&route) == Some(&load))
    };
    let mut next = more;
    let mut batches = 0;
    while let Some(from) = next.take() {
        batches += 1;
        if batches > MAX_BATCHES || !latest() {
            break;
        }
        match session.more_tracks(&from).await {
            Ok((tracks, after)) if !tracks.is_empty() => {
                shared.send(Event::MoreRows {
                    route: route.clone(),
                    load,
                    tracks,
                });
                next = after;
            }
            Ok(_) => break,
            Err(e) => {
                log::warn!("the rest of a list did not load");
                api_error(shared, e);
                break;
            }
        }
    }
}

/// Takes out the library's row of buttons (Playlists, Songs...): YTFast
/// shows its own tabs there.
fn drop_chips(page: &mut Page) {
    page.sections.retain(|section| {
        let buttons = section.title.is_empty()
            && section.shape == Shape::Grid
            && section
                .items
                .iter()
                .all(|item| matches!(item, Item::Card(card) if card.thumbnail.is_none()));
        !buttons
    });
}

/// The most batches of a long list loaded (about 10,000 songs).
const MAX_BATCHES: usize = 100;

/// Only a problem with this one song lets the window move on to the next.
/// Anything else (the sign-in, the network, YouTube slowing the account
/// down, yt-dlp unable to get audio) would fail the same way for every
/// song, so moving on would only run through the queue.
fn classify(error: PrepareError) -> Failure {
    let song_only = matches!(
        error,
        PrepareError::NoPlayableAudio | PrepareError::Unavailable(_)
    );
    Failure {
        message: error.to_string(),
        song_only,
    }
}

async fn prepare(shared: &Shared, entry: u64, video_id: String, play: bool) {
    if shared.demo {
        if play {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            shared.send(Event::Prepared {
                entry,
                result: Ok(Ready {
                    data: SongData::complete(Vec::new()),
                    gain: 1.0,
                    info: PlayerInfo::default(),
                    format: "Demo (silent)".into(),
                    premium: true,
                    length: Some(demo::length(&video_id)),
                    find_time: std::time::Duration::ZERO,
                    start_time: std::time::Duration::ZERO,
                    direct: true,
                }),
            });
        }
        return;
    }
    let Some(preparer) = shared.preparer().await else {
        if play {
            let failure = Failure {
                message: "Not signed in.".into(),
                song_only: false,
            };
            shared.send(Event::Prepared {
                entry,
                result: Err(failure),
            });
        }
        return;
    };
    let cell = {
        let mut guard = shared.prepared.lock().await;
        let (cells, order) = &mut *guard;
        let cell = cells.entry(video_id.clone()).or_default().clone();
        if !order.contains(&video_id) {
            order.push_back(video_id.clone());
        }
        // Keep only the newest few ready.
        while order.len() > READY_AHEAD {
            if let Some(old) = order.pop_front() {
                cells.remove(&old);
            }
        }
        cell
    };
    let result = cell
        .get_or_init(|| async {
            preparer
                .prepare(&video_id, None)
                .await
                .map(Arc::new)
                .map_err(classify)
        })
        .await
        .clone();
    if !play {
        // A song that could not be made ready ahead is tried again when it
        // is played, rather than failing from memory.
        if result.is_err() {
            let mut guard = shared.prepared.lock().await;
            let (cells, order) = &mut *guard;
            cells.remove(&video_id);
            order.retain(|v| v != &video_id);
        }
        return;
    }
    // Handed to the player: no need to keep it here.
    {
        let mut guard = shared.prepared.lock().await;
        let (cells, order) = &mut *guard;
        cells.remove(&video_id);
        order.retain(|v| v != &video_id);
    }
    if let Err(failure) = &result
        && !failure.song_only
    {
        log::warn!("preparing a song failed: {}", failure.message);
    }
    let result = result.map(|p| Ready {
        data: Arc::clone(&p.data),
        gain: p.gain,
        info: p.info.clone(),
        format: p.format.clone(),
        premium: p.premium,
        length: p.duration_seconds,
        find_time: p.find_time,
        start_time: p.start_time,
        direct: p.direct,
    });
    shared.send(Event::Prepared { entry, result });
}

async fn up_next(shared: &Shared, video_id: String, playlist_id: Option<String>, queue: u64) {
    let result = if shared.demo {
        Ok(demo::up_next(&video_id))
    } else {
        match shared.preparer().await {
            Some(p) => p
                .session
                .up_next(&video_id, playlist_id.as_deref())
                .await
                .map_err(|e| api_error(shared, e)),
            None => Err("Not signed in.".into()),
        }
    };
    shared.send(Event::UpNext {
        video_id,
        playlist_id,
        queue,
        result,
    });
}

async fn suggest(shared: &Shared, text: String) {
    let found = if shared.demo {
        demo::suggestions(&text)
    } else if let Some(p) = shared.preparer().await {
        p.session
            .search_suggestions(&text)
            .await
            .unwrap_or_else(|e| {
                log::info!("search suggestions did not load: {e}");
                Default::default()
            })
    } else {
        Default::default()
    };
    shared.send(Event::Suggestions(text, found));
}

async fn playlist_queue(
    shared: &Shared,
    playlist_id: String,
    mode: crate::app::QueueMode,
    ticket: u64,
) {
    let result = if shared.demo {
        Ok(demo::playlist_songs(&playlist_id))
    } else {
        match shared.preparer().await {
            Some(p) => p
                .session
                .playlist_queue(&playlist_id)
                .await
                .map_err(|e| api_error(shared, e)),
            None => Err("Not signed in.".into()),
        }
    };
    shared.send(Event::PlaylistQueue {
        playlist_id,
        mode,
        ticket,
        result,
    });
}

async fn report(shared: &Shared, url: String) {
    if shared.demo {
        return;
    }
    if let Some(p) = shared.preparer().await {
        match p.session.send_report(&url).await {
            Ok(status) if status < 300 => {}
            Ok(status) => log::warn!("a History report got HTTP {status}"),
            Err(e) => log::warn!("a History report failed: {e}"),
        }
    }
}

/// How many pictures may be asked for after one, while it waits, before it
/// is taken for scrolled past: more than a window shows at once.
const STALE_PICTURES: u64 = 40;

/// The largest side a picture is kept at. Covers are drawn at most this
/// big (the player page's); larger pictures only cost memory.
const IMAGE_SIDE: u32 = 544;
/// The same for an artist's wide picture, drawn across the page.
const WIDE_IMAGE_SIDE: u32 = 1280;

/// The most songs whose details are kept.
const MAX_DETAILS: usize = 50;

/// What YouTube says about a song: the account's like, and where its
/// lyrics and related songs are. Asked once per song (the website asks
/// the same when a song starts); a failure is asked again next time.
async fn details(
    shared: &Shared,
    session: &Session,
    video_id: &str,
) -> Result<SongDetails, ApiError> {
    let cell = {
        let mut cells = shared.details.lock().await;
        if cells.len() >= MAX_DETAILS && !cells.contains_key(video_id) {
            cells.clear();
        }
        Arc::clone(cells.entry(video_id.to_string()).or_default())
    };
    cell.get_or_try_init(|| session.song_details(video_id, None))
        .await
        .cloned()
}

async fn song_started(shared: &Shared, video_id: String) {
    if shared.demo {
        return;
    }
    let Some(p) = shared.preparer().await else {
        return;
    };
    match details(shared, &p.session, &video_id).await {
        Ok(found) => {
            if let Some(rating) = found.like {
                shared.send(Event::Liked(video_id, like_state(rating)));
            }
        }
        Err(e) => log::info!("the song's details did not load: {e}"),
    }
}

fn like_state(rating: Rating) -> crate::app::LikeState {
    match rating {
        Rating::Like => crate::app::LikeState::Liked,
        Rating::Dislike => crate::app::LikeState::Disliked,
        Rating::Indifferent => crate::app::LikeState::Neutral,
    }
}

fn rating(like: crate::app::LikeState) -> Rating {
    match like {
        crate::app::LikeState::Liked => Rating::Like,
        crate::app::LikeState::Disliked => Rating::Dislike,
        crate::app::LikeState::Neutral => Rating::Indifferent,
    }
}

async fn lyrics(
    shared: &Shared,
    video_id: String,
    title: String,
    artist: String,
    album: Option<String>,
    duration: Option<f64>,
) {
    let found = if shared.demo {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        Some(demo::lyrics(&video_id))
    } else if let Some(p) = shared.preparer().await {
        let song = Song {
            video_id: &video_id,
            title: &title,
            artist: &artist,
            album: album.as_deref(),
            duration,
        };
        find_lyrics(shared, &p.session, &song)
            .await
            .map(crate::lyrics::Lyrics::from)
    } else {
        None
    };
    shared.send(Event::Lyrics(video_id, found));
}

/// The song whose lyrics are wanted.
struct Song<'a> {
    video_id: &'a str,
    title: &'a str,
    artist: &'a str,
    album: Option<&'a str>,
    duration: Option<f64>,
}

/// Lyrics that follow the song when any can be found: YouTube Music's own
/// timed lyrics, else LRCLIB's, else YouTube Music's plain ones.
async fn find_lyrics(
    shared: &Shared,
    session: &Session,
    song: &Song<'_>,
) -> Option<ytfast_core::lyrics::Lyrics> {
    let youtube = match details(shared, session, song.video_id).await {
        Ok(SongDetails {
            lyrics_id: Some(id),
            ..
        }) => session.lyrics(&id).await.unwrap_or_else(|e| {
            log::info!("YouTube Music's lyrics did not load: {e}");
            None
        }),
        Ok(_) => None,
        Err(e) => {
            log::info!("the song's details did not load: {e}");
            None
        }
    };
    if youtube.as_ref().is_some_and(|l| l.synced) {
        return youtube;
    }
    match ytfast_core::lyrics::lrclib(
        &shared.download,
        song.title,
        song.artist,
        song.album,
        song.duration,
    )
    .await
    {
        Ok(Some(found)) if found.synced || youtube.is_none() => Some(found),
        Ok(_) => youtube,
        Err(e) => {
            log::info!("LRCLIB did not answer: {e}");
            youtube
        }
    }
}

async fn related(shared: &Shared, video_id: String) {
    let result = if shared.demo {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        Ok(demo::related(&video_id))
    } else if let Some(p) = shared.preparer().await {
        match details(shared, &p.session, &video_id).await {
            Ok(SongDetails {
                related_id: Some(id),
                ..
            }) => p
                .session
                .page(&id, None)
                .await
                .map_err(|e| api_error(shared, e)),
            Ok(_) => Err("YouTube Music has nothing related to this song.".into()),
            Err(e) => Err(api_error(shared, e)),
        }
    } else {
        Err("Not signed in.".into())
    };
    shared.send(Event::Related(video_id, result));
}

async fn edit(shared: &Shared, change: Edit) {
    if shared.demo {
        if matches!(change, Edit::CreatePlaylist { .. }) {
            shared.send(Event::Edited(change));
        }
        return;
    }
    let Some(p) = shared.preparer().await else {
        shared.send(Event::EditFailed(change, "Not signed in.".into()));
        return;
    };
    let session = &p.session;
    let result = match &change {
        Edit::Rate { video_id, like } => session.rate_song(video_id, rating(*like)).await,
        Edit::AddToPlaylist {
            playlist_id,
            video_ids,
        } => session.add_to_playlist(playlist_id, video_ids).await,
        Edit::RemoveFromPlaylist { playlist_id, songs } => {
            session.remove_from_playlist(playlist_id, songs).await
        }
        Edit::CreatePlaylist {
            title,
            description,
            privacy,
            video_ids,
        } => session
            .create_playlist(title, description, *privacy, video_ids)
            .await
            .map(|_| ()),
        Edit::RenamePlaylist {
            playlist_id,
            name,
            description,
            privacy,
        } => {
            session
                .edit_playlist_details(playlist_id, name, description, *privacy)
                .await
        }
        Edit::DeletePlaylist { playlist_id } => session.delete_playlist(playlist_id).await,
        Edit::Save { playlist_id, save } => session.save_to_library(playlist_id, *save).await,
        Edit::Subscribe {
            channel_id,
            subscribe,
        } => session.subscribe(channel_id, *subscribe).await,
        Edit::ForgetSearch { token, .. } => session.forget_search(token).await,
    };
    match result {
        Ok(()) => shared.send(Event::Edited(change)),
        Err(e) => {
            let message = api_error(shared, e);
            shared.send(Event::EditFailed(change, message));
        }
    }
}

async fn image(shared: &Shared, url: String) {
    if shared.demo {
        shared.send(Event::Image(
            url.clone(),
            Some(Picture::new(demo::cover(&url))),
        ));
        return;
    }
    use std::sync::atomic::Ordering;
    let asked = shared.pictures_asked.fetch_add(1, Ordering::AcqRel) + 1;
    let Ok(_permit) = shared.images.acquire().await else {
        return;
    };
    // Many pictures asked for since this one, while it waited its turn:
    // it scrolled past. The window asks again if it shows once more.
    if shared.pictures_asked.load(Ordering::Acquire) - asked > STALE_PICTURES {
        shared.send(Event::ImageSkipped(url));
        return;
    }
    let bytes = match shared
        .download
        .get(&url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
        Ok(response) => response.bytes().await.ok(),
        Err(_) => None,
    };
    // An artist's wide picture (asked for cropped, `-p`) is drawn across
    // the page; covers are never drawn larger than the player page's.
    let side = if url.contains("-p-l90-rj") {
        WIDE_IMAGE_SIDE
    } else {
        IMAGE_SIDE
    };
    let picture = match bytes {
        Some(bytes) => {
            tokio::task::spawn_blocking(move || decode_picture(&bytes, side).map(Picture::new))
                .await
                .ok()
                .flatten()
        }
        None => None,
    };
    shared.send(Event::Image(url, picture));
}

fn decode_picture(bytes: &[u8], side: u32) -> Option<egui::ColorImage> {
    let picture = image::load_from_memory(bytes).ok()?;
    let picture = if picture.width() > side || picture.height() > side {
        picture.thumbnail(side, side)
    } else {
        picture
    };
    let rgba = picture.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(
        size,
        rgba.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ytfast_core::ytdlp::YtDlpError;

    fn failed(details: &str) -> YtDlpError {
        YtDlpError::Failed {
            summary: "That browser's sign-in data was not found.".into(),
            details: details.into(),
        }
    }

    /// On a Mac without Full Disk Access, a browser's data that cannot be
    /// found or read is put down to that, whichever the browser, in words
    /// that bring up the sign-in screen's Open Full Disk Access button.
    #[test]
    fn missing_full_disk_access_is_named_for_every_browser() {
        let missing = failed("ERROR: could not find chrome cookies database in \"/x\"");
        for browser in Browser::ALL {
            let said = sign_in_problem(&missing, browser, Some(false));
            assert!(said.contains("Full Disk Access is not given"), "{said}");
            assert!(said.contains(browser.label()), "{said}");
        }
        // With it, or where it cannot be told (Windows), yt-dlp's own words.
        for access in [Some(true), None] {
            assert_eq!(
                sign_in_problem(&missing, Browser::Chrome, access),
                "That browser's sign-in data was not found."
            );
        }
        // Another problem is never put down to it.
        let other = failed("ERROR: Sign in to confirm you're not a bot");
        assert!(!sign_in_problem(&other, Browser::Chrome, Some(false)).contains("Full Disk"));
    }

    #[test]
    fn full_disk_access_is_only_asked_on_a_mac() {
        if !cfg!(target_os = "macos") {
            assert_eq!(ytfast_core::cookies::full_disk_access(), None);
        }
    }
}
