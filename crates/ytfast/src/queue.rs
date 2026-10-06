//! What plays now and next.
//!
//! Every entry gets its own ID, so the same song twice in a queue is two
//! entries, and an answer about one entry (a finished download, a report)
//! can never be applied to another.

use ytfast_core::read::Track;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: u64,
    pub track: Track,
}

#[derive(Debug, Default)]
pub struct Queue {
    entries: Vec<Entry>,
    current: Option<usize>,
    next_id: u64,
    /// Where more songs come from when the queue runs out: the playlist or
    /// album being played, else a radio of the last song.
    pub source: Option<String>,
}

impl Queue {
    fn entry(&mut self, track: Track) -> Entry {
        self.next_id += 1;
        Entry {
            id: self.next_id,
            track,
        }
    }

    /// Plays `tracks`, starting at `start`. Returns the entry to play.
    pub fn replace(
        &mut self,
        tracks: Vec<Track>,
        start: usize,
        source: Option<String>,
    ) -> Option<&Entry> {
        let entries: Vec<Entry> = tracks.into_iter().map(|t| self.entry(t)).collect();
        self.current = (!entries.is_empty()).then(|| start.min(entries.len() - 1));
        self.entries = entries;
        self.source = source;
        self.current()
    }

    pub fn current(&self) -> Option<&Entry> {
        self.current.and_then(|i| self.entries.get(i))
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    /// The entry after the current one (to prepare ahead).
    pub fn peek_next(&self) -> Option<&Entry> {
        self.current.and_then(|i| self.entries.get(i + 1))
    }

    /// How many entries come after the current one.
    pub fn remaining(&self) -> usize {
        self.current
            .map_or(0, |i| self.entries.len().saturating_sub(i + 1))
    }

    /// Moves to the next entry. `None` (and nothing moves) at the end.
    pub fn advance(&mut self) -> Option<&Entry> {
        let next = self.current? + 1;
        if next >= self.entries.len() {
            return None;
        }
        self.current = Some(next);
        self.current()
    }

    /// Moves to the previous entry, or stays on the first.
    pub fn back(&mut self) -> Option<&Entry> {
        let i = self.current?;
        self.current = Some(i.saturating_sub(1));
        self.current()
    }

    /// Moves to the entry with this ID.
    pub fn jump(&mut self, id: u64) -> Option<&Entry> {
        let i = self.entries.iter().position(|e| e.id == id)?;
        self.current = Some(i);
        self.current()
    }

    /// Adds songs at the end (more of the radio or playlist). A song that
    /// is already coming up is not added twice.
    pub fn append(&mut self, tracks: Vec<Track>) -> usize {
        let start = self.current.unwrap_or(0);
        let mut added = 0;
        for track in tracks {
            let known = self.entries[start..]
                .iter()
                .any(|e| e.track.video_id == track.video_id);
            if !known {
                let entry = self.entry(track);
                self.entries.push(entry);
                added += 1;
            }
        }
        if self.current.is_none() && !self.entries.is_empty() {
            self.current = Some(0);
        }
        added
    }

    /// Puts a song right after the current one.
    pub fn play_next(&mut self, track: Track) {
        let entry = self.entry(track);
        match self.current {
            Some(i) => self.entries.insert(i + 1, entry),
            None => {
                self.entries.push(entry);
                self.current = Some(self.entries.len() - 1);
            }
        }
    }

    /// Puts a song at the end.
    pub fn add_to_end(&mut self, track: Track) {
        let entry = self.entry(track);
        self.entries.push(entry);
        if self.current.is_none() {
            self.current = Some(0);
        }
    }

    /// Puts the songs after the current one in a random order.
    pub fn shuffle_upcoming(&mut self) {
        use rand::seq::SliceRandom;
        let from = self.current.map_or(0, |i| i + 1);
        if from < self.entries.len() {
            self.entries[from..].shuffle(&mut rand::rng());
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = None;
        self.source = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ytfast_core::read::TrackKind;

    fn song(id: &str) -> Track {
        Track {
            video_id: id.into(),
            set_video_id: None,
            title: id.to_uppercase(),
            artists: String::new(),
            album: None,
            duration_seconds: Some(180),
            kind: TrackKind::Song,
            thumbnail: None,
        }
    }

    fn ids(q: &Queue) -> Vec<&str> {
        q.entries()
            .iter()
            .map(|e| e.track.video_id.as_str())
            .collect()
    }

    #[test]
    fn plays_from_the_chosen_song() {
        let mut q = Queue::default();
        let first = q
            .replace(vec![song("a"), song("b"), song("c")], 1, None)
            .unwrap();
        assert_eq!(first.track.video_id, "b");
        assert_eq!(q.peek_next().unwrap().track.video_id, "c");
        assert_eq!(q.remaining(), 1);
        assert_eq!(q.advance().unwrap().track.video_id, "c");
        assert!(q.advance().is_none());
        assert_eq!(
            q.current().unwrap().track.video_id,
            "c",
            "the end does not move"
        );
        assert_eq!(q.back().unwrap().track.video_id, "b");
    }

    #[test]
    fn the_same_song_twice_is_two_entries() {
        let mut q = Queue::default();
        q.replace(vec![song("a"), song("b"), song("a")], 0, None);
        let e = q.entries();
        assert_ne!(e[0].id, e[2].id);
        let last = e[2].id;
        assert_eq!(q.jump(last).unwrap().id, last);
        assert_eq!(q.current_index(), Some(2));
    }

    #[test]
    fn appending_skips_songs_already_coming_up() {
        let mut q = Queue::default();
        q.replace(vec![song("a"), song("b")], 0, None);
        let added = q.append(vec![song("b"), song("c"), song("d")]);
        assert_eq!(added, 2);
        assert_eq!(ids(&q), ["a", "b", "c", "d"]);
    }

    #[test]
    fn play_next_goes_right_after_the_current_song() {
        let mut q = Queue::default();
        q.replace(vec![song("a"), song("b")], 0, None);
        q.play_next(song("x"));
        q.add_to_end(song("z"));
        assert_eq!(ids(&q), ["a", "x", "b", "z"]);
        let mut empty = Queue::default();
        empty.play_next(song("solo"));
        assert_eq!(empty.current().unwrap().track.video_id, "solo");
    }

    #[test]
    fn an_empty_queue_has_nothing() {
        let mut q = Queue::default();
        assert!(q.replace(Vec::new(), 3, None).is_none());
        assert!(q.advance().is_none());
        assert!(q.back().is_none());
        assert_eq!(q.remaining(), 0);
    }

    #[test]
    fn shuffling_keeps_the_current_song_and_every_song() {
        let mut q = Queue::default();
        let songs: Vec<Track> = (0..30).map(|i| song(&format!("s{i}"))).collect();
        q.replace(songs, 4, None);
        q.shuffle_upcoming();
        // The songs up to the current one stay where they were.
        assert_eq!(&ids(&q)[..5], ["s0", "s1", "s2", "s3", "s4"]);
        assert_eq!(q.current().unwrap().track.video_id, "s4");
        let mut after: Vec<String> = ids(&q)[5..].iter().map(|s| s.to_string()).collect();
        after.sort();
        let mut expected: Vec<String> = (5..30).map(|i| format!("s{i}")).collect();
        expected.sort();
        assert_eq!(after, expected);
    }
}
