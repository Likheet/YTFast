//! Lyrics as the player page shows them.

/// One line of lyrics. `start` (seconds into the song) when the lyrics are
/// time-synced.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub start: Option<f64>,
    pub text: String,
}

/// A song's lyrics.
#[derive(Clone, Debug, PartialEq)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    /// Every line has a time, so the current one can be lit.
    pub synced: bool,
    /// Where they came from ("LRCLIB", "Musixmatch").
    pub source: String,
}

impl Lyrics {
    /// The line being sung at `position`: the last line started by then.
    pub fn current(&self, position: f64) -> Option<usize> {
        if !self.synced {
            return None;
        }
        // A little early, so the line lights as it is sung, not after.
        let at = position + 0.2;
        self.lines
            .iter()
            .rposition(|l| l.start.is_some_and(|s| s <= at))
    }
}

impl From<ytfast_core::lyrics::Lyrics> for Lyrics {
    fn from(found: ytfast_core::lyrics::Lyrics) -> Self {
        Self {
            lines: found
                .lines
                .into_iter()
                .map(|line| Line {
                    start: line.start_ms.map(|ms| ms as f64 / 1000.0),
                    text: line.text,
                })
                .collect(),
            synced: found.synced,
            source: found.source,
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
                },
                Line {
                    start: Some(5.0),
                    text: "two".into(),
                },
            ],
            synced: true,
            source: String::new(),
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
}
