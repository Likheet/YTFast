//! The song playing on the user's Discord profile ("Listening to ..."),
//! with Settings' switch on (off by default; the owner's wish, 19 October
//! 2026). YTFast tells the Discord app on the same computer, through the
//! local connection Discord offers every app for this (its "IPC": a named
//! pipe on Windows, a socket file on a Mac), as Discord's own Rich
//! Presence does. Nothing goes over the network from YTFast; Discord shows
//! it to the user's friends.
//!
//! A thread of its own does the talking, so a Discord that is slow, closed
//! or not installed never holds anything up: it looks for Discord again
//! every [`RETRY`], and tells it only what changed, at most once every
//! [`LEAST_GAP`] (Discord takes five changes in twenty seconds).

use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// The Discord application YTFast shows as ("Listening to YTFast"),
/// made in Discord's developer portal by the owner (19 October 2026). Its
/// ID is public: every app that shows on Discord sends its own.
pub const APPLICATION_ID: &str = "1561723298881085531";

/// How often a Discord not found (closed, not installed) is looked for.
const RETRY: Duration = Duration::from_secs(15);
/// The least time between two changes told to Discord.
const LEAST_GAP: Duration = Duration::from_secs(4);
/// The largest reply read from Discord (its replies are a few hundred
/// bytes).
const LARGEST_REPLY: usize = 64 * 1024;

/// What Discord shows of the song playing.
#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    pub title: String,
    pub artists: String,
    pub album: Option<String>,
    /// The cover's address (a square picture), shown beside the song.
    pub cover: Option<String>,
    /// Its YouTube ID, for the profile's "Listen on YouTube Music" button.
    pub video_id: String,
    /// When the song began and when it ends, in milliseconds since 1970
    /// (Discord draws its bar from them).
    pub start_ms: i64,
    pub end_ms: Option<i64>,
}

impl Activity {
    /// The same song, at the same place: its times within two seconds (a
    /// song's start, worked out again each frame, wavers by a few
    /// milliseconds; a jump moves it by more).
    fn same_as(&self, other: &Self) -> bool {
        let near = |a: i64, b: i64| (a - b).abs() < 2000;
        let ends = match (self.end_ms, other.end_ms) {
            (Some(a), Some(b)) => near(a, b),
            (a, b) => a == b,
        };
        self.title == other.title
            && self.artists == other.artists
            && self.album == other.album
            && self.cover == other.cover
            && self.video_id == other.video_id
            && near(self.start_ms, other.start_ms)
            && ends
    }

    /// Discord's form of it: listening, the song's name in the status, the
    /// artists under it, the cover, the bar, and a button to the song.
    fn to_json(&self) -> Value {
        let mut activity = json!({
            // Listening.
            "type": 2,
            // The status says the song's name: "Listening to Lemon".
            "status_display_type": 2,
            "details": text_for_discord(&self.title),
            "state": text_for_discord(&self.artists),
            "timestamps": { "start": self.start_ms },
            "buttons": [{
                "label": "Listen on YouTube Music",
                "url": format!("https://music.youtube.com/watch?v={}", self.video_id),
            }],
        });
        if let Some(end) = self.end_ms {
            activity["timestamps"]["end"] = json!(end);
        }
        // Discord takes a picture's address of at most 256 characters.
        if let Some(cover) = self.cover.as_ref().filter(|c| c.len() <= 256) {
            activity["assets"] = json!({ "large_image": cover });
            if let Some(album) = self.album.as_deref().filter(|a| !a.is_empty()) {
                activity["assets"]["large_text"] = json!(text_for_discord(album));
            }
        }
        activity
    }
}

/// Words as Discord takes them: 2 to 128 characters.
fn text_for_discord(text: &str) -> String {
    let mut text: String = text.chars().take(128).collect();
    while text.chars().count() < 2 {
        text.push(' ');
    }
    text
}

/// The Discord status, as the window tells it.
pub struct Discord {
    sender: Sender<Option<Activity>>,
    /// What was last told, so only a change is.
    told: Option<Activity>,
}

