//! Lyrics as the player page shows them.

/// The language lyrics are translated into.
pub const TRANSLATE_TO: &str = "en";

/// How much earlier than the song's position a word starts to light, in
/// seconds: the eye needs a moment to follow.
const WORD_LEAD: f64 = 0.1;
/// How much earlier a line lights: a little, so it lights as it is sung,
/// not after.
const LINE_LEAD: f64 = 0.2;
/// How often the lyrics are drawn while a word lights (25 times a second,
/// as a video's pictures).
pub const WORD_FRAME: f64 = 0.04;

/// One line of lyrics. `start` (seconds into the song) when the lyrics are
/// time-synced.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub start: Option<f64>,
    pub text: String,
    /// Its words' times and places in `text`: the source's own (Musixmatch
    /// times each word), else estimated from the line's
    /// (`ytfast_core::lyrics::estimate_words`). Empty for lyrics without
    /// times.
    pub words: Vec<Word>,
}

/// A word of a line, and when it is sung (seconds into the song).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Word {
    pub start: f64,
    pub end: f64,
    /// Where it is in the line's text, in bytes.
    pub from: usize,
    pub to: usize,
}

impl Word {
    /// How much of the word is sung at `position` (seconds into the song):
    /// 0 before it starts, 1 once it ends.
    pub fn sung(&self, position: f64) -> f32 {
        let at = position + WORD_LEAD;
        if self.end <= self.start {
            return if at >= self.start { 1.0 } else { 0.0 };
        }
        ((at - self.start) / (self.end - self.start)).clamp(0.0, 1.0) as f32
    }
}

/// A song's lyrics.
#[derive(Clone, Debug, PartialEq)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    /// Every line has a time, so the current one can be lit.
    pub synced: bool,
    /// Where they came from ("LRCLIB", "Musixmatch").
    pub source: String,
    /// The words' times are the source's own, not estimated.
    pub timed_words: bool,
    /// The song on Musixmatch, for people's translation of its lyrics.
    pub musixmatch: Option<ytfast_core::musixmatch::Song>,
}

impl Lyrics {
    /// The line being sung at `position`: the last line started by then.
    pub fn current(&self, position: f64) -> Option<usize> {
        if !self.synced {
            return None;
        }
        let at = position + LINE_LEAD;
        self.lines
            .iter()
            .rposition(|l| l.start.is_some_and(|s| s <= at))
    }

    /// How long until the lyrics look different as the song plays on from
    /// `position`, in seconds: [`WORD_FRAME`] while a word of the line being
    /// sung lights, else until the next word or line starts. `None` when
    /// nothing more changes (lyrics without times, or after the last line's
    /// last word). Between words the window need not be drawn.
    pub fn next_change(&self, position: f64) -> Option<f64> {
        if !self.synced {
            return None;
        }
        let current = self.current(position);
        let mut next: Option<f64> = None;
        let mut sooner = |wait: f64| {
            if wait > 0.0 {
                next = Some(next.map_or(wait, |n: f64| n.min(wait)));
            }
        };
        if let Some(line) = current.and_then(|i| self.lines.get(i)) {
            for word in &line.words {
                let sung = word.sung(position);
                if sung > 0.0 && sung < 1.0 {
                    return Some(WORD_FRAME);
                }
                sooner(word.start - WORD_LEAD - position);
            }
        }
        let after = current.map_or(0, |i| i + 1);
        if let Some(start) = self
            .lines
            .get(after..)
            .and_then(|rest| rest.iter().find_map(|l| l.start))
        {
            sooner(start - LINE_LEAD - position);
        }
        next
    }

    /// The lines' words, to translate.
    pub fn texts(&self) -> Vec<String> {
        self.lines.iter().map(|line| line.text.clone()).collect()
    }
}

impl From<ytfast_core::lyrics::Lyrics> for Lyrics {
    /// Lines timed only by the line get their words' times estimated, so
    /// every timed song's words light as they are sung.
    fn from(mut found: ytfast_core::lyrics::Lyrics) -> Self {
        if found.synced {
            ytfast_core::lyrics::estimate_words(&mut found.lines);
        }
        let seconds = |ms: u64| ms as f64 / 1000.0;
        Self {
            lines: found
                .lines
                .into_iter()
                .map(|line| Line {
                    start: line.start_ms.map(seconds),
                    words: line
                        .words
                        .iter()
                        .filter(|w| w.from < w.to && line.text.get(w.from..w.to).is_some())
                        .map(|w| Word {
                            start: seconds(w.start_ms),
                            end: seconds(w.end_ms),
                            from: w.from,
                            to: w.to,
                        })
                        .collect(),
                    text: line.text,
                })
                .collect(),
            synced: found.synced,
            source: found.source,
            timed_words: found.timed_words,
            musixmatch: found.musixmatch,
        }
    }
}

