//! Pages: what a browse, search or Up next reply holds, in the shape the
//! app draws it. A page has an optional header (an album's cover and
//! title, an artist's photo) and sections; a section holds songs (drawn as
//! a list) or cards (albums, playlists, artists, drawn as a row of tiles).
//!
//! YouTube Music builds every page from the same few "shelves", so one
//! reader serves Home, Explore, search, albums, artists and playlists.

use serde_json::Value;

use super::{
    Byline, Links, PAGE_TYPE, Track, TrackKind, collect, find_key, parse_duration, text, toggle_on,
    track,
};

/// The largest picture YouTube offered. [`Thumb::sized`] asks for another
/// size where YouTube allows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thumb {
    pub url: String,
    pub width: u32,
}

impl Thumb {
    /// The largest entry of the first `thumbnails` list under `node`.
    pub fn best(node: &Value) -> Option<Self> {
        find_key(node, "thumbnails")?
            .as_array()?
            .iter()
            .filter_map(|t| {
                let url = t.get("url")?.as_str()?;
                let url = if let Some(rest) = url.strip_prefix("//") {
                    format!("https://{rest}")
                } else {
                    url.to_string()
                };
                url.starts_with("https://").then(|| Self {
                    url,
                    width: t.get("width").and_then(Value::as_u64).unwrap_or(0) as u32,
                })
            })
            .max_by_key(|t| t.width)
    }

    /// The address for a square picture of `pixels` pixels. Google's image
    /// servers take the size in the address (`=w226-h226-...`). A video's
    /// still (a music video's or an episode's) comes in a few fixed sizes:
    /// the 320 by 180 one for small pictures (a row's), else the one
    /// YouTube gave.
    pub fn sized(&self, pixels: u32) -> String {
        let resizable =
            self.url.contains("googleusercontent.com") || self.url.contains("ggpht.com");
        match self.url.rfind("=w") {
            Some(at) if resizable => format!("{}=w{pixels}-h{pixels}-l90-rj", &self.url[..at]),
            _ if pixels <= SMALL_STILL.1 && self.width > SMALL_STILL.0 => {
                video_id_of_still(&self.url).map_or_else(
                    || self.url.clone(),
                    |id| format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"),
                )
            }
            _ => self.url.clone(),
        }
    }

    /// The address for a wide picture (an artist's, across the top of
    /// their page), cropped to `width` by `height` pixels.
    pub fn wide(&self, width: u32, height: u32) -> String {
        let resizable =
            self.url.contains("googleusercontent.com") || self.url.contains("ggpht.com");
        match self.url.rfind("=w") {
            Some(at) if resizable => {
                format!("{}=w{width}-h{height}-p-l90-rj", &self.url[..at])
            }
            _ => self.url.clone(),
        }
    }
}

/// The size of the smaller still every YouTube video has
/// (`mqdefault.jpg`): wide, without black bars.
const SMALL_STILL: (u32, u32) = (320, 180);

/// The video of a still on YouTube's image server
/// (`https://i.ytimg.com/vi/<video>/sddefault.jpg?...`).
fn video_id_of_still(url: &str) -> Option<&str> {
    let path = url.strip_prefix("https://i.ytimg.com/")?;
    let rest = path
        .strip_prefix("vi/")
        .or_else(|| path.strip_prefix("vi_webp/"))?;
    let id = rest.split('/').next()?;
    (!id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
    .then_some(id)
}

/// What kind of page a card opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageKind {
    Album,
    Playlist,
    Artist,
    Podcast,
    Episode,
    Other,
}

impl PageKind {
    pub(crate) fn from_page_type(page_type: Option<&str>) -> Self {
        match page_type {
            Some("MUSIC_PAGE_TYPE_ALBUM" | "MUSIC_PAGE_TYPE_AUDIOBOOK") => Self::Album,
            Some("MUSIC_PAGE_TYPE_PLAYLIST") => Self::Playlist,
            Some(
                "MUSIC_PAGE_TYPE_ARTIST"
                | "MUSIC_PAGE_TYPE_USER_CHANNEL"
                | "MUSIC_PAGE_TYPE_LIBRARY_ARTIST",
            ) => Self::Artist,
            Some("MUSIC_PAGE_TYPE_PODCAST_SHOW_DETAIL_PAGE") => Self::Podcast,
            Some("MUSIC_PAGE_TYPE_NON_MUSIC_AUDIO_TRACK_PAGE") => Self::Episode,
            _ => Self::Other,
        }
    }
}

/// Where a card or a header button leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Another page.
    Browse {
        id: String,
        kind: PageKind,
        /// Some pages (moods and genres) need this as well as the ID.
        params: Option<String>,
    },
    /// Playing: a song, a playlist or album (as a queue), or both.
    Watch {
        video_id: Option<String>,
        playlist_id: Option<String>,
    },
    /// Searching (search's filter buttons): narrowed by `params`, or all
    /// results without.
    Search {
        query: String,
        params: Option<String>,
    },
}

impl Target {
    /// Reads a `browseEndpoint`, `watchEndpoint` or `watchPlaylistEndpoint`
    /// directly inside `node`.
    fn from_endpoint(node: &Value) -> Option<Self> {
        if let Some(browse) = node.get("browseEndpoint") {
            let id = browse.get("browseId")?.as_str()?.to_string();
            let kind = PageKind::from_page_type(browse.pointer(PAGE_TYPE).and_then(Value::as_str));
            let params = browse
                .get("params")
                .and_then(Value::as_str)
                .map(str::to_string);
            return Some(Self::Browse { id, kind, params });
        }
        let watch = node
            .get("watchEndpoint")
            .or_else(|| node.get("watchPlaylistEndpoint"))?;
        let field = |k: &str| watch.get(k).and_then(Value::as_str).map(str::to_string);
        let (video_id, playlist_id) = (field("videoId"), field("playlistId"));
        (video_id.is_some() || playlist_id.is_some()).then_some(Self::Watch {
            video_id,
            playlist_id,
        })
    }
}

/// An album, playlist, artist, mood or episode, drawn as a tile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub title: String,
    pub subtitle: String,
    pub thumbnail: Option<Thumb>,
    /// Artists are drawn round.
    pub round: bool,
    /// What clicking the card opens.
    pub open: Option<Target>,
    /// What its play button plays, when it has one.
    pub play: Option<Target>,
    /// An episode's podcast (the tile's second title), shown as the
    /// episode's artist when it plays.
    pub podcast: Option<String>,
    /// How it is drawn, beyond a cover and words.
    pub look: CardLook,
}

/// How a card is drawn beyond a cover and words: a button's icon and a
/// mood's coloured stripe (cards without pictures), a wide picture, a
/// chosen chip.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CardLook {
    /// YouTube's name for a button's icon (Explore's `MUSIC_NEW_RELEASE`,
    /// `TRENDING_UP`, `STICKER_EMOTICON`).
    pub icon: Option<String>,
    /// A mood's colour, the stripe at its button's left (`0xRRGGBB`).
    pub stripe: Option<u32>,
    /// A 16:9 picture (a video), not a square one.
    pub wide: bool,
    /// The chip chosen now (Home's mood being shown).
    pub chosen: bool,
    /// Marked explicit (YouTube's "E" before its subtitle).
    pub explicit: bool,
}

impl Card {
    /// The song a click on this card plays, for a card of one song (a tile
    /// on Home or Explore) or an episode, with the card's name, artist and
    /// cover. `None` for a card that opens a page.
    pub fn song(&self) -> Option<Track> {
        let single = |target: Option<&Target>| match target {
            Some(Target::Watch {
                video_id: Some(video_id),
                ..
            }) => Some(video_id.clone()),
            _ => None,
        };
        // An episode's tile opens the episode's page, and its play button
        // plays the episode alone.
        let episode = matches!(
            self.open,
            Some(Target::Browse {
                kind: PageKind::Episode,
                ..
            })
        );
        let video_id = match &self.open {
            None => single(self.play.as_ref()),
            Some(_) if episode => single(self.play.as_ref()),
            open => single(open.as_ref()),
        }?;
        let byline = Byline::parse(&self.subtitle);
        let (artists, album, duration_seconds) = if episode {
            // An episode's subtitle is when it came out and how long it
            // is ("2d ago • 15 min").
            (self.podcast.clone().unwrap_or_default(), None, None)
        } else {
            (byline.artists, byline.album, byline.duration_seconds)
        };
        Some(Track {
            video_id,
            title: self.title.clone(),
            artists,
            album,
            duration_seconds,
            thumbnail: self.thumbnail.clone(),
            ..Track::default()
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Track(Track),
    Card(Card),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub items: Vec<Item>,
    /// The page with all of the section ("More").
    pub more: Option<Target>,
    /// How YouTube Music lays the section out.
    pub shape: Shape,
    /// A search's top result: the first item is drawn as a big card, the
    /// rest (a few of its songs) at the card's right.
    pub top: Option<Box<TopResult>>,
    /// Its title is YouTube Music's smaller one (`DISPLAY_TWO`), as an
    /// artist's shelves have.
    pub small_title: bool,
    /// The small words above its title ("DEMO LISTENER", "SIMILAR TO"), as a
    /// signed-in Home's shelves have.
    pub strapline: String,
    /// The picture before its title (the account's photo for "Listen
    /// again", an artist's), and whether it is round.
    pub picture: Option<Thumb>,
    pub round_picture: bool,
}

/// What a search's top result card shows besides its picture and name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TopResult {
    /// The line under the name, as YouTube Music writes it ("Artist •
    /// 332M monthly audience").
    pub subtitle: String,
    /// Its buttons (an artist's Shuffle and Mix), in YouTube's order.
    pub buttons: Vec<CardButton>,
}

/// A button on a top result card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardButton {
    pub text: String,
    /// YouTube's name for its icon (`MUSIC_SHUFFLE`, `MIX`, `PLAY_ARROW`).
    pub icon: Option<String>,
    /// White with dark words (the first, main button); else outlined.
    pub filled: bool,
    pub target: Option<Target>,
}

/// How a section is laid out on YouTube Music.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    /// One item under another (a playlist's songs, search results).
    #[default]
    List,
    /// A row that scrolls sideways (Home's shelves).
    Carousel,
    /// Rows that wrap (the Library, a "More" page).
    Grid,
}

