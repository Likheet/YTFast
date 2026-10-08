//! The results of a check: shown at the end and saved as a text file the
//! user can share. Nothing personal goes in it: no account name, no song
//! titles, no cookies, no stream addresses, no sound device's name, and
//! no home folder (it holds the computer's user name).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Fail,
    Skip,
}

impl Outcome {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Ok => "[ OK ]",
            Self::Fail => "[FAIL]",
            Self::Skip => "[SKIP]",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Step {
    pub name: &'static str,
    pub outcome: Outcome,
    pub detail: String,
}

/// What happened to one song.
#[derive(Clone, Debug, Default)]
pub struct SongResult {
    pub format: String,
    pub premium: bool,
    pub size_bytes: u64,
    pub find_seconds: f64,
    pub download_seconds: f64,
    pub decoded_seconds: Option<f64>,
    pub expected_seconds: Option<f64>,
    pub loudness_db: Option<f64>,
    pub played_seconds: f64,
    pub started_report: Option<String>,
    pub listened_report: Option<String>,
    pub problem: Option<String>,
    /// Only this song cannot be played here (removed, or not offered in
    /// this country): it is skipped, which is not a failure.
    pub unavailable: bool,
}

#[derive(Default)]
pub struct Report {
    pub steps: Vec<Step>,
    pub songs: Vec<SongResult>,
    pub facts: Vec<(String, String)>,
}

impl Report {
    pub fn step(&mut self, name: &'static str, outcome: Outcome, detail: impl Into<String>) {
        self.steps.push(Step {
            name,
            outcome,
            detail: detail.into(),
        });
    }

    pub fn fact(&mut self, name: &str, value: impl Into<String>) {
        self.facts.push((name.to_string(), value.into()));
    }

    /// No step failed. Skipped steps were not checked: see
    /// [`Report::skipped`].
    pub fn passed(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|s| s.outcome != Outcome::Fail)
    }

    /// How many steps were skipped.
    pub fn skipped(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| s.outcome == Outcome::Skip)
            .count()
    }

    /// "PASSED", "PASSED (2 steps skipped)" or "NOT PASSED".
    pub fn verdict(&self) -> String {
        match (self.passed(), self.skipped()) {
            (false, _) => "NOT PASSED".into(),
            (true, 0) => "PASSED".into(),
            (true, 1) => "PASSED (1 step skipped)".into(),
            (true, n) => format!("PASSED ({n} steps skipped)"),
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "YtFast check report (step 0)");
        let _ = writeln!(out, "Made: {} UTC", utc_now());
        let _ = writeln!(
            out,
            "Computer: {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        let _ = writeln!(out, "Check version: {}", env!("CARGO_PKG_VERSION"));
        for (name, value) in &self.facts {
            let _ = writeln!(out, "{name}: {value}");
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "Result: {}", self.verdict());
        let _ = writeln!(out);
        let _ = writeln!(out, "Steps:");
        for step in &self.steps {
            let _ = writeln!(
                out,
                "  {} {}: {}",
                step.outcome.tag(),
                step.name,
                step.detail
            );
        }
        if !self.songs.is_empty() {
            let _ = writeln!(out);
            let _ = writeln!(out, "Songs (no titles are recorded):");
            for (i, song) in self.songs.iter().enumerate() {
                let _ = writeln!(out, "  Song {}:", i + 1);
                if let Some(problem) = &song.problem {
                    let _ = writeln!(out, "    problem: {problem}");
                }
                if !song.format.is_empty() {
                    let _ = writeln!(
                        out,
                        "    audio: {}{}",
                        song.format,
                        if song.premium {
                            " (Premium quality)"
                        } else {
                            ""
                        }
                    );
                    let _ = writeln!(
                        out,
                        "    found in {:.1} s; downloaded {:.1} MB in {:.1} s",
                        song.find_seconds,
                        song.size_bytes as f64 / 1_048_576.0,
                        song.download_seconds
                    );
                }
                if let Some(decoded) = song.decoded_seconds {
                    let expected = song
                        .expected_seconds
                        .map(|e| format!(" (YouTube says {})", clock(e)))
                        .unwrap_or_default();
                    let _ = writeln!(out, "    length: {}{expected}", clock(decoded));
                }
                if let Some(db) = song.loudness_db {
                    let _ = writeln!(out, "    loudness: {db:+.1} dB");
                }
                if song.played_seconds > 0.0 {
                    let _ = writeln!(out, "    played: {}", clock(song.played_seconds));
                }
                if let Some(r) = &song.started_report {
                    let _ = writeln!(out, "    'started' report: {r}");
                }
                if let Some(r) = &song.listened_report {
                    let _ = writeln!(out, "    'listened' report: {r}");
                }
            }
        }
        // Error messages can name files in the home folder.
        crate::scrub::without_home(&out)
    }

    /// Saves the report in the current folder, or the home folder when the
    /// current one is not writable. Returns where it went.
    pub fn save(&self) -> std::io::Result<PathBuf> {
        let name = "ytfast-check-report.txt";
        let text = self.render();
        // The full path, without canonicalize's "\\?\" prefix on Windows.
        let here = std::env::current_dir()
            .map(|dir| dir.join(name))
            .unwrap_or_else(|_| Path::new(name).to_path_buf());
        match std::fs::write(&here, &text) {
            Ok(()) => Ok(here),
            Err(_) => {
                let home = directories::UserDirs::new()
                    .map(|u| u.home_dir().to_path_buf())
                    .unwrap_or_else(std::env::temp_dir);
                let path = home.join(name);
                std::fs::write(&path, &text)?;
                Ok(path)
            }
        }
    }
}

/// `m:ss`.
pub fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// The date and time now, in UTC, without a date library.
fn utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // Howard Hinnant's days-to-civil algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rest / 3_600,
        rest % 3_600 / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_format() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(61.4), "1:01");
        assert_eq!(clock(3_725.0), "62:05");
    }

    #[test]
    fn verdict_follows_failures() {
        let mut report = Report::default();
        assert!(!report.passed());
        report.step("A", Outcome::Ok, "fine");
        assert_eq!(report.verdict(), "PASSED");
        report.step("B", Outcome::Skip, "not run");
        assert!(report.passed());
        // A skipped step was not checked: the verdict says so.
        assert_eq!(report.skipped(), 1);
        assert!(report.render().contains("Result: PASSED (1 step skipped)"));
        report.step("D", Outcome::Skip, "not run");
        assert_eq!(report.verdict(), "PASSED (2 steps skipped)");
        report.step("C", Outcome::Fail, "broken");
        assert!(!report.passed());
        let text = report.render();
        assert!(text.contains("NOT PASSED"));
        assert!(text.contains("[FAIL] C: broken"));
    }

    #[test]
    fn date_is_plausible() {
        let now = utc_now();
        assert_eq!(now.len(), "2026-10-06 13:20".len());
        assert!(now.starts_with("20"));
    }
}