/// Lyrics for one song, as they load.
#[derive(Clone, Debug)]
pub enum State {
    Loading,
    Ready(Lyrics),
    /// The song has none (or none were found).
    Missing,
}

/// A song's lyrics translated, as it loads.
#[derive(Clone, Debug)]
pub enum Translation {
    Loading,
    Ready(ytfast_core::translate::Translated),
    /// It could not be had (the translator did not answer).
    Missing,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_line() {
        let lyrics = Lyrics {
            lines: vec![
                Line {
                    start: Some(1.0),
                    text: "one".into(),
                    words: Vec::new(),
                },
                Line {
                    start: Some(5.0),
                    text: "two".into(),
                    words: Vec::new(),
                },
            ],
            synced: true,
            source: String::new(),
            timed_words: false,
            musixmatch: None,
        };
        assert_eq!(lyrics.current(0.0), None);
        assert_eq!(lyrics.current(0.9), Some(0));
        assert_eq!(lyrics.current(4.0), Some(0));
        assert_eq!(lyrics.current(6.0), Some(1));
        let plain = Lyrics {
            synced: false,
            ..lyrics
        };
        assert_eq!(plain.current(6.0), None);
    }

    /// Lyrics timed by the line come with their words timed, in order; a
    /// word lights through its time, a little ahead of the song.
    #[test]
    fn words_light_through_their_time() {
        use ytfast_core::lyrics as core;
        let found = core::Lyrics {
            lines: vec![
                core::LyricLine {
                    start_ms: Some(1_000),
                    end_ms: Some(4_000),
                    text: "Paper planes tonight".into(),
                    words: Vec::new(),
                },
                core::LyricLine {
                    start_ms: Some(4_000),
                    end_ms: None,
                    text: String::new(),
                    words: Vec::new(),
                },
            ],
            synced: true,
            ..core::Lyrics::default()
        };
        let lyrics = Lyrics::from(found);
        let words = &lyrics.lines[0].words;
        let line = &lyrics.lines[0].text;
        let texts: Vec<&str> = words.iter().map(|w| &line[w.from..w.to]).collect();
        assert_eq!(texts, ["Paper", "planes", "tonight"]);
        assert_eq!(words[0].start, 1.0);
        assert!(words.windows(2).all(|w| w[0].end == w[1].start));
        assert!(lyrics.lines[1].words.is_empty());
        let word = Word {
            start: 2.0,
            end: 3.0,
            from: 0,
            to: 1,
        };
        assert_eq!(word.sung(1.0), 0.0);
        assert!((word.sung(2.4) - 0.5).abs() < 1e-6);
        assert_eq!(word.sung(3.5), 1.0);
        // A word without length lights at once.
        let instant = Word { end: 2.0, ..word };
        assert_eq!(instant.sung(1.85), 0.0);
        assert_eq!(instant.sung(1.95), 1.0);
    }

    /// The window is drawn often only while a word lights; otherwise not
    /// before the next word or line starts.
    #[test]
    fn drawn_often_only_while_a_word_lights() {
        let word = |start: f64, end: f64| Word {
            start,
            end,
            from: 0,
            to: 1,
        };
        let lyrics = Lyrics {
            lines: vec![
                Line {
                    start: Some(1.0),
                    text: "a b".into(),
                    words: vec![word(1.0, 1.5), word(2.0, 2.5)],
                },
                Line {
                    start: Some(6.0),
                    text: String::new(),
                    words: Vec::new(),
                },
            ],
            synced: true,
            source: String::new(),
            timed_words: false,
            musixmatch: None,
        };
        let close = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
        // Before the first line: when it lights (0.2 s early).
        assert!(close(lyrics.next_change(0.0), 0.8));
        // A word lighting: a frame's time.
        assert_eq!(lyrics.next_change(1.2), Some(WORD_FRAME));
        // Between two words: when the next starts (0.1 s early).
        assert!(close(lyrics.next_change(1.6), 0.3));
        // The line sung: when the next line lights.
        assert!(close(lyrics.next_change(3.0), 2.8));
        // After the last line: nothing more.
        assert_eq!(lyrics.next_change(7.0), None);
        let plain = Lyrics {
            synced: false,
            ..lyrics
        };
        assert_eq!(plain.next_change(1.2), None);
    }
}
