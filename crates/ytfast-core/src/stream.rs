//! A song's audio as it arrives: the player starts on the first bytes
//! while the rest downloads in the background.
//!
//! The download fills a [`SongData`]; the decoder reads it through a
//! [`SongReader`], which waits when it gets ahead of the download. Once the
//! whole song has arrived it stays in memory, so pausing for hours or
//! changing Wi-Fi cannot break it (YouTube's stream addresses expire, but
//! nothing more needs fetching).

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use crate::redact;

/// Downloads are fetched in pieces this size. YouTube slows down single
/// requests for whole files; players ask for ranges.
const PIECE: u64 = 2 * 1024 * 1024;
/// No song's audio should be anywhere near this; a guard against a runaway
/// download.
const MAX_SIZE: u64 = 200 * 1024 * 1024;
/// How long a reader waits for bytes that have not arrived before giving
/// up (the network stalled).
const READ_WAIT: Duration = Duration::from_secs(20);
/// How long a download waits for its next bytes before it takes the
/// connection for broken and asks again, well inside [`READ_WAIT`].
const STALL: Duration = Duration::from_secs(8);
/// How many times in a row a download asks again for the rest of a song
/// after its connection broke, waiting this long before the first time
/// (and twice as long each time after).
const RETRIES: u32 = 3;
const RETRY_PAUSE: Duration = Duration::from_millis(500);

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    /// The whole size, once known.
    total: Option<u64>,
    done: bool,
    failed: Option<String>,
}

/// A song's bytes, filled by a download and read by the player.
#[derive(Default)]
pub struct SongData {
    state: Mutex<State>,
    arrived: Condvar,
}

impl std::fmt::Debug for SongData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.lock();
        f.debug_struct("SongData")
            .field("have", &state.bytes.len())
            .field("total", &state.total)
            .field("done", &state.done)
            .finish()
    }
}

impl SongData {
    /// An empty song, to be filled by [`fetch`].
    pub fn new(total: Option<u64>) -> Arc<Self> {
        let data = Self::default();
        {
            let mut state = data.lock();
            state.total = total;
            if let Some(total) = total.filter(|t| *t <= MAX_SIZE) {
                state.bytes.reserve_exact(total as usize);
            }
        }
        Arc::new(data)
    }

