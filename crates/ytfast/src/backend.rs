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
use ytfast_core::net;
use ytfast_core::prepare::{PrepareError, Prepared, Preparer};
use ytfast_core::read::{Page, PlayerInfo, Track};
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
}

impl Route {
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
    /// The songs of a playlist or album, to play it.
    PlaylistQueue {
        playlist_id: String,
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
    /// Like, dislike, or neither.
    Rate {
        video_id: String,
        like: crate::app::LikeState,
    },
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
        result: Result<Vec<Track>, String>,
    },
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
    #[allow(dead_code)] // Sent once song details are read from YouTube.
    Liked(String, crate::app::LikeState),
    #[allow(dead_code)]
    RateFailed(String, String),
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
}

impl Shared {
    fn send(&self, event: Event) {
        let _ = self.events.send(event);
        (self.wake)();
    }

    async fn preparer(&self) -> Option<Preparer> {
        self.signed_in.read().await.clone()
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
    });
    // Rest the solver when YtFast is not being used.
    {
        let shared = Arc::clone(&shared);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                if let Some(direct) = shared.preparer().await.and_then(|p| p.direct) {
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
                Request::PlaylistQueue { playlist_id } => {
                    playlist_queue(&shared, playlist_id).await
                }
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
                Request::Rate { video_id, like } => rate(&shared, video_id, like).await,
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

async fn playlist_queue(shared: &Shared, playlist_id: String) {
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

async fn lyrics(
    shared: &Shared,
    video_id: String,
    _title: String,
    _artist: String,
    _album: Option<String>,
    _duration: Option<f64>,
) {
    let found = if shared.demo {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        Some(demo::lyrics(&video_id))
    } else {
        None
    };
    shared.send(Event::Lyrics(video_id, found));
}

async fn related(shared: &Shared, video_id: String) {
    let result = if shared.demo {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        Ok(demo::page(&Route::Explore))
    } else {
        Err("Related songs are not available yet.".to_string())
    };
    shared.send(Event::Related(video_id, result));
}

async fn rate(shared: &Shared, video_id: String, like: crate::app::LikeState) {
    if shared.demo {
        return;
    }
    let _ = (video_id, like);
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
