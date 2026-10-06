//! Made-up music for `--demo`: the window with no account and no network,
//! for trying the interface and for screenshots. Every name here is
//! invented.
//!
//! A demo page's ID says what it is: `demo-album-Postcards`,
//! `demo-artist-Mara Sol`, `demo-playlist-Road trip`, `demo-mood-Chill`.

use ytfast_core::read::{
    Card, Header, Item, Page, PageKind, Section, Shape, Target, Thumb, Track, TrackKind,
};

use crate::backend::Route;

/// Title, artist, album.
const SONGS: [(&str, &str, &str); 24] = [
    ("Neon Harbour", "The Paper Kites Club", "Night Ferries"),
    ("Slow Motion Summer", "Ivy Lane", "Postcards"),
    ("Glass Hearts", "Mara Sol", "Glass Hearts"),
    ("Midnight Arcade", "Pixel Parade", "Insert Coin"),
    ("Low Tide", "Coastal Drive", "Low Tide"),
    ("Paper Planes Again", "June & The Kites", "Skylines"),
    ("Velvet Static", "Echo Room", "Signals"),
    ("Orange Streetlights", "Mara Sol", "Glass Hearts"),
    ("Faraway Radio", "Northern Static", "Frequencies"),
    ("Saturday Satellites", "Ivy Lane", "Postcards"),
    ("Honey & Thunder", "The Wild Orchards", "Harvest"),
    ("City of Small Lights", "Coastal Drive", "Low Tide"),
    ("Rewind the Rain", "Pixel Parade", "Insert Coin"),
    ("Afterglow Avenue", "Echo Room", "Signals"),
    ("Kites Over Lisbon", "June & The Kites", "Skylines"),
    ("Gold Hours", "The Wild Orchards", "Harvest"),
    ("Moonlit Motorway", "Northern Static", "Frequencies"),
    ("Lemon Skies", "Ivy Lane", "Postcards"),
    ("Daydream Engine", "Pixel Parade", "Insert Coin"),
    ("Blue Note Bicycle", "The Paper Kites Club", "Night Ferries"),
    ("Echoes in Amber", "Echo Room", "Signals"),
    ("Polaroid Summer", "Mara Sol", "Glass Hearts"),
    ("Harbour Lights", "Coastal Drive", "Low Tide"),
    ("Wildflower Static", "The Wild Orchards", "Harvest"),
];