impl Discord {
    /// Starts the thread that talks to Discord.
    pub fn start() -> Self {
        let (sender, receiver) = mpsc::channel();
        let started = std::thread::Builder::new()
            .name("ytfast-discord".into())
            .spawn(move || run(&receiver, APPLICATION_ID));
        if let Err(e) = started {
            log::warn!("Discord: could not start: {e}");
        }
        Self { sender, told: None }
    }

    /// Discord shows `activity`, or nothing: told only when it changed.
    pub fn show(&mut self, activity: Option<Activity>) {
        let same = match (&self.told, &activity) {
            (Some(told), Some(new)) => told.same_as(new),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.told.clone_from(&activity);
        let _ = self.sender.send(activity);
    }
}

/// The thread: what is wanted is told to Discord once it is found, and
/// again once it changes.
fn run(changes: &Receiver<Option<Activity>>, application: &str) {
    let mut connection: Option<Connection<Pipe>> = None;
    let mut wanted: Option<Activity> = None;
    // What Discord was told, once it was (`None` inside: nothing shown).
    let mut shown: Option<Option<Activity>> = None;
    let mut looked: Option<Instant> = None;
    let mut told: Option<Instant> = None;
    loop {
        // Nothing to tell: Discord shows what is wanted, or nothing is
        // wanted of a Discord not found yet.
        let settled = |shown: &Option<Option<Activity>>, wanted: &Option<Activity>| match shown {
            Some(shown) => shown == wanted,
            None => wanted.is_none(),
        };
        let wait = if settled(&shown, &wanted) {
            Duration::from_secs(3600)
        } else {
            RETRY
        };
        match changes.recv_timeout(wait) {
            Ok(change) => wanted = change,
            Err(RecvTimeoutError::Timeout) => {}
            // YTFast is ending: Discord takes the status away itself once
            // the connection closes.
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // Not too often: the newest of the changes meanwhile.
        if let Some(at) = told {
            std::thread::sleep(LEAST_GAP.saturating_sub(at.elapsed()));
        }
        while let Ok(change) = changes.try_recv() {
            wanted = change;
        }
        if settled(&shown, &wanted) {
            continue;
        }
        if connection.is_none() {
            if looked.is_some_and(|at| at.elapsed() < RETRY) {
                continue;
            }
            looked = Some(Instant::now());
            match Connection::open(application) {
                Ok(found) => connection = Some(found),
                Err(e) => {
                    log::debug!("Discord: not found ({e})");
                    continue;
                }
            }
        }
        let Some(open) = &mut connection else {
            continue;
        };
        told = Some(Instant::now());
        match open.set_activity(wanted.as_ref()) {
            Ok(()) => shown = Some(wanted.clone()),
            Err(e) => {
                log::info!("Discord: the status was not taken ({e})");
                connection = None;
                // Shown nowhere now: told again once Discord is found.
                shown = None;
                looked = Some(Instant::now());
            }
        }
    }
}

#[cfg(windows)]
type Pipe = std::fs::File;
#[cfg(unix)]
type Pipe = std::os::unix::net::UnixStream;

/// Discord's local connection, open, its hello said.
struct Connection<P> {
    pipe: P,
    nonce: u64,
    /// This Discord refused the status as listening (an older one): it is
    /// told the song in the plain form every version takes.
    plain: bool,
}

/// The kinds of message Discord's connection carries (and 2: it closes,
/// as for an application it does not know).
const HANDSHAKE: u32 = 0;
const FRAME: u32 = 1;

impl Connection<Pipe> {
    /// Finds Discord (it listens on the first free of ten names) and says
    /// hello as `application`.
    fn open(application: &str) -> io::Result<Self> {
        if application.is_empty() {
            return Err(io::Error::other("no Discord application"));
        }
        // Why the first name failed: the others are usually not there.
        let mut first = None;
        for n in 0..10 {
            let opened = open_pipe(n).and_then(|pipe| {
                let mut connection = Self {
                    pipe,
                    nonce: 0,
                    plain: false,
                };
                connection.hello(application).map(|()| connection)
            });
            match opened {
                Ok(connection) => return Ok(connection),
                Err(e) => {
                    first.get_or_insert(e);
                }
            }
        }
        Err(first.unwrap_or_else(|| io::ErrorKind::NotFound.into()))
    }
}

#[cfg(windows)]
fn open_pipe(n: u32) -> io::Result<Pipe> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!(r"\\.\pipe\discord-ipc-{n}"))
}