impl Section {
    /// The section's songs, in order, greyed-out ones included.
    pub fn tracks(&self) -> Vec<Track> {
        self.items
            .iter()
            .filter_map(|i| match i {
                Item::Track(t) => Some(t.clone()),
                Item::Card(_) => None,
            })
            .collect()
    }
}

/// The top of an album, playlist or artist page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub title: String,
    /// "Album • 2017", or an artist's listener count.
    pub subtitle: String,
    /// "11 songs • 22 minutes".
    pub detail: String,
    /// The album's artist, or the playlist's owner.
    pub owner: String,
    /// The owner's small round picture (an album's artist's, a playlist
    /// maker's), when the reply has it.
    pub owner_picture: Option<Thumb>,
    /// The description as written (an album's or an artist's, from
    /// Wikipedia; a playlist's own).
    pub description: String,
    /// An artist's subscribers, as Subscribe shows them ("28.6M").
    pub subscribers: String,
    pub thumbnail: Option<Thumb>,
    /// Artists are drawn round.
    pub round: bool,
    /// An artist's channel (`UC...`), for Subscribe.
    pub channel_id: Option<String>,
    /// Whether the account is subscribed to this artist.
    pub subscribed: Option<bool>,
    /// The playlist to save to (or remove from) the library: an album's
    /// own playlist (`OLAK5uy_...`), or the playlist itself.
    pub library_id: Option<String>,
    /// Whether it is in the library.
    pub saved: Option<bool>,
    /// The account's own playlist: it can be renamed and deleted, and
    /// songs removed from it.
    pub editable: bool,
    /// What the header's buttons play, when it has any: read them with
    /// [`Header::play`], [`Header::shuffle`] and [`Header::radio`]. Kept
    /// apart so that a page (which the app keeps many of) stays small.
    pub buttons: Option<Box<HeaderButtons>>,
}

impl Header {
    /// What the header's round play button plays: an album's or a
    /// playlist's songs, or an episode.
    pub fn play(&self) -> Option<&Target> {
        self.buttons.as_ref()?.play.as_ref()
    }

    /// An artist's Shuffle: their songs, shuffled (a playlist `RDAO...`).
    pub fn shuffle(&self) -> Option<&Target> {
        self.buttons.as_ref()?.shuffle.as_ref()
    }

    /// An artist's Mix: a radio of their songs and others like them (a
    /// playlist `RDEM...`).
    pub fn radio(&self) -> Option<&Target> {
        self.buttons.as_ref()?.radio.as_ref()
    }
}

/// What a header's buttons play ([`Header::buttons`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeaderButtons {
    pub play: Option<Target>,
    pub shuffle: Option<Target>,
    pub radio: Option<Target>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub header: Option<Header>,
    pub sections: Vec<Section>,
    /// Its sort button (the Library's tabs have one).
    pub sort: Option<super::SortMenu>,
}

impl Page {
    /// Every song on the page, in order (for playing a whole album),
    /// greyed-out ones included ([`Track::playable`]).
    pub fn tracks(&self) -> Vec<Track> {
        self.sections.iter().flat_map(Section::tracks).collect()
    }

    /// An album's songs come without a picture or a link to their album:
    /// they are the album's (its page is `album_id`, `MPREb_...`), as
    /// YouTube Music shows them.
    pub fn fill_album_songs(&mut self, album_id: &str) {
        let Some(header) = &self.header else { return };
        for section in &mut self.sections {
            for item in &mut section.items {
                if let Item::Track(track) = item {
                    if track.thumbnail.is_none() {
                        track.thumbnail = header.thumbnail.clone();
                    }
                    if track.album.is_none() {
                        track.album = Some(header.title.clone());
                    }
                    if track.album_id.is_none() && album_id.starts_with("MPRE") {
                        track.album_id = Some(album_id.to_string());
                    }
                }
            }
        }
    }

