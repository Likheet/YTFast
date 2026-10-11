//! Made-up music for `--demo`: the window with no account and no network,
//! for trying the interface and for screenshots. Every name here is
//! invented.
//!
//! A demo page's ID says what it is: `demo-album-Postcards`,
//! `demo-artist-Mara Sol`, `demo-playlist-Road trip`, `demo-mood-Chill`.

use ytfast_core::library::LibraryTab;
use ytfast_core::read::{
    Card, CardButton, CardLook, Header, Item, Page, PageKind, Section, Shape, SortMenu, SortOrder,
    Target, Thumb, TopResult, Track, TrackKind, TrackMore,
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

/// The made-up account's past searches, newest first (lowercase, as
/// YouTube keeps them).
const PAST_SEARCHES: [&str; 5] = [
    "mara sol",
    "neon harbour",
    "road trip songs",
    "echo room signals",
    "lemon skies",
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

/// Whether a made-up song has a (made-up) music video: most do, about a
/// quarter not, as with real songs.
pub fn has_video(video_id: &str) -> bool {
    seed(video_id) % 4 != 3
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
        artist_id: Some(id_for(artist, PageKind::Artist)),
        album_id: Some(id_for(album, PageKind::Album)),
        playable: true,
        // Some made-up songs are explicit, as real ones are.
        more: (i % 5 == 1).then(|| {
            Box::new(TrackMore {
                counterpart: None,
                segments: Vec::new(),
                explicit: true,
                ..TrackMore::default()
            })
        }),
    }
}

fn songs(from: usize, count: usize) -> Vec<Item> {
    (from..from + count).map(|i| Item::Track(song(i))).collect()
}

/// A chart's songs: numbered, with their views, as Explore's Trending.
fn chart(from: usize, count: usize) -> Vec<Item> {
    (0..count)
        .map(|place| {
            let mut track = song(from + place);
            let views = 40 - 3 * place;
            track.album = None;
            track.more = Some(Box::new(TrackMore {
                counterpart: None,
                segments: Vec::new(),
                rank: Some((place + 1).to_string()),
                count: Some(format!("{views}M views")),
                explicit: false,
            }));
            Item::Track(track)
        })
        .collect()
}

/// A music video's card: a wide picture, the artist and the views.
fn video(i: usize) -> Item {
    let track = song(i);
    Item::Card(Card {
        subtitle: format!("{} \u{2022} {}M views", track.artists, 2 + i % 9),
        title: track.title.clone(),
        thumbnail: track.thumbnail.clone(),
        round: false,
        open: None,
        play: Some(Target::Watch {
            video_id: Some(track.video_id),
            playlist_id: None,
        }),
        podcast: None,
        look: CardLook {
            wide: true,
            ..CardLook::default()
        },
    })
}

/// A card without a picture (a mood, a button), with how it looks.
fn plain(title: &str, look: CardLook) -> Item {
    let Item::Card(mut card) = card(title, "", PageKind::Other) else {
        unreachable!("a card")
    };
    card.look = look;
    Item::Card(card)
}

/// The moods, each with its own colour (YouTube Music's own for these).
const MOODS: [(&str, u32); 8] = [
    ("Chill", 0xa4c5ff),
    ("Commute", 0xffc200),
    ("Energize", 0xffe780),
    ("Feel good", 0xfdf28f),
    ("Focus", 0x77cdf6),
    ("Party", 0xff5ac6),
    ("Romance", 0xf34a7b),
    ("Sleep", 0x2d3a7a),
];

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
        // A playlist's page is its ID after `VL`, as on YouTube Music.
        let page = if kind == PageKind::Playlist {
            format!("VL{id}")
        } else {
            id.clone()
        };
        (page, id)
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
        look: CardLook::default(),
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
        podcast: None,
    })
}