#[cfg(unix)]
fn open_pipe(n: u32) -> io::Result<Pipe> {
    // Where Discord puts its socket: the user's own temporary folder (a
    // Mac's TMPDIR), or the runtime folder, Flatpak's and Snap's within it.
    let roots: Vec<std::path::PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .filter_map(std::env::var_os)
        .map(std::path::PathBuf::from)
        .chain([std::path::PathBuf::from("/tmp")])
        .collect();
    let mut last = io::Error::from(io::ErrorKind::NotFound);
    for root in &roots {
        for inside in ["", "app/com.discordapp.Discord", "snap.discord"] {
            let path = root.join(inside).join(format!("discord-ipc-{n}"));
            match std::os::unix::net::UnixStream::connect(&path) {
                Ok(stream) => {
                    // A Discord that stops answering must not hold the
                    // thread for ever.
                    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
                    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
                    return Ok(stream);
                }
                Err(e) => last = e,
            }
        }
    }
    Err(last)
}

impl<P: Read + Write> Connection<P> {
    /// The hello: Discord answers ready, or closes for an application it
    /// does not know.
    fn hello(&mut self, application: &str) -> io::Result<()> {
        self.send(HANDSHAKE, &json!({ "v": 1, "client_id": application }))?;
        let (kind, reply) = self.receive()?;
        if kind == FRAME && reply["evt"] == "READY" {
            Ok(())
        } else {
            Err(refused(&reply))
        }
    }

    /// Shows `activity` on the profile, or nothing. A Discord that refuses
    /// it as listening (one older than that) is told it plainly instead.
    fn set_activity(&mut self, activity: Option<&Activity>) -> io::Result<()> {
        let mut form = activity.map(Activity::to_json);
        if !self.plain {
            match self.command(form.clone()) {
                Err(e) if e.kind() == io::ErrorKind::InvalidInput && form.is_some() => {
                    log::info!("Discord: {e}; telling it plainly");
                    self.plain = true;
                }
                done => return done,
            }
        }
        if let Some(Value::Object(fields)) = &mut form {
            fields.remove("type");
            fields.remove("status_display_type");
        }
        self.command(form)
    }

    /// One SET_ACTIVITY, and Discord's answer.
    fn command(&mut self, activity: Option<Value>) -> io::Result<()> {
        self.nonce += 1;
        let command = json!({
            "cmd": "SET_ACTIVITY",
            "args": {
                "pid": std::process::id(),
                "activity": activity,
            },
            "nonce": self.nonce.to_string(),
        });
        self.send(FRAME, &command)?;
        let (kind, reply) = self.receive()?;
        if kind == FRAME && reply["evt"] != "ERROR" {
            Ok(())
        } else {
            Err(refused(&reply))
        }
    }

    /// A message: its kind and length (4 bytes each, least first), then
    /// its JSON.
    fn send(&mut self, kind: u32, body: &Value) -> io::Result<()> {
        let body = serde_json::to_vec(body)?;
        let length = u32::try_from(body.len()).map_err(io::Error::other)?;
        let mut message = Vec::with_capacity(8 + body.len());
        message.extend_from_slice(&kind.to_le_bytes());
        message.extend_from_slice(&length.to_le_bytes());
        message.extend_from_slice(&body);
        self.pipe.write_all(&message)?;
        self.pipe.flush()
    }

    fn receive(&mut self) -> io::Result<(u32, Value)> {
        let mut head = [0; 8];
        self.pipe.read_exact(&mut head)?;
        let kind = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let length = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        if length > LARGEST_REPLY {
            return Err(io::Error::other("Discord's reply was too long"));
        }
        let mut body = vec![0; length];
        self.pipe.read_exact(&mut body)?;
        Ok((kind, serde_json::from_slice(&body)?))
    }
}

