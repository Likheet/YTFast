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
    /// servers take the size in the address (`=w226-h226-...`); other
    /// pictures (video stills) come as they are.
    pub fn sized(&self, pixels: u32) -> String {
        let resizable =
            self.url.contains("googleusercontent.com") || self.url.contains("ggpht.com");
        match self.url.rfind("=w") {
            Some(at) if resizable => format!("{}=w{pixels}-h{pixels}-l90-rj", &self.url[..at]),
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
    /// The section's songs, in order.
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
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub header: Option<Header>,
    pub sections: Vec<Section>,
}

impl Page {
    /// Every song on the page, in order (for playing a whole album).
    pub fn tracks(&self) -> Vec<Track> {
        self.sections.iter().flat_map(Section::tracks).collect()
    }

    /// An album's songs come without a picture or an album name: they are
    /// the album's, as YouTube Music shows them.
    pub fn fill_album_songs(&mut self) {
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
                }
            }
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
    walk(reply, &mut sections);
    sections.retain(|s| !s.items.is_empty());
    Page { header, sections }
}

/// The row of buttons above Home's shelves (Energize, Relax, Workout...),
/// as a section of cards without pictures. Each opens Home for that mood.
/// Search's filter buttons search rather than open a page, and are left
/// out.
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
            let browse = chip.pointer("/navigationEndpoint/browseEndpoint")?;
            let open = Target::Browse {
                id: browse.get("browseId")?.as_str()?.to_string(),
                kind: PageKind::Other,
                params: browse
                    .get("params")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            };
            Some(Item::Card(Card {
                title,
                subtitle: String::new(),
                thumbnail: None,
                round: false,
                open: Some(open),
                play: None,
            }))
        })
        .collect();
    if items.is_empty() {
        return None;
    }
    Some(Section {
        title: String::new(),
        items,
        more: None,
        shape: Shape::Grid,
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
fn walk(node: &Value, out: &mut Vec<Section>) {
    match node {
        Value::Object(map) => {
            for (key, value) in map {
                if SHELVES.contains(&key.as_str()) {
                    out.extend(shelf(key, value));
                } else if !HEADERS.contains(&key.as_str()) {
                    walk(value, out);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
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
        });

    let mut items = Vec::new();
    if key == "musicCardShelfRenderer" {
        // The top search result: one big card, then a few songs.
        if let Some(card) = top_result(shelf) {
            items.push(card);
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
    vec![Section {
        title,
        items,
        more,
        shape,
    }]
}

fn item(row: &Value) -> Option<Item> {
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
        return Some(Item::Card(Card {
            title: button.get("buttonText").and_then(text)?,
            subtitle: String::new(),
            thumbnail: None,
            round: false,
            open: button.get("clickCommand").and_then(Target::from_endpoint),
            play: None,
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
        }));
    }
    None
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
    })
}

/// The big "Top result" card of a search.
fn top_result(shelf: &Value) -> Option<Item> {
    let title = shelf.get("title").and_then(text)?;
    let open = shelf
        .pointer("/title/runs/0/navigationEndpoint")
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
        return Some(Item::Track(Track {
            video_id: video_id.clone(),
            set_video_id: None,
            title,
            artists: byline.artists,
            album: byline.album.or(links.album),
            duration_seconds: byline.duration_seconds,
            kind: TrackKind::Unknown,
            thumbnail,
            artist_id: links.artist_id,
            album_id: links.album_id,
        }));
    }
    Some(Item::Card(Card {
        title,
        subtitle,
        thumbnail,
        round: shelf.get("thumbnail").is_some_and(is_round),
        open,
        play: play_button(shelf),
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
    let mut header = Header {
        title: t("title"),
        subtitle,
        detail: t("secondSubtitle"),
        owner: t("straplineTextOne"),
        thumbnail: h.get("thumbnail").and_then(Thumb::best),
        round: key == "musicImmersiveHeaderRenderer",
        ..Header::default()
    };
    // An artist's (or a channel's) Subscribe button.
    if let Some(button) = find_key(h, "subscribeButtonRenderer") {
        header.channel_id = button
            .get("channelId")
            .or_else(|| find_key(button, "channelIds").and_then(|ids| ids.get(0)))
            .and_then(Value::as_str)
            .map(str::to_string);
        header.subscribed = button.get("subscribed").and_then(Value::as_bool);
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

/// The songs of an Up next or radio list (the `next` reply). Unplayable
/// rows are left out; where a song also exists as a video, the song is
/// kept.
pub fn up_next(reply: &Value) -> Vec<Track> {
    let mut panels = Vec::new();
    collect(reply, "playlistPanelRenderer", &mut panels);
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
        // Filled in from the album.
        let mut page = page;
        page.fill_album_songs();
        for track in page.tracks() {
            assert_eq!(track.album.as_deref(), Some("17"));
            assert_eq!(track.thumbnail, header.thumbnail);
        }
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
    fn up_next_list() {
        let tracks = up_next(&fixture("next_synthetic.json"));
        let ids: Vec<&str> = tracks.iter().map(|t| t.video_id.as_str()).collect();
        // The video counterpart and the unplayable row are left out.
        assert_eq!(ids, ["nextSong001", "nextSong002"]);
        assert_eq!(tracks[0].artists, "Artist One");
        assert_eq!(tracks[0].album.as_deref(), Some("Album One"));
        assert_eq!(tracks[0].duration_seconds, Some(215));
        assert_eq!(tracks[1].kind, TrackKind::Song);
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
        let still = Thumb {
            url: "https://i.ytimg.com/vi/x/hqdefault.jpg".into(),
            width: 480,
        };
        assert_eq!(still.sized(96), still.url);
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
        assert_eq!(artist.library_id, None);
        assert_eq!(artist.saved, None);
        assert!(!artist.editable);
        // An album (signed out): its own playlist, from the play button.
        let album = page(&fixture("album.json")).header.unwrap();
        assert_eq!(
            album.library_id.as_deref(),
            Some("OLAK5uy_kW9hN-oBmekJ06jhhfStpwRd5pcRKIztY")
        );
        assert_eq!(album.saved, Some(false));
        assert_eq!(album.channel_id, None);
        assert!(!album.editable);
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
    fn home_moods_come_first() {
        let reply = serde_json::json!({"contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {
            "contents": [
                {"musicCarouselShelfRenderer": {
                    "header": {"musicCarouselShelfBasicHeaderRenderer": {"title": {"runs": [{"text": "Listen again"}]}}},
                    "contents": [{"musicTwoRowItemRenderer": {"title": {"runs": [{"text": "An album"}]}}}]
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

        // Search's filter buttons search instead: no section for them.
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
        assert_eq!(titles(&page(&search)), ["Songs"]);
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