fn seed(text: &str) -> u64 {
    // FNV-1a: a stable number from a string.
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn thumb(name: &str) -> Option<Thumb> {
    Some(Thumb {
        url: format!("demo://cover/{name}"),
        width: 544,
    })
}

/// The song's length in seconds, made up but always the same.
pub fn length(video_id: &str) -> f64 {
    150.0 + (seed(video_id) % 150) as f64
}

fn song(i: usize) -> Track {
    let (title, artist, album) = SONGS[i % SONGS.len()];
    let video_id = format!("demo{:07}", i % SONGS.len());
    Track {
        duration_seconds: Some(length(&video_id) as u32),
        video_id,
        set_video_id: None,
        title: title.into(),
        artists: artist.into(),
        album: Some(album.into()),
        kind: TrackKind::Song,
        thumbnail: thumb(album),
        ..Track::default()
    }
}

fn songs(from: usize, count: usize) -> Vec<Item> {
    (from..from + count).map(|i| Item::Track(song(i))).collect()
}

/// The songs whose artist or album is `name`.
fn songs_of(name: &str) -> Vec<Item> {
    (0..SONGS.len())
        .filter(|&i| SONGS[i].1 == name || SONGS[i].2 == name)
        .map(|i| Item::Track(song(i)))
        .collect()
}

fn id_for(title: &str, kind: PageKind) -> String {
    let tag = match kind {
        PageKind::Album => "album",
        PageKind::Artist => "artist",
        PageKind::Playlist => "playlist",
        _ => "mood",
    };
    format!("demo-{tag}-{title}")
}

fn card(title: &str, subtitle: &str, kind: PageKind) -> Item {
    let (id, playlist_id) = if title == "Liked Music" {
        ("VLLM".to_string(), "LM".to_string())
    } else {
        let id = id_for(title, kind);
        (id.clone(), id)
    };
    let playable = matches!(kind, PageKind::Album | PageKind::Playlist);
    Item::Card(Card {
        title: title.into(),
        subtitle: subtitle.into(),
        // Moods and genres are buttons, without a picture.
        thumbnail: if kind == PageKind::Other {
            None
        } else {
            thumb(title)
        },
        round: kind == PageKind::Artist,
        open: Some(Target::Browse {
            id,
            kind,
            params: None,
        }),
        play: playable.then_some(Target::Watch {
            video_id: None,
            playlist_id: Some(playlist_id),
        }),
    })
}

fn album_card(album: &str) -> Item {
    let artist = SONGS.iter().find(|s| s.2 == album).map_or("", |s| s.1);
    card(album, &format!("Album • {artist}"), PageKind::Album)
}

/// A list of songs, or a carousel of cards.
fn section(title: &str, items: Vec<Item>) -> Section {
    let cards = items.iter().any(|i| matches!(i, Item::Card(_)));
    let shape = if cards { Shape::Carousel } else { Shape::List };
    shaped(title, items, shape)
}

fn shaped(title: &str, items: Vec<Item>, shape: Shape) -> Section {
    Section {
        title: title.into(),
        items,
        more: None,
        shape,
    }
}

/// "12 songs • 41 minutes".
fn detail(items: &[Item]) -> String {
    let seconds: u32 = items
        .iter()
        .filter_map(|i| match i {
            Item::Track(t) => t.duration_seconds,
            Item::Card(_) => None,
        })
        .sum();
    format!("{} songs • {} minutes", items.len(), seconds.div_ceil(60))
}

pub fn page(route: &Route) -> Page {
    match route {
        Route::Home => home(),
        Route::Explore => explore(),
        Route::Library => Page {
            header: None,
            sections: vec![shaped(
                "Playlists",
                vec![
                    card("Liked Music", "Auto playlist", PageKind::Playlist),
                    card("Road trip", "Playlist • 14 songs", PageKind::Playlist),
                    card("Late night", "Playlist • 14 songs", PageKind::Playlist),
                    card("Gym", "Playlist • 14 songs", PageKind::Playlist),
                    card("Sunday morning", "Playlist • 14 songs", PageKind::Playlist),
                ],
                Shape::Grid,
            )],
        },
        Route::Liked => {
            let liked = songs(0, SONGS.len());
            Page {
                header: Some(Header {
                    title: "Liked Music".into(),
                    subtitle: "Auto playlist".into(),
                    detail: detail(&liked),
                    owner: "Demo listener".into(),
                    thumbnail: thumb("Liked Music"),
                    round: false,
                    ..Header::default()
                }),
                sections: vec![section("", liked)],
            }
        }
        Route::Browse { id, .. } => browse(id),
        Route::Search(query) => search(query),
    }
}

fn home() -> Page {
    Page {
        header: None,
        sections: vec![
            shaped("Quick picks", songs(0, 12), Shape::Carousel),
            section(
                "Listen again",
                vec![
                    album_card("Night Ferries"),
                    card("Mara Sol", "Artist", PageKind::Artist),
                    album_card("Postcards"),
                    album_card("Signals"),
                    card("Coastal Drive", "Artist", PageKind::Artist),
                    album_card("Insert Coin"),
                    album_card("Harvest"),
                ],
            ),
            section(
                "Mixed for you",
                vec![
                    card(
                        "My Supermix",
                        "Mix • Ivy Lane, Echo Room and more",
                        PageKind::Playlist,
                    ),
                    card("Discover Mix", "Mix • New to you", PageKind::Playlist),
                    card("Replay Mix", "Mix • Your favourites", PageKind::Playlist),
                    card(
                        "Chill Mix",
                        "Mix • Coastal Drive, Mara Sol",
                        PageKind::Playlist,
                    ),
                    card(
                        "Energy Mix",
                        "Mix • Pixel Parade, Northern Static",
                        PageKind::Playlist,
                    ),
                ],
            ),
            shaped("Trending", songs(12, 8), Shape::Carousel),
        ],
    }
}

fn explore() -> Page {
    Page {
        header: None,
        sections: vec![
            section(
                "New albums & singles",
                [
                    "Frequencies",
                    "Skylines",
                    "Glass Hearts",
                    "Low Tide",
                    "Harvest",
                ]
                .into_iter()
                .map(album_card)
                .collect(),
            ),
            section(
                "Moods & genres",
                [
                    "Chill",
                    "Focus",
                    "Workout",
                    "Party",
                    "Sleep",
                    "Romance",
                    "Feel good",
                    "Commute",
                ]
                .into_iter()
                .map(|m| card(m, "", PageKind::Other))
                .collect(),
            ),
            shaped("Trending", songs(14, 8), Shape::Carousel),
        ],
    }
}

fn browse(id: &str) -> Page {
    let rest = id.strip_prefix("demo-").unwrap_or(id);
    let (tag, name) = rest.split_once('-').unwrap_or(("playlist", rest));
    match tag {
        "album" => {
            let tracks = songs_of(name);
            let artist = SONGS.iter().find(|s| s.2 == name).map_or("", |s| s.1);
            Page {
                header: Some(Header {
                    title: name.into(),
                    subtitle: "Album • 2026".into(),
                    detail: detail(&tracks),
                    owner: artist.into(),
                    thumbnail: thumb(name),
                    round: false,
                    ..Header::default()
                }),
                sections: vec![
                    section("", tracks),
                    section(
                        "You might also like",
                        ["Signals", "Harvest", "Skylines", "Postcards"]
                            .into_iter()
                            .filter(|a| *a != name)
                            .map(album_card)
                            .collect(),
                    ),
                ],
            }
        }
        "artist" => {
            let mut albums: Vec<&str> = SONGS.iter().filter(|s| s.1 == name).map(|s| s.2).collect();
            albums.dedup();
            let listeners = 200 + seed(name) % 1800;
            Page {
                header: Some(Header {
                    title: name.into(),
                    subtitle: format!(
                        "{}.{}M monthly audience",
                        listeners / 1000,
                        listeners % 1000 / 100
                    ),
                    detail: String::new(),
                    owner: String::new(),
                    thumbnail: thumb(name),
                    round: true,
                    ..Header::default()
                }),
                sections: vec![
                    section("Top songs", songs_of(name)),
                    section("Albums", albums.into_iter().map(album_card).collect()),
                    section(
                        "Fans might also like",
                        ["Echo Room", "Ivy Lane", "Pixel Parade", "Northern Static"]
                            .into_iter()
                            .filter(|a| *a != name)
                            .map(|a| card(a, "Artist", PageKind::Artist))
                            .collect(),
                    ),
                ],
            }
        }
        "mood" => Page {
            header: Some(Header {
                title: name.into(),
                ..Header::default()
            }),
            sections: vec![shaped(
                &format!("{name} playlists"),
                ["Essentials", "Hits", "Deep cuts", "Fresh finds"]
                    .into_iter()
                    .map(|p| {
                        card(
                            &format!("{name} {p}"),
                            "Playlist • YouTube Music",
                            PageKind::Playlist,
                        )
                    })
                    .collect(),
                Shape::Grid,
            )],
        },
        _ => {
            let tracks = songs((seed(name) % 24) as usize, 14);
            let mix = name.ends_with("Mix");
            Page {
                header: Some(Header {
                    title: name.into(),
                    subtitle: if mix { "Mix" } else { "Playlist • 2026" }.into(),
                    detail: detail(&tracks),
                    owner: if mix {
                        "YouTube Music"
                    } else {
                        "Demo listener"
                    }
                    .into(),
                    thumbnail: thumb(name),
                    round: false,
                    ..Header::default()
                }),
                sections: vec![section("", tracks)],
            }
        }
    }
}

fn search(query: &str) -> Page {
    let query = query.to_lowercase();
    let mut found: Vec<usize> = (0..SONGS.len())
        .filter(|&i| {
            let (title, artist, album) = SONGS[i];
            [title, artist, album]
                .iter()
                .any(|s| s.to_lowercase().contains(&query))
        })
        .collect();
    if found.is_empty() {
        // Something, as YouTube Music always finds something.
        let start = (seed(&query) % 24) as usize;
        found = (start..start + 5).map(|i| i % SONGS.len()).collect();
    }
    let mut albums: Vec<&str> = Vec::new();
    let mut artists: Vec<&str> = Vec::new();
    for &i in &found {
        let (_, artist, album) = SONGS[i];
        if !albums.contains(&album) {
            albums.push(album);
        }
        if !artists.contains(&artist) {
            artists.push(artist);
        }
    }
    let mut sections = vec![section("Top result", vec![Item::Track(song(found[0]))])];
    if found.len() > 1 {
        let more = found[1..]
            .iter()
            .take(4)
            .map(|&i| Item::Track(song(i)))
            .collect();
        sections.push(section("Songs", more));
    }
    sections.push(shaped(
        "Albums",
        albums.into_iter().take(4).map(album_card).collect(),
        Shape::List,
    ));
    sections.push(shaped(
        "Artists",
        artists
            .into_iter()
            .take(4)
            .map(|a| card(a, "Artist", PageKind::Artist))
            .collect(),
        Shape::List,
    ));
    Page {
        header: None,
        sections,
    }
}

/// The songs a "play" button plays: an album's or playlist's own songs.
pub fn playlist_songs(playlist_id: &str) -> Vec<Track> {
    let route = if playlist_id == "LM" {
        Route::Liked
    } else {
        Route::Browse {
            id: playlist_id.to_string(),
            params: None,
        }
    };
    page(&route).tracks()
}

/// A made-up Up next list.
pub fn up_next(key: &str) -> Vec<Track> {
    let start = (seed(key) % 24) as usize;
    (start..start + 12).map(song).collect()
}

/// A made-up cover: two colours from the name, as a diagonal gradient.
pub fn cover(url: &str) -> egui::ColorImage {
    const SIDE: usize = 96;
    let s = seed(url);
    let colour = |shift: u32| {
        let hue = ((s >> shift) % 360) as f32;
        egui::ecolor::Hsva::new(hue / 360.0, 0.55, 0.75, 1.0)
    };
    let (a, b) = (colour(0), colour(20));
    let mut pixels = Vec::with_capacity(SIDE * SIDE);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let t = (x + y) as f32 / (2 * SIDE) as f32;
            let mix = egui::ecolor::Hsva::new(
                a.h + (b.h - a.h) * t,
                a.s + (b.s - a.s) * t,
                a.v + (b.v - a.v) * t,
                1.0,
            );
            pixels.push(egui::Color32::from(mix));
        }
    }
    egui::ColorImage::new([SIDE, SIDE], pixels)
}

