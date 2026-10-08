//! The puzzle YouTube puts on its stream addresses, solved the way the
//! website does: with YouTube's own player code, loaded once and kept
//! ready.
//!
//! The solver is yt-dlp's (its EJS scripts, which come inside the yt-dlp
//! download and so update with it), run by Deno in a small program that
//! stays running: it reads one request per line and answers one line each.
//! Turning a new YouTube player into solving functions takes a second or
//! two, once per player version (YouTube changes it every few weeks); the
//! result is kept on disk. Each song's puzzle then takes milliseconds.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;

/// The loop around yt-dlp's solver. `jsc` (from the core script) turns a
/// player into a "preprocessed" script that sets `_result.n` and
/// `_result.sig`; those functions are kept between requests.
const SERVER: &str = r#"
let solvers = null;

function load(request) {
  let preprocessed = request.preprocessed;
  let fresh = false;
  if (!preprocessed) {
    const output = jsc({ type: 'player', player: request.player, requests: [], output_preprocessed: true });
    if (output.type !== 'result') throw new Error(output.error);
    preprocessed = output.preprocessed_player;
    fresh = true;
  }
  const made = { n: null, sig: null };
  Function('_result', preprocessed)(made);
  solvers = made;
  return fresh ? { preprocessed } : {};
}

function solve(request) {
  if (!solvers) throw new Error('no player loaded');
  const answer = {};
  for (const kind of ['n', 'sig']) {
    const challenges = request[kind] || [];
    if (!challenges.length) continue;
    if (!solvers[kind]) throw new Error(`this player has no ${kind} function`);
    answer[kind] = Object.fromEntries(challenges.map((c) => [c, solvers[kind](c)]));
  }
  return answer;
}

const encoder = new TextEncoder();
async function reply(message) {
  const bytes = encoder.encode(JSON.stringify(message) + '\n');
  let written = 0;
  while (written < bytes.length) written += await Deno.stdout.write(bytes.subarray(written));
}

let pending = '';
for await (const chunk of Deno.stdin.readable.pipeThrough(new TextDecoderStream())) {
  let from = pending.length;
  pending += chunk;
  let newline;
  while ((newline = pending.indexOf('\n', from)) >= 0) {
    const line = pending.slice(0, newline);
    pending = pending.slice(newline + 1);
    from = 0;
    if (!line.trim()) continue;
    let request = null;
    try {
      request = JSON.parse(line);
      const result = request.op === 'load' ? load(request) : solve(request);
      await reply({ id: request.id, ok: true, ...result });
    } catch (e) {
      const id = request ? request.id : null;
      await reply({ id, ok: false, error: e instanceof Error ? e.message : String(e) });
    }
  }
}
"#;

/// How long the first load of a new player may take (it is parsed and
/// analysed whole).
const LOAD_TIMEOUT: Duration = Duration::from_secs(90);
const SOLVE_TIMEOUT: Duration = Duration::from_secs(10);

/// Where a player comes from: its code, or the result of an earlier load.
pub enum PlayerCode {
    Code(String),
    Preprocessed(String),
}

/// A song's puzzles solved: challenge to answer.
#[derive(Debug, Default)]
pub struct Answers {
    pub n: HashMap<String, String>,
    pub sig: HashMap<String, String>,
}

/// Why a request to the solver failed.
#[derive(Debug, PartialEq, Eq)]
enum Failure {
    /// The program answered with an error (a player it cannot read, say).
    /// It carries on, with the player it had.
    Refused(String),
    /// The program stopped, said something unreadable or took too long. It
    /// is started afresh next time.
    Broken(String),
}

impl From<Failure> for String {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Refused(message) | Failure::Broken(message) => message,
        }
    }
}

/// The two ends of the solver's pipe: each request goes in as one line,
/// and each answer comes back as one line carrying the request's number.
struct Pipe<W, R> {
    input: W,
    output: R,
    next_id: u64,
}

impl<W: AsyncWrite + Unpin, R: AsyncBufRead + Unpin> Pipe<W, R> {
    /// Sends one request and reads its answer.
    async fn ask(&mut self, mut request: Value, timeout: Duration) -> Result<Value, Failure> {
        self.next_id += 1;
        let id = self.next_id;
        request["id"] = json!(id);
        let mut line = request.to_string();
        line.push('\n');
        let exchange = async {
            self.input
                .write_all(line.as_bytes())
                .await
                .map_err(|e| Failure::Broken(format!("the solver stopped: {e}")))?;
            self.input
                .flush()
                .await
                .map_err(|e| Failure::Broken(e.to_string()))?;
            loop {
                let mut answer = String::new();
                let read = self
                    .output
                    .read_line(&mut answer)
                    .await
                    .map_err(|e| Failure::Broken(e.to_string()))?;
                if read == 0 {
                    return Err(Failure::Broken("the solver stopped".to_string()));
                }
                let answer: Value = serde_json::from_str(&answer).map_err(|e| {
                    Failure::Broken(format!("the solver said something unreadable: {e}"))
                })?;
                // Answers to earlier, timed-out requests are skipped.
                if answer.get("id").and_then(Value::as_u64) == Some(id) {
                    return Ok(answer);
                }
            }
        };
        let answer = tokio::time::timeout(timeout, exchange)
            .await
            .map_err(|_| Failure::Broken("the solver took too long".to_string()))??;
        if answer.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(answer)
        } else {
            Err(Failure::Refused(
                answer
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("the solver failed")
                    .chars()
                    .take(300)
                    .collect(),
            ))
        }
    }
}