    /// Puts more of the page's list at its end: the next items of a long
    /// list in the library ([`more_items`]).
    pub fn extend_list(&mut self, items: Vec<Item>) {
        if let Some(list) = self.sections.last_mut() {
            list.items.extend(items);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.header.is_none() && self.sections.iter().all(|s| s.items.is_empty())
    }
}

/// The shelves YouTube Music builds pages from.
const SHELVES: [&str; 6] = [
    "musicCarouselShelfRenderer",
    "musicShelfRenderer",
    "musicPlaylistShelfRenderer",
    "gridRenderer",
    "musicCardShelfRenderer",
    "musicImmersiveCarouselShelfRenderer",
];

const HEADERS: [&str; 4] = [
    "musicResponsiveHeaderRenderer",
    "musicImmersiveHeaderRenderer",
    "musicDetailHeaderRenderer",
    "musicVisualHeaderRenderer",
];

/// Reads any browse or search reply as a page.
pub fn page(reply: &Value) -> Page {
    let header = HEADERS
        .iter()
        .find_map(|key| find_key(reply, key).map(|h| (*key, h)))
        .map(|(key, h)| header(key, h, reply))
        // A mood or genre page has only a title.
        .or_else(|| {
            let title = reply
                .pointer("/header/musicHeaderRenderer/title")
                .and_then(text)?;
            Some(Header {
                title,
                ..Header::default()
            })
        });
    let mut sections = Vec::new();
    // Home's moods come first, above its shelves.
    sections.extend(chips(reply));
    walk(reply, &mut sections, &mut None);
    sections.retain(|s| !s.items.is_empty());
    Page {
        header,
        sections,
        sort: super::sort::sort_menu(reply),
    }
}

/// The row of buttons above Home's shelves (Energize, Relax, Workout...),
/// as a section of cards without pictures. Each opens Home for that mood.
/// Search's filter buttons (Songs, Albums...) search instead.
fn chips(reply: &Value) -> Option<Section> {
    let items: Vec<Item> = chip_cloud(reply)?
        .get("chips")?
        .as_array()?
        .iter()
        .filter_map(|chip| {
            let chip = chip.get("chipCloudChipRenderer")?;
            let title = chip
                .get("text")
                .and_then(text)
                .filter(|t| !t.trim().is_empty())?;
            let params = |endpoint: &Value| {
                endpoint
                    .get("params")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            };
            let open = if let Some(search) = chip.pointer("/navigationEndpoint/searchEndpoint") {
                Target::Search {
                    query: search.get("query")?.as_str()?.to_string(),
                    params: params(search),
                }
            } else {
                let browse = chip.pointer("/navigationEndpoint/browseEndpoint")?;
                Target::Browse {
                    id: browse.get("browseId")?.as_str()?.to_string(),
                    kind: PageKind::Other,
                    params: params(browse),
                }
            };
            Some(Item::Card(Card {
                title,
                subtitle: String::new(),
                thumbnail: None,
                round: false,
                open: Some(open),
                play: None,
                podcast: None,
                look: CardLook {
                    chosen: chip.get("isSelected").and_then(Value::as_bool) == Some(true),
                    ..CardLook::default()
                },
            }))
        })
        .collect();
    if items.is_empty() {
        return None;
    }
    Some(Section {
        items,
        shape: Shape::Grid,
        ..Section::default()
    })
}

/// The first row of buttons (`chipCloudRenderer`) outside the shelves and
/// the header.
fn chip_cloud(node: &Value) -> Option<&Value> {
    match node {
        Value::Object(map) => map.iter().find_map(|(key, value)| match key.as_str() {
            "chipCloudRenderer" => Some(value),
            key if SHELVES.contains(&key) || HEADERS.contains(&key) => None,
            _ => chip_cloud(value),
        }),
        Value::Array(items) => items.iter().find_map(chip_cloud),
        _ => None,
    }
}

/// Collects shelves in document order, without looking inside headers
/// (an album header holds a description "shelf") or inside a shelf.
///
/// Search's results come one per `itemSectionRenderer`, with no shelf
/// around them: those in a row become one list (`results` is where it
/// is in `out`, while it can still grow).
fn walk(node: &Value, out: &mut Vec<Section>, results: &mut Option<usize>) {
    match node {
        Value::Object(map) => {
            for (key, value) in map {
                if SHELVES.contains(&key.as_str()) {
                    out.extend(shelf(key, value));
                    *results = None;
                } else if key == "itemSectionRenderer" {
                    let rows = value.get("contents").and_then(Value::as_array);
                    let items: Vec<Item> = rows.into_iter().flatten().filter_map(item).collect();
                    if items.is_empty() {
                        walk(value, out, results);
                        continue;
                    }
                    match *results {
                        Some(at) if at + 1 == out.len() => out[at].items.extend(items),
                        _ => {
                            out.push(Section {
                                items,
                                ..Section::default()
                            });
                            *results = Some(out.len() - 1);
                        }
                    }
                } else if !HEADERS.contains(&key.as_str()) {
                    walk(value, out, results);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|v| walk(v, out, results)),
        _ => {}
    }
}

fn shelf(key: &str, shelf: &Value) -> Vec<Section> {
    let title_at = |pointer: &str| shelf.pointer(pointer).and_then(text).unwrap_or_default();
    let title = match key {
        "musicCarouselShelfRenderer" => {
            title_at("/header/musicCarouselShelfBasicHeaderRenderer/title")
        }
        "gridRenderer" => title_at("/header/gridHeaderRenderer/title"),
        "musicCardShelfRenderer" => title_at("/header/musicCardShelfHeaderBasicRenderer/title"),
        "musicImmersiveCarouselShelfRenderer" => {
            title_at("/header/musicCarouselShelfBasicHeaderRenderer/title")
        }
        _ => shelf.get("title").and_then(text).unwrap_or_default(),
    };
    let more = shelf
        .pointer("/header/musicCarouselShelfBasicHeaderRenderer/moreContentButton/buttonRenderer/navigationEndpoint")
        .and_then(Target::from_endpoint)
        .or_else(|| {
            shelf
                .pointer("/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/navigationEndpoint")
                .and_then(Target::from_endpoint)
        })
        .or_else(|| {
            // A list's "Show all" under it (an artist's Top songs), or its
            // title's link. Search's "Show all" searches instead, and is
            // not read here.
            if key != "musicShelfRenderer" {
                return None;
            }
            shelf
                .get("bottomEndpoint")
                .and_then(Target::from_endpoint)
                .or_else(|| {
                    shelf
                        .pointer("/title/runs/0/navigationEndpoint")
                        .and_then(Target::from_endpoint)
                })
        });

    let mut items = Vec::new();
    let mut top = None;
    if key == "musicCardShelfRenderer" {
        // The top search result: one big card, then a few songs.
        if let Some(card) = top_result(shelf) {
            items.push(card);
            top = Some(Box::new(TopResult {
                subtitle: shelf.get("subtitle").and_then(text).unwrap_or_default(),
                buttons: card_buttons(shelf),
            }));
        }
    }
    let rows = shelf
        .get("contents")
        .or_else(|| shelf.get("items"))
        .and_then(Value::as_array);
    items.extend(rows.into_iter().flatten().filter_map(item));
    let shape = match key {
        "musicCarouselShelfRenderer" | "musicImmersiveCarouselShelfRenderer" => Shape::Carousel,
        "gridRenderer" => Shape::Grid,
        _ => Shape::List,
    };
    let basic = shelf.pointer("/header/musicCarouselShelfBasicHeaderRenderer");
    let small_title = basic
        .and_then(|h| h.get("headerStyle"))
        .and_then(Value::as_str)
        == Some("MUSIC_CAROUSEL_SHELF_BASIC_HEADER_STYLE_DISPLAY_TWO");
    let strapline = basic
        .and_then(|h| h.get("strapline"))
        .and_then(text)
        .unwrap_or_default();
    let thumbnail = basic.and_then(|h| h.pointer("/thumbnail/musicThumbnailRenderer"));
    let picture = thumbnail.and_then(Thumb::best);
    let round_picture = thumbnail
        .and_then(|t| t.get("thumbnailCrop"))
        .and_then(Value::as_str)
        == Some("MUSIC_THUMBNAIL_CROP_CIRCLE");
    vec![Section {
        title,
        items,
        more,
        shape,
        top,
        small_title,
        strapline,
        picture,
        round_picture,
    }]
}

/// A top result card's buttons: what each says and plays.
fn card_buttons(shelf: &Value) -> Vec<CardButton> {
    let buttons = shelf.get("buttons").and_then(Value::as_array);
    buttons
        .into_iter()
        .flatten()
        .filter_map(|b| b.get("buttonRenderer"))
        .filter_map(|b| {
            Some(CardButton {
                text: b.get("text").and_then(text)?,
                icon: b
                    .pointer("/icon/iconType")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                filled: b.get("style").and_then(Value::as_str) == Some("STYLE_DARK_ON_WHITE"),
                target: ["command", "navigationEndpoint"]
                    .into_iter()
                    .find_map(|k| b.get(k).and_then(Target::from_endpoint)),
            })
        })
        .collect()
}

/// The rows shelves and grids hold.
const ITEMS: [&str; 4] = [
    "musicResponsiveListItemRenderer",
    "musicTwoRowItemRenderer",
    "musicNavigationButtonRenderer",
    "musicMultiRowListItemRenderer",
];

/// One row of a shelf or grid: a song, or a tile. A tile that leads
/// nowhere is left out: the library's "New playlist" opens a dialog of
/// YouTube's own, not a page (ytmusicapi skips it too).
pub(super) fn item(row: &Value) -> Option<Item> {
    match any_item(row)? {
        Item::Card(card) if card.open.is_none() && card.play.is_none() => None,
        item => Some(item),
    }
}

fn any_item(row: &Value) -> Option<Item> {
    if let Some(list_row) = row.get("musicResponsiveListItemRenderer") {
        return match track(list_row) {
            Some(t) => Some(Item::Track(t)),
            None => list_row_card(list_row).map(Item::Card),
        };
    }
    if let Some(two_row) = row.get("musicTwoRowItemRenderer") {
        return two_row_card(two_row).map(Item::Card);
    }
    if let Some(button) = row.get("musicNavigationButtonRenderer") {
        // An ARGB number: the stripe's colour is its lower three bytes.
        let stripe = button
            .pointer("/solid/leftStripeColor")
            .and_then(Value::as_u64)
            .map(|argb| (argb & 0x00ff_ffff) as u32);
        let icon = button
            .pointer("/iconStyle/icon/iconType")
            .and_then(Value::as_str)
            .map(str::to_string);
        return Some(Item::Card(Card {
            title: button.get("buttonText").and_then(text)?,
            subtitle: String::new(),
            thumbnail: None,
            round: false,
            open: button.get("clickCommand").and_then(Target::from_endpoint),
            play: None,
            podcast: None,
            look: CardLook {
                icon,
                stripe,
                ..CardLook::default()
            },
        }));
    }
    if let Some(episode) = row.get("musicMultiRowListItemRenderer") {
        return Some(Item::Card(Card {
            title: episode.get("title").and_then(text)?,
            subtitle: episode.get("subtitle").and_then(text).unwrap_or_default(),
            thumbnail: episode.get("thumbnail").and_then(Thumb::best),
            round: false,
            open: episode
                .pointer("/title/runs/0/navigationEndpoint")
                .and_then(Target::from_endpoint),
            play: play_button(episode),
            podcast: episode
                .get("secondTitle")
                .and_then(text)
                .filter(|p| !p.trim().is_empty()),
            look: CardLook::default(),
        }));
    }
    None
}

/// The next items of a long list (the library's playlists, albums or
/// artists), from a reply with more of it ([`super::item_continuation`]
/// says where they come from).
pub fn more_items(reply: &Value) -> Vec<Item> {
    fn rows(node: &Value) -> Option<&Vec<Value>> {
        match node {
            Value::Object(map) => {
                // Older replies: `continuationContents` with `items` (a
                // grid) or `contents` (a list). Newer ones:
                // `continuationItems`.
                let own = ["items", "contents", "continuationItems"]
                    .iter()
                    .find_map(|key| map.get(*key).and_then(Value::as_array))
                    .filter(|rows| {
                        rows.iter()
                            .any(|row| ITEMS.iter().any(|kind| row.get(*kind).is_some()))
                    });
                own.or_else(|| map.values().find_map(rows))
            }
            Value::Array(items) => items.iter().find_map(rows),
            _ => None,
        }
    }
    rows(reply).into_iter().flatten().filter_map(item).collect()
}

/// The target of the play button drawn over a picture.
fn play_button(node: &Value) -> Option<Target> {
    find_key(node, "musicPlayButtonRenderer")
        .and_then(|b| b.get("playNavigationEndpoint"))
        .and_then(Target::from_endpoint)
}

fn is_round(node: &Value) -> bool {
    find_key(node, "thumbnailCrop").and_then(Value::as_str) == Some("MUSIC_THUMBNAIL_CROP_CIRCLE")
}

fn two_row_card(row: &Value) -> Option<Card> {
    Some(Card {
        title: row.get("title").and_then(text)?,
        subtitle: row.get("subtitle").and_then(text).unwrap_or_default(),
        thumbnail: row.get("thumbnailRenderer").and_then(Thumb::best),
        round: row.get("thumbnailRenderer").is_some_and(is_round),
        open: row
            .get("navigationEndpoint")
            .and_then(Target::from_endpoint),
        play: row.get("thumbnailOverlay").and_then(play_button),
        podcast: None,
        look: CardLook {
            wide: row.get("aspectRatio").and_then(Value::as_str)
                == Some("MUSIC_TWO_ROW_ITEM_THUMBNAIL_ASPECT_RATIO_RECTANGLE_16_9"),
            explicit: row.get("subtitleBadges").is_some_and(super::is_explicit),
            ..CardLook::default()
        },
    })
}

/// A list row that is not a song: an artist, album or playlist in search
/// results or in the library.
fn list_row_card(row: &Value) -> Option<Card> {
    let column = |i: usize| {
        row.pointer(&format!(
            "/flexColumns/{i}/musicResponsiveListItemFlexColumnRenderer/text"
        ))
        .and_then(text)
        .unwrap_or_default()
    };
    let title = column(0);
    if title.is_empty() {
        return None;
    }
    Some(Card {
        title,
        subtitle: column(1),
        thumbnail: row.get("thumbnail").and_then(Thumb::best),
        round: row.get("thumbnail").is_some_and(is_round),
        open: row
            .get("navigationEndpoint")
            .and_then(Target::from_endpoint),
        play: row.get("overlay").and_then(play_button),
        podcast: None,
        look: CardLook::default(),
    })
}

/// The big "Top result" card of a search.
fn top_result(shelf: &Value) -> Option<Item> {
    let title = shelf.get("title").and_then(text)?;
    let title_link = shelf.pointer("/title/runs/0/navigationEndpoint");
    let open = title_link
        .and_then(Target::from_endpoint)
        .or_else(|| shelf.get("onTap").and_then(Target::from_endpoint));
    let subtitle = shelf.get("subtitle").and_then(text).unwrap_or_default();
    let thumbnail = shelf.get("thumbnail").and_then(Thumb::best);
    // A song as the top result plays like any other song row.
    if let Some(Target::Watch {
        video_id: Some(video_id),
        ..
    }) = &open
    {
        let byline = Byline::parse(&subtitle);
        let mut links = Links::default();
        for part in ["subtitle", "menu"] {
            if let Some(node) = shelf.get(part) {
                links.add(node);
            }
        }
        // Whether it is a song or a music video, as its link says.
        let kind = TrackKind::from_music_video_type(
            [title_link, shelf.get("onTap")]
                .into_iter()
                .flatten()
                .find_map(|link| find_key(link, "watchEndpointMusicConfig"))
                .and_then(|c| c.get("musicVideoType"))
                .and_then(Value::as_str),
        );
        return Some(Item::Track(Track {
            video_id: video_id.clone(),
            set_video_id: None,
            title,
            artists: byline.artists,
            album: byline.album.or(links.album),
            duration_seconds: byline.duration_seconds,
            kind,
            thumbnail,
            artist_id: links.artist_id,
            album_id: links.album_id,
            playable: true,
            more: None,
        }));
    }
    Some(Item::Card(Card {
        title,
        subtitle,
        thumbnail,
        round: shelf.get("thumbnail").is_some_and(is_round),
        open,
        play: play_button(shelf),
        podcast: None,
        look: CardLook::default(),
    }))
}

/// `reply` is the whole page, for what lies outside the header.
fn header(key: &str, h: &Value, reply: &Value) -> Header {
    let t = |field: &str| h.get(field).and_then(text).unwrap_or_default();
    let subtitle = match key {
        // An artist page has no subtitle; YouTube shows listeners instead.
        "musicImmersiveHeaderRenderer" => t("monthlyListenerCount"),
        _ => t("subtitle"),
    };
    // A playlist's maker, in its "facepile": a picture and a name.
    let face = h.pointer("/facepile/avatarStackViewModel");
    let face_picture = face
        .and_then(|f| f.pointer("/avatars/0/avatarViewModel/image/sources/0/url"))
        .and_then(Value::as_str)
        .map(|url| Thumb {
            url: url.to_string(),
            width: 48,
        });
    // Its name is its text, or (several makers) what it says to screen
    // readers.
    let face_name = face
        .and_then(|f| {
            [
                "/text/content",
                "/rendererContext/accessibilityContext/label",
            ]
            .into_iter()
            .find_map(|at| f.pointer(at).and_then(Value::as_str))
        })
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string);
    let mut owner = t("straplineTextOne");
    if owner.is_empty() {
        owner = face_name.unwrap_or_default();
    }
    let mut header = Header {
        title: t("title"),
        subtitle,
        detail: t("secondSubtitle"),
        owner,
        owner_picture: h
            .get("straplineThumbnail")
            .and_then(Thumb::best)
            .or(face_picture),
        // An album's comes in a shelf of its own; an artist's as text.
        description: h
            .pointer("/description/musicDescriptionShelfRenderer/description")
            .or_else(|| h.get("description"))
            .and_then(text)
            .unwrap_or_default(),
        thumbnail: h.get("thumbnail").and_then(Thumb::best),
        round: key == "musicImmersiveHeaderRenderer",
        ..Header::default()
    };
    // The round play button, and an artist's Shuffle and Mix buttons.
    let target_of = |name: &str| {
        h.get(name)
            .and_then(|b| find_key(b, "navigationEndpoint"))
            .and_then(Target::from_endpoint)
    };
    let buttons = HeaderButtons {
        play: play_button(h),
        shuffle: target_of("playButton"),
        radio: target_of("startRadioButton"),
    };
    if buttons != HeaderButtons::default() {
        header.buttons = Some(Box::new(buttons));
    }
    // An artist's (or a channel's) Subscribe button.
    if let Some(button) = find_key(h, "subscribeButtonRenderer") {
        header.channel_id = button
            .get("channelId")
            .or_else(|| find_key(button, "channelIds").and_then(|ids| ids.get(0)))
            .and_then(Value::as_str)
            .map(str::to_string);
        header.subscribed = button.get("subscribed").and_then(Value::as_bool);
        header.subscribers = button
            .get("subscriberCountText")
            .and_then(text)
            .unwrap_or_default();
    }
    // Albums and playlists can be saved to the library.
    if matches!(
        key,
        "musicResponsiveHeaderRenderer" | "musicDetailHeaderRenderer"
    ) {
        let (saves, saved) = save_button(h);
        // The account's own playlists come inside an editing frame, and
        // have no Save button.
        let own = find_key(reply, "musicEditablePlaylistDetailHeaderRenderer");
        header.editable = own.is_some();
        header.saved = saved;
        header.library_id = saves
            .or_else(|| {
                own.and_then(|o| o.get("playlistId"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            // What the header's play button plays: an album's own
            // playlist, or the playlist itself.
            .or_else(|| match play_button(h) {
                Some(Target::Watch { playlist_id, .. }) => playlist_id,
                _ => None,
            })
            .or_else(|| {
                find_key(reply, "musicPlaylistShelfRenderer")
                    .and_then(|s| s.get("playlistId"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
    }
    header
}

/// The header's Save button: the playlist it saves (when it says), and
/// whether that is in the library.
fn save_button(h: &Value) -> (Option<String>, Option<bool>) {
    // The header's own buttons, or an older header's menu buttons; not
    // the description's "More" toggle.
    let buttons: Vec<&Value> = h
        .get("buttons")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            find_key(h, "topLevelButtons")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
        .collect();
    let saves = |node: &Value| {
        find_key(node, "likeEndpoint")
            .and_then(|like| like.pointer("/target/playlistId"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    // Signed in, the button saves with a like.
    for button in &buttons {
        for kind in ["toggleButtonRenderer", "buttonRenderer"] {
            if let Some(b) = button.get(kind)
                && let Some(on) = toggle_on(b)
            {
                return (saves(b), Some(on));
            }
        }
        if let Some(like) = button.get("likeButtonRenderer") {
            let saved = like
                .get("likeStatus")
                .and_then(Value::as_str)
                .map(|status| status == "LIKE");
            let id = like
                .pointer("/target/playlistId")
                .and_then(Value::as_str)
                .map(str::to_string);
            return (id, saved);
        }
    }
    // Or the menu's "Save to library".
    let mut entries = Vec::new();
    collect(h, "toggleMenuServiceItemRenderer", &mut entries);
    if let Some((entry, on)) = entries.iter().find_map(|e| Some((*e, toggle_on(e)?))) {
        return (saves(entry), Some(on));
    }
    // Signed out, the button only asks to sign in, but shows the state.
    let saved = buttons
        .iter()
        .find_map(|b| b.get("toggleButtonRenderer"))
        .and_then(|t| t.get("isToggled"))
        .and_then(Value::as_bool);
    (None, saved)
}

/// The queue's lists in a `next` reply: the first, or (in a reply with
/// more of it) the next.
fn queue_panels(reply: &Value) -> Vec<&Value> {
    let mut panels = Vec::new();
    collect(reply, "playlistPanelRenderer", &mut panels);
    collect(reply, "playlistPanelContinuation", &mut panels);
    panels
}

/// What the songs of a `next` reply play from, as Up next names it under
/// "Playing from" ("Yellow Mix", a playlist's name): its queue header's
/// subtitle, else the list's own title.
pub fn queue_title(reply: &Value) -> Option<String> {
    find_key(reply, "musicQueueHeaderRenderer")
        .and_then(|h| h.get("subtitle"))
        .or_else(|| {
            queue_panels(reply)
                .into_iter()
                .find_map(|panel| panel.get("title"))
        })
        .and_then(|t| text(t).or_else(|| t.as_str().map(str::to_string)))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Where the next songs of a queue come from (the `next` reply's
/// continuation, as ytmusicapi's `get_watch_playlist` follows it), when
/// there are more: a playlist's (`nextContinuationData`) or a radio's
/// (`nextRadioContinuationData`). The token goes in the address
/// (`ctoken`), with the same request.
pub fn queue_continuation(reply: &Value) -> Option<String> {
    queue_panels(reply).into_iter().find_map(|panel| {
        panel
            .get("continuations")?
            .as_array()?
            .iter()
            .find_map(|c| {
                c.get("nextContinuationData")
                    .or_else(|| c.get("nextRadioContinuationData"))
            })?
            .get("continuation")?
            .as_str()
            .filter(|token| !token.is_empty())
            .map(str::to_string)
    })
}

/// The songs of an Up next or radio list (the `next` reply, or a reply
/// with more of it). Unplayable rows are left out; where a song also
/// exists as a video, the song is kept.
pub fn up_next(reply: &Value) -> Vec<Track> {
    let panels = queue_panels(reply);
    let rows = panels
        .into_iter()
        .filter_map(|p| p.get("contents").and_then(Value::as_array))
        .flatten();
    rows.filter_map(|row| {
        let video = row
            .pointer(
                "/playlistPanelVideoWrapperRenderer/primaryRenderer/playlistPanelVideoRenderer",
            )
            .or_else(|| row.get("playlistPanelVideoRenderer"))?;
        if video.get("unplayableText").is_some() {
            return None;
        }
        let byline = Byline::parse(
            &video
                .get("longBylineText")
                .and_then(text)
                .unwrap_or_default(),
        );
        let mut links = Links::default();
        for part in ["longBylineText", "menu"] {
            if let Some(node) = video.get(part) {
                links.add(node);
            }
        }
        Some(Track {
            video_id: video.get("videoId")?.as_str()?.to_string(),
            set_video_id: video
                .get("playlistSetVideoId")
                .and_then(Value::as_str)
                .map(str::to_string),
            title: video.get("title").and_then(text).unwrap_or_default(),
            artists: byline.artists,
            album: byline.album.or(links.album),
            duration_seconds: video
                .get("lengthText")
                .and_then(text)
                .and_then(|l| parse_duration(&l))
                .or(byline.duration_seconds),
            kind: TrackKind::from_music_video_type(
                find_key(video, "watchEndpointMusicConfig")
                    .and_then(|c| c.get("musicVideoType"))
                    .and_then(Value::as_str),
            ),
            thumbnail: video.get("thumbnail").and_then(Thumb::best),
            artist_id: links.artist_id,
            album_id: links.album_id,
            playable: true,
            more: None,
        })
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn titles(page: &Page) -> Vec<&str> {
        page.sections.iter().map(|s| s.title.as_str()).collect()
    }

    #[test]
    fn real_explore_page() {
        let page = page(&fixture("explore.json"));
        assert_eq!(
            titles(&page),
            [
                "",
                "New albums & singles",
                "Moods & genres",
                "Popular episodes",
                "Trending",
                "New music videos"
            ]
        );
        // The top buttons wrap; the shelves scroll sideways.
        assert_eq!(page.sections[0].shape, Shape::Grid);
        assert_eq!(page.sections[1].shape, Shape::Carousel);
        // Albums: a card that opens the album and plays it.
        let Item::Card(album) = &page.sections[1].items[0] else {
            panic!("a card")
        };
        assert_eq!(album.title, "Call Me");
        assert!(album.subtitle.starts_with("Single"));
        assert!(!album.round);
        assert!(
            matches!(&album.open, Some(Target::Browse { kind: PageKind::Album, id, .. }) if id.starts_with("MPREb_"))
        );
        assert!(
            matches!(&album.play, Some(Target::Watch { playlist_id: Some(p), .. }) if p.starts_with("OLAK5uy_"))
        );
        assert!(album.thumbnail.is_some());
        // Moods need their params.
        let Item::Card(mood) = &page.sections[2].items[0] else {
            panic!("a card")
        };
        assert_eq!(mood.title, "Chill");
        assert!(matches!(
            &mood.open,
            Some(Target::Browse {
                params: Some(_),
                ..
            })
        ));
        // Trending is songs.
        assert!(
            page.sections[4]
                .items
                .iter()
                .all(|i| matches!(i, Item::Track(_)))
        );
        assert!(
            matches!(&page.sections[1].more, Some(Target::Browse { id, .. }) if id == "FEmusic_new_releases_albums")
        );
        assert_eq!(page.header, None);
    }

    #[test]
    fn a_card_of_one_song_is_that_song() {
        let page = page(&fixture("explore.json"));
        // New music videos: each card plays one song.
        let Item::Card(video) = &page.sections[5].items[0] else {
            panic!("a card")
        };
        let song = video.song().expect("a song");
        assert_eq!(song.video_id, "BWYIt1UNbyQ");
        assert_eq!(song.title, "mil preguntas");
        assert_eq!(song.artists, "Zhamira");
        assert!(song.thumbnail.is_some());
        assert_eq!(song.thumbnail, video.thumbnail);
        // An album's card opens its page.
        let Item::Card(album) = &page.sections[1].items[0] else {
            panic!("a card")
        };
        assert_eq!(album.song(), None);
    }

    #[test]
    fn an_episode_tile_plays_its_episode() {
        let page = page(&fixture("explore.json"));
        assert_eq!(page.sections[3].title, "Popular episodes");
        let Item::Card(episode) = &page.sections[3].items[0] else {
            panic!("a card")
        };
        // It opens the episode's page...
        assert!(matches!(
            &episode.open,
            Some(Target::Browse {
                kind: PageKind::Episode,
                ..
            })
        ));
        // ...and its play button plays the episode, named, with its
        // podcast as the artist and its picture.
        let song = episode.song().expect("the episode");
        assert_eq!(song.video_id, "C1HSXeW-8MU");
        assert_eq!(
            song.title,
            "Tom Swarbrick Starts to Doubt That Diversity Is Our Strength"
        );
        assert_eq!(song.artists, "The Podcast of the Lotus Eaters");
        assert_eq!(song.album, None);
        assert_eq!(song.thumbnail, episode.thumbnail);
        assert!(song.playable);
    }

    #[test]
    fn real_album_page() {
        let page = page(&fixture("album.json"));
        let header = page.header.clone().expect("a header");
        assert_eq!(header.title, "17");
        assert_eq!(header.subtitle, "Album • 2017");
        assert_eq!(header.detail, "11 songs • 22 minutes");
        assert_eq!(header.owner, "XXXTENTACION");
        assert!(header.thumbnail.is_some());
        let tracks = page.tracks();
        assert_eq!(tracks.len(), 3);
        assert!(
            tracks
                .iter()
                .all(|t| !t.title.is_empty() && t.duration_seconds.is_some())
        );
        assert!(titles(&page).contains(&"Releases for you"));
        // The third column is the play count, not an album.
        assert!(
            tracks
                .iter()
                .all(|t| t.album.as_deref().is_none_or(|a| !a.ends_with("plays")))
        );
        // Filled in from the album, with a link to it for "Go to album".
        assert!(tracks.iter().all(|t| t.album_id.is_none()));
        let mut page = page;
        page.fill_album_songs("MPREb_album17");
        for track in page.tracks() {
            assert_eq!(track.album.as_deref(), Some("17"));
            assert_eq!(track.thumbnail, header.thumbnail);
            assert_eq!(track.album_id.as_deref(), Some("MPREb_album17"));
        }
        // Only an album's page is linked to.
        let mut other = super::page(&fixture("album.json"));
        other.fill_album_songs("VLPLsomething");
        assert!(other.tracks().iter().all(|t| t.album_id.is_none()));
        // A card YouTube marks explicit says so.
        assert!(
            page.sections
                .iter()
                .flat_map(|s| &s.items)
                .any(|i| matches!(
                    i,
                    Item::Card(c) if c.look.explicit
                ))
        );
    }

    #[test]
    fn real_artist_page() {
        let page = page(&fixture("artist.json"));
        let header = page.header.clone().expect("a header");
        assert_eq!(header.title, "Hatsune Miku");
        assert!(header.round);
        let names = titles(&page);
        assert_eq!(names[0], "Top songs");
        assert!(names.contains(&"Albums"));
        assert!(
            page.sections[0]
                .items
                .iter()
                .all(|i| matches!(i, Item::Track(_)))
        );
        // Its description, and its shelves' smaller titles (Top songs is
        // a list, with a title of the usual size).
        assert!(header.description.starts_with("Hatsune Miku, officially"));
        assert!(!page.sections[0].small_title);
        assert!(page.sections[1..].iter().all(|s| s.small_title));
        // Top songs' "Show all": the artist's whole list of top songs.
        assert_eq!(
            page.sections[0].more,
            Some(Target::Browse {
                id: "VLOLAK5uy_nEdP9bp8c7oZ_p_4F7ipSRnfZt32crS94".into(),
                kind: PageKind::Playlist,
                params: Some("ggMCCAI%3D".into()),
            })
        );
    }

    #[test]
    fn real_playlist_page() {
        let page = page(&fixture("playlist_signed_in_premium.json"));
        assert_eq!(page.header.clone().unwrap().title, "03 Jan 12:09");
        assert_eq!(page.tracks().len(), 3);
        assert_eq!(page.sections[0].shape, Shape::List);
    }

    #[test]
    fn home_and_search() {
        let home = page(&fixture("home_synthetic.json"));
        assert_eq!(titles(&home), ["Quick picks", "Listen again"]);
        assert!(
            matches!(&home.sections[0].items[0], Item::Track(t) if t.artists == "Kendrick Lamar")
        );
        let Item::Card(artist) = &home.sections[1].items[1] else {
            panic!("a card")
        };
        assert!(artist.round);

        let search = page(&fixture("search_synthetic.json"));
        assert_eq!(titles(&search), ["Top result", "Songs", "Albums"]);
        // A song as the top result is a playable row.
        assert!(
            matches!(&search.sections[0].items[0], Item::Track(t) if t.video_id == "searchSong1")
        );
        // Search's lists have no page with all of them.
        assert_eq!(search.sections[1].more, None);
        assert_eq!(search.sections[0].more, None);
        let songs = search.sections[1].tracks();
        assert_eq!(songs[0].artists, "Daft Punk");
        assert_eq!(songs[0].album.as_deref(), Some("Discovery"));
        assert_eq!(songs[0].duration_seconds, Some(320));
        // An album row in search results is a card.
        let Item::Card(album) = &search.sections[2].items[0] else {
            panic!("a card")
        };
        assert!(matches!(
            &album.open,
            Some(Target::Browse {
                kind: PageKind::Album,
                ..
            })
        ));
    }

    #[test]
    fn real_headers_description_and_owner_picture() {
        let album = page(&fixture("album.json"));
        let header = album.header.as_ref().unwrap();
        assert!(
            header
                .description
                .starts_with("17 is the debut studio album")
        );
        // A collaborative playlist's maker, with their picture.
        let playlist = page(&fixture("playlist_collaborative.json"));
        let header = playlist.header.as_ref().unwrap();
        assert!(!header.owner.is_empty());
        assert!(
            header
                .owner_picture
                .as_ref()
                .is_some_and(|p| p.url.starts_with("https://yt3.ggpht.com/"))
        );
        assert_eq!(header.description, "a description");
    }

    #[test]
    fn real_explore_buttons_moods_and_chart() {
        let explore = page(&fixture("explore.json"));
        let cards: Vec<&Card> = explore
            .sections
            .iter()
            .flat_map(|s| &s.items)
            .filter_map(|i| match i {
                Item::Card(c) => Some(c),
                Item::Track(_) => None,
            })
            .collect();
        let card = |title: &str| cards.iter().find(|c| c.title == title).unwrap();
        // The three big buttons, with YouTube's names for their icons.
        assert_eq!(
            card("New releases").look.icon.as_deref(),
            Some("MUSIC_NEW_RELEASE")
        );
        assert_eq!(card("Charts").look.icon.as_deref(), Some("TRENDING_UP"));
        // The moods, with their colours.
        assert_eq!(card("Chill").look.stripe, Some(0xa4c5ff));
        assert_eq!(card("Commute").look.stripe, Some(0xffc200));
        // Square covers are not wide.
        assert!(!card("Call Me").look.wide);
        // Trending: places and views.
        let chart: Vec<&Track> = explore
            .sections
            .iter()
            .flat_map(|s| &s.items)
            .filter_map(|i| match i {
                Item::Track(t) if t.rank().is_some() => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(chart[0].rank(), Some("1"));
        assert_eq!(chart[0].count(), Some("28M views"));
        assert_eq!(chart[0].artists, "Daddy Yankee");
    }

    #[test]
    fn real_search_top_result() {
        let search = page(&fixture("search.json"));
        // YouTube's own filter buttons first, each a search of its own.
        let chips: Vec<&Card> = search.sections[0]
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Card(c) => Some(c),
                Item::Track(_) => None,
            })
            .collect();
        assert_eq!(chips.len(), 9);
        assert_eq!(chips[0].title, "Artists");
        assert_eq!(chips[1].title, "Community playlists");
        assert!(chips.iter().all(|c| !c.look.chosen));
        assert_eq!(
            chips[0].open,
            Some(Target::Search {
                query: "coldplay".into(),
                params: Some("EgWKAQIgAWoSEAUQChAJEAMQBBAQEA4QFRAR".into()),
            })
        );
        // Then the top result card, then one result per section, in
        // YouTube's order, with no headings.
        let top = &search.sections[1];
        let card = top.top.as_ref().expect("the top result card");
        assert_eq!(card.subtitle, "Artist \u{2022} 332M monthly audience");
        let Item::Card(artist) = &top.items[0] else {
            panic!("the artist's card")
        };
        assert_eq!(artist.title, "Coldplay");
        assert!(artist.round);
        assert!(matches!(
            &artist.open,
            Some(Target::Browse {
                kind: PageKind::Artist,
                ..
            })
        ));
        // Its three songs come after it, for the card's right half.
        let songs = top.tracks();
        assert_eq!(songs.len(), 3);
        assert_eq!(songs[0].title, "Always in My Head");
        assert_eq!(songs[0].duration_seconds, Some(217));
        // Its own buttons: Shuffle (the main one) and Mix.
        let words: Vec<(&str, bool)> = card
            .buttons
            .iter()
            .map(|b| (b.text.as_str(), b.filled))
            .collect();
        assert_eq!(words, [("Shuffle", true), ("Mix", false)]);
        assert_eq!(card.buttons[0].icon.as_deref(), Some("MUSIC_SHUFFLE"));
        assert!(card.buttons.iter().all(|b| matches!(
            &b.target,
            Some(Target::Watch {
                playlist_id: Some(_),
                ..
            })
        )));
        // Then every other result in one list, in YouTube's order:
        // playlists, then singles (albums), then a playlist.
        assert_eq!(search.sections.len(), 3);
        let results = &search.sections[2];
        assert!(results.top.is_none() && results.title.is_empty());
        let names: Vec<&str> = results
            .items
            .iter()
            .map(|item| match item {
                Item::Card(card) => card.title.as_str(),
                Item::Track(track) => track.title.as_str(),
            })
            .collect();
        assert_eq!(
            names,
            [
                "Top 20 - Le migliori canzoni dei Coldplay",
                "Coldplay - Greatest Hits (Full Album, Super Collection)",
                "Fix You",
                "Violet Hill",
                "Everyday Life",
                "Coldplay - A Head Full Of Dreams",
            ]
        );
    }

    #[test]
    fn up_next_list() {
        let reply = fixture("next_synthetic.json");
        // What they play from, as Up next names it.
        assert_eq!(queue_title(&reply).as_deref(), Some("Pop Mix"));
        let tracks = up_next(&reply);
        let ids: Vec<&str> = tracks.iter().map(|t| t.video_id.as_str()).collect();
        // The video counterpart and the unplayable row are left out.
        assert_eq!(ids, ["nextSong001", "nextSong002"]);
        assert_eq!(tracks[0].artists, "Artist One");
        assert_eq!(tracks[0].album.as_deref(), Some("Album One"));
        assert_eq!(tracks[0].duration_seconds, Some(215));
        assert_eq!(tracks[1].kind, TrackKind::Song);
        // Nothing after these.
        assert_eq!(queue_continuation(&reply), None);
    }

    #[test]
    fn a_queue_in_batches() {
        // A playlist's queue in the layout ytmusicapi's
        // `get_watch_playlist` reads: the first songs, and where the next
        // come from.
        let row = |id: &str| {
            serde_json::json!({"playlistPanelVideoRenderer": {
                "videoId": id,
                "title": {"runs": [{"text": id}]},
                "longBylineText": {"runs": [{"text": "A singer"}]}
            }})
        };
        let first = serde_json::json!({"contents": {"singleColumnMusicWatchNextResultsRenderer": {"tabbedRenderer": {
            "watchNextTabbedResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"musicQueueRenderer": {"content": {
                "playlistPanelRenderer": {
                    "contents": [row("queueSong01"), row("queueSong02")],
                    "continuations": [{"nextContinuationData": {"continuation": "BATCH2"}}]
                }
            }}}}}]}
        }}}});
        assert_eq!(up_next(&first).len(), 2);
        assert_eq!(queue_continuation(&first).as_deref(), Some("BATCH2"));
        // The reply with more: its songs, and (a radio's) next token.
        let more = serde_json::json!({"continuationContents": {"playlistPanelContinuation": {
            "contents": [row("queueSong03")],
            "continuations": [{"nextRadioContinuationData": {"continuation": "BATCH3"}}]
        }}});
        let ids: Vec<String> = up_next(&more).into_iter().map(|t| t.video_id).collect();
        assert_eq!(ids, ["queueSong03"]);
        assert_eq!(queue_continuation(&more).as_deref(), Some("BATCH3"));
        // The last: no token.
        let last = serde_json::json!({"continuationContents": {"playlistPanelContinuation": {
            "contents": [row("queueSong04")]
        }}});
        assert_eq!(queue_continuation(&last), None);
        assert_eq!(queue_continuation(&Value::Null), None);
    }

    #[test]
    fn picture_sizes() {
        let thumb = Thumb {
            url: "https://lh3.googleusercontent.com/abc=w226-h226-l90-rj".into(),
            width: 226,
        };
        assert_eq!(
            thumb.sized(96),
            "https://lh3.googleusercontent.com/abc=w96-h96-l90-rj"
        );
        // A music video's still, as a playlist row gives it: the smaller
        // still for a row, the one given for a large picture.
        let still = Thumb {
            url: "https://i.ytimg.com/vi/dQw4w9WgXcQ/sddefault.jpg?sqp=-oaymwEWCJADEOEBIAQqCghqEJQEGHgg6AJIWg&rs=AMzJL3lNNudXg7f4Qf7PiE9tvCAkHTjJ0w".into(),
            width: 400,
        };
        assert_eq!(
            still.sized(120),
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg"
        );
        assert_eq!(still.sized(60), still.sized(120));
        assert_eq!(still.sized(226), still.url);
        assert_eq!(still.sized(544), still.url);
        // A still no larger than the smaller one stays as it is.
        let small = Thumb {
            url: "https://i.ytimg.com/vi/dQw4w9WgXcQ/default.jpg".into(),
            width: 120,
        };
        assert_eq!(small.sized(60), small.url);
        // Pictures elsewhere come as they are.
        let other = Thumb {
            url: "https://a.example/vi/x/hqdefault.jpg".into(),
            width: 480,
        };
        assert_eq!(other.sized(96), other.url);
        let best = Thumb::best(&serde_json::json!({ "thumbnails": [
            { "url": "//a.example/small", "width": 60 },
            { "url": "https://a.example/big", "width": 544 }
        ]}));
        assert_eq!(best.unwrap().url, "https://a.example/big");
    }

    #[test]
    fn real_headers_library_details() {
        // An artist: Subscribe, nothing to save.
        let artist = page(&fixture("artist.json")).header.unwrap();
        assert_eq!(
            artist.channel_id.as_deref(),
            Some("UCJwGWV914kBlV4dKRn7AEFA")
        );
        assert_eq!(artist.subscribed, Some(false));
        assert_eq!(artist.subscribers, "4.41M");
        assert_eq!(artist.library_id, None);
        assert_eq!(artist.saved, None);
        assert!(!artist.editable);
        // Its own Shuffle (all their songs, shuffled) and Mix.
        assert_eq!(
            artist.shuffle(),
            Some(&Target::Watch {
                video_id: Some("G2PDJTkFiA8".into()),
                playlist_id: Some("RDAOA703AE9PNaNYb3T-XAhR9g".into()),
            })
        );
        assert_eq!(
            artist.radio(),
            Some(&Target::Watch {
                video_id: None,
                playlist_id: Some("RDEMA703AE9PNaNYb3T-XAhR9g".into()),
            })
        );
        // An album (signed out): its own playlist, from the play button.
        let album = page(&fixture("album.json")).header.unwrap();
        assert_eq!(
            album.library_id.as_deref(),
            Some("OLAK5uy_kW9hN-oBmekJ06jhhfStpwRd5pcRKIztY")
        );
        assert_eq!(album.saved, Some(false));
        assert_eq!(album.channel_id, None);
        assert!(!album.editable);
        // Its play button plays the album; no Shuffle or Mix of an artist.
        assert!(matches!(
            album.play(),
            Some(Target::Watch { playlist_id: Some(p), .. })
                if p == "OLAK5uy_kW9hN-oBmekJ06jhhfStpwRd5pcRKIztY"
        ));
        assert_eq!(album.shuffle(), None);
        assert_eq!(album.radio(), None);
        // A page with neither (a mood's) keeps no buttons at all.
        let mood = page(&serde_json::json!({
            "header": {"musicHeaderRenderer": {"title": {"runs": [{"text": "Chill"}]}}}
        }));
        assert_eq!(mood.header.unwrap().buttons, None);
        // The account's own playlist: editable, and no Save button.
        let own = page(&fixture("playlist_signed_in_premium.json"))
            .header
            .unwrap();
        assert!(own.editable);
        assert_eq!(
            own.library_id.as_deref(),
            Some("PLaZPMsuQNCsWn0iVMtGbaUXO6z-EdZaZm")
        );
        assert_eq!(own.saved, None);
        // Someone else's playlist (signed out).
        let other = page(&fixture("playlist_collaborative.json"))
            .header
            .unwrap();
        assert!(!other.editable);
        assert_eq!(
            other.library_id.as_deref(),
            Some("PLxyTaDz8f5PBc-8kE36gvB-eflhODG2dw")
        );
        assert_eq!(other.saved, Some(false));
    }

    #[test]
    fn signed_in_save_buttons() {
        // An album's Save button signed in, as in ytmusicapi's saved
        // album reply (March 2024).
        let album = |saved: bool| {
            serde_json::json!({"contents": {"twoColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [
                {"musicResponsiveHeaderRenderer": {
                    "title": {"runs": [{"text": "Revival"}]},
                    "buttons": [
                        {"toggleButtonRenderer": {
                            "isToggled": saved,
                            "defaultIcon": {"iconType": "LIBRARY_ADD"},
                            "defaultServiceEndpoint": {"likeEndpoint": {"status": "LIKE", "target": {"playlistId": "OLAK5uy_saves"}}},
                            "toggledIcon": {"iconType": "LIBRARY_SAVED"},
                            "toggledServiceEndpoint": {"likeEndpoint": {"status": "INDIFFERENT", "target": {"playlistId": "OLAK5uy_saves"}}}
                        }},
                        {"musicPlayButtonRenderer": {"playNavigationEndpoint": {"watchEndpoint": {"videoId": "abcdefghijk", "playlistId": "OLAK5uy_plays"}}}}
                    ]
                }}
            ]}}}}]}}})
        };
        let saved = page(&album(true)).header.unwrap();
        assert_eq!(saved.saved, Some(true));
        assert_eq!(saved.library_id.as_deref(), Some("OLAK5uy_saves"));
        assert_eq!(page(&album(false)).header.unwrap().saved, Some(false));

        // An older header: a like button among its menu's buttons.
        let older = serde_json::json!({"header": {"musicDetailHeaderRenderer": {
            "title": {"runs": [{"text": "Old album"}]},
            "menu": {"menuRenderer": {"topLevelButtons": [
                {"buttonRenderer": {"navigationEndpoint": {"watchPlaylistEndpoint": {"playlistId": "OLAK5uy_old"}}}},
                {"likeButtonRenderer": {"likeStatus": "LIKE", "target": {"playlistId": "OLAK5uy_old"}}}
            ]}}
        }}});
        let header = page(&older).header.unwrap();
        assert_eq!(header.saved, Some(true));
        assert_eq!(header.library_id.as_deref(), Some("OLAK5uy_old"));
    }

    #[test]
    fn a_shelf_header_with_its_strapline_and_picture() {
        // As a signed-in Home's "Listen again" comes (keys seen on the live
        // page; the words made up).
        let reply = serde_json::json!({"contents": [{"musicCarouselShelfRenderer": {
            "header": {"musicCarouselShelfBasicHeaderRenderer": {
                "title": {"runs": [{"text": "Listen again"}]},
                "strapline": {"runs": [{"text": "SAMPLE LISTENER"}]},
                "headerStyle": "MUSIC_CAROUSEL_SHELF_BASIC_HEADER_STYLE_DEFAULT",
                "thumbnail": {"musicThumbnailRenderer": {
                    "thumbnail": {"thumbnails": [{"url": "https://yt3.ggpht.com/sample=s88", "width": 0, "height": 0}]},
                    "thumbnailCrop": "MUSIC_THUMBNAIL_CROP_CIRCLE"
                }}
            }},
            "contents": [{"musicTwoRowItemRenderer": {
                "title": {"runs": [{"text": "A playlist"}]},
                "navigationEndpoint": {"browseEndpoint": {"browseId": "VLPLsample"}}
            }}]
        }}]});
        let page = page(&reply);
        let shelf = &page.sections[0];
        assert_eq!(shelf.title, "Listen again");
        assert_eq!(shelf.strapline, "SAMPLE LISTENER");
        assert!(shelf.round_picture);
        assert_eq!(
            shelf.picture.as_ref().map(|p| p.url.as_str()),
            Some("https://yt3.ggpht.com/sample=s88")
        );
        // A shelf without them has none.
        let home = page_without_extras();
        assert!(
            home.sections
                .iter()
                .all(|s| s.strapline.is_empty() && s.picture.is_none())
        );
    }

    fn page_without_extras() -> Page {
        page(&fixture("artist.json"))
    }

    #[test]
    fn home_moods_come_first() {
        let reply = serde_json::json!({"contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {
            "contents": [
                {"musicCarouselShelfRenderer": {
                    "header": {"musicCarouselShelfBasicHeaderRenderer": {"title": {"runs": [{"text": "Listen again"}]}}},
                    "contents": [{"musicTwoRowItemRenderer": {
                        "title": {"runs": [{"text": "An album"}]},
                        "navigationEndpoint": {"browseEndpoint": {"browseId": "MPREb_album01"}}
                    }}]
                }}
            ],
            "header": {"chipCloudRenderer": {"chips": [
                {"chipCloudChipRenderer": {
                    "text": {"runs": [{"text": "Energize"}]},
                    "navigationEndpoint": {"browseEndpoint": {"browseId": "FEmusic_home", "params": "ggMPOg1uX1BmNzc2V2p0YXJ5"}},
                    "isSelected": false
                }},
                {"chipCloudChipRenderer": {
                    "text": {"runs": [{"text": "Relax"}]},
                    "navigationEndpoint": {"browseEndpoint": {"browseId": "FEmusic_home", "params": "ggMPOg1uX1JtNHZ1a0RYWFhK"}}
                }},
                // Nothing to open: left out.
                {"chipCloudChipRenderer": {"text": {"runs": [{"text": "Broken"}]}}}
            ]}}
        }}}}]}}});
        let home = page(&reply);
        assert_eq!(titles(&home), ["", "Listen again"]);
        let moods = &home.sections[0];
        assert_eq!(moods.shape, Shape::Grid);
        assert_eq!(moods.items.len(), 2);
        let Item::Card(energize) = &moods.items[0] else {
            panic!("a card")
        };
        assert_eq!(energize.title, "Energize");
        assert_eq!(energize.thumbnail, None);
        assert_eq!(
            energize.open,
            Some(Target::Browse {
                id: "FEmusic_home".into(),
                kind: PageKind::Other,
                params: Some("ggMPOg1uX1BmNzc2V2p0YXJ5".into())
            })
        );

        // Search's filter buttons search instead.
        let search = serde_json::json!({"contents": {"tabbedSearchResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {
            "header": {"chipCloudRenderer": {"chips": [
                {"chipCloudChipRenderer": {
                    "text": {"runs": [{"text": "Songs"}]},
                    "navigationEndpoint": {"searchEndpoint": {"query": "daft punk", "params": "EgWKAQIIAWoMEA4QChADEAQQCRAF"}}
                }}
            ]}},
            "contents": [{"musicShelfRenderer": {
                "title": {"runs": [{"text": "Songs"}]},
                "contents": [{"musicResponsiveListItemRenderer": {"playlistItemData": {"videoId": "abcdefghijk"}}}]
            }}]
        }}}}]}}});
        let search = page(&search);
        assert_eq!(titles(&search), ["", "Songs"]);
        let Item::Card(songs) = &search.sections[0].items[0] else {
            panic!("a chip")
        };
        assert_eq!(
            songs.open,
            Some(Target::Search {
                query: "daft punk".into(),
                params: Some("EgWKAQIIAWoMEA4QChADEAQQCRAF".into())
            })
        );
    }

    #[test]
    fn top_result_and_up_next_link_artist_and_album() {
        let link = |words: &str, id: &str, page_type: &str| {
            serde_json::json!({"text": words, "navigationEndpoint": {"browseEndpoint": {
                "browseId": id,
                "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {"pageType": page_type}}
            }}})
        };
        let artist = "MUSIC_PAGE_TYPE_ARTIST";
        let album = "MUSIC_PAGE_TYPE_ALBUM";
        let search = serde_json::json!({"contents": [{"musicCardShelfRenderer": {
            "title": {"runs": [{"text": "One More Time", "navigationEndpoint": {"watchEndpoint": {"videoId": "searchSong1"}}}]},
            "subtitle": {"runs": [
                {"text": "Song"}, {"text": " • "},
                link("Daft Punk", "UCdaftpunk01", artist), {"text": " • "},
                link("Discovery", "MPREb_discovery", album), {"text": " • "},
                {"text": "5:20"}
            ]}
        }}]});
        let Item::Track(top) = &page(&search).sections[0].items[0] else {
            panic!("a song")
        };
        assert_eq!(top.artist_id.as_deref(), Some("UCdaftpunk01"));
        assert_eq!(top.album_id.as_deref(), Some("MPREb_discovery"));
        assert_eq!(top.album.as_deref(), Some("Discovery"));

        let next = serde_json::json!({"playlistPanelRenderer": {"contents": [{"playlistPanelVideoRenderer": {
            "videoId": "nextSong001",
            "title": {"runs": [{"text": "First"}]},
            "longBylineText": {"runs": [
                link("Artist One", "UCartist0001", artist), {"text": " & "},
                link("Artist Two", "UCartist0002", artist), {"text": " • "},
                link("Album One", "MPREb_album0001", album), {"text": " • "},
                {"text": "2020"}
            ]}
        }}]}});
        let tracks = up_next(&next);
        assert_eq!(tracks[0].artists, "Artist One & Artist Two");
        assert_eq!(tracks[0].artist_id.as_deref(), Some("UCartist0001"));
        assert_eq!(tracks[0].album_id.as_deref(), Some("MPREb_album0001"));
        assert_eq!(tracks[0].album.as_deref(), Some("Album One"));
    }

    #[test]
    fn a_video_as_the_top_result_says_so() {
        // The top result's link says what it plays, as ytmusicapi reads
        // it: in the title's link, or else in `onTap`.
        let top = |title_link: Value, on_tap: Value| {
            let reply = serde_json::json!({"contents": [{"musicCardShelfRenderer": {
                "title": {"runs": [{"text": "Never Gonna Give You Up", "navigationEndpoint": title_link}]},
                "subtitle": {"runs": [{"text": "Video"}, {"text": " • "}, {"text": "Rick Astley"}, {"text": " • "}, {"text": "3:33"}]},
                "onTap": on_tap
            }}]});
            match page(&reply).sections[0].items.first() {
                Some(Item::Track(t)) => t.clone(),
                other => panic!("a song, not {other:?}"),
            }
        };
        let watch = |kind: Option<&str>| {
            let mut endpoint = serde_json::json!({"watchEndpoint": {"videoId": "dQw4w9WgXcQ"}});
            if let Some(kind) = kind {
                endpoint["watchEndpoint"]["watchEndpointMusicSupportedConfigs"] =
                    serde_json::json!({"watchEndpointMusicConfig": {"musicVideoType": kind}});
            }
            endpoint
        };
        let video = top(watch(Some("MUSIC_VIDEO_TYPE_OMV")), Value::Null);
        assert_eq!(video.kind, TrackKind::MusicVideo);
        assert_eq!(video.artists, "Rick Astley");
        let video = top(watch(None), watch(Some("MUSIC_VIDEO_TYPE_OMV")));
        assert_eq!(video.kind, TrackKind::MusicVideo);
        let song = top(watch(Some("MUSIC_VIDEO_TYPE_ATV")), Value::Null);
        assert_eq!(song.kind, TrackKind::Song);
        assert_eq!(top(watch(None), Value::Null).kind, TrackKind::Unknown);
    }

    #[test]
    fn the_library_grid_and_its_next_items() {
        // The library's playlists in the layout ytmusicapi's
        // `get_library_playlists` reads. Its first tile ("New playlist")
        // opens a dialog, not a page or a song: left out.
        let tile = |name: &str, id: &str| {
            serde_json::json!({"musicTwoRowItemRenderer": {
                "title": {"runs": [{"text": name}]},
                "subtitle": {"runs": [{"text": "Playlist"}]},
                "navigationEndpoint": {"browseEndpoint": {
                    "browseId": id,
                    "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {
                        "pageType": "MUSIC_PAGE_TYPE_PLAYLIST"
                    }}
                }}
            }})
        };
        let first = serde_json::json!({"contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {
            "contents": [{"gridRenderer": {
                "items": [
                    {"musicTwoRowItemRenderer": {
                        "title": {"runs": [{"text": "New playlist"}]},
                        "navigationEndpoint": {"commandExecutorCommand": {"commands": []}}
                    }},
                    tile("Liked Music", "VLLM"),
                    tile("Road trip", "VLPLroad")
                ],
                "continuations": [{"nextContinuationData": {"continuation": "GRID2"}}]
            }}]
        }}}}]}}});
        let mut page = page(&first);
        let names = |page: &Page| -> Vec<String> {
            page.sections
                .iter()
                .flat_map(|s| &s.items)
                .map(|i| match i {
                    Item::Card(c) => c.title.clone(),
                    Item::Track(t) => t.title.clone(),
                })
                .collect()
        };
        assert_eq!(names(&page), ["Liked Music", "Road trip"]);
        assert_eq!(page.sections[0].shape, Shape::Grid);
        // The next items, at the end of the same grid.
        let more = serde_json::json!({"continuationContents": {"gridContinuation": {
            "items": [tile("Gym", "VLPLgym"), tile("Sleep", "VLPLsleep")]
        }}});
        page.extend_list(more_items(&more));
        assert_eq!(names(&page), ["Liked Music", "Road trip", "Gym", "Sleep"]);
        // A newer reply with more: its items, not the token's row.
        let newer = serde_json::json!({"onResponseReceivedActions": [{"appendContinuationItemsAction": {"continuationItems": [
            tile("Focus", "VLPLfocus"),
            {"continuationItemRenderer": {"continuationEndpoint": {"continuationCommand": {"token": "GRID3"}}}}
        ]}}]});
        assert_eq!(more_items(&newer).len(), 1);
        // The library's artists: a list of rows that open their pages.
        let artists = serde_json::json!({"continuationContents": {"musicShelfContinuation": {"contents": [
            {"musicResponsiveListItemRenderer": {
                "navigationEndpoint": {"browseEndpoint": {"browseId": "MPLAUCartist01"}},
                "flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "An artist"}]}}}]
            }}
        ]}}});
        assert!(matches!(&more_items(&artists)[..], [Item::Card(c)] if c.title == "An artist"));
        assert!(more_items(&Value::Null).is_empty());
    }

    #[test]
    fn mood_page_title() {
        let reply = serde_json::json!({
            "header": {"musicHeaderRenderer": {"title": {"runs": [{"text": "Chill"}]}}},
            "contents": {}
        });
        let header = page(&reply).header.expect("a header");
        assert_eq!(header.title, "Chill");
        assert!(header.thumbnail.is_none());
    }
}
