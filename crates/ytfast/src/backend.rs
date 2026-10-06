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
use ytfast_core::innertube::{ApiError, Session};
use ytfast_core::library::{LibraryTab, Privacy, SearchFilter};
use ytfast_core::net;
use ytfast_core::prepare::{PrepareError, Prepared, Preparer};
use ytfast_core::read::{Page, PlayerInfo, Rating, SongDetails, Track};
use ytfast_core::solver::{self, Solver};
use ytfast_core::stream::SongData;
use ytfast_core::ytdlp::{Browser, YtDlp};

use crate::{colors, demo};

/// A page the window can show.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Route {
    Home,
    Explore,
    /// Saved playlists.
    Library,
    Liked,
    /// Any other page: an album, playlist, artist, mood.
    Browse {
        id: String,
        params: Option<String>,
    },
    Search(String),
    /// Search results of one kind only.
    SearchOnly(String, SearchKind),
    /// The library's other tabs.
    LibrarySongs,
    LibraryAlbums,
    LibraryArtists,
    History,
    /// YTFast's own settings (not loaded from YouTube).
    Settings,
}

/// The kinds search results can be narrowed to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SearchKind {
    Songs,
    Albums,
    Artists,
    Playlists,
}

impl SearchKind {
    pub const ALL: [Self; 4] = [Self::Songs, Self::Albums, Self::Artists, Self::Playlists];

    pub fn label(self) -> &'static str {
        match self {
            Self::Songs => "Songs",
            Self::Albums => "Albums",
            Self::Artists => "Artists",
            Self::Playlists => "Playlists",
        }
    }

    fn filter(self) -> SearchFilter {
        match self {
            Self::Songs => SearchFilter::Songs,
            Self::Albums => SearchFilter::Albums,
            Self::Artists => SearchFilter::Artists,
            Self::Playlists => SearchFilter::Playlists,
        }
    }
}

impl Route {
    /// Pages that are not loaded from YouTube.
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Settings)
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
}

pub enum Request {
    SignIn(Browser),
    /// Forget the sign-in (the browser keeps its own).
    SignOut,
    Page(Route),
    /// Get a song ready. With `play`, the window gets [`Event::Prepared`];
    /// without, the song is only made ready ahead of time.
    Prepare {
        entry: u64,
        video_id: String,
        play: bool,
    },
    /// What plays after a song (its playlist, or a radio).
    UpNext {
        video_id: String,
        playlist_id: Option<String>,
    },
    /// The songs of a playlist or album, to play it (or queue it).
    PlaylistQueue {
        playlist_id: String,
        mode: crate::app::QueueMode,
    },
    /// Suggestions for what is being typed in the search box.
    Suggest(String),
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
    /// A picture, by address.
    Image(String),
}

/// A cover, decoded, with the colours the look takes from it.
pub struct Picture {
    pub image: egui::ColorImage,
    pub summary: colors::Summary,
}