/// Made-up, time-synced lyrics.
pub fn lyrics(video_id: &str) -> crate::lyrics::Lyrics {
    const VERSES: [&str; 12] = [
        "Streetlights hum a song we used to know",
        "Every window glowing soft and slow",
        "We ran the harbour roads till morning came",
        "And every echo seemed to call your name",
        "Hold on, hold on, the night is young",
        "A thousand lights and only one",
        "We keep the radio low, we keep it near",
        "The city sings the things we never hear",
        "Paper planes above the sleeping town",
        "We never learned the way to settle down",
        "So turn it up, the chorus coming through",
        "Every road I take comes back to you",
    ];
    let seed = seed(video_id) as usize;
    let length = length(video_id);
    let mut lines = Vec::new();
    let mut at = 6.0;
    let mut i = 0;
    while at < length - 8.0 {
        let text = if i % 9 == 8 {
            "♪".to_string()
        } else {
            VERSES[(seed + i) % VERSES.len()].to_string()
        };
        lines.push(crate::lyrics::Line {
            start: Some(at),
            text,
        });
        at += 3.5 + ((seed + i * 7) % 5) as f64 * 0.6;
        i += 1;
    }
    crate::lyrics::Lyrics {
        lines,
        synced: true,
        source: "Demo".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_demo_page_has_something() {
        let mut routes = vec![
            Route::Home,
            Route::Explore,
            Route::Library,
            Route::Liked,
            Route::Search("glass".into()),
        ];
        // Every card on those pages opens a page with something on it.
        for route in routes.clone() {
            for section in page(&route).sections {
                for item in section.items {
                    if let Item::Card(Card {
                        open: Some(Target::Browse { id, params, .. }),
                        ..
                    }) = item
                    {
                        routes.push(Route::browse(id, params));
                    }
                }
            }
        }
        for route in routes {
            assert!(!page(&route).is_empty(), "{route:?} is empty");
        }
    }

    #[test]
    fn play_buttons_play_the_page_songs() {
        assert_eq!(playlist_songs("LM").len(), SONGS.len());
        let album = playlist_songs("demo-album-Postcards");
        assert_eq!(album.len(), 3);
        assert!(
            album
                .iter()
                .all(|t| t.album.as_deref() == Some("Postcards"))
        );
    }
}
