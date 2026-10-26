//! A song's genre in a few plain words ("J-Pop", "J-Rock", "Hip-Hop"),
//! from Musixmatch's genres for it, which are Apple's (iTunes') genres:
//! a song has a few, each with its parent ("West Coast Rap" under "Hip
//! Hop/Rap"). Radar groups songs by these families to tell listeners
//! which genres they play, and to mix what it suggests.
//!
//! A place or a scene comes first (J-Pop, K-Pop, Indian, Latin, Anime),
//! as a listener names it; then the song's first style (Rock, Hip-Hop,
//! Electronic...). A Japanese rock song is "J-Rock".

/// One of Musixmatch's genres for a song: its ID, its parent's ID (34,
/// "Music", for the main ones) and its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tag<'a> {
    pub id: u64,
    pub parent: u64,
    pub name: &'a str,
}

/// Apple's main genres by ID that keep their own name.
const NAMED: [(u64, &str); 16] = [
    (2, "Blues"),
    (4, "Children's"),
    (5, "Classical"),
    (6, "Country"),
    (9, "Classical"),
    (10, "Singer-Songwriter"),
    (11, "Jazz"),
    (13, "New Age"),
    (16, "Soundtrack"),
    (19, "World"),
    (22, "Christian"),
    (23, "Vocal"),
    (24, "Reggae"),
    (25, "Easy Listening"),
    (53, "Instrumental"),
    (1122, "Brazilian"),
];

/// Where a genre comes from, when it says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Anime,
    Japan,
    Korea,
    China,
    India,
    Latin,
}

/// What a genre sounds like, when it says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    Metal,
    HipHop,
    Rnb,
    Electronic,
    Rock,
    Pop,
    Named(&'static str),
}

fn is(tag: &Tag<'_>, ids: &[u64]) -> bool {
    ids.contains(&tag.id) || ids.contains(&tag.parent)
}

fn place(tag: &Tag<'_>) -> Option<Place> {
    let name = tag.name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if is(tag, &[29]) || has(&["anime"]) {
        Some(Place::Anime)
    } else if is(tag, &[27, 28, 30]) || has(&["j-pop", "j-rock", "japan", "enka", "city pop"]) {
        Some(Place::Japan)
    } else if is(tag, &[51]) || has(&["k-pop", "korea"]) {
        Some(Place::Korea)
    } else if has(&["mandopop", "cantopop", "c-pop", "chinese"]) {
        Some(Place::China)
    } else if is(tag, &[1262])
        || has(&[
            "indian",
            "bollywood",
            "punjabi",
            "tamil",
            "telugu",
            "bhangra",
            "desi",
        ])
    {
        Some(Place::India)
    } else if is(tag, &[12])
        || has(&[
            "latin",
            "reggaeton",
            "salsa",
            "bachata",
            "cumbia",
            "in spanish",
            "en español",
        ])
    {
        Some(Place::Latin)
    } else {
        None
    }
}

fn style(tag: &Tag<'_>) -> Option<Style> {
    let name = tag.name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if has(&["metal"]) {
        Some(Style::Metal)
    } else if is(tag, &[18]) || has(&["hip hop", "hip-hop", "rap", "trap", "drill"]) {
        Some(Style::HipHop)
    } else if is(tag, &[15]) || has(&["r&b", "soul", "funk"]) {
        Some(Style::Rnb)
    } else if is(tag, &[7, 17])
        || has(&[
            "electronic",
            "dance",
            "house",
            "techno",
            "trance",
            "edm",
            "dubstep",
            "drum & bass",
        ])
    {
        Some(Style::Electronic)
    } else if is(tag, &[20, 21]) || has(&["rock", "alternative", "punk", "grunge", "indie"]) {
        Some(Style::Rock)
    } else if let Some((_, named)) = NAMED.iter().find(|(id, _)| is(tag, &[*id])) {
        Some(Style::Named(named))
    } else if is(tag, &[14]) || has(&["pop"]) {
        Some(Style::Pop)
    } else {
        None
    }
}