struct Running {
    child: Child,
    pipe: Pipe<ChildStdin, BufReader<ChildStdout>>,
    /// The player loaded, by ID.
    loaded: Option<String>,
}

/// The solver program, started when first needed and stopped when idle.
pub struct Solver {
    deno: PathBuf,
    script: PathBuf,
    running: Mutex<Option<Running>>,
    last_used: StdMutex<Instant>,
}

impl Solver {
    /// `ejs` is the folder with yt-dlp's `core.min.js` and `lib.min.js`
    /// ([`find_ejs`]); the solver program is written into `work`.
    pub fn new(deno: PathBuf, ejs: &Path, work: &Path) -> std::io::Result<Self> {
        let lib = std::fs::read_to_string(ejs.join("lib.min.js"))?;
        let core = std::fs::read_to_string(ejs.join("core.min.js"))?;
        std::fs::create_dir_all(work)?;
        let script = work.join("ytfast-solver.js");
        let text = format!("{lib}\nObject.assign(globalThis, lib);\n{core}\n{SERVER}");
        std::fs::write(&script, text)?;
        Ok(Self {
            deno,
            script,
            running: Mutex::new(None),
            last_used: StdMutex::new(Instant::now()),
        })
    }

    fn touch(&self) {
        *self
            .last_used
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
    }

    fn start(&self) -> Result<Running, String> {
        let mut command = tokio::process::Command::new(&self.deno);
        command
            .arg("run")
            .args([
                "--ext=js",
                "--no-code-cache",
                "--no-prompt",
                "--no-remote",
                "--no-lock",
                "--node-modules-dir=none",
                "--no-config",
                "--no-npm",
                // No optimising compiler: a solver with a player loaded
                // takes about 70 MB instead of 100 to 270. Solving stays a
                // few milliseconds; preparing a new player takes a few
                // seconds instead of one, once per player.
                "--v8-flags=--lite-mode",
            ])
            .arg(&self.script)
            .env("NO_COLOR", "1")
            .env("DENO_NO_UPDATE_CHECK", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        crate::ytdlp::no_console_window(&mut command);
        let mut child = command
            .spawn()
            .map_err(|e| format!("Deno could not start: {e}"))?;
        let stdin = child.stdin.take().ok_or("no input to Deno")?;
        let stdout = child.stdout.take().ok_or("no output from Deno")?;
        Ok(Running {
            child,
            pipe: Pipe {
                input: stdin,
                output: BufReader::with_capacity(1 << 20, stdout),
                next_id: 0,
            },
            loaded: None,
        })
    }

    /// The player loaded now, if any. Asking counts as using the solver:
    /// a solve follows, and the program must not be stopped for idleness
    /// meanwhile (while the song's `player` request is answered, say).
    pub async fn loaded(&self) -> Option<String> {
        self.touch();
        self.running
            .lock()
            .await
            .as_ref()
            .and_then(|r| r.loaded.clone())
    }

    /// Loads player `id`. Returns the preprocessed form when it was made
    /// from the code, to keep for next time.
    pub async fn load(&self, id: &str, code: PlayerCode) -> Result<Option<String>, String> {
        self.touch();
        let mut guard = self.running.lock().await;
        if guard.is_none() {
            *guard = Some(self.start()?);
        }
        let running = guard.as_mut().expect("just started");
        let request = match &code {
            PlayerCode::Code(code) => json!({ "op": "load", "player": code }),
            PlayerCode::Preprocessed(pre) => json!({ "op": "load", "preprocessed": pre }),
        };
        match running.pipe.ask(request, LOAD_TIMEOUT).await {
            Ok(answer) => {
                running.loaded = Some(id.to_string());
                Ok(answer
                    .get("preprocessed")
                    .and_then(Value::as_str)
                    .map(str::to_string))
            }
            // The program keeps the player it had.
            Err(Failure::Refused(e)) => Err(e),
            Err(Failure::Broken(e)) => {
                // Start afresh next time.
                *guard = None;
                Err(e)
            }
        }
    }

    /// Solves `n` and `sig` challenges with player `id`, which must be
    /// loaded ([`Solver::load`]).
    pub async fn solve(&self, id: &str, n: &[String], sig: &[String]) -> Result<Answers, String> {
        self.touch();
        let mut guard = self.running.lock().await;
        let Some(running) = guard.as_mut().filter(|r| r.loaded.as_deref() == Some(id)) else {
            return Err("the solver has not loaded this player".into());
        };
        let answer = match running
            .pipe
            .ask(json!({ "op": "solve", "n": n, "sig": sig }), SOLVE_TIMEOUT)
            .await
        {
            Ok(answer) => answer,
            // A clean error from the player's code: the program is fine.
            Err(Failure::Refused(e)) => return Err(e),
            Err(Failure::Broken(e)) => {
                *guard = None;
                return Err(e);
            }
        };
        let map = |kind: &str| -> HashMap<String, String> {
            answer
                .get(kind)
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                        .collect()
                })
                .unwrap_or_default()
        };
        Ok(Answers {
            n: map("n"),
            sig: map("sig"),
        })
    }

    /// Stops the solver now; the next request starts it afresh.
    pub async fn stop(&self) {
        if let Some(mut running) = self.running.lock().await.take() {
            let _ = running.child.start_kill();
        }
    }

    /// Stops the program when unused for `idle`, to give back its memory.
    /// It starts again when next needed.
    pub async fn stop_if_idle(&self, idle: Duration) {
        let unused = self
            .last_used
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .elapsed();
        if unused >= idle
            && let Ok(mut guard) = self.running.try_lock()
            && let Some(mut running) = guard.take()
        {
            let _ = running.child.start_kill();
        }
    }
}