    /// A song that has fully arrived.
    pub fn complete(bytes: Vec<u8>) -> Arc<Self> {
        let total = bytes.len() as u64;
        Arc::new(Self {
            state: Mutex::new(State {
                bytes,
                total: Some(total),
                done: true,
                failed: None,
            }),
            arrived: Condvar::new(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(&self, chunk: &[u8]) {
        self.lock().bytes.extend_from_slice(chunk);
        self.arrived.notify_all();
    }

    /// The whole size, as the server says it (which wins over a size given
    /// beforehand).
    fn set_total(&self, total: u64) {
        let mut state = self.lock();
        state.total = Some(total);
        let more = (total as usize).saturating_sub(state.bytes.len());
        if total <= MAX_SIZE {
            state.bytes.reserve_exact(more);
        }
    }

    fn finish(&self) {
        let mut state = self.lock();
        state.done = true;
        let have = state.bytes.len() as u64;
        state.total = Some(have);
        state.bytes.shrink_to_fit();
        drop(state);
        self.arrived.notify_all();
    }

    #[cfg(test)]
    pub(crate) fn push_for_test(&self, chunk: &[u8]) {
        self.push(chunk);
    }

    #[cfg(test)]
    pub(crate) fn finish_for_test(&self) {
        self.finish();
    }

    fn fail(&self, message: String) {
        self.lock().failed = Some(message);
        self.arrived.notify_all();
    }

    /// Bytes arrived so far.
    pub fn len(&self) -> usize {
        self.lock().bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The whole song has arrived.
    pub fn is_complete(&self) -> bool {
        self.lock().done
    }

    /// Why the download stopped, if it failed.
    pub fn failure(&self) -> Option<String> {
        self.lock().failed.clone()
    }

    /// Waits until at least `len` bytes have arrived (or all of a shorter
    /// song). `Err` when the download failed or `timeout` passed first.
    pub fn wait_for(&self, len: usize, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        let mut state = self.lock();
        loop {
            if state.bytes.len() >= len || state.done {
                return Ok(());
            }
            if let Some(failed) = &state.failed {
                return Err(failed.clone());
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("the audio download stalled".into());
            }
            state = self
                .arrived
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Waits until the whole song has arrived.
    pub fn wait_complete(&self, timeout: Duration) -> Result<(), String> {
        self.wait_for(usize::MAX, timeout)
    }

    /// A copy of every byte arrived so far.
    pub fn to_vec(&self) -> Vec<u8> {
        self.lock().bytes.clone()
    }

    /// A reader from the start. `seekable` readers report the whole size,
    /// which lets the decoder jump around, but make it read the whole file
    /// when it opens; others play from the first bytes.
    pub fn reader(self: &Arc<Self>, seekable: bool) -> SongReader {
        SongReader {
            data: Arc::clone(self),
            position: 0,
            seekable,
        }
    }
}

/// Reads a [`SongData`], waiting for bytes still downloading.
pub struct SongReader {
    data: Arc<SongData>,
    position: u64,
    seekable: bool,
}

impl Read for SongReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let deadline = Instant::now() + READ_WAIT;
        let mut state = self.data.lock();
        loop {
            let have = state.bytes.len() as u64;
            if self.position < have {
                let start = self.position as usize;
                let count = buf.len().min(have as usize - start);
                buf[..count].copy_from_slice(&state.bytes[start..start + count]);
                self.position += count as u64;
                return Ok(count);
            }
            if state.done {
                return Ok(0);
            }
            if let Some(failed) = &state.failed {
                return Err(io::Error::other(failed.clone()));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the audio download stalled",
                ));
            }
            state = self
                .data
                .arrived
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl Seek for SongReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let total = {
            let state = self.data.lock();
            state.total.unwrap_or(state.bytes.len() as u64)
        };
        let target = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(back) => total.checked_add_signed(back),
            SeekFrom::Current(by) => self.position.checked_add_signed(by),
        };
        match target {
            Some(at) => {
                self.position = at;
                Ok(at)
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a jump before the start",
            )),
        }
    }
}

impl symphonia::core::io::MediaSource for SongReader {
    fn is_seekable(&self) -> bool {
        self.seekable
    }

    fn byte_len(&self) -> Option<u64> {
        if !self.seekable {
            return None;
        }
        let state = self.data.lock();
        state.total.or(Some(state.bytes.len() as u64))
    }
}

/// Where a stream comes from.
#[derive(Clone)]
pub struct Source {
    /// The stream address. Short-lived and tied to this session; never shown.
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// The size, when YouTube said.
    pub size: Option<u64>,
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source").field("size", &self.size).finish()
    }
}

/// How a download ended, when it did not fail.
#[derive(Debug, PartialEq, Eq)]
pub enum Fetched {
    /// The whole song arrived, in this long.
    Whole(Duration),
    /// Nobody wanted the song any more (it was skipped, or let go after
    /// being made ready ahead), so the rest of it was not fetched.
    Unwanted,
}

/// Downloads `source` into `data`, a piece at a time, handing each part to
/// the player as it arrives. A connection that breaks or stalls is asked
/// again for the rest, from where it stopped. On failure, `data` says why
/// too, so a player waiting on it stops.
///
/// `data` is held weakly: once the player and the songs made ready ahead
/// have all let go of it, the download stops.
pub async fn fetch(
    http: &reqwest::Client,
    source: &Source,
    data: Weak<SongData>,
) -> Result<Fetched, String> {
    let started = Instant::now();
    match fetch_pieces(http, source, &data).await {
        Ok(true) => {
            if let Some(data) = data.upgrade() {
                data.finish();
            }
            Ok(Fetched::Whole(started.elapsed()))
        }
        Ok(false) => Ok(Fetched::Unwanted),
        Err(message) => {
            let message = redact::urls(&message);
            if let Some(data) = data.upgrade() {
                data.fail(message.clone());
            }
            Err(message)
        }
    }
}

/// What came of asking for one piece.
enum Piece {
    /// More of the song is still to come.
    More,
    /// The end of the song.
    End,
    Unwanted,
}

/// Why a piece did not arrive.
enum Broke {
    /// Asking again would get the same answer.
    Refused(String),
    /// The connection broke, stalled, or the server stumbled: asking again
    /// may work.
    Lost(String),
}

/// Fetches the song piece by piece: `Ok(true)` once all of it arrived,
/// `Ok(false)` when it was no longer wanted.
async fn fetch_pieces(
    http: &reqwest::Client,
    source: &Source,
    data: &Weak<SongData>,
) -> Result<bool, String> {
    let mut total = source.size;
    let mut have = {
        let Some(song) = data.upgrade() else {
            return Ok(false);
        };
        if let Some(total) = total {
            song.set_total(total);
        }
        song.len() as u64
    };
    let mut broken = 0;
    loop {
        if total.is_some_and(|t| have >= t) {
            return Ok(true);
        }
        match fetch_piece(http, source, data, &mut have, &mut total).await {
            Ok(Piece::More) => broken = 0,
            Ok(Piece::End) => return Ok(true),
            Ok(Piece::Unwanted) => return Ok(false),
            Err(Broke::Refused(message)) => return Err(message),
            Err(Broke::Lost(message)) => {
                broken += 1;
                if broken > RETRIES {
                    return Err(message);
                }
                log::info!("a song's download broke off ({message}); asking again for the rest");
                tokio::time::sleep(RETRY_PAUSE * 2u32.pow(broken - 1)).await;
            }
        }
    }
}

/// Asks for the next piece of the song, from `have` on, and hands it to the
/// player as it arrives.
async fn fetch_piece(
    http: &reqwest::Client,
    source: &Source,
    data: &Weak<SongData>,
    have: &mut u64,
    total: &mut Option<u64>,
) -> Result<Piece, Broke> {
    if data.strong_count() == 0 {
        return Ok(Piece::Unwanted);
    }
    let start = *have;
    let end = start + PIECE - 1;
    let end = total.map_or(end, |t| end.min(t - 1));
    let mut request = http
        .get(&source.url)
        .header("Range", format!("bytes={start}-{end}"));
    for (name, value) in &source.headers {
        // Compression would make byte ranges meaningless.
        if !name.eq_ignore_ascii_case("accept-encoding") {
            request = request.header(name.as_str(), value.as_str());
        }
    }
    let lost = |e: String| Broke::Lost(e);
    let stalled = || Broke::Lost("the connection stalled".into());
    let mut response = tokio::time::timeout(STALL, request.send())
        .await
        .map_err(|_| stalled())?
        .map_err(|e| lost(e.to_string()))?;
    let status = response.status().as_u16();
    match status {
        206 => {
            // The server's word on the whole size wins over the one given.
            if let Some(t) = response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(content_range_total)
                && *total != Some(t)
            {
                *total = Some(t);
                match data.upgrade() {
                    Some(song) => song.set_total(t),
                    None => return Ok(Piece::Unwanted),
                }
            }
        }
        // The server sent everything at once.
        200 if start == 0 => {}
        // Asked from past the end: the song was shorter than said, and
        // all of it has arrived.
        416 if start > 0 => return Ok(Piece::End),
        403 => return Err(Broke::Refused("YouTube refused the stream (403)".into())),
        500..=599 => return Err(lost(format!("YouTube answered HTTP {status}"))),
        _ => return Err(Broke::Refused(format!("YouTube answered HTTP {status}"))),
    }
    let whole = status == 200;
    let mut got = 0u64;
    while let Some(chunk) = tokio::time::timeout(STALL, response.chunk())
        .await
        .map_err(|_| stalled())?
        .map_err(|e| lost(e.to_string()))?
    {
        let Some(song) = data.upgrade() else {
            return Ok(Piece::Unwanted);
        };
        song.push(&chunk);
        got += chunk.len() as u64;
        *have += chunk.len() as u64;
        if *have > MAX_SIZE {
            return Err(Broke::Refused(
                "the stream was far larger than any song".into(),
            ));
        }
    }
    if got == 0 {
        if *have == 0 {
            return Err(Broke::Refused("YouTube sent no audio".into()));
        }
        return Ok(Piece::End);
    }
    // Everything at once, or less than asked for with no size known: the
    // end of the song.
    if whole || (total.is_none() && got < end - start + 1) {
        return Ok(Piece::End);
    }
    Ok(Piece::More)
}

/// The total from `Content-Range: bytes 0-99/1234`.
fn content_range_total(value: &str) -> Option<u64> {
    value.rsplit_once('/')?.1.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_waits_for_bytes_still_coming() {
        let data = SongData::new(Some(6));
        let writer = Arc::clone(&data);
        let feeder = std::thread::spawn(move || {
            writer.push(b"abc");
            std::thread::sleep(Duration::from_millis(50));
            writer.push(b"def");
            writer.finish();
        });
        let mut all = Vec::new();
        data.reader(false).read_to_end(&mut all).unwrap();
        feeder.join().unwrap();
        assert_eq!(all, b"abcdef");
        assert!(data.is_complete());
    }

    #[test]
    fn a_failed_download_stops_the_reader() {
        let data = SongData::new(None);
        data.push(b"ab");
        data.fail("gone".into());
        let mut reader = data.reader(false);
        let mut buf = [0u8; 8];
        assert_eq!(reader.read(&mut buf).unwrap(), 2);
        assert!(reader.read(&mut buf).is_err());
        assert!(data.wait_complete(Duration::from_millis(10)).is_err());
    }

    #[test]
    fn seekable_readers_know_the_size() {
        use symphonia::core::io::MediaSource;
        let data = SongData::complete(vec![1, 2, 3, 4]);
        let mut reader = data.reader(true);
        assert_eq!(reader.byte_len(), Some(4));
        reader.seek(SeekFrom::End(-1)).unwrap();
        let mut buf = [0u8; 4];
        assert_eq!(reader.read(&mut buf).unwrap(), 1);
        assert_eq!(buf[0], 4);
        assert_eq!(data.reader(false).byte_len(), None);
    }

    #[test]
    fn waiting_for_part_of_a_song() {
        let data = SongData::new(Some(10));
        data.push(&[0; 4]);
        assert!(data.wait_for(4, Duration::from_millis(10)).is_ok());
        assert!(data.wait_for(5, Duration::from_millis(10)).is_err());
        data.finish();
        assert!(data.wait_for(100, Duration::from_millis(10)).is_ok());
    }

    #[test]
    fn content_range() {
        assert_eq!(content_range_total("bytes 0-99/1234"), Some(1234));
        assert_eq!(content_range_total("bytes 0-99/*"), None);
    }

    /// What goes wrong when the test server answers.
    #[derive(Clone, Copy)]
    enum Trouble {
        /// The first connection breaks off halfway through its piece.
        BreakOff,
        /// The stream is refused.
        Refuse,
        /// Each piece comes in two halves, a moment apart.
        Slow,
    }

    /// A web server on this computer that serves `body` by byte ranges, as
    /// YouTube's servers do. Returns its address.
    fn server(body: Vec<u8>, trouble: Trouble) -> String {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/song", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for (n, stream) in listener.incoming().enumerate() {
                let Ok(mut stream) = stream else { return };
                let mut request = Vec::new();
                let mut buf = [0u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => request.extend_from_slice(&buf[..read]),
                    }
                }
                if matches!(trouble, Trouble::Refuse) {
                    let _ = stream.write_all(
                        b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    continue;
                }
                let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
                let (start, end) = request
                    .lines()
                    .find_map(|line| line.strip_prefix("range: bytes="))
                    .and_then(|range| range.split_once('-'))
                    .and_then(|(a, b)| Some((a.trim().parse().ok()?, b.trim().parse().ok()?)))
                    .unwrap_or((0, body.len() - 1));
                let end = end.min(body.len() - 1);
                let part = &body[start..=end];
                let _ = write!(
                    stream,
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len(),
                    part.len()
                );
                let (first, rest) = part.split_at(part.len() / 2);
                let _ = stream.write_all(first);
                match trouble {
                    // The connection closes with half the piece sent.
                    Trouble::BreakOff if n == 0 => continue,
                    Trouble::Slow => {
                        let _ = stream.flush();
                        std::thread::sleep(Duration::from_millis(300));
                    }
                    _ => {}
                }
                let _ = stream.write_all(rest);
            }
        });
        url
    }

    fn source(url: String) -> Source {
        Source {
            url,
            headers: Vec::new(),
            size: None,
        }
    }

    #[tokio::test]
    async fn a_broken_connection_is_asked_again_for_the_rest() {
        // Two pieces' worth, so the song arrives over several requests.
        let body: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        let source = source(server(body.clone(), Trouble::BreakOff));
        let data = SongData::new(None);
        let http = crate::net::download_client();
        let fetched = fetch(&http, &source, Arc::downgrade(&data)).await;
        assert!(matches!(fetched, Ok(Fetched::Whole(_))), "{fetched:?}");
        assert!(data.is_complete());
        assert!(data.to_vec() == body, "every byte, in order");
    }

    #[tokio::test]
    async fn a_refused_stream_fails_at_once() {
        let source = source(server(vec![1; 1000], Trouble::Refuse));
        let data = SongData::new(None);
        let http = crate::net::download_client();
        let fetched = fetch(&http, &source, Arc::downgrade(&data)).await;
        assert_eq!(fetched, Err("YouTube refused the stream (403)".into()));
        assert!(data.failure().is_some(), "a player waiting on it stops");
    }

    #[tokio::test]
    async fn a_song_nobody_wants_stops_downloading() {
        let source = source(server(vec![7; 3_000_000], Trouble::Slow));
        let data = SongData::new(None);
        let task = {
            let weak = Arc::downgrade(&data);
            tokio::spawn(async move {
                let http = crate::net::download_client();
                fetch(&http, &source, weak).await
            })
        };
        // The first bytes arrive, then the song is let go (skipped).
        let arriving = Arc::clone(&data);
        tokio::task::spawn_blocking(move || arriving.wait_for(1, Duration::from_secs(10)))
            .await
            .unwrap()
            .unwrap();
        drop(data);
        assert_eq!(task.await.unwrap(), Ok(Fetched::Unwanted));
    }
}