/// Why Discord refused, as it says (an error of its own kind, apart from
/// a connection's).
fn refused(reply: &Value) -> io::Error {
    let said = reply["data"]["message"]
        .as_str()
        .or_else(|| reply["message"].as_str())
        .unwrap_or("no reason given");
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("Discord refused: {said}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for Discord's connection: what it will answer, and what
    /// it was sent.
    struct FakePipe {
        replies: io::Cursor<Vec<u8>>,
        sent: Vec<u8>,
    }

    impl FakePipe {
        fn answering(replies: &[(u32, Value)]) -> Self {
            let mut bytes = Vec::new();
            for (kind, body) in replies {
                let body = serde_json::to_vec(body).unwrap();
                bytes.extend_from_slice(&kind.to_le_bytes());
                bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
                bytes.extend_from_slice(&body);
            }
            Self {
                replies: io::Cursor::new(bytes),
                sent: Vec::new(),
            }
        }

        /// The messages sent: each one's kind and JSON.
        fn messages(&self) -> Vec<(u32, Value)> {
            let mut messages = Vec::new();
            let mut rest = self.sent.as_slice();
            while rest.len() >= 8 {
                let kind = u32::from_le_bytes(rest[..4].try_into().unwrap());
                let length = u32::from_le_bytes(rest[4..8].try_into().unwrap()) as usize;
                let body = serde_json::from_slice(&rest[8..8 + length]).unwrap();
                messages.push((kind, body));
                rest = &rest[8 + length..];
            }
            messages
        }
    }

    impl Read for FakePipe {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.replies.read(buf)
        }
    }

    impl Write for FakePipe {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.sent.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn song() -> Activity {
        Activity {
            title: "Lemon".into(),
            artists: "Kenshi Yonezu".into(),
            album: Some("Lemon".into()),
            cover: Some("https://lh3.googleusercontent.com/abc=w512-h512-l90-rj".into()),
            video_id: "aBcDeFgHiJk".into(),
            start_ms: 1_760_000_000_000,
            end_ms: Some(1_760_000_255_000),
        }
    }

    /// The hello names the application; the song goes as listening, its
    /// name in the status, the artists, the cover and album, the bar's
    /// times and a button to it; nothing playing, the status goes.
    #[test]
    fn discord_is_told_the_song_as_its_own_status_says() {
        let ready = json!({ "cmd": "DISPATCH", "evt": "READY", "data": {} });
        let done = json!({ "cmd": "SET_ACTIVITY", "data": {} });
        let mut connection = Connection {
            pipe: FakePipe::answering(&[(FRAME, ready), (FRAME, done.clone()), (FRAME, done)]),
            nonce: 0,
            plain: false,
        };
        connection.hello("1234").unwrap();
        connection.set_activity(Some(&song())).unwrap();
        connection.set_activity(None).unwrap();
        let sent = connection.pipe.messages();
        assert_eq!(sent[0], (HANDSHAKE, json!({ "v": 1, "client_id": "1234" })));
        let (kind, command) = &sent[1];
        assert_eq!(*kind, FRAME);
        assert_eq!(command["cmd"], "SET_ACTIVITY");
        let activity = &command["args"]["activity"];
        assert_eq!(activity["type"], 2);
        assert_eq!(activity["status_display_type"], 2);
        assert_eq!(activity["details"], "Lemon");
        assert_eq!(activity["state"], "Kenshi Yonezu");
        assert_eq!(activity["timestamps"]["start"], 1_760_000_000_000_i64);
        assert_eq!(activity["timestamps"]["end"], 1_760_000_255_000_i64);
        assert_eq!(
            activity["assets"]["large_image"],
            "https://lh3.googleusercontent.com/abc=w512-h512-l90-rj"
        );
        assert_eq!(activity["assets"]["large_text"], "Lemon");
        assert_eq!(
            activity["buttons"][0]["url"],
            "https://music.youtube.com/watch?v=aBcDeFgHiJk"
        );
        assert_ne!(sent[1].1["nonce"], sent[2].1["nonce"], "each its own");
        assert!(sent[2].1["args"]["activity"].is_null(), "taken away");
    }

    /// Discord closing at the hello (an application it does not know), or
    /// answering an error, is a failure with its reason.
    #[test]
    fn a_refusal_is_a_failure() {
        const CLOSE: u32 = 2;
        let closed = json!({ "code": 4000, "message": "Invalid Client ID" });
        let mut connection = Connection {
            pipe: FakePipe::answering(&[(CLOSE, closed)]),
            nonce: 0,
            plain: false,
        };
        let error = connection.hello("0").unwrap_err();
        assert!(error.to_string().contains("Invalid Client ID"), "{error}");

        let error = json!({ "evt": "ERROR", "data": { "message": "bad activity" } });
        let mut connection = Connection {
            pipe: FakePipe::answering(&[(FRAME, error)]),
            nonce: 0,
            plain: false,
        };
        assert!(connection.set_activity(Some(&song())).is_err());
        // Nothing answered at all (Discord gone): a failure too.
        let mut connection = Connection {
            pipe: FakePipe::answering(&[]),
            nonce: 0,
            plain: false,
        };
        assert!(connection.set_activity(None).is_err());
    }

    /// A Discord that refuses the song as listening (an older one) is told
    /// it plainly, then and from then on.
    #[test]
    fn an_older_discord_is_told_plainly() {
        let refused = json!({ "evt": "ERROR", "data": { "message": "unknown field" } });
        let done = json!({ "cmd": "SET_ACTIVITY", "data": {} });
        let mut connection = Connection {
            pipe: FakePipe::answering(&[(FRAME, refused), (FRAME, done.clone()), (FRAME, done)]),
            nonce: 0,
            plain: false,
        };
        connection.set_activity(Some(&song())).unwrap();
        connection.set_activity(Some(&song())).unwrap();
        let sent = connection.pipe.messages();
        assert_eq!(sent.len(), 3, "once as listening, then plainly");
        assert_eq!(sent[0].1["args"]["activity"]["type"], 2);
        for (_, plain) in &sent[1..] {
            let activity = &plain["args"]["activity"];
            assert!(activity.get("type").is_none());
            assert!(activity.get("status_display_type").is_none());
            assert_eq!(activity["details"], "Lemon");
        }
    }

    /// The same song a moment later (its start worked out again) is no
    /// change; a jump, a pause or another song is.
    #[test]
    fn only_a_change_is_told() {
        let (sender, receiver) = mpsc::channel();
        let mut discord = Discord { sender, told: None };
        discord.show(Some(song()));
        let later = Activity {
            start_ms: song().start_ms + 40,
            end_ms: song().end_ms.map(|end| end + 40),
            ..song()
        };
        discord.show(Some(later));
        let jumped = Activity {
            start_ms: song().start_ms - 60_000,
            end_ms: song().end_ms.map(|end| end - 60_000),
            ..song()
        };
        discord.show(Some(jumped.clone()));
        discord.show(None);
        discord.show(None);
        let told: Vec<Option<Activity>> = receiver.try_iter().collect();
        assert_eq!(told, [Some(song()), Some(jumped), None]);
    }

    /// Discord itself, only when `YTFAST_TEST_DISCORD` is set and the
    /// Discord app is open: it answers YTFast's hello, and takes the status
    /// away. Nothing shows on the profile; no song is sent.
    #[test]
    fn discord_itself() {
        if std::env::var_os("YTFAST_TEST_DISCORD").is_none() {
            return;
        }
        let mut connection = Connection::open(APPLICATION_ID).expect("Discord answers");
        connection
            .set_activity(None)
            .expect("it takes the status away");
    }

    /// Discord's limits: words of 2 to 128 characters, a picture's address
    /// of at most 256.
    #[test]
    fn words_and_pictures_within_discords_limits() {
        assert_eq!(text_for_discord("A"), "A ");
        assert_eq!(text_for_discord(&"x".repeat(300)).chars().count(), 128);
        let long = Activity {
            cover: Some(format!("https://example.com/{}", "a".repeat(300))),
            ..song()
        };
        assert!(long.to_json().get("assets").is_none());
    }
}