impl Picture {
    fn new(image: egui::ColorImage) -> Self {
        Self {
            summary: colors::summarize(&image),
            image,
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
        video_id: String,
    },
    RemoveFromPlaylist {
        playlist_id: String,
        video_id: String,
        set_video_id: String,
    },
    CreatePlaylist {
        title: String,
        video_ids: Vec<String>,
    },
    RenamePlaylist {
        playlist_id: String,
        name: String,
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
    },
    SignInFailed(String),
    /// YouTube treated a request as signed out.
    SignedOut,
    Page(Route, Result<Page, String>),
    Prepared {
        entry: u64,
        result: Result<Ready, Failure>,
    },
    UpNext {
        video_id: String,
        result: Result<Vec<Track>, String>,
    },
    PlaylistQueue {
        playlist_id: String,
        mode: crate::app::QueueMode,
        result: Result<Vec<Track>, String>,
    },
    Suggestions(String, Vec<String>),
    /// More songs of a long list already shown (a playlist, Liked
    /// Music), loaded after its first ones.
    MoreRows {
        route: Route,
        tracks: Vec<Track>,
    },
    Image(String, Option<Picture>),
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
}

impl Backend {
    /// Starts the backend thread. `wake` asks the window to redraw.
    pub fn start(wake: impl Fn() + Send + Sync + 'static, demo: bool) -> std::io::Result<Self> {
        let folders = Folders::new()?;
        let session_dir = folders.session.clone();
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
        })
    }

    pub fn send(&self, request: Request) {
        let _ = self.requests.send(request);
    }

    pub fn try_recv(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    /// Removes this run's sign-in copy. Called when the window closes.
    pub fn forget_sign_in(&self) {
        let _ = std::fs::remove_dir_all(&self.session_dir);
    }
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
    fn new() -> std::io::Result<Self> {
        let dirs = directories::ProjectDirs::from("", "", "YtFast")
            .ok_or_else(|| std::io::Error::other("no home folder"))?;
        let cache = dirs.cache_dir().to_path_buf();
        // Left over from a run that did not close properly.
        let sessions = cache.join("app-sessions");
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
        let session = sessions.join(std::process::id().to_string());
        std::fs::create_dir_all(&session)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&session, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            helpers: dirs.data_local_dir().join("helpers"),
            yt_dlp_cache: cache.join("yt-dlp"),
            player: cache.join("player"),
            solver: cache.join("solver"),
            session,
        })
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
    while let Some(request) = requests.recv().await {
        let shared = Arc::clone(&shared);
        tokio::spawn(async move {
            match request {
                Request::SignIn(browser) => sign_in(&shared, browser).await,
                Request::SignOut => {
                    *shared.signed_in.write().await = None;
                    let _ =
                        std::fs::remove_file(shared.folders.session.join("youtube-cookies.txt"));
                    let mut guard = shared.prepared.lock().await;
                    guard.0.clear();
                    guard.1.clear();
                }
                Request::Page(route) => page(&shared, route).await,
                Request::Prepare {
                    entry,
                    video_id,
                    play,
                } => prepare(&shared, entry, video_id, play).await,
                Request::UpNext {
                    video_id,
                    playlist_id,
                } => up_next(&shared, video_id, playlist_id).await,
                Request::PlaylistQueue { playlist_id, mode } => {
                    playlist_queue(&shared, playlist_id, mode).await
                }
                Request::Suggest(text) => suggest(&shared, text).await,
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
                Request::Edit(change) => edit(&shared, change).await,
                Request::FastWay(on) => shared
                    .fast_way
                    .store(on, std::sync::atomic::Ordering::Relaxed),
                Request::Report(url) => report(&shared, url).await,
                Request::Image(url) => image(&shared, url).await,
            }
        });
    }
}

async fn sign_in(shared: &Shared, browser: Browser) {
    if shared.demo {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        shared.send(Event::SignedIn {
            name: "Demo listener".into(),
        });
        return;
    }
    match try_sign_in(shared, browser).await {
        Ok(name) => shared.send(Event::SignedIn { name }),
        Err(message) => shared.send(Event::SignInFailed(message)),
    }
}