/// The family of a song's genres (`primary` first, as Musixmatch lists
/// them, then `secondary`): its place or scene when one says (a Japanese
/// rock song is "J-Rock"), else its first style. `None` when they say
/// nothing (no genres, or only "Music").
pub fn family(primary: &[Tag<'_>], secondary: &[Tag<'_>]) -> Option<String> {
    let tags: Vec<&Tag<'_>> = primary.iter().chain(secondary).collect();
    let first_style = tags.iter().find_map(|tag| style(tag));
    let family = match tags.iter().find_map(|tag| place(tag)) {
        Some(Place::Anime) => "Anime",
        Some(Place::Japan) => match first_style {
            Some(Style::Rock | Style::Metal) => "J-Rock",
            Some(Style::HipHop) => "Hip-Hop",
            _ => "J-Pop",
        },
        Some(Place::Korea) => "K-Pop",
        Some(Place::China) => "C-Pop",
        Some(Place::India) => "Indian",
        Some(Place::Latin) => "Latin",
        None => match first_style? {
            Style::Metal => "Metal",
            Style::HipHop => "Hip-Hop",
            Style::Rnb => "R&B",
            Style::Electronic => "Electronic",
            Style::Rock => "Rock",
            Style::Pop => "Pop",
            Style::Named(name) => name,
        },
    };
    Some(family.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(id: u64, parent: u64, name: &str) -> Tag<'_> {
        Tag { id, parent, name }
    }

    /// What Musixmatch said for public songs (asked 27 October 2026), and
    /// the family each falls in.
    #[test]
    fn musixmatchs_genres_fall_in_their_families() {
        let cases: Vec<(&str, Vec<Tag<'_>>, Vec<Tag<'_>>, &str)> = vec![
            ("Lemon", vec![tag(27, 34, "J-Pop")], vec![], "J-Pop"),
            (
                "Kaikai Kitan",
                vec![tag(21, 34, "Rock")],
                vec![tag(27, 34, "J-Pop")],
                "J-Rock",
            ),
            (
                "Bling-Bang-Bang-Born",
                vec![tag(18, 34, "Hip Hop/Rap")],
                vec![],
                "Hip-Hop",
            ),
            (
                "Blinding Lights",
                vec![tag(7, 34, "Electronic")],
                vec![],
                "Electronic",
            ),
            (
                "Dynamite",
                vec![
                    tag(14, 34, "Pop"),
                    tag(51, 34, "K-Pop"),
                    tag(1137, 15, "Disco"),
                ],
                vec![],
                "K-Pop",
            ),
            (
                "HUMBLE.",
                vec![tag(1078, 18, "West Coast Rap"), tag(1076, 18, "Rap")],
                vec![tag(18, 34, "Hip Hop/Rap")],
                "Hip-Hop",
            ),
            (
                "Smells Like Teen Spirit",
                vec![tag(21, 34, "Rock")],
                vec![
                    tag(1144, 21, "Adult Alternative"),
                    tag(20, 34, "Alternative"),
                ],
                "Rock",
            ),
            ("Levels", vec![tag(17, 34, "Dance")], vec![], "Electronic"),
            (
                "Kesariya",
                vec![tag(1262, 34, "Indian")],
                vec![tag(1263, 1262, "Bollywood")],
                "Indian",
            ),
            (
                "Despacito",
                vec![
                    tag(1117, 12, "Pop in Spanish"),
                    tag(1119, 12, "Latin Urban"),
                    tag(14, 34, "Pop"),
                ],
                vec![],
                "Latin",
            ),
            ("Snooze", vec![tag(15, 34, "R&B/Soul")], vec![], "R&B"),
        ];
        for (song, primary, secondary, family_of) in cases {
            assert_eq!(
                family(&primary, &secondary).as_deref(),
                Some(family_of),
                "{song}"
            );
        }
    }

    #[test]
    fn scenes_and_plain_names() {
        assert_eq!(
            family(&[tag(29, 34, "Anime")], &[tag(27, 34, "J-Pop")]).as_deref(),
            Some("Anime")
        );
        assert_eq!(
            family(&[tag(1153, 21, "Metal")], &[]).as_deref(),
            Some("Metal")
        );
        assert_eq!(
            family(&[tag(16, 34, "Soundtrack")], &[]).as_deref(),
            Some("Soundtrack")
        );
        assert_eq!(family(&[tag(11, 34, "Jazz")], &[]).as_deref(), Some("Jazz"));
        // Nothing to go by.
        assert_eq!(family(&[], &[]), None);
        assert_eq!(family(&[tag(34, 0, "Music")], &[]), None);
    }
}