/// The folder with yt-dlp's solver scripts, inside its download (next to
/// the program, in `_internal/yt_dlp_ejs/yt/solver`).
pub fn find_ejs(yt_dlp: &Path) -> Option<PathBuf> {
    let root = yt_dlp.parent()?;
    let expected = root.join("_internal/yt_dlp_ejs/yt/solver");
    if expected.join("core.min.js").is_file() {
        return Some(expected);
    }
    // Elsewhere in the folder, in case the layout changes.
    fn search(dir: &Path, depth: u32) -> Option<PathBuf> {
        if dir.join("core.min.js").is_file() && dir.join("lib.min.js").is_file() {
            return Some(dir.to_path_buf());
        }
        if depth == 0 {
            return None;
        }
        std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .find_map(|e| search(&e.path(), depth - 1))
    }
    search(root, 6)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{DuplexStream, ReadHalf, WriteHalf};

    /// Runs the real solver program with Deno and yt-dlp's scripts when
    /// `YTFAST_TEST_DENO` and `YTFAST_TEST_EJS` say where they are;
    /// otherwise does nothing. YouTube's player cannot be fetched in tests,
    /// so a stand-in "preprocessed" player provides the functions.
    #[tokio::test]
    async fn the_solver_program_answers() {
        let (Ok(deno), Ok(ejs)) = (
            std::env::var("YTFAST_TEST_DENO"),
            std::env::var("YTFAST_TEST_EJS"),
        ) else {
            return;
        };
        let work = tempfile::tempdir().unwrap();
        let solver = Solver::new(deno.into(), Path::new(&ejs), work.path()).unwrap();
        assert!(solver.solve("p1", &["x".into()], &[]).await.is_err());
        // Not a YouTube player: a clear error, and the solver carries on.
        let wrong = solver
            .load("p0", PlayerCode::Code("var a = 1;".into()))
            .await;
        assert!(wrong.is_err(), "{wrong:?}");
        let stand_in =
            "_result.n = (x) => x.split('').reverse().join(''); _result.sig = (x) => x + '!';";
        let made = solver
            .load("p1", PlayerCode::Preprocessed(stand_in.into()))
            .await
            .unwrap();
        assert!(made.is_none());
        assert_eq!(solver.loaded().await.as_deref(), Some("p1"));
        let answers = solver
            .solve("p1", &["abc".into()], &["s".into()])
            .await
            .unwrap();
        assert_eq!(answers.n["abc"], "cba");
        assert_eq!(answers.sig["s"], "s!");
        // A player it cannot read is refused, and the one loaded stays.
        assert!(
            solver
                .load(
                    "p3",
                    PlayerCode::Preprocessed("throw new Error('no')".into())
                )
                .await
                .is_err()
        );
        assert_eq!(solver.loaded().await.as_deref(), Some("p1"));
        assert_eq!(
            solver.solve("p1", &["xyz".into()], &[]).await.unwrap().n["xyz"],
            "zyx"
        );
        // A big request (a player is megabytes) gets through too.
        let big = format!("{stand_in} var filler = '{}';", "x".repeat(3_000_000));
        solver
            .load("p2", PlayerCode::Preprocessed(big))
            .await
            .unwrap();
        let answers = solver.solve("p2", &["xy".into()], &[]).await.unwrap();
        assert_eq!(answers.n["xy"], "yx");
        // Stopped when idle, and loading starts it again.
        solver.stop_if_idle(Duration::ZERO).await;
        assert_eq!(solver.loaded().await, None);
        solver
            .load("p1", PlayerCode::Preprocessed(stand_in.into()))
            .await
            .unwrap();
        assert_eq!(
            solver.solve("p1", &["ab".into()], &[]).await.unwrap().n["ab"],
            "ba"
        );
        // Stopped at once (after preparing a player), and started again.
        solver.stop().await;
        assert_eq!(solver.loaded().await, None);
        solver
            .load("p1", PlayerCode::Preprocessed(stand_in.into()))
            .await
            .unwrap();
        assert_eq!(
            solver.solve("p1", &[], &["t".into()]).await.unwrap().sig["t"],
            "t!"
        );
    }

    type FakePipe = Pipe<WriteHalf<DuplexStream>, BufReader<ReadHalf<DuplexStream>>>;

    /// A stand-in for the solver program, at the other end of an in-memory
    /// pipe. What it does with a request depends on the request's `op`.
    fn fake_solver() -> FakePipe {
        let (ours, theirs) = tokio::io::duplex(64 * 1024);
        tokio::spawn(async move {
            let (from_us, mut to_us) = tokio::io::split(theirs);
            let mut lines = BufReader::new(from_us).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let request: Value = serde_json::from_str(&line).unwrap();
                let id = request["id"].as_u64().unwrap();
                let answer = match request["op"].as_str().unwrap() {
                    // An answer to an earlier request that timed out comes
                    // first, then this one's.
                    "late" => format!(
                        "{}\n{}\n",
                        json!({ "id": id - 1, "ok": true, "which": "earlier" }),
                        json!({ "id": id, "ok": true, "which": "this" })
                    ),
                    "fail" => format!(
                        "{}\n",
                        json!({ "id": id, "ok": false, "error": "no player loaded" })
                    ),
                    "garble" => "not an answer\n".to_string(),
                    "silent" => continue,
                    // Stops, as a program that crashed.
                    _ => return,
                };
                to_us.write_all(answer.as_bytes()).await.unwrap();
            }
        });
        let (from_them, to_them) = tokio::io::split(ours);
        Pipe {
            input: to_them,
            output: BufReader::new(from_them),
            next_id: 0,
        }
    }

    #[tokio::test]
    async fn answers_are_matched_to_their_request() {
        let mut pipe = fake_solver();
        let wait = Duration::from_secs(5);
        // Each time, the answer to the request before comes first and is
        // skipped.
        pipe.ask(json!({ "op": "late" }), wait).await.unwrap();
        let answer = pipe.ask(json!({ "op": "late" }), wait).await.unwrap();
        assert_eq!(answer["which"], "this");
        assert_eq!(answer["id"], 2);
    }

    #[tokio::test]
    async fn the_solvers_errors_are_told_apart() {
        let mut pipe = fake_solver();
        let wait = Duration::from_secs(5);
        // An error from the player's code: the program carries on.
        assert_eq!(
            pipe.ask(json!({ "op": "fail" }), wait).await,
            Err(Failure::Refused("no player loaded".into()))
        );
        assert!(pipe.ask(json!({ "op": "late" }), wait).await.is_ok());
        // No answer in time.
        assert_eq!(
            pipe.ask(json!({ "op": "silent" }), Duration::from_millis(100))
                .await,
            Err(Failure::Broken("the solver took too long".into()))
        );
        // Something that is not an answer.
        let garbled = pipe.ask(json!({ "op": "garble" }), wait).await;
        assert!(
            matches!(&garbled, Err(Failure::Broken(m)) if m.contains("unreadable")),
            "{garbled:?}"
        );
        // The program stopped.
        assert_eq!(
            pipe.ask(json!({ "op": "stop" }), wait).await,
            Err(Failure::Broken("the solver stopped".into()))
        );
    }

    #[test]
    fn finds_the_scripts_next_to_yt_dlp() {
        let dir = tempfile::tempdir().unwrap();
        let solver = dir.path().join("_internal/yt_dlp_ejs/yt/solver");
        std::fs::create_dir_all(&solver).unwrap();
        std::fs::write(solver.join("core.min.js"), "").unwrap();
        std::fs::write(solver.join("lib.min.js"), "").unwrap();
        let exe = dir.path().join("yt-dlp");
        assert_eq!(find_ejs(&exe), Some(solver));
        assert_eq!(
            find_ejs(&tempfile::tempdir().unwrap().path().join("yt-dlp")),
            None
        );
    }
}