async fn try_sign_in(shared: &Shared, browser: Browser) -> Result<String, String> {
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
        .map_err(|e| e.to_string())?;
    if !jar.looks_signed_in() {
        return Err(format!(
            "{} has YouTube data but is not signed in. Open music.youtube.com there, sign in, then try again.",
            browser.label()
        ));
    }
    let cookies_file = shared.folders.session.join("youtube-cookies.txt");
    write_private(&cookies_file, &jar.to_netscape())
        .map_err(|e| format!("Could not keep the sign-in: {e}"))?;

    shared.send(Event::Progress("Opening YouTube Music...".into()));
    let session = Session::start(net::api_client(), &jar)
        .await
        .map_err(|e| e.to_string())?;
    let (account, flags) = session.account().await.map_err(|e| e.to_string())?;
    if flags.logged_in == Some(false) {
        return Err(
            "YouTube Music treated the sign-in as signed out. Sign in again in your browser."
                .into(),
        );
    }
    let name = account
        .map(|a| a.name)
        .unwrap_or_else(|| "your account".into());
    let session = Arc::new(session);
    let direct = fast_way(shared, &helpers, &session);
    *shared.signed_in.write().await = Some(Preparer {
        session,
        yt_dlp,
        download: shared.download.clone(),
        cookies_file,
        direct,
    });
    Ok(name)
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

fn api_error(shared: &Shared, error: ApiError) -> String {
    if matches!(error, ApiError::SignedOut) {
        shared.send(Event::SignedOut);
    }
    error.to_string()
}

async fn page(shared: &Shared, route: Route) {
    if shared.demo {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        shared.send(Event::Page(route.clone(), Ok(demo::page(&route))));
        return;
    }
    let Some(preparer) = shared.preparer().await else {
        shared.send(Event::Page(route, Err("Not signed in.".into())));
        return;
    };
    let session = &preparer.session;
    let result = match &route {
        Route::Home => session.home().await.map(|p| (p, None)),
        Route::Explore => session
            .page("FEmusic_explore", None)
            .await
            .map(|p| (p, None)),
        Route::Library => session.library_playlists().await.map(|p| (p, None)),
        Route::Liked => session.long_page("VLLM", None).await,
        Route::Browse { id, params } => session.long_page(id, params.as_deref()).await,
        Route::Search(query) => session.search(query).await.map(|p| (p, None)),
        Route::SearchOnly(query, kind) => session
            .search_filtered(query, kind.filter())
            .await
            .map(|p| (p, None)),
        Route::LibrarySongs => session.long_page(LibraryTab::Songs.browse_id(), None).await,
        Route::LibraryAlbums => session.library(LibraryTab::Albums).await.map(|p| (p, None)),
        Route::LibraryArtists => session
            .library(LibraryTab::Artists)
            .await
            .map(|p| (p, None)),
        Route::History => session.history_page().await.map(|p| (p, None)),
        Route::Settings => return,
    };
    let more = match result {
        Ok((page, more)) => {
            shared.send(Event::Page(route.clone(), Ok(page)));
            more
        }
        Err(e) => {
            let message = api_error(shared, e);
            shared.send(Event::Page(route, Err(message)));
            return;
        }
    };
    // The rest of a long list, a hundred or so songs at a time.
    let mut next = more;
    let mut batches = 0;
    while let Some(from) = next.take() {
        batches += 1;
        if batches > MAX_BATCHES {
            break;
        }
        match session.more_tracks(&from).await {
            Ok((tracks, after)) if !tracks.is_empty() => {
                shared.send(Event::MoreRows {
                    route: route.clone(),
                    tracks,
                });
                next = after;
            }
            Ok(_) => break,
            Err(e) => {
                log::warn!("the rest of a list did not load: {}", api_error(shared, e));
                break;
            }
        }
    }
}

/// The most batches of a long list loaded (about 10,000 songs).
const MAX_BATCHES: usize = 100;

fn classify(error: PrepareError) -> Failure {
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    let song_only = match &error {
        PrepareError::NoPlayableAudio => true,
        PrepareError::Find(_) => {
            lower.contains("not available")
                || lower.contains("unavailable")
                || lower.contains("private")
        }
        PrepareError::Download(_) => false,
    };
    Failure { message, song_only }
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

async fn up_next(shared: &Shared, video_id: String, playlist_id: Option<String>) {
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
    shared.send(Event::UpNext { video_id, result });
}

async fn suggest(shared: &Shared, text: String) {
    let found = if shared.demo {
        let lower = text.to_lowercase();
        [
            "Glass Hearts",
            "Mara Sol",
            "Midnight Arcade",
            "Night Ferries",
            "Low Tide",
            "Lemon Skies",
        ]
        .iter()
        .filter(|s| s.to_lowercase().contains(&lower))
        .map(|s| s.to_string())
        .collect()
    } else if let Some(p) = shared.preparer().await {
        p.session
            .search_suggestions(&text)
            .await
            .unwrap_or_else(|e| {
                log::info!("search suggestions did not load: {e}");
                Vec::new()
            })
    } else {
        Vec::new()
    };
    shared.send(Event::Suggestions(text, found));
}

async fn playlist_queue(shared: &Shared, playlist_id: String, mode: crate::app::QueueMode) {
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

/// The largest side a picture is kept at. Covers are drawn at most this
/// big (the player page's); larger pictures only cost memory.
const IMAGE_SIDE: u32 = 544;

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
        Ok(demo::page(&Route::Explore))
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
            video_id,
        } => {
            session
                .add_to_playlist(playlist_id, std::slice::from_ref(video_id))
                .await
        }
        Edit::RemoveFromPlaylist {
            playlist_id,
            video_id,
            set_video_id,
        } => {
            session
                .remove_from_playlist(playlist_id, &[(video_id.clone(), set_video_id.clone())])
                .await
        }
        Edit::CreatePlaylist { title, video_ids } => session
            .create_playlist(title, "", Privacy::Private, video_ids)
            .await
            .map(|_| ()),
        Edit::RenamePlaylist { playlist_id, name } => {
            session.rename_playlist(playlist_id, name).await
        }
        Edit::DeletePlaylist { playlist_id } => session.delete_playlist(playlist_id).await,
        Edit::Save { playlist_id, save } => session.save_to_library(playlist_id, *save).await,
        Edit::Subscribe {
            channel_id,
            subscribe,
        } => session.subscribe(channel_id, *subscribe).await,
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
    let Ok(_permit) = shared.images.acquire().await else {
        return;
    };
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
    let picture = match bytes {
        Some(bytes) => {
            tokio::task::spawn_blocking(move || decode_picture(&bytes).map(Picture::new))
                .await
                .ok()
                .flatten()
        }
        None => None,
    };
    shared.send(Event::Image(url, picture));
}

fn decode_picture(bytes: &[u8]) -> Option<egui::ColorImage> {
    let picture = image::load_from_memory(bytes).ok()?;
    let picture = if picture.width() > IMAGE_SIDE || picture.height() > IMAGE_SIDE {
        picture.thumbnail(IMAGE_SIDE, IMAGE_SIDE)
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