fn album_card(album: &str) -> Item {
    let artist = SONGS.iter().find(|s| s.2 == album).map_or("", |s| s.1);
    let Item::Card(mut card) = card(album, &format!("Album • {artist}"), PageKind::Album) else {
        unreachable!("a card")
    };
    // Two made-up albums are explicit.
    card.look.explicit = matches!(album, "Night Ferries" | "Insert Coin");
    Item::Card(card)
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
        shape,
        ..Section::default()
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

/// A page of made-up music; a Library tab with its sort button, in the
/// order `order` asks for (a demo order's `params`).
pub fn page(route: &Route, order: Option<&str>) -> Page {
    let mut page = unsorted(route);
    if let Some(tab) = route.library_tab() {
        sort(&mut page, tab, order);
    }
    page
}

/// A Library tab's sort button, as YouTube Music's (the front page's
/// orders, else the other tabs'), and its items in the order chosen: A to
/// Z and Z to A by title, "Recently played" the other way round.
fn sort(page: &mut Page, tab: LibraryTab, order: Option<&str>) {
    let titles: &[(&str, &str)] = if tab == LibraryTab::Recent {
        &[
            ("Recent activity", "demo:recent"),
            ("Recently saved", "demo:saved"),
            ("Recently played", "demo:played"),
        ]
    } else {
        &[
            ("Recently saved", "demo:saved"),
            ("A to Z", "demo:az"),
            ("Z to A", "demo:za"),
        ]
    };
    let chosen = titles
        .iter()
        .find(|(_, params)| Some(*params) == order)
        .unwrap_or(&titles[0]);
    let title = |item: &Item| match item {
        Item::Track(t) => t.title.to_lowercase(),
        Item::Card(c) => c.title.to_lowercase(),
    };
    if let Some(list) = page.sections.last_mut() {
        match chosen.1 {
            "demo:az" => list.items.sort_by_key(title),
            "demo:za" => {
                list.items.sort_by_key(title);
                list.items.reverse();
            }
            "demo:played" => list.items.reverse(),
            _ => {}
        }
    }
    page.sort = Some(SortMenu {
        chosen: chosen.0.to_string(),
        orders: titles
            .iter()
            .map(|(title, params)| SortOrder {
                title: (*title).to_string(),
                browse_id: tab.browse_id().to_string(),
                params: (*params).to_string(),
            })
            .collect(),
    });
}

fn unsorted(route: &Route) -> Page {
    match route {
        Route::Home => home(),
        Route::Explore => explore(),
        Route::Library => Page {
            sort: None,
            header: None,
            sections: vec![shaped(
                "",
                vec![
                    card("Liked Music", "Auto playlist", PageKind::Playlist),
                    // Whose each is, then how long, as the Library shows
                    // them.
                    card("Road trip", "Demo listener • 14 songs", PageKind::Playlist),
                    card("Late night", "Demo listener • 14 songs", PageKind::Playlist),
                    card("Gym", "Demo listener • 14 songs", PageKind::Playlist),
                    card(
                        "Sunday morning",
                        "Demo listener • 14 songs",
                        PageKind::Playlist,
                    ),
                ],
                Shape::Grid,
            )],
        },
        // The front page: everything, most recently used first.
        Route::LibraryRecent => Page {
            sort: None,
            header: None,
            sections: vec![shaped(
                "",
                vec![
                    card("Liked Music", "Auto playlist", PageKind::Playlist),
                    card(
                        "Road trip",
                        "Playlist • Demo listener • 14 tracks",
                        PageKind::Playlist,
                    ),
                    album_card("Night Ferries"),
                    card("Mara Sol", "Artist • 1.2M subscribers", PageKind::Artist),
                    card(
                        "Late night",
                        "Playlist • Demo listener • 14 tracks",
                        PageKind::Playlist,
                    ),
                    album_card("Postcards"),
                    card("Ivy Lane", "Artist • 850K subscribers", PageKind::Artist),
                    card(
                        "Gym",
                        "Playlist • Demo listener • 14 tracks",
                        PageKind::Playlist,
                    ),
                    album_card("Glass Hearts"),
                    card(
                        "Sunday morning",
                        "Playlist • Demo listener • 14 tracks",
                        PageKind::Playlist,
                    ),
                ],
                Shape::Grid,
            )],
        },
        Route::Liked => {
            let liked = songs(0, SONGS.len());
            Page {
                sort: None,
                header: Some(Header {
                    title: "Liked Music".into(),
                    subtitle: "Auto playlist".into(),
                    detail: detail(&liked),
                    owner: "Demo listener".into(),
                    thumbnail: thumb("Liked Music"),
                    round: false,
                    ..Header::default()
                }),
                sections: vec![
                    // Liked Music's own filters, as YouTube Music has them.
                    shaped(
                        "",
                        [
                            "Pop",
                            "Indie",
                            "Chill",
                            "Feel good",
                            "Rock",
                            "Energize",
                            "Focus",
                        ]
                        .into_iter()
                        .map(|m| plain(m, CardLook::default()))
                        .collect(),
                        Shape::Grid,
                    ),
                    section("", liked),
                ],
            }
        }
        Route::Browse { id, .. } => browse(id),
        Route::Search(query) => search(query),
        Route::SearchOnly(query, params) => search_only(query, params),
        Route::LibrarySongs | Route::History => Page {
            sort: None,
            header: None,
            sections: vec![section("", songs(3, 16))],
        },
        Route::LibraryAlbums => Page {
            sort: None,
            header: None,
            sections: vec![shaped(
                "",
                [
                    "Night Ferries",
                    "Postcards",
                    "Glass Hearts",
                    "Signals",
                    "Harvest",
                ]
                .into_iter()
                .map(album_card)
                .collect(),
                Shape::Grid,
            )],
        },
        Route::LibraryArtists => Page {
            sort: None,
            header: None,
            sections: vec![shaped(
                "",
                ["Mara Sol", "Ivy Lane", "Echo Room", "Coastal Drive"]
                    .into_iter()
                    .map(|a| card(a, "Artist", PageKind::Artist))
                    .collect(),
                Shape::Grid,
            )],
        },
        // People followed: rows with a round picture, as the artists.
        Route::LibraryProfiles => Page {
            sort: None,
            header: None,
            sections: vec![shaped(
                "",
                ["Demo friend", "Weekend DJ"]
                    .into_iter()
                    .map(|p| card(p, "Profile", PageKind::Artist))
                    .collect(),
                Shape::Grid,
            )],
        },
        Route::LibraryPodcasts => Page {
            sort: None,
            header: None,
            sections: vec![shaped(
                "",
                vec![
                    card("Studio Stories", "Podcast • Demo radio", PageKind::Playlist),
                    card("Liner Notes", "Podcast • Demo radio", PageKind::Playlist),
                ],
                Shape::Grid,
            )],
        },
        Route::Settings => Page::default(),
    }
}

fn home() -> Page {
    let chips = [
        "Energize",
        "Relax",
        "Workout",
        "Commute",
        "Focus",
        "Feel good",
        "Party",
    ]
    .into_iter()
    .map(|m| plain(m, CardLook::default()))
    .collect();
    Page {
        sort: None,
        header: None,
        sections: vec![
            shaped("", chips, Shape::Grid),
            shaped("Quick picks", songs(0, 12), Shape::Carousel),
            // As a signed-in Home's: the listener's name above the title,
            // their photo before it.
            Section {
                strapline: "Demo listener".into(),
                picture: thumb("Demo listener"),
                round_picture: true,
                ..section(
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
                )
            },
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
    let button = |title: &str, icon: &str| {
        plain(
            title,
            CardLook {
                icon: Some(icon.into()),
                ..CardLook::default()
            },
        )
    };
    let buttons = vec![
        button("New releases", "MUSIC_NEW_RELEASE"),
        button("Charts", "TRENDING_UP"),
        button("Moods & genres", "STICKER_EMOTICON"),
    ];
    Page {
        sort: None,
        header: None,
        sections: vec![
            shaped("", buttons, Shape::Grid),
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
                MOODS
                    .iter()
                    .map(|(m, colour)| {
                        plain(
                            m,
                            CardLook {
                                stripe: Some(*colour),
                                ..CardLook::default()
                            },
                        )
                    })
                    .collect(),
            ),
            shaped("Trending", chart(14, 8), Shape::Carousel),
            section("New music videos", (3..9).map(video).collect()),
        ],
    }
}

fn browse(id: &str) -> Page {
    let id = id.strip_prefix("VL").unwrap_or(id);
    let rest = id.strip_prefix("demo-").unwrap_or(id);
    let (tag, name) = rest.split_once('-').unwrap_or(("playlist", rest));
    match tag {
        "album" => {
            // An album's rows say how often each song was played.
            let mut tracks = songs_of(name);
            for (row, item) in tracks.iter_mut().enumerate() {
                if let Item::Track(track) = item {
                    let plays = 9 + seed(&track.video_id) % 40 - row as u64;
                    track.more = Some(Box::new(TrackMore {
                        counterpart: None,
                        segments: Vec::new(),
                        rank: None,
                        count: Some(format!("{plays}M plays")),
                        explicit: false,
                    }));
                }
            }
            let artist = SONGS.iter().find(|s| s.2 == name).map_or("", |s| s.1);
            Page {
                sort: None,
                header: Some(Header {
                    title: name.into(),
                    subtitle: "Album • 2026".into(),
                    detail: detail(&tracks),
                    owner: artist.into(),
                    thumbnail: thumb(name),
                    round: false,
                    // Its own songs, when queued from its menu.
                    library_id: Some(format!("demo-album-{name}")),
                    saved: Some(false),
                    description: format!(
                        "{name} is the second album by {artist}, recorded over one summer in a \
                         borrowed room by the sea. Its songs were written on the road and \
                         finished in a week, with friends playing whatever was at hand."
                    ),
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
            // Top songs as YouTube sends them: plays, no length, and
            // "Show all" (here, their first album).
            let mut top = songs_of(name);
            for item in &mut top {
                if let Item::Track(track) = item {
                    track.duration_seconds = None;
                    track.more = Some(Box::new(TrackMore {
                        counterpart: None,
                        segments: Vec::new(),
                        rank: None,
                        count: Some(format!("{}M plays", 20 + seed(&track.video_id) % 900)),
                        explicit: false,
                    }));
                }
            }
            let mut top = section("Top songs", top);
            top.more = albums.first().map(|album| Target::Browse {
                id: id_for(album, PageKind::Album),
                kind: PageKind::Album,
                params: None,
            });
            // An artist's shelves have YouTube's smaller titles.
            let small = |mut section: Section| {
                section.small_title = true;
                section
            };
            Page {
                sort: None,
                header: Some(Header {
                    title: name.into(),
                    subtitle: monthly_audience(name),
                    detail: String::new(),
                    owner: String::new(),
                    description: format!(
                        "{name} began as a bedroom project and grew into a band that tours \
                         every summer. Their songs mix bright guitars with quiet, close \
                         singing, and most of them were first played live before they were \
                         recorded. They have made records with friends from the same city, \
                         and still answer letters from listeners themselves."
                    ),
                    subscribers: format!("{}.{}M", 1 + seed(name) % 9, seed(name) % 10),
                    thumbnail: thumb(name),
                    round: true,
                    channel_id: Some(format!("UCdemo{}", seed(name))),
                    ..Header::default()
                }),
                sections: vec![
                    top,
                    small(section(
                        "Albums",
                        albums.into_iter().map(album_card).collect(),
                    )),
                    small(section(
                        "Fans might also like",
                        ["Echo Room", "Ivy Lane", "Pixel Parade", "Northern Static"]
                            .into_iter()
                            .filter(|a| *a != name)
                            .map(|a| card(a, "Artist", PageKind::Artist))
                            .collect(),
                    )),
                ],
            }
        }
        // As YouTube Music's page: sections of striped buttons in grids.
        "mood" if name == "Moods & genres" => {
            let striped = |names: &[(&str, u32)]| {
                names
                    .iter()
                    .map(|(m, colour)| {
                        plain(
                            m,
                            CardLook {
                                stripe: Some(*colour),
                                ..CardLook::default()
                            },
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let genres = [
                ("Rock", 0xcccc00),
                ("Pop", 0xff5ac6),
                ("Hip-hop", 0xff8a00),
                ("Indie & alternative", 0x7cc0ff),
                ("Jazz", 0xb388ff),
                ("Classical", 0xe0e0e0),
                ("Electronic", 0x00e5b4),
                ("Folk & acoustic", 0xa1887f),
                ("R&B & soul", 0xf34a7b),
                ("Country", 0xffd54f),
            ];
            Page {
                sort: None,
                header: Some(Header {
                    title: name.into(),
                    ..Header::default()
                }),
                // Grids' titles are the second size, as the reader marks
                // them.
                sections: [
                    shaped("For you", striped(&MOODS[..5]), Shape::Grid),
                    shaped("Moods & moments", striped(&MOODS), Shape::Grid),
                    shaped("Genres", striped(&genres), Shape::Grid),
                ]
                .into_iter()
                .map(|section| Section {
                    small_title: true,
                    ..section
                })
                .collect(),
            }
        }
        "mood" => Page {
            sort: None,
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
            let mut tracks = songs((seed(name) % 24) as usize, 14);
            let mix = name.ends_with("Mix");
            if !mix {
                // The listener's own: its rows can be taken out.
                for (row, item) in tracks.iter_mut().enumerate() {
                    if let Item::Track(track) = item {
                        track.set_video_id = Some(format!("demorow{row}"));
                        // One song YouTube no longer offers, greyed out.
                        track.playable = row != 6;
                    }
                }
            }
            Page {
                sort: None,
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
                    editable: !mix,
                    description: if mix {
                        String::new()
                    } else {
                        "Songs for the long way round.".into()
                    },
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
    // As YouTube Music's search: the best match's artist as a big card,
    // with three of their songs, then everything else in one mixed list.
    let best = artists[0];
    let mut top: Vec<Item> = vec![card(best, "Artist", PageKind::Artist)];
    top.extend(songs_of(best).into_iter().take(3));
    let first_song = match top.get(1) {
        Some(Item::Track(track)) => Some(track.video_id.clone()),
        _ => None,
    };
    let button = |text: &str, icon: &str, filled: bool| CardButton {
        text: text.into(),
        icon: Some(icon.into()),
        filled,
        target: first_song.clone().map(|id| Target::Watch {
            video_id: Some(id),
            playlist_id: None,
        }),
    };
    let top = Section {
        items: top,
        top: Some(Box::new(TopResult {
            subtitle: format!("Artist \u{2022} {}", monthly_audience(best)),
            buttons: vec![
                button("Shuffle", "MUSIC_SHUFFLE", true),
                button("Mix", "MIX", false),
            ],
        })),
        ..Section::default()
    };
    let mut songs = found.iter().take(6).map(|&i| Item::Track(song(i)));
    let mut albums = albums.into_iter().take(4).map(album_card);
    let mut artists = artists
        .into_iter()
        .skip(1)
        .take(3)
        .map(|a| card(a, "Artist", PageKind::Artist));
    let mut rest: Vec<Item> = Vec::new();
    loop {
        let before = rest.len();
        rest.extend(songs.next());
        rest.extend(songs.next());
        rest.extend(albums.next());
        rest.extend(artists.next());
        if rest.len() == before {
            break;
        }
    }
    Page {
        sort: None,
        header: None,
        sections: vec![
            search_chips(&query, None),
            top,
            shaped("", rest, Shape::List),
        ],
    }
}

/// The kinds a search can be narrowed to, as YouTube Music's buttons.
const SEARCH_KINDS: [&str; 5] = [
    "Songs",
    "Albums",
    "Artists",
    "Community playlists",
    "Featured playlists",
];

/// Search's filter buttons, `chosen` marked; each searches with `demo:`
/// and its name.
fn search_chips(query: &str, chosen: Option<&str>) -> Section {
    let chips = SEARCH_KINDS
        .into_iter()
        .map(|name| {
            plain(
                name,
                CardLook {
                    chosen: chosen == Some(name),
                    ..CardLook::default()
                },
            )
        })
        .map(|item| match item {
            Item::Card(mut card) => {
                card.open = Some(Target::Search {
                    query: query.to_string(),
                    params: Some(format!("demo:{}", card.title)),
                });
                Item::Card(card)
            }
            track => track,
        })
        .collect();
    shaped("", chips, Shape::Grid)
}

/// What the search box suggests for `text`: names that hold it, then (as
/// YouTube Music) an artist and a few songs, with their pictures.
pub fn suggestions(text: &str) -> ytfast_core::read::Suggestions {
    use ytfast_core::read::SuggestedWords;
    let lower = text.to_lowercase();
    let holds = |s: &str| s.to_lowercase().contains(&lower);
    // Made-up past searches: all of them with nothing typed, else those
    // beginning as typed, first (as YouTube Music lists them).
    let mut words: Vec<SuggestedWords> = PAST_SEARCHES
        .iter()
        .filter(|past| past.starts_with(&lower))
        .map(|past| SuggestedWords {
            text: (*past).to_string(),
            forget: Some(format!("demo:{past}")),
        })
        .collect();
    if text.is_empty() {
        return ytfast_core::read::Suggestions {
            words,
            items: Vec::new(),
        };
    }
    for (title, artist, album) in SONGS {
        for name in [title, artist, album] {
            if holds(name) && !words.iter().any(|w| w.text == name.to_lowercase()) {
                words.push(SuggestedWords {
                    text: name.to_lowercase(),
                    forget: None,
                });
            }
        }
    }
    words.truncate(6);
    let mut items = Vec::new();
    if let Some((_, artist, _)) = SONGS.iter().find(|s| holds(s.1)) {
        let Item::Card(mut card) = card(artist, "", PageKind::Artist) else {
            unreachable!("a card")
        };
        card.subtitle = monthly_audience(artist);
        items.push(Item::Card(card));
    }
    for i in (0..SONGS.len())
        .filter(|&i| holds(SONGS[i].0) || holds(SONGS[i].1))
        .take(3)
    {
        let mut track = song(i);
        track.more = Some(Box::new(TrackMore {
            counterpart: None,
            segments: Vec::new(),
            rank: None,
            count: Some(format!("{}M plays", 20 + seed(&track.video_id) % 900)),
            explicit: false,
        }));
        items.push(Item::Track(track));
    }
    ytfast_core::read::Suggestions { words, items }
}

/// "1.6M monthly audience", the same for an artist everywhere.
fn monthly_audience(name: &str) -> String {
    let listeners = 200 + seed(name) % 1800;
    format!(
        "{}.{}M monthly audience",
        listeners / 1000,
        listeners % 1000 / 100
    )
}

/// Search with one kind of result: its section alone, as the real
/// filtered search shows it.
/// The next results of a search of one kind: more made-up songs (or
/// cards), once.
/// Home's next shelves, as a signed-in Home has several batches of them:
/// one batch here.
pub fn more_shelves(route: &Route) -> Vec<Section> {
    if *route != Route::Home {
        return Vec::new();
    }
    vec![
        section(
            "Albums for you",
            vec![
                album_card("Harvest"),
                album_card("Postcards"),
                album_card("Insert Coin"),
                album_card("Signals"),
                album_card("Night Ferries"),
            ],
        ),
        shaped("Forgotten favourites", songs(4, 8), Shape::Carousel),
        section(
            "From the community",
            vec![
                card(
                    "Late night drive",
                    "Playlist • Demo listener",
                    PageKind::Playlist,
                ),
                card(
                    "Sunday slow",
                    "Playlist • Demo listener",
                    PageKind::Playlist,
                ),
                card(
                    "Coastal summer",
                    "Playlist • Demo listener",
                    PageKind::Playlist,
                ),
            ],
        ),
    ]
}

pub fn more_results(query: &str, params: &str) -> Vec<Item> {
    let kind = params.strip_prefix("demo:").unwrap_or(params);
    if kind == "Songs" {
        let shown: Vec<String> = search_only(query, params)
            .tracks()
            .into_iter()
            .map(|t| t.video_id)
            .collect();
        (0..SONGS.len())
            .map(song)
            .filter(|t| !shown.contains(&t.video_id))
            .take(10)
            .map(Item::Track)
            .collect()
    } else {
        Vec::new()
    }
}

fn search_only(query: &str, params: &str) -> Page {
    let kind = params.strip_prefix("demo:").unwrap_or(params);
    let all = search(query);
    let mut sections = match kind {
        // Every song found, top result included, in one list.
        "Songs" => {
            let songs = all
                .sections
                .iter()
                .flat_map(|s| s.items.iter())
                .filter(|i| matches!(i, Item::Track(_)))
                .cloned()
                .collect();
            vec![section("", songs)]
        }
        "Albums" | "Artists" => {
            let wanted = if kind == "Albums" {
                PageKind::Album
            } else {
                PageKind::Artist
            };
            let cards = all
                .sections
                .iter()
                .flat_map(|s| s.items.iter())
                .filter(|i| match i {
                    Item::Card(Card {
                        open: Some(Target::Browse { kind, .. }),
                        ..
                    }) => *kind == wanted,
                    _ => false,
                })
                .cloned()
                .collect();
            vec![shaped("", cards, Shape::List)]
        }
        _ => vec![shaped(
            "",
            vec![card(
                &format!("{query} mix"),
                "Playlist • YouTube Music",
                PageKind::Playlist,
            )],
            Shape::List,
        )],
    };
    // Headed with its kind, as YouTube Music's.
    if let Some(results) = sections.first_mut() {
        results.title = kind.to_string();
    }
    sections.insert(0, search_chips(query, Some(kind)));
    Page {
        sort: None,
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
    page(&route, None).tracks()
}

/// A made-up Up next list.
/// What the player page's Related tab shows for a song, in YouTube Music's
/// order: songs like it, playlists, artists like its, and more from its
/// artist.
pub fn related(video_id: &str) -> Page {
    let at = (seed(video_id) % 24) as usize;
    let (_, artist, _) = SONGS[at % SONGS.len()];
    let mut albums: Vec<&str> = SONGS
        .iter()
        .filter(|s| s.1 == artist)
        .map(|s| s.2)
        .collect();
    albums.dedup();
    let likes: Vec<Item> = (at + 1..at + 13).map(|i| Item::Track(song(i))).collect();
    Page {
        sort: None,
        header: None,
        sections: vec![
            shaped("You might also like", likes, Shape::Carousel),
            section(
                "Recommended playlists",
                ["Road trip", "Late night", "Sunday morning"]
                    .into_iter()
                    .map(|p| card(p, "Playlist • YouTube Music", PageKind::Playlist))
                    .collect(),
            ),
            section(
                "Similar artists",
                ["Echo Room", "Ivy Lane", "Pixel Parade", "Northern Static"]
                    .into_iter()
                    .filter(|a| *a != artist)
                    .map(|a| card(a, "Artist", PageKind::Artist))
                    .collect(),
            ),
            section(
                &format!("More from {artist}"),
                albums.into_iter().map(album_card).collect(),
            ),
        ],
    }
}

/// More songs for a queue, and (a radio) what they play from: "<song>
/// Mix", as YouTube Music names a song's radio.
pub fn up_next(key: &str) -> (Vec<Track>, Option<String>) {
    let start = (seed(key) % 24) as usize;
    let title = SONGS
        .iter()
        .enumerate()
        .find(|(i, _)| format!("demo{i:07}") == key)
        .map(|(_, s)| format!("{} Mix", s.0));
    ((start..start + 12).map(song).collect(), title)
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

/// Made-up lines in Japanese, in Latin letters and in English, for the
/// songs sung in Japanese ([`sung_in_japanese`]).
const JAPANESE: [(&str, &str, &str); 8] = [
    (
        "夜の街に光がともる",
        "Yoru no machi ni hikari ga tomoru",
        "The lights come on in the night town",
    ),
    (
        "君の声が聞こえる",
        "Kimi no koe ga kikoeru",
        "I can hear your voice",
    ),
    (
        "風に乗って歌おう",
        "Kaze ni notte utaou",
        "Let's sing, riding the wind",
    ),
    (
        "星空の下で踊ろう",
        "Hoshizora no shita de odorou",
        "Let's dance under the starry sky",
    ),
    (
        "明日へ走り出す",
        "Ashita e hashiridasu",
        "Running toward tomorrow",
    ),
    (
        "忘れないでこの夜",
        "Wasurenaide kono yoru",
        "Don't forget this night",
    ),
    (
        "ずっとそばにいて",
        "Zutto soba ni ite",
        "Stay by my side forever",
    ),
    (
        "心が叫んでる",
        "Kokoro ga sakenderu",
        "My heart is crying out",
    ),
];

/// Whether a made-up song is sung in Japanese: about one in five.
pub fn sung_in_japanese(video_id: &str) -> bool {
    seed(video_id) % 5 == 1
}

/// The made-up lyrics translated: the Japanese ones into English and Latin
/// letters; the English ones need neither.
pub fn translation(lines: &[String]) -> ytfast_core::translate::Translated {
    let find = |line: &String| JAPANESE.iter().find(|(words, _, _)| words == line);
    let japanese = lines.iter().any(|line| find(line).is_some());
    ytfast_core::translate::Translated {
        language: Some(if japanese { "ja" } else { "en" }.into()),
        lines: lines
            .iter()
            .map(|line| find(line).map(|(_, _, english)| english.to_string()))
            .collect(),
        latin: lines
            .iter()
            .map(|line| find(line).map(|(_, latin, _)| latin.to_string()))
            .collect(),
        source: "Demo".into(),
    }
}

/// Made-up, time-synced lyrics, their words timed as real lyrics timed
/// by the line are (estimated).
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
    let japanese = sung_in_japanese(video_id);
    let seed = seed(video_id) as usize;
    let length = length(video_id);
    let mut lines: Vec<ytfast_core::lyrics::LyricLine> = Vec::new();
    let mut at = 6.0;
    let mut i = 0;
    while at < length - 8.0 {
        let text = if i % 9 == 8 {
            "♪".to_string()
        } else if japanese {
            JAPANESE[(seed + i) % JAPANESE.len()].0.to_string()
        } else {
            VERSES[(seed + i) % VERSES.len()].to_string()
        };
        let start_ms = Some((at * 1000.0) as u64);
        if let Some(before) = lines.last_mut() {
            before.end_ms = start_ms;
        }
        lines.push(ytfast_core::lyrics::LyricLine {
            start_ms,
            text,
            ..Default::default()
        });
        at += 3.5 + ((seed + i * 7) % 5) as f64 * 0.6;
        i += 1;
    }
    crate::lyrics::Lyrics::from(ytfast_core::lyrics::Lyrics {
        lines,
        synced: true,
        source: "Demo".into(),
        ..Default::default()
    })
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
            for section in page(&route, None).sections {
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
            assert!(!page(&route, None).is_empty(), "{route:?} is empty");
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
        // An album's menu queues the album's own songs.
        let page = browse("demo-album-Postcards");
        let id = page.header.and_then(|h| h.library_id).expect("an ID");
        assert_eq!(playlist_songs(&id), album);
    }

    #[test]
    fn filtered_search_shows_only_its_kind() {
        let only = |kind: &str| {
            page(
                &Route::SearchOnly("glass".into(), format!("demo:{kind}")),
                None,
            )
        };
        // The filter buttons first, the chosen one marked; then the results.
        let songs = only("Songs");
        let Item::Card(chip) = &songs.sections[0].items[0] else {
            panic!("a filter button")
        };
        assert!(chip.title == "Songs" && chip.look.chosen);
        assert!(!songs.tracks().is_empty());
        assert!(
            songs.sections[1..]
                .iter()
                .flat_map(|s| &s.items)
                .all(|i| matches!(i, Item::Track(_)))
        );
        for kind in ["Albums", "Artists", "Community playlists"] {
            let page = only(kind);
            assert!(page.tracks().is_empty(), "{kind} has no songs");
            assert!(page.sections.len() > 1, "{kind} finds something");
        }
    }

    #[test]
    fn library_tabs_sort() {
        let titles = |order| -> Vec<String> {
            let page = page(&Route::Library, order);
            page.sections[0]
                .items
                .iter()
                .map(|i| match i {
                    Item::Card(c) => c.title.clone(),
                    Item::Track(t) => t.title.clone(),
                })
                .collect()
        };
        let mut sorted = titles(None);
        sorted.sort_by_key(|t| t.to_lowercase());
        assert_eq!(titles(Some("demo:az")), sorted);
        let menu = page(&Route::Library, Some("demo:az"))
            .sort
            .expect("a sort button");
        assert_eq!(menu.chosen, "A to Z");
        assert_eq!(menu.orders[0].browse_id, "FEmusic_liked_playlists");
        // The front page has orders of its own.
        let front = page(&Route::LibraryRecent, None)
            .sort
            .expect("a sort button");
        assert_eq!(front.chosen, "Recent activity");
        assert!(page(&Route::Home, None).sort.is_none());
    }
}
