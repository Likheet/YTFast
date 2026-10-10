//! The page in the middle: Home, Explore, Library, search results, an
//! album, a playlist or an artist. All of them are a header and sections,
//! laid out as YouTube Music lays them out: an album or playlist with its
//! cover and buttons on the left and its songs on the right, an artist
//! under a wide picture, everything else as shelves under one another.

use egui::{Align, Color32, CornerRadius, Layout, Rect, Sense, UiBuilder, Vec2, pos2, vec2};
use ytfast_core::read::{
    Card, Header, Item, Page, Section, Shape, SortMenu, Target, TopResult, Track,
};

use crate::app::{Action, App, Dialog, Loadable, QueueMode};
use crate::backend::Route;
use crate::theme::{self, Icon, PALETTE, Pill, Round};
use crate::views::backdrop;
use crate::views::widgets::{self, Row};

/// How a page's sections are drawn: large on a page, smaller in the
/// player page's side panel.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    /// The size of section titles.
    pub title: f32,
    /// The size of a list's title (an artist's Top songs).
    pub list_title: f32,
    /// The side of a card's cover.
    pub card: f32,
    /// The space between cards in a shelf.
    pub gap: f32,
    /// Song rows in lists.
    pub rows: Row,
    /// The width of one column of songs in a shelf that scrolls sideways.
    pub column: f32,
    /// The space between those columns.
    pub column_gap: f32,
    /// The space above a shelf's title.
    pub above_title: f32,
    /// The space after each shelf but the last.
    pub between: f32,
}

impl Look {
    /// For shelves `width` wide on `grid`; `beside_header` on an album's
    /// or playlist's page, whose shelves have small cards.
    fn page(route: &Route, grid: &theme::Grid, width: f32, beside_header: bool) -> Self {
        let (card, gap) = grid.cards(width, beside_header);
        let rows = match route {
            Route::Search(_) => Row::SEARCH,
            Route::SearchOnly(..) => Row::SEARCH_ONLY,
            Route::Liked => Row::PLAYLIST,
            Route::Browse { id, .. } if id.starts_with("VL") => Row::PLAYLIST,
            _ if is_album(route) => Row::LIST,
            _ => Row::SHELF,
        }
        .themed();
        Self {
            // Beside an album's or playlist's header, 28 at every width
            // (`--rhs-subheading-font-size`).
            // Premium: 28 from 1150, else 24.
            title: if theme::premium() {
                if grid.window >= 1150.0 { 28.0 } else { 24.0 }
            } else if beside_header {
                28.0
            } else {
                theme::display1(grid.window)
            },
            list_title: theme::display2(grid.window),
            card,
            gap,
            rows,
            column: grid.song_column(width),
            column_gap: grid.song_column_gap(),
            above_title: grid.above_title(),
            between: grid.between_shelves(),
        }
    }

    /// In the player page's side panel (its Related tab, measured: cards
    /// 160 and columns of songs 216, both 16 apart).
    pub const PANEL: Self = Self {
        title: 24.0,
        list_title: 24.0,
        card: 160.0,
        gap: 16.0,
        rows: Row::GRID,
        column: 216.0,
        column_gap: 16.0,
        above_title: 32.0,
        between: 24.0,
    };
}

/// The space between cards in a grid that wraps (the Library).
const CARD_GAP: f32 = 24.0;

pub fn show(app: &App, ui: &mut egui::Ui) {
    let route = app.route.clone();
    let area = ui.max_rect();
    if route == Route::Settings {
        app.page_scrolled.set(false);
        let window = ui.ctx().content_rect().width();
        let grid = theme::Grid::new(window, area.width());
        // Premium centres its column in the whole page (its scroll bar at
        // the window's edge).
        let inner = if theme::premium() {
            area
        } else {
            Rect::from_min_size(
                pos2(area.left() + grid.left, area.top()),
                vec2(grid.width, area.height()),
            )
        };
        let mut ui = ui.new_child(UiBuilder::new().max_rect(inner));
        crate::views::settings::show(app, &mut ui);
        return;
    }
    match app.pages.get(&route) {
        None | Some(Loadable::Loading) => {
            if let Some((previous, page)) = stand_in(app, &route) {
                // The page just left stays, dimmed, while this one loads,
                // its button for this one lit at once.
                app.standing_in.set(true);
                let shown = egui::ScrollArea::vertical()
                    .id_salt(("page", previous))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_opacity(0.5);
                        one_column(app, ui, previous, page);
                    });
                app.standing_in.set(false);
                scrolled(app, ui, shown.state.offset.y);
                let spot = Rect::from_center_size(
                    pos2(area.center().x, area.top() + 140.0),
                    Vec2::splat(32.0),
                );
                ui.put(
                    spot,
                    egui::Spinner::new().size(32.0).color(PALETTE.secondary),
                );
                return;
            }
            app.page_scrolled.set(false);
            ui.add_space(96.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(32.0).color(PALETTE.secondary));
            });
        }
        Some(Loadable::Failed(message)) => {
            app.page_scrolled.set(false);
            ui.add_space(96.0);
            ui.vertical_centered(|ui| {
                theme::label(
                    ui,
                    "This page could not load",
                    theme::bold(24.0),
                    PALETTE.text,
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(message)
                        .font(theme::regular(14.0))
                        .color(PALETTE.secondary),
                );
                ui.add_space(16.0);
                if theme::pill_button(ui, "Try again", true).clicked() {
                    app.act(Action::Retry(route.clone()));
                }
            });
        }
        Some(Loadable::Ready(page)) => {
            match &page.header {
                // An album or a playlist, in a window wide enough for both
                // columns.
                Some(header)
                    if !theme::premium()
                        && in_two_columns(header, ui.ctx().content_rect().width()) =>
                {
                    two_columns(app, ui, &route, page, header);
                }
                _ => {
                    let shown = from_top(app, egui::ScrollArea::vertical())
                        .id_salt(("page", &route))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            scroll_by_keys(ui);
                            one_column(app, ui, &route, page);
                        });
                    scrolled(app, ui, shown.state.offset.y);
                }
            }
            app.fresh_page.set(false);
        }
    }
}

/// Notes whether the page is scrolled from its top, for the top bar; when
/// that changes, the bar is drawn again.
fn scrolled(app: &App, ui: &egui::Ui, offset: f32) {
    if app.page_offset.replace(offset) != offset {
        ui.ctx().request_repaint();
    }
    let now = offset > 0.5;
    if app.page_scrolled.replace(now) != now {
        ui.ctx().request_repaint();
    }
}

/// A page opened anew starts at its top (Back and Forward return to where
/// it was left).
fn from_top(app: &App, area: egui::ScrollArea) -> egui::ScrollArea {
    if app.fresh_page.get() {
        area.vertical_scroll_offset(0.0)
    } else {
        area
    }
}

/// While `route` loads: the page just left, when it is the same page with
/// another of its buttons chosen (Home and its moods, Podcasts...; Liked
/// Music and its filters) and is still here, so the page does not empty
/// and fill again.
fn stand_in<'a>(app: &'a App, route: &Route) -> Option<(&'a Route, &'a Page)> {
    let previous = app.came_from()?;
    if !same_page(previous, route) {
        return None;
    }
    match app.pages.get(previous) {
        Some(Loadable::Ready(page)) => Some((previous, page)),
        _ => None,
    }
}

/// Whether `a` and `b` are one page with different buttons chosen: the
/// same YouTube page ID, asked for differently.
fn same_page(a: &Route, b: &Route) -> bool {
    let id = |route: &Route| match route {
        Route::Home => Some("FEmusic_home".to_string()),
        Route::Liked => Some("VLLM".to_string()),
        Route::Browse { id, .. } => Some(id.clone()),
        _ => None,
    };
    a != b && id(a).is_some() && id(a) == id(b)
}

/// Page Up and Page Down, Home and End scroll the page, as in a browser.
fn scroll_by_keys(ui: &egui::Ui) {
    // Home and End move in a text box instead.
    if ui.ctx().text_edit_focused() {
        return;
    }
    use egui::{Key, Modifiers};
    let screen = ui.clip_rect().height() * 0.9;
    // Further than any page reaches: the page stops at its end.
    let all = 1.0e7;
    let delta = ui.input_mut(|i| {
        [
            (Key::PageDown, -screen),
            (Key::PageUp, screen),
            (Key::End, -all),
            (Key::Home, all),
        ]
        .into_iter()
        .find(|(key, _)| i.consume_key(Modifiers::NONE, *key))
        .map(|(_, delta)| delta)
    });
    if let Some(delta) = delta {
        ui.scroll_with_delta(vec2(0.0, delta));
    }
}

/// The playlist songs on this page come from, for more songs later.
fn source(route: &Route) -> Option<String> {
    route.playlist()
}

/// Home's row of moods at its top: buttons without pictures or icons.
fn is_chip_row(route: &Route, section: &Section) -> bool {
    *route == Route::Home
        && section.title.is_empty()
        && section.items.iter().all(|i| match i {
            Item::Card(card) => card.thumbnail.is_none() && card.look.stripe.is_none(),
            Item::Track(_) => false,
        })
}

/// Explore's three big buttons: buttons with icons.
fn is_big_buttons(section: &Section) -> bool {
    section.items.iter().any(|i| match i {
        Item::Card(card) => card.thumbnail.is_none() && card.look.icon.is_some(),
        Item::Track(_) => false,
    })
}

/// Search's results, all kinds together (not narrowed to one kind).
fn is_search(route: &Route) -> bool {
    matches!(route, Route::Search(_))
}

/// What a click on a song does on this page. In search results, as on
/// YouTube Music, the song plays and then its radio; elsewhere the songs
/// after it in the list follow.
fn play_song(route: &Route, tracks: &[Track], start: usize) -> Action {
    match route {
        Route::Search(_) | Route::SearchOnly(..) => {
            let track = tracks[start].clone();
            let radio = Target::Watch {
                video_id: Some(track.video_id.clone()),
                playlist_id: None,
            };
            Action::Play(radio, Some(track))
        }
        _ => Action::PlayTracks {
            tracks: tracks.to_vec(),
            start,
            source: source(route),
        },
    }
}

/// An album's page, whose songs are numbered. (The demo's albums have IDs
/// of their own.)
fn is_album(route: &Route) -> bool {
    matches!(route, Route::Browse { id, .. }
        if id.starts_with("MPRE") || id.starts_with("demo-album-"))
}

/// Whether a page this wide shows an album or playlist as two columns
/// (its header standing on the left) rather than one.
fn in_two_columns(header: &Header, window: f32) -> bool {
    // From a window 1150 wide, as YouTube Music.
    !is_artist(header) && header.thumbnail.is_some() && window >= 1150.0
}

/// An artist's (or a channel's) header: a wide picture, not a cover.
fn is_artist(header: &Header) -> bool {
    header.round || header.channel_id.is_some()
}

fn songs(section: &Section) -> impl Iterator<Item = &Track> {
    section.items.iter().filter_map(|i| match i {
        Item::Track(t) => Some(t),
        Item::Card(_) => None,
    })
}

/// A page as one column that scrolls: what stands at its top, then its
/// sections, between the page's margins.
fn one_column(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page) {
    let full = ui.available_rect_before_wrap();
    let window = ui.ctx().content_rect().width();
    let grid = if matches!(route, Route::Search(_) | Route::SearchOnly(..)) {
        theme::Grid::search(window, full.width())
    } else {
        theme::Grid::new(window, full.width())
    };
    let look = Look::page(route, &grid, grid.width, false);
    if let Some(header) = page.header.as_ref().filter(|h| is_artist(h)) {
        artist_header(app, ui, route, page, header, grid.left);
        // The header's margin.
        ui.add_space(24.0);
    }
    let inner = Rect::from_min_size(
        pos2(full.left() + grid.left, ui.cursor().top()),
        vec2(grid.width, (full.bottom() - ui.cursor().top()).max(0.0)),
    );
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            ui.set_width(inner.width());
            ui.spacing_mut().item_spacing.y = 0.0;
            match (&page.header, route) {
                (Some(header), _) if is_artist(header) => {}
                (Some(header), _) => {
                    if theme::premium() {
                        premium_header(app, ui, route, page, header);
                    } else {
                        stacked_header(app, ui, route, page, header, full.width() - 12.0);
                    }
                }
                (None, Route::Search(query) | Route::SearchOnly(query, _)) => {
                    ui.add_space(6.0);
                    search_chips(app, ui, route, page, query);
                }
                (None, route) if is_library(route) => library_header(app, ui, route, page),
                (None, Route::History) => title(ui, "History"),
                (None, Route::Home | Route::Explore) => {}
                _ => ui.add_space(8.0),
            }
            if page.sections.is_empty() {
                ui.add_space(32.0);
                theme::label(
                    ui,
                    "Nothing here yet.",
                    theme::regular(16.0),
                    PALETTE.secondary,
                );
            }
            sections(app, ui, route, page, &look);
            // A search of one kind loads its next results once its end
            // comes into view, as YouTube Music's does.
            if matches!(route, Route::SearchOnly(..)) {
                let (end, _) = ui.allocate_exact_size(vec2(1.0, 1.0), Sense::hover());
                if ui.is_rect_visible(end) {
                    app.act(Action::MoreResults(route.clone()));
                }
            }
            ui.add_space(theme::page_foot());
        },
    );
}

/// A page's sections, without its header, with YouTube Music's space
/// after each but the last: 24 after a shelf, 32 after a list (its
/// `ytmusic-shelf-renderer`; a top result card keeps 32 of its own).
pub fn sections(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page, look: &Look) {
    let count = page.sections.len();
    for (index, section) in page.sections.iter().enumerate() {
        // Search's filter buttons are drawn above (`search_chips`).
        if is_search_chips(section) {
            continue;
        }
        // A playlist's Sort, just over its songs (16 in, 16 above them).
        let first_songs = page
            .sections
            .iter()
            .position(|s| s.items.iter().any(|i| matches!(i, Item::Track(_))));
        if route.playlist().is_some() && first_songs == Some(index) {
            sort_control(app, ui, route);
            ui.add_space(16.0);
        }
        section_block(app, ui, route, section, index, look);
        if index + 1 < count && section.top.is_none() {
            let after = if is_chip_row(route, section) {
                // The chips' 8 and their margin of 6 (`chip-cloud`).
                14.0
            } else if is_big_buttons(section) {
                // Explore's buttons: 56 under them.
                56.0
            } else if section.shape == Shape::List {
                32.0
            } else {
                look.between
            };
            ui.add_space(after);
        }
    }
}

/// A playlist's Sort, as YouTube Music's: 16 in, a sort icon 24 and 8
/// after it "Sort" (14/500 white), 26.7 high; its menu lists the orders,
/// a tick before the one shown (YouTube Music's newest and oldest added
/// first are left out: YTFast cannot tell when a song was added).
fn sort_control(app: &App, ui: &mut egui::Ui, route: &Route) {
    use crate::app::PlaylistSort;
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.7), Sense::hover());
    let words = ui
        .painter()
        .layout_no_wrap("Sort".into(), theme::medium(14.0), PALETTE.text);
    let button = Rect::from_min_size(
        pos2(row.left() + 16.0, row.top()),
        vec2(24.0 + 8.0 + words.size().x, 26.7),
    );
    let response = ui.interact(button, ui.id().with(("sort", route)), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Sort"));
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let icon = Rect::from_min_size(
        pos2(button.left(), button.center().y - 12.0),
        Vec2::splat(24.0),
    );
    theme::paint_icon(ui, Icon::Sort, icon, 24.0, PALETTE.text);
    ui.painter().galley(
        pos2(icon.right() + 8.0, button.center().y - words.size().y / 2.0),
        words,
        PALETTE.text,
    );
    let chosen = app.playlist_sort.get(route).copied();
    theme::menu_popup(&response).show(|ui| {
        theme::menu(ui);
        let choices = std::iter::once((None, "Default ordering"))
            .chain(PlaylistSort::ALL.iter().map(|o| (Some(*o), o.words())));
        for (order, words) in choices {
            if theme::menu_choice(ui, order == chosen, words).clicked() {
                app.act(Action::SortPlaylist(route.clone(), order));
                ui.close();
            }
        }
    });
}

/// Search's filter buttons: a section of chips that search.
fn is_search_chips(section: &Section) -> bool {
    !section.items.is_empty()
        && section.items.iter().all(|i| {
            matches!(
                i,
                Item::Card(Card {
                    open: Some(Target::Search { .. }),
                    ..
                })
            )
        })
}

/// Search's filter buttons as YouTube Music sends them (Artists, Community
/// playlists, Songs...), 32 high and 12 apart, in a row that scrolls
/// sideways when it is too wide. With one chosen, it is white, and a white
/// square with × before the row goes back to all results (as choosing it
/// again does).
fn search_chips(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page, query: &str) {
    let chips: Vec<&Card> = page
        .sections
        .iter()
        .filter(|s| is_search_chips(s))
        .flat_map(|s| s.items.iter())
        .filter_map(|i| match i {
            Item::Card(card) => Some(card),
            Item::Track(_) => None,
        })
        .collect();
    let all = Route::Search(query.to_string());
    egui::ScrollArea::horizontal()
        .id_salt(("search-chips", query))
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if matches!(route, Route::SearchOnly(..)) {
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "All results")
                    });
                    let fill = if response.hovered() {
                        egui::Color32::from_rgb(0xd9, 0xd9, 0xd9)
                    } else {
                        PALETTE.text
                    };
                    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
                    theme::paint_icon(ui, Icon::Close, rect, 24.0, PALETTE.window);
                    if response.clicked() {
                        app.act(Action::Navigate(all.clone()));
                    }
                }
                for chip in &chips {
                    if theme::chip(ui, &chip.title, chip.look.chosen, 32.0).clicked() {
                        if chip.look.chosen {
                            app.act(Action::Navigate(all.clone()));
                        } else if let Some(target) = chip.open.clone() {
                            app.act(Action::Open(target, None));
                        }
                    }
                }
            });
        });
    // The results start 53.3 under the chips' top (measured at 1280).
    ui.add_space(21.3);
}

/// One of the Library's pages.
fn is_library(route: &Route) -> bool {
    route.library_tab().is_some()
}

/// The Library's top, as YouTube Music's (measured signed in): a row of
/// tabs (LIBRARY, 14/500 capitals, a white line 2 under it, a white@0.10
/// hairline under the row, 51.3 high); 26 under it the chips (32 high, 12
/// apart): on the front page Playlists, Songs, Albums and Artists, else a
/// white square with × (back to the front page) and the chosen one; at
/// the row's right its sort button; 36 under them the page.
fn library_header(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page) {
    if theme::premium() {
        premium_library_top(app, ui);
    } else {
        library_tabs(app, ui);
    }
    ui.add_space(26.0);
    // The row is 44 high, the chips 6 down it.
    let row_top = ui.cursor().top();
    let mut chips_right = ui.max_rect().left();
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let kinds = [
            (Route::Library, "Playlists"),
            (Route::LibrarySongs, "Songs"),
            (Route::LibraryAlbums, "Albums"),
            (Route::LibraryArtists, "Artists"),
            (Route::LibraryProfiles, "Profiles"),
            (Route::LibraryPodcasts, "Podcasts"),
        ];
        if theme::premium() {
            // Premium: every chip stays, the chosen one lit; choosing it
            // again goes back to the front page.
            for (kind, name) in kinds {
                let chosen = *route == kind;
                if theme::chip(ui, name, chosen, 32.0).clicked() {
                    app.act(Action::Navigate(if chosen {
                        Route::LibraryRecent
                    } else {
                        kind
                    }));
                }
            }
        } else if *route == Route::LibraryRecent {
            for (kind, name) in kinds {
                if theme::chip(ui, name, false, 32.0).clicked() {
                    app.act(Action::Navigate(kind));
                }
            }
        } else {
            let (square, cleared) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
            cleared.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "All of the library")
            });
            let fill = if cleared.hovered() {
                egui::Color32::from_rgb(0xd9, 0xd9, 0xd9)
            } else {
                PALETTE.text
            };
            ui.painter()
                .rect_filled(square, CornerRadius::same(8), fill);
            theme::paint_icon(ui, Icon::Close, square, 24.0, PALETTE.window);
            if cleared.clicked() {
                app.act(Action::Navigate(Route::LibraryRecent));
            }
            if let Some((_, name)) = kinds.iter().find(|(kind, _)| kind == route)
                && theme::chip(ui, name, true, 32.0).clicked()
            {
                app.act(Action::Navigate(Route::LibraryRecent));
            }
        }
        chips_right = ui.min_rect().right();
    });
    // The sort button at the row's top right; when it does not fit 24
    // after the chips, on a line of its own under them, at the left (the
    // row wraps, as YouTube Music's does at 960).
    let mut wrapped = false;
    if let Some(sort) = &page.sort {
        let content = ui.max_rect();
        sort_button(app, ui, sort, |size| {
            if chips_right + 24.0 + size.x <= content.right() {
                pos2(content.right() - size.x, row_top)
            } else {
                wrapped = true;
                pos2(content.left(), row_top + 44.0)
            }
        });
    }
    ui.add_space(6.0 + 36.0 + if wrapped { 36.0 } else { 0.0 });
}

/// Premium's top of the Library: "Your library", large; a click on it goes
/// back to the front page.
fn premium_library_top(app: &App, ui: &mut egui::Ui) {
    ui.add_space(32.0);
    let response = theme::label(
        ui,
        "Your library",
        theme::bold(theme::display1(ui.ctx().content_rect().width())),
        PALETTE.text,
    );
    let home = ui.interact(response.rect, ui.id().with("library-tab"), Sense::click());
    home.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "All of the library")
    });
    if home.clicked() {
        app.act(Action::Navigate(Route::LibraryRecent));
    }
}

/// YouTube Music's row of tabs over the Library: LIBRARY alone.
fn library_tabs(app: &App, ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(vec2(width, 51.3), Sense::hover());
    // The tabs' row starts 16 before the content, each tab 16 in.
    ui.painter().hline(
        (row.left() - 16.0)..=(row.right() - 16.0),
        row.bottom() - 0.5,
        egui::Stroke::new(1.0, PALETTE.outline),
    );
    let words = ui
        .painter()
        .layout_no_wrap("LIBRARY".into(), theme::medium(14.0), PALETTE.text);
    let tab = Rect::from_min_size(row.min, vec2(words.size().x, 50.7));
    let response = ui.interact(tab, ui.id().with("library-tab"), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, true, "Library")
    });
    ui.painter().galley(
        pos2(tab.left(), tab.center().y - words.size().y / 2.0),
        words,
        PALETTE.text,
    );
    ui.painter().hline(
        tab.x_range(),
        tab.bottom() - 1.0,
        egui::Stroke::new(2.0, PALETTE.text),
    );
    if response.clicked() {
        app.act(Action::Navigate(Route::LibraryRecent));
    }
}

/// A Library tab's sort button, as YouTube Music's
/// (`ytmusic-sort-filter-button-renderer`, measured signed in): the order
/// shown in 14/500 white, an 18 chevron 8 after it, padding 8 12 8 16, on
/// white@0.10 with a white@0.10 border, r 20, at most 272 wide, where
/// `place` puts it (its top left, for its size). Its menu (`ytmusic-multi-select-menu-renderer`)
/// opens 8 under it, at its right: `#212121`, a white@0.10 border, r 2,
/// 305 wide; "Sort by" (14 white) over a white@0.10 line, 63.3 down; then,
/// 8 down, one row 48 high per order, a white tick (24, 14 in) on the one
/// shown, the words 14 white at 54 in, white@0.05 under the pointer.
fn sort_button(
    app: &App,
    ui: &mut egui::Ui,
    sort: &SortMenu,
    place: impl FnOnce(Vec2) -> egui::Pos2,
) {
    // The order chosen in YTFast shows at once, before its page arrives.
    let chosen = sort
        .orders
        .first()
        .and_then(|o| app.settings.library_order.get(&o.browse_id))
        .and_then(|params| sort.orders.iter().find(|o| o.params == *params))
        .map_or(sort.chosen.as_str(), |o| o.title.as_str());
    let words = theme::fit(
        ui,
        chosen,
        theme::medium(14.0),
        PALETTE.text,
        272.0 - 16.0 - 8.0 - 18.0 - 12.0 - 1.3,
        1,
    );
    let size = vec2(16.0 + words.size().x + 8.0 + 18.0 + 12.0 + 1.3, 35.3);
    let rect = Rect::from_min_size(place(size), size);
    let response = ui.interact(rect, ui.id().with("sort"), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::ComboBox,
            true,
            format!("Sort by: {chosen}"),
        )
    });
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.painter().rect(
        rect,
        CornerRadius::same(20),
        PALETTE.surface,
        egui::Stroke::new(1.0, PALETTE.outline),
        egui::StrokeKind::Inside,
    );
    let at = pos2(rect.left() + 16.7, rect.center().y - words.size().y / 2.0);
    let chevron = Rect::from_min_size(
        pos2(at.x + words.size().x + 8.0, rect.center().y - 9.0),
        Vec2::splat(18.0),
    );
    ui.painter().galley(at, words, PALETTE.text);
    theme::paint_icon(ui, Icon::ExpandMore, chevron, 18.0, PALETTE.text);

    let frame = egui::Frame::new()
        .fill(PALETTE.panel)
        .stroke(egui::Stroke::new(1.0, theme::menu_edge()))
        .corner_radius(CornerRadius::same(2));
    egui::Popup::menu(&response)
        .frame(frame)
        .width(305.0)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(8.0)
        .show(|ui| {
            ui.set_width(305.0);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let (title, _) = ui.allocate_exact_size(vec2(305.0, 62.6), Sense::hover());
            ui.painter().text(
                pos2(title.left() + 28.0, title.top() + 22.0),
                egui::Align2::LEFT_TOP,
                "Sort by",
                theme::regular(14.0),
                PALETTE.text,
            );
            ui.painter().hline(
                title.x_range(),
                title.bottom() - 0.5,
                egui::Stroke::new(1.0, PALETTE.outline),
            );
            ui.add_space(8.0);
            for order in &sort.orders {
                let (row, pick) = ui.allocate_exact_size(vec2(305.0, 48.0), Sense::click());
                let shown = order.title == chosen;
                pick.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Button, true, shown, &order.title)
                });
                if pick.hovered() || pick.has_focus() {
                    ui.painter()
                        .rect_filled(row, 0.0, egui::Color32::from_white_alpha(13));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if shown {
                    let tick = Rect::from_min_size(
                        pos2(row.left() + 14.0, row.top() + 12.0),
                        Vec2::splat(24.0),
                    );
                    theme::paint_icon(ui, Icon::Check, tick, 24.0, PALETTE.text);
                }
                theme::paint_line(
                    ui,
                    pos2(row.left() + 54.0, row.top() + 14.2),
                    &order.title,
                    theme::regular(14.0),
                    PALETTE.text,
                    305.0 - 54.0 - 16.0,
                );
                if pick.clicked() {
                    if !shown {
                        app.act(Action::SortLibrary(order.clone()));
                    }
                    ui.close();
                }
            }
            ui.add_space(8.0);
        });
}

/// The Library's grids (`ytmusic-grid-renderer[grid-type=library]`): as
/// many cards across as the window allows (5 under 1150, 6, 7 from 1364, 8
/// from 1578, 9 from 1800), 16 apart (24 from 1364), rows 40 apart; the
/// card's side, `width` shared out.
fn library_grid(window: f32, width: f32) -> (f32, f32) {
    // Premium: the same cards as the shelves elsewhere.
    if theme::premium() {
        let (card, gap) = theme::Grid::new(window, width).cards(width, false);
        return ((card - 0.01).max(0.0), gap);
    }
    let across: f32 = if window >= 1800.0 {
        9.0
    } else if window >= 1578.0 {
        8.0
    } else if window >= 1364.0 {
        7.0
    } else if window >= 1150.0 {
        6.0
    } else {
        5.0
    };
    let gap = if window >= 1364.0 { 24.0 } else { 16.0 };
    // A hair under, so the last card is not wrapped by rounding.
    (
        ((width - gap * (across - 1.0)) / across - 0.01).max(80.0),
        gap,
    )
}

fn title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(24.0);
    let size = theme::display1(ui.ctx().content_rect().width());
    theme::label(ui, text, theme::bold(size), PALETTE.text);
    ui.add_space(8.0);
}

// ---- An album or a playlist ----

/// The cover, the words and the buttons on the left, standing still; the
/// songs (and whatever follows them) on the right, scrolling. Placed as
/// YouTube Music places them (`ytmusic-two-column-browse-results-renderer`,
/// measured at 1280: the pair 882 wide, the header 280 at x 329, the songs
/// 546 at x 649 with the menu open).
fn two_columns(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page, header: &Header) {
    let area = ui.max_rect();
    let window = ui.ctx().content_rect().width();
    album_backdrop(app, ui, header);
    // The columns' widest (`--max-width-lhs`, `--max-width-rhs`), by window.
    let (most_left, most_right) = if window >= 1578.0 {
        (477.0, 977.0)
    } else if window >= 1364.0 {
        (405.0, 835.0)
    } else {
        (424.0, 602.0)
    };
    // The page less YouTube's scroll bar room.
    let room = area.width() - 12.0;
    let left = (room - 32.0).min(most_left - 56.0) - 56.0;
    let right = most_right - 56.0;
    let pair = left + 24.0 + right;
    let x0 = area.left() + ((room - pair) / 2.0).max(0.0);
    let first = Rect::from_min_max(
        pos2(x0 + 16.0, area.top()),
        pos2(x0 + left - 16.0, area.bottom()),
    );
    // The songs scroll with their bar at the window's edge.
    let second = Rect::from_min_max(pos2(x0 + left + 24.0, area.top()), area.max);
    let second_width = right.min(area.right() - second.left());

    let mut first_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(first)
            .layout(Layout::top_down(Align::Center)),
    );
    from_top(app, egui::ScrollArea::vertical())
        .id_salt(("page-header", route))
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(&mut first_ui, |ui| {
            ui.add_space(64.0);
            header_column(app, ui, route, page, header);
            ui.add_space(32.0);
        });

    let grid = theme::Grid::new(window, area.width());
    let look = Look::page(route, &grid, second_width, true);
    let mut second_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(second)
            .layout(Layout::top_down(Align::Min)),
    );
    let shown = from_top(app, egui::ScrollArea::vertical())
        .id_salt(("page", route))
        .auto_shrink([false, false])
        .show(&mut second_ui, |ui| {
            scroll_by_keys(ui);
            ui.set_max_width(second_width);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(64.0);
            if page.sections.is_empty() {
                theme::label(
                    ui,
                    "No songs here yet.",
                    theme::regular(16.0),
                    PALETTE.secondary,
                );
            }
            sections(app, ui, route, page, &look);
            ui.add_space(theme::page_foot());
        });
    scrolled(app, ui, shown.state.offset.y);
}

/// Behind an album's or playlist's page: its cover, blurred, under a dark
/// gradient (`backdrop::album_cover`), moving up with the page and fading
/// in over 0.5 s once the cover has arrived.
fn album_backdrop(app: &App, ui: &egui::Ui, header: &Header) {
    if header.thumbnail.is_some() {
        app.page_backdrop.set(true);
    }
    let (Some(thumb), Some(slot)) = (&header.thumbnail, app.backdrop_slot.get()) else {
        return;
    };
    let ctx = ui.ctx();
    let Some((texture, since)) = app.page_cover(ctx, &thumb.sized(120)) else {
        return;
    };
    let shown = ((ctx.input(|i| i.time) - since) / 0.5).clamp(0.0, 1.0) as f32;
    if shown < 1.0 {
        ctx.request_repaint();
    }
    backdrop::album_cover(
        ui,
        slot,
        ctx.content_rect(),
        texture,
        shown,
        app.page_offset.get(),
    );
}

/// The same header above the songs, for a narrow window: as YouTube Music
/// below 1150, up to 500 wide, centred in the page (`page`, its width less
/// the scroll bar's room), 24 from the top and 36 above the songs.
fn stacked_header(
    app: &App,
    ui: &mut egui::Ui,
    route: &Route,
    page: &Page,
    header: &Header,
    room: f32,
) {
    if header.thumbnail.is_none() {
        // A mood or genre: only words.
        title(ui, &header.title);
        return;
    }
    album_backdrop(app, ui, header);
    ui.add_space(24.0);
    let width = room.min(500.0);
    // The page's middle: the songs' column is centred in it too.
    let middle = ui.max_rect().center().x;
    let top = ui.cursor().top();
    let column = Rect::from_min_max(
        pos2(middle - width / 2.0, top),
        pos2(middle + width / 2.0, f32::INFINITY),
    );
    let mut column_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(column)
            .layout(Layout::top_down(Align::Center)),
    );
    header_column(app, &mut column_ui, route, page, header);
    let used = column_ui.min_rect();
    ui.allocate_exact_size(vec2(ui.available_width(), used.height()), Sense::hover());
    ui.add_space(36.0);
}

/// What an album's or playlist's header holds, centred, as YouTube Music's
/// (`ytmusic-responsive-header-renderer`): an album's artist (a small round
/// picture and the name) 16 above its cover, the cover (r 12), 16 under
/// it the title (up to 2 lines); a playlist's maker (picture 24 and name
/// 12/400) 8 under that; 8 under, what it is and how long; 8 under, the
/// description (2 lines, a click shows it whole); 16 under, the buttons.
fn header_column(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page, header: &Header) {
    let width = ui.available_width();
    let window = ui.ctx().content_rect().width();
    ui.spacing_mut().item_spacing.y = 0.0;
    let album = is_album(route);
    let line = |ui: &mut egui::Ui, text: &str, font: egui::FontId, color, rows: usize| {
        let galley = theme::fit_centered(ui, text, font, color, width, rows);
        let (rect, response) = ui.allocate_exact_size(vec2(width, galley.size().y), Sense::click());
        ui.painter()
            .galley(pos2(rect.center().x, rect.top()), galley, color);
        response
    };
    // A picture and a name, side by side and centred together.
    let face = |ui: &mut egui::Ui, side: f32, gap: f32, font: egui::FontId, height: f32| {
        let words = theme::fit(ui, &header.owner, font, PALETTE.text, width - side - gap, 1);
        let picture = header.owner_picture.as_ref();
        let lead = if picture.is_some() { side + gap } else { 0.0 };
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let left = rect.center().x - (lead + words.size().x) / 2.0;
        if let Some(picture) = picture {
            let at =
                Rect::from_min_size(pos2(left, rect.center().y - side / 2.0), Vec2::splat(side));
            widgets::cover(app, ui, at, Some(picture), true);
        }
        ui.painter().galley(
            pos2(left + lead, rect.center().y - words.size().y / 2.0),
            words,
            PALETTE.text,
        );
    };
    // `--ytmusic-responsive-font-size`: 16 from 1364.
    let words = theme::responsive(window);
    if album && !header.owner.is_empty() {
        face(ui, 16.0, 4.0, theme::regular(words), words * 1.2);
        ui.add_space(16.0);
    }
    let side: f32 = if window >= 1364.0 {
        264.0
    } else if window >= 936.0 {
        240.0
    } else {
        200.0
    };
    let side = side.min(width - 16.0).max(0.0);
    let (art, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    widgets::cover_with(
        app,
        ui,
        art,
        header.thumbnail.as_ref(),
        CornerRadius::same(12),
    );
    ui.add_space(16.0);
    // `--lhs-title-font-size`: 28 from 1150, else display-1 (24).
    let size = if window >= 1150.0 {
        28.0
    } else {
        theme::display1(window)
    };
    line(ui, &header.title, theme::bold(size), PALETTE.text, 2);
    if !album && !header.owner.is_empty() {
        ui.add_space(8.0);
        face(ui, 24.0, 8.0, theme::regular(12.0), 32.0);
    }
    ui.add_space(8.0);
    for text in [&header.subtitle, &header.detail] {
        if !text.is_empty() {
            line(ui, text, theme::regular(words), PALETTE.secondary, 1);
        }
    }
    if !header.description.is_empty() {
        ui.add_space(8.0);
        let response = line(
            ui,
            &header.description,
            theme::regular(words),
            PALETTE.secondary,
            2,
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Description")
        });
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            app.act(Action::OpenDialog(Dialog::Description {
                title: header.title.clone(),
                text: header.description.clone(),
            }));
        }
    }
    ui.add_space(16.0);
    header_buttons(app, ui, route, page, header);
}

/// Save (or rename, for the account's own playlist), the big Play, and
/// the menu.
/// Premium's album or playlist header: the cover (248, or 192 in less
/// room; r 12) beside the words, left-aligned and at most 760 wide, over
/// the songs at full width; in a narrow page the cover (160) over the
/// words.
fn premium_header(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page, header: &Header) {
    if header.thumbnail.is_none() {
        title(ui, &header.title);
        return;
    }
    album_backdrop(app, ui, header);
    ui.add_space(32.0);
    let width = ui.available_width();
    if width >= 580.0 {
        let side = if width >= 900.0 { 248.0 } else { 192.0 };
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 40.0;
            let (art, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
            widgets::cover_with(
                app,
                ui,
                art,
                header.thumbnail.as_ref(),
                CornerRadius::same(12),
            );
            ui.vertical(|ui| {
                ui.set_width((width - side - 40.0).clamp(0.0, 760.0));
                premium_header_details(app, ui, route, page, header);
            });
        });
    } else {
        let (art, _) = ui.allocate_exact_size(Vec2::splat(160.0_f32.min(width)), Sense::hover());
        widgets::cover_with(
            app,
            ui,
            art,
            header.thumbnail.as_ref(),
            CornerRadius::same(12),
        );
        ui.add_space(20.0);
        premium_header_details(app, ui, route, page, header);
    }
    ui.add_space(40.0);
}

/// Premium's header words: what it is, the title (48, or 32 in less room),
/// the maker with their picture, how long, the description (3 lines, a
/// click shows it whole), then the buttons.
fn premium_header_details(
    app: &App,
    ui: &mut egui::Ui,
    route: &Route,
    page: &Page,
    header: &Header,
) {
    let width = ui.available_width();
    ui.spacing_mut().item_spacing.y = 0.0;
    let line = |ui: &mut egui::Ui, text: &str, font: egui::FontId, color, rows: usize| {
        let galley = theme::fit(ui, text, font, color, width, rows);
        let (rect, response) = ui.allocate_exact_size(vec2(width, galley.size().y), Sense::click());
        ui.painter().galley(rect.min, galley, color);
        response
    };
    if !header.subtitle.is_empty() {
        line(
            ui,
            &header.subtitle,
            theme::medium(13.0),
            PALETTE.secondary,
            1,
        );
        ui.add_space(12.0);
    }
    let size = if width >= 560.0 { 48.0 } else { 32.0 };
    line(ui, &header.title, theme::bold(size), PALETTE.text, 2);
    ui.add_space(12.0);
    if !header.owner.is_empty() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if let Some(picture) = &header.owner_picture {
                let (art, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                widgets::cover(app, ui, art, Some(picture), true);
            }
            theme::label(ui, &header.owner, theme::medium(14.0), PALETTE.text);
        });
        ui.add_space(8.0);
    }
    if !header.detail.is_empty() {
        line(
            ui,
            &header.detail,
            theme::regular(14.0),
            PALETTE.secondary,
            1,
        );
    }
    if !header.description.is_empty() {
        ui.add_space(8.0);
        let response = line(
            ui,
            &header.description,
            theme::regular(14.0),
            PALETTE.secondary,
            3,
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Description")
        });
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            app.act(Action::OpenDialog(Dialog::Description {
                title: header.title.clone(),
                text: header.description.clone(),
            }));
        }
    }
    ui.add_space(20.0);
    header_buttons(app, ui, route, page, header);
}

fn header_buttons(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page, header: &Header) {
    let count = page
        .sections
        .iter()
        .map(|s| songs(s).count())
        .sum::<usize>();
    let own = match route {
        Route::Browse { id, .. } if header.editable => {
            Some(id.strip_prefix("VL").unwrap_or(id).to_string())
        }
        _ => None,
    };
    // The playlist to queue as a whole: the page's, or an album's own.
    let playlist = source(route).or_else(|| header.library_id.clone());
    // The account's own playlist has more buttons, 16 apart (measured
    // signed in: Edit, Play, Share and ⋮; its Download is left out).
    // Premium: Play 52, everything 12 apart.
    let premium = theme::premium();
    let play = if premium { 52.0 } else { 64.0 };
    let (width, gap) = match (own.is_some(), premium) {
        (true, false) => (40.0 + 16.0 + 64.0 + 16.0 + 40.0 + 16.0 + 40.0, 16.0),
        (false, false) => (40.0 + 32.0 + 64.0 + 32.0 + 40.0, 32.0),
        (true, true) => (40.0 + 12.0 + 52.0 + 12.0 + 40.0 + 12.0 + 40.0, 12.0),
        (false, true) => (40.0 + 12.0 + 52.0 + 12.0 + 40.0, 12.0),
    };
    ui.allocate_ui_with_layout(
        vec2(width, play),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let small = |ui: &mut egui::Ui, icon: Icon, tip: &str| {
                theme::round_button(ui, icon, 40.0, 20.0, Round::Tonal, PALETTE.text, tip)
            };
            // Only where YouTube Music offers Save (not on the account's
            // own playlists, nor on mixes).
            let save = header
                .library_id
                .as_ref()
                .filter(|_| !header.editable && header.saved.is_some());
            if let Some(playlist_id) = &own {
                if small(ui, Icon::Edit, "Edit playlist").clicked() {
                    app.act(Action::OpenDialog(Dialog::Rename {
                        playlist_id: playlist_id.clone(),
                        name: header.title.clone(),
                        description: header.description.clone(),
                        privacy: ytfast_core::library::Privacy::from_subtitle(&header.subtitle)
                            .unwrap_or_default(),
                    }));
                }
            } else if let Some(id) = save {
                let saved = app.saved.get(id).copied().or(header.saved).unwrap_or(false);
                let (icon, tip) = if saved {
                    (Icon::SavedToLibrary, "Remove from library")
                } else {
                    (Icon::Library, "Save to library")
                };
                if small(ui, icon, tip).clicked() {
                    app.act(Action::ToggleSave {
                        playlist_id: id.clone(),
                        save: !saved,
                    });
                }
            } else {
                ui.add_enabled_ui(count > 1, |ui| {
                    if small(ui, Icon::Shuffle, "Shuffle play").clicked() {
                        app.act(Action::Shuffle {
                            tracks: page.tracks(),
                            source: source(route),
                        });
                    }
                });
            }

            // A page with no songs listed (an episode's) plays what its
            // header's own button plays.
            let header_play = header.play().cloned().filter(|_| count == 0);
            ui.add_enabled_ui(count > 0 || header_play.is_some(), |ui| {
                if theme::round_button(
                    ui,
                    Icon::Play,
                    play,
                    play / 2.0,
                    Round::Filled,
                    PALETTE.text,
                    "Play",
                )
                .clicked()
                {
                    app.act(match header_play {
                        Some(target) => Action::Play(target, None),
                        None => Action::PlayTracks {
                            tracks: page.tracks(),
                            start: 0,
                            source: source(route),
                        },
                    });
                }
            });

            if let Some(playlist_id) = &own
                && small(ui, Icon::Share, "Share").clicked()
            {
                ui.ctx().copy_text(format!(
                    "https://music.youtube.com/playlist?list={playlist_id}"
                ));
                app.act(Action::Notify("Link copied to clipboard".into()));
            }

            let more = theme::round_button(
                ui,
                Icon::MoreVertical,
                40.0,
                24.0,
                Round::Tonal,
                PALETTE.text,
                "More",
            );
            theme::menu_popup(&more).show(|ui| {
                theme::menu(ui);
                let item = |ui: &mut egui::Ui, icon: Icon, text: &str, action: Action| {
                    if theme::menu_item(ui, icon, text).clicked() {
                        app.act(action);
                        ui.close();
                    }
                };
                if count > 1 {
                    item(
                        ui,
                        Icon::Shuffle,
                        "Shuffle play",
                        Action::Shuffle {
                            tracks: page.tracks(),
                            source: source(route),
                        },
                    );
                }
                if let Some(id) = &playlist {
                    item(
                        ui,
                        Icon::PlayNext,
                        "Play next",
                        Action::QueuePlaylist(id.clone(), QueueMode::Next),
                    );
                    item(
                        ui,
                        Icon::AddToQueue,
                        "Add to queue",
                        Action::QueuePlaylist(id.clone(), QueueMode::End),
                    );
                }
                if let Some(playlist_id) = &own {
                    item(
                        ui,
                        Icon::Edit,
                        "Edit playlist",
                        Action::OpenDialog(Dialog::Rename {
                            playlist_id: playlist_id.clone(),
                            name: header.title.clone(),
                            description: header.description.clone(),
                            privacy: ytfast_core::library::Privacy::from_subtitle(&header.subtitle)
                                .unwrap_or_default(),
                        }),
                    );
                    item(
                        ui,
                        Icon::Delete,
                        "Delete playlist",
                        Action::OpenDialog(Dialog::Delete {
                            playlist_id: playlist_id.clone(),
                            title: header.title.clone(),
                        }),
                    );
                }
            });
        },
    );
}

// ---- An artist ----

/// An artist's header paddings by window width: above the words' fade
/// (`--ytmusic-immersive-header-padding-top`), and inside it above the
/// name (`--ytmusic-immersive-header-gradient-padding-top`).
fn artist_paddings(window: f32) -> (f32, f32) {
    if window >= 1578.0 {
        (368.0, 97.0)
    } else if window >= 1364.0 {
        (288.0, 75.0)
    } else if window >= 1150.0 {
        (228.0, 60.0)
    } else if window >= 936.0 {
        (168.0, 44.0)
    } else {
        (128.0, 34.0)
    }
}

/// An artist's (or a channel's) header, as YouTube Music's
/// (`ytmusic-immersive-header-renderer`, measured at 1280): it starts at the
/// window's top, its picture behind the top bar and the menu
/// (`backdrop::artist_picture`); `artist_paddings` above the words; the
/// name (display-1 bold, wrapping within 800), right under it the monthly
/// audience (14, 16 from 1364, white@0.70); 16 under, the description (14
/// on 19.6 lines, 640 wide, 2 lines, with MORE when cut; hidden below
/// 936); 18 under, the buttons; 8 under them, the header's foot. `margin`
/// is the page's left margin.
fn artist_header(
    app: &App,
    ui: &mut egui::Ui,
    route: &Route,
    page: &Page,
    header: &Header,
    margin: f32,
) {
    let window = ui.ctx().content_rect().width();
    let (pad_top, pad_words) = artist_paddings(window);
    let full = ui.available_rect_before_wrap();
    // The page starts under the top bar; the header at the window's top.
    let origin = full.top() - theme::top_bar_height();
    let left = full.left() + margin;
    let width = (full.width() - 2.0 * margin).max(0.0);

    let size = theme::display1(window);
    let name = theme::fit_lines(
        ui,
        &header.title,
        theme::bold(size),
        PALETTE.text,
        width.min(800.0),
        3,
        size * 1.2,
    );
    let words = theme::responsive(window);
    let audience = theme::fit(
        ui,
        &header.subtitle,
        theme::regular(words),
        PALETTE.secondary,
        width,
        1,
    );
    let described = !header.description.is_empty() && window >= 936.0;
    let open_id = ui.id().with(("artist-description", route));
    let open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or(false);
    let description = |rows: usize| {
        theme::fit_lines(
            ui,
            &header.description,
            theme::regular(14.0),
            PALETTE.text,
            width.min(640.0),
            rows,
            19.6,
        )
    };
    let shown = described.then(|| description(if open { 15 } else { 2 }));
    // MORE shows only when two lines cut it.
    let cut = described && (open || description(3).rows.len() > 2);
    let mut block = name.size().y + words * 1.2 + 16.0 + 36.0 + 8.0;
    if let Some(shown) = &shown {
        block += shown.size().y + 18.0;
        if cut {
            block += 8.0 + 17.0 + 8.0;
        }
    }
    let height = pad_top + pad_words + block;
    let (rect, _) = ui.allocate_exact_size(
        vec2(full.width(), (height - theme::top_bar_height()).max(0.0)),
        Sense::hover(),
    );
    artist_picture(app, ui, header, origin, height, pad_top);
    if !ui.is_rect_visible(rect) {
        return;
    }

    let mut y = origin + pad_top + pad_words;
    let name_height = name.size().y;
    ui.painter().galley(pos2(left, y), name, PALETTE.text);
    y += name_height;
    let line = words * 1.2;
    let at = pos2(left, y + (line - audience.size().y) / 2.0);
    ui.painter().galley(at, audience, PALETTE.secondary);
    y += line + 16.0;
    if let Some(shown) = shown {
        let shown_height = shown.size().y;
        ui.painter().galley(pos2(left, y), shown, PALETTE.text);
        y += shown_height;
        if cut {
            y += 8.0;
            let label = if open { "LESS" } else { "MORE" };
            let galley =
                ui.painter()
                    .layout_no_wrap(label.into(), theme::medium(14.0), PALETTE.text);
            let spot = Rect::from_min_size(pos2(left, y), vec2(galley.size().x, 17.0));
            let response = ui.interact(spot, open_id.with("button"), Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    if open { "Less" } else { "More" },
                )
            });
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            ui.painter().galley(
                pos2(left, spot.center().y - galley.size().y / 2.0),
                galley,
                PALETTE.text,
            );
            if response.clicked() {
                ui.data_mut(|d| d.insert_temp(open_id, !open));
            }
            y += 17.0 + 8.0;
        }
        y += 18.0;
    }

    let count = page
        .sections
        .iter()
        .map(|s| songs(s).count())
        .sum::<usize>();
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(pos2(left, y), vec2(width, 36.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 8.0;
    // YouTube's own Shuffle (all the artist's songs) and Mix, as its
    // buttons give them; else made from the top songs shown. Each at
    // least 136 wide.
    let shuffle = header.shuffle().cloned();
    let play_shuffle = |app: &App| match shuffle.clone() {
        Some(target) => app.act(Action::Play(target, None)),
        None => app.act(Action::Shuffle {
            tracks: page.tracks(),
            source: source(route),
        }),
    };
    let mix = header.radio().cloned();
    let play_mix = |app: &App| match mix.clone() {
        Some(target) => app.act(Action::Play(target, None)),
        // Their best-known song, then songs like it.
        None => {
            if let Some(first) = page.tracks().into_iter().find(|t| t.playable) {
                let radio = Target::Watch {
                    video_id: Some(first.video_id.clone()),
                    playlist_id: None,
                };
                app.act(Action::Play(radio, Some(first)));
            }
        }
    };
    let wide = |ui: &egui::Ui, text: &str| {
        let words = ui
            .painter()
            .layout_no_wrap(text.into(), theme::medium(14.0), PALETTE.text)
            .size()
            .x;
        Some((words + 24.0 + 32.0).max(136.0))
    };
    let can_shuffle = shuffle.is_some() || count > 1;
    let can_mix = mix.is_some() || count > 0;
    if can_shuffle {
        let width = wide(&row, "Shuffle");
        if theme::pill_sized(
            &mut row,
            Some(Icon::Shuffle),
            "Shuffle",
            Pill::Filled,
            width,
        )
        .clicked()
        {
            play_shuffle(app);
        }
    }
    if can_mix {
        let width = wide(&row, "Mix");
        if theme::pill_sized(&mut row, Some(Icon::Mix), "Mix", Pill::Filled, width).clicked() {
            play_mix(app);
        }
    }
    let subscription = header.channel_id.as_ref().map(|channel| {
        let subscribed = app
            .subscribed
            .get(channel)
            .copied()
            .or(header.subscribed)
            .unwrap_or(false);
        (channel.clone(), subscribed)
    });
    if let Some((channel, subscribed)) = &subscription {
        // "Subscribe 28.6M" in red, ringed red (15 each side).
        let (label, style) = if *subscribed {
            ("Subscribed".to_string(), Pill::Tonal)
        } else if header.subscribers.is_empty() {
            ("Subscribe".to_string(), Pill::Ringed(PALETTE.subscribe))
        } else {
            (
                format!("Subscribe {}", header.subscribers),
                Pill::Ringed(PALETTE.subscribe),
            )
        };
        let words = row
            .painter()
            .layout_no_wrap(label.clone(), theme::medium(14.0), PALETTE.text)
            .size()
            .x;
        if theme::pill_sized(&mut row, None, &label, style, Some(words + 32.0)).clicked() {
            app.act(Action::ToggleSubscribe {
                channel_id: channel.clone(),
                subscribe: !subscribed,
            });
        }
    }
    // ⋮: what the buttons do, and the artist's link.
    let more = theme::round_button(
        &mut row,
        Icon::MoreVertical,
        36.0,
        24.0,
        Round::Plain,
        PALETTE.text,
        "More actions",
    );
    theme::menu_popup(&more).show(|ui| {
        if can_shuffle && theme::menu_item(ui, Icon::Shuffle, "Shuffle play").clicked() {
            play_shuffle(app);
            ui.close();
        }
        if can_mix && theme::menu_item(ui, Icon::Mix, "Start mix").clicked() {
            play_mix(app);
            ui.close();
        }
        if let Some((channel, subscribed)) = &subscription {
            let label = if *subscribed {
                "Unsubscribe"
            } else {
                "Subscribe"
            };
            if theme::menu_item(ui, Icon::Artist, label).clicked() {
                app.act(Action::ToggleSubscribe {
                    channel_id: channel.clone(),
                    subscribe: !subscribed,
                });
                ui.close();
            }
            widgets::share(
                app,
                ui,
                format!("https://music.youtube.com/channel/{channel}"),
            );
        }
    });
}

/// Behind an artist's header: its picture and the fade to the page
/// (`backdrop::artist_picture`), `top` the header's top in the window and
/// `height` its height.
fn artist_picture(app: &App, ui: &egui::Ui, header: &Header, top: f32, height: f32, fade: f32) {
    app.page_backdrop.set(true);
    let Some(slot) = app.backdrop_slot.get() else {
        return;
    };
    let screen = ui.ctx().content_rect();
    // From the menu's width less 72 (the window's left with the menu
    // closed), as wide as the window less its scroll bar's room.
    let x = screen.left() + crate::views::sidebar::width(app) - 72.0;
    let frame = Rect::from_min_size(pos2(x, top), vec2(screen.width() - 12.0, height));
    let texture = header
        .thumbnail
        .as_ref()
        .and_then(|t| app.picture(&t.wide(1440, 600)));
    backdrop::artist_picture(
        ui,
        slot,
        screen,
        frame,
        texture.as_ref().map(|t| (t.id(), t.size())),
        top + fade,
    );
}

// ---- Sections ----

/// One section: its title (with More, and arrows when it scrolls
/// sideways), then its cards and its songs.
fn section_block(
    app: &App,
    ui: &mut egui::Ui,
    route: &Route,
    section: &Section,
    index: usize,
    look: &Look,
) {
    let cards: Vec<&Card> = section
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Card(c) => Some(c),
            Item::Track(_) => None,
        })
        .collect();
    let tracks: Vec<&Track> = songs(section).collect();
    let pictures = cards.iter().any(|c| c.thumbnail.is_some());
    let song_shelf = section.shape == Shape::Carousel && tracks.len() > GRID_ROWS;
    // Explore's moods: buttons with a coloured stripe, in a sideways shelf.
    let mood_shelf = section.shape == Shape::Carousel
        && !pictures
        && cards.iter().any(|c| c.look.stripe.is_some());
    let card_shelf = section.shape == Shape::Carousel && (pictures || mood_shelf);
    let shelf = ui.id().with(("shelf", route, index));
    // Shelves that scroll sideways and grids have large titles; lists
    // (search results, an artist's top songs) and an artist's shelves
    // (`DISPLAY_TWO`) smaller ones.
    let list = section.shape == Shape::List;
    let title_size = if list || section.small_title {
        look.list_title
    } else {
        look.title
    };
    // A list's "Show all" goes under its songs, not beside its title.
    let show_all = list && !is_search(route);

    if !section.title.is_empty() {
        // A list's title has 16 above it (`.header.ytmusic-shelf-renderer`).
        ui.add_space(if list { 16.0 } else { look.above_title });
        let title_row = |ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                // At most `--ytmusic-header-title-max-width`, as the window.
                let window = ui.ctx().content_rect().width();
                let widest = if window >= 1578.0 {
                    800.0
                } else if window >= 1364.0 {
                    640.0
                } else if window >= 1150.0 {
                    560.0
                } else {
                    480.0
                };
                // Beside it: the arrows (36, 16, 36, then 8) and More.
                let mut beside = 0.0;
                if card_shelf || song_shelf {
                    beside += 88.0 + 8.0 + 16.0;
                }
                if section.more.is_some() && !show_all {
                    let more = ui.painter().layout_no_wrap(
                        "More".into(),
                        theme::medium(14.0),
                        PALETTE.text,
                    );
                    beside += more.size().x + 32.0 + 16.0;
                }
                let room = (ui.available_width() - beside).clamp(120.0, widest);
                let title = theme::fit(
                    ui,
                    &section.title,
                    theme::bold(title_size),
                    PALETTE.text,
                    room,
                    2,
                );
                let (rect, title_response) = ui.allocate_exact_size(title.size(), Sense::click());
                ui.painter().galley(rect.min, title, PALETTE.text);
                // A title with a page of its own opens it (underlined under
                // the pointer).
                if let Some(more) = &section.more {
                    if title_response.hovered() {
                        ui.painter().hline(
                            rect.x_range(),
                            rect.bottom() - 2.0,
                            egui::Stroke::new(1.0, PALETTE.text),
                        );
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if title_response.clicked() {
                        app.act(Action::Open(more.clone(), None));
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    // The arrows 16 apart, More 24 before them.
                    ui.spacing_mut().item_spacing.x = 16.0;
                    if card_shelf || song_shelf {
                        arrows(ui, shelf);
                        ui.add_space(8.0);
                    }
                    if let Some(more) = section.more.as_ref().filter(|_| !show_all)
                        && theme::pill(ui, None, "More", Pill::Outline(PALETTE.button)).clicked()
                    {
                        app.act(Action::Open(more.clone(), None));
                    }
                });
            });
        };
        // A signed-in Home's shelves: small words above the title (14
        // `#aaa`, 16 from a window 1364 wide, capitals, 2 above it), and a
        // picture 56 before both (16 after it, round when YouTube crops it
        // so).
        let strapline = |ui: &mut egui::Ui| {
            if !section.strapline.is_empty() {
                let size = theme::text_size(ui);
                theme::label(
                    ui,
                    &section.strapline.to_uppercase(),
                    theme::regular(size),
                    PALETTE.dim,
                );
                ui.add_space(2.0);
            }
        };
        if let Some(picture) = &section.picture {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                let (spot, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
                if section.round_picture {
                    widgets::cover(app, ui, spot, Some(picture), true);
                } else {
                    widgets::cover_with(app, ui, spot, Some(picture), CornerRadius::same(2));
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    strapline(ui);
                    title_row(ui);
                });
            });
        } else {
            strapline(ui);
            title_row(ui);
        }
        ui.add_space(16.0);
    }

    // Search's best match: a big card, with a few of its songs.
    if let Some(top) = &section.top {
        top_result(app, ui, route, section, top);
        return;
    }
    // Search's other results: one list, in YouTube's order, cards and
    // songs mixed.
    if is_search(route) && section.shape == Shape::List {
        results(app, ui, route, look.rows, section);
        return;
    }

    if !cards.is_empty() {
        match section.shape {
            // Moods and genres: buttons. One row that scrolls sideways at
            // the top of Home, wrapped elsewhere.
            // Explore's three big buttons: equal columns, 24 apart (16
            // under 1150, one column under 936), 32 above them.
            _ if is_big_buttons(section) => {
                let window = ui.ctx().content_rect().width();
                let (columns, gap) = if window < 936.0 {
                    (1.0, 16.0)
                } else if window < 1150.0 {
                    (3.0, 16.0)
                } else {
                    (3.0, 24.0)
                };
                let full = ui.available_width();
                let width = (full - gap * (columns - 1.0)) / columns;
                // The words: a 45th of the content up to 1363 wide, then 24;
                // the icon's gap 8 under 1150, 12, then 16 from 1364.
                let (size, icon_gap) = if window >= 1364.0 {
                    (24.0, 16.0)
                } else if window >= 1150.0 {
                    (full / 45.0, 12.0)
                } else {
                    (full / 45.0, 8.0)
                };
                ui.add_space(if window >= 1578.0 { 40.0 } else { 32.0 });
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(gap, gap);
                    for card in &cards {
                        widgets::big_button(app, ui, card, width, icon_gap, size);
                    }
                });
            }
            // Explore's moods: columns of 4 that scroll sideways, each as
            // wide as a card and 48 high, 16 apart down.
            _ if mood_shelf => {
                let width = look.card;
                carousel(ui, shelf, |ui| {
                    ui.spacing_mut().item_spacing.x = look.gap;
                    for column in cards.chunks(GRID_ROWS) {
                        ui.allocate_ui_with_layout(
                            vec2(
                                width,
                                48.0 * GRID_ROWS as f32 + 16.0 * (GRID_ROWS - 1) as f32,
                            ),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.spacing_mut().item_spacing.y = 16.0;
                                for card in column {
                                    widgets::mood_button(app, ui, card, width);
                                }
                            },
                        );
                    }
                });
            }
            // The Moods & genres page: each section a grid of the same
            // striped buttons, 4 across (5 from a window 1578 wide), 16
            // apart both ways (`grid-type=moods_and_genres`; 238.3 wide at
            // 1707, measured).
            _ if section.shape == Shape::Grid
                && !pictures
                && cards.iter().any(|c| c.look.stripe.is_some()) =>
            {
                let window = ui.ctx().content_rect().width();
                let columns: f32 = if window >= 1578.0 { 5.0 } else { 4.0 };
                let width = (ui.available_width() - 16.0 * (columns - 1.0)) / columns;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(16.0, 16.0);
                    for card in &cards {
                        widgets::mood_button(app, ui, card, width);
                    }
                });
            }
            _ if !pictures => {
                if *route == Route::Home {
                    // Home's moods: 46 under the top bar, 12 apart.
                    if index == 0 {
                        ui.add_space(46.0);
                    }
                    carousel(ui, shelf.with("chips"), |ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        for card in &cards {
                            widgets::chip(app, ui, card);
                        }
                    });
                } else {
                    // Elsewhere (over Liked Music's songs, measured signed
                    // in): one row, 32 high, 12 apart, that scrolls
                    // sideways; 6 above it and 21.3 under it.
                    ui.add_space(6.0);
                    carousel(ui, shelf.with("chips"), |ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        for card in &cards {
                            widgets::chip_sized(app, ui, card, 32.0);
                        }
                    });
                    ui.add_space(21.3);
                }
            }
            Shape::Carousel => {
                carousel(ui, shelf, |ui| {
                    ui.spacing_mut().item_spacing.x = look.gap;
                    for card in &cards {
                        widgets::card(app, ui, card, look.card);
                    }
                });
            }
            Shape::Grid => {
                // The Library's grid keeps its words 14 at every width
                // (`grid-type=library`).
                let (card, across, down, text) = if is_library(route) {
                    let window = ui.ctx().content_rect().width();
                    let (card, gap) = library_grid(window, ui.available_width());
                    (card, gap, 40.0, 14.0)
                } else {
                    (look.card, CARD_GAP, 16.0, theme::text_size(ui))
                };
                // Cards line up by their tops, whatever their words' height.
                ui.with_layout(
                    Layout::left_to_right(Align::Min).with_main_wrap(true),
                    |ui| {
                        ui.spacing_mut().item_spacing = vec2(across, down);
                        for item in &cards {
                            widgets::card_with_text(app, ui, item, card, text);
                        }
                    },
                );
            }
            // Search results: one under another.
            Shape::List => {
                for card in &cards {
                    widgets::card_row(app, ui, look.rows, card);
                }
            }
        }
    }

    let playing = app
        .playback
        .entry
        .as_ref()
        .map(|e| e.track.video_id.as_str());
    let play_from = |start: usize| play_song(route, &section.tracks(), start);
    if song_shelf {
        // Quick picks: columns of four songs that scroll sideways.
        let width = look.column;
        carousel(ui, shelf, |ui| {
            ui.spacing_mut().item_spacing.x = look.column_gap;
            for (column, chunk) in tracks.chunks(GRID_ROWS).enumerate() {
                ui.allocate_ui_with_layout(
                    vec2(
                        width,
                        (Row::GRID.height + Row::GRID.gap) * GRID_ROWS as f32 - Row::GRID.gap,
                    ),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (row, track) in chunk.iter().enumerate() {
                            let start = column * GRID_ROWS + row;
                            let is_playing = playing == Some(track.video_id.as_str());
                            widgets::track_row(app, ui, Row::GRID, track, None, is_playing, || {
                                play_from(start)
                            });
                        }
                    },
                );
            }
        });
        return;
    }
    let style = look.rows;
    // An album's own songs are numbered; shelves under it are not.
    let numbered = is_album(route) && section.title.is_empty();
    widgets::rows(ui, style, tracks.len(), |ui, start| {
        let track = tracks[start];
        let is_playing = playing == Some(track.video_id.as_str());
        let number = numbered.then_some(start + 1);
        widgets::track_row(app, ui, style, track, number, is_playing, || {
            play_from(start)
        });
    });
    // "Show all", right under the songs (an artist's top songs).
    if show_all
        && let Some(more) = &section.more
        && theme::pill(ui, None, "Show all", Pill::Outline(PALETTE.button)).clicked()
    {
        app.act(Action::Open(more.clone(), None));
    }
}

/// Search's other results as one list, in YouTube's order: a row for each
/// artist, album or playlist, and for each song.
fn results(app: &App, ui: &mut egui::Ui, route: &Route, style: Row, section: &Section) {
    let tracks = section.tracks();
    let playing = app
        .playback
        .entry
        .as_ref()
        .map(|e| e.track.video_id.as_str());
    // Each song's place among the songs, for what a click on it plays.
    let mut song_at = Vec::with_capacity(section.items.len());
    let mut songs = 0;
    for item in &section.items {
        song_at.push(songs);
        if matches!(item, Item::Track(_)) {
            songs += 1;
        }
    }
    widgets::rows(ui, style, section.items.len(), |ui, index| {
        match &section.items[index] {
            Item::Card(card) => widgets::card_row(app, ui, style, card),
            Item::Track(track) => {
                let is_playing = playing == Some(track.video_id.as_str());
                widgets::track_row(app, ui, style, track, None, is_playing, || {
                    play_song(route, &tracks, song_at[index])
                });
            }
        }
    });
}

/// Search's best match, as YouTube Music's top result card
/// (`ytmusic-card-shelf-renderer`): two halves, the match on the left (its
/// picture, its name, what it is, and its own buttons) and a few of its
/// songs on the right.
fn top_result(app: &App, ui: &mut egui::Ui, route: &Route, section: &Section, top: &TopResult) {
    let Some(best) = section.items.first() else {
        return;
    };
    let songs: Vec<Track> = section.items[1..]
        .iter()
        .filter_map(|i| match i {
            Item::Track(t) => Some(t.clone()),
            Item::Card(_) => None,
        })
        .take(3)
        .collect();
    // With songs, the card is as tall as its three rows; without, as
    // its picture with 16 around it (not measured: see gaps.md).
    let height = if songs.is_empty() { 132.0 } else { 232.0 };
    let width = ui.available_width().min(860.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    ui.add_space(32.0);
    if !ui.is_rect_visible(rect) {
        return;
    }
    let half = if songs.is_empty() {
        rect
    } else {
        Rect::from_min_max(rect.min, pos2(rect.center().x, rect.bottom()))
    };
    let (outer, inner) = (8, if songs.is_empty() { 8 } else { 0 });
    let corners = CornerRadius {
        nw: outer,
        sw: outer,
        ne: inner,
        se: inner,
    };
    ui.painter().rect_filled(half, corners, PALETTE.surface);
    if !songs.is_empty() {
        let right = Rect::from_min_max(pos2(half.right(), rect.top()), rect.max);
        let corners = CornerRadius {
            nw: 0,
            sw: 0,
            ne: 8,
            se: 8,
        };
        ui.painter()
            .rect_filled(right, corners, Color32::from_white_alpha(13));
    }

    let (title, thumbnail, round, open) = match best {
        Item::Track(track) => (track.title.as_str(), track.thumbnail.as_ref(), false, None),
        Item::Card(card) => (
            card.title.as_str(),
            card.thumbnail.as_ref(),
            card.round,
            card.open.clone(),
        ),
    };
    let response = ui.interact(half, ui.id().with(("top-result", title)), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));

    // The picture: 100 across, 16 in, centred down.
    let art = Rect::from_min_size(
        pos2(half.left() + 16.0, rect.center().y - 50.0),
        Vec2::splat(100.0),
    );
    if round {
        widgets::cover(app, ui, art, thumbnail, true);
    } else {
        widgets::cover_with(app, ui, art, thumbnail, CornerRadius::same(2));
    }

    // The words and buttons, 16 after the picture, up to the small arrow
    // (24, and 8 after it) at the top right; the block centred down.
    let left = art.right() + 16.0;
    let room = (half.right() - 32.0 - left).max(0.0);
    let size = theme::display2(ui.ctx().content_rect().width());
    let name = theme::fit(ui, title, theme::bold(size), PALETTE.text, room, 2);
    // Its lines are 1.2 times the size apart (`theme::fit`).
    let name_height = name.size().y;
    let buttons = &top.buttons;
    // Its second line 14 (16 from a window 1364 wide) on a line 1.2 times
    // as tall.
    let words = theme::text_size(ui);
    let line_height = words * 1.2;
    let block =
        name_height + 8.0 + line_height + if buttons.is_empty() { 0.0 } else { 12.0 + 36.0 };
    let mut y = rect.center().y - block / 2.0;
    ui.painter().galley(pos2(left, y), name, PALETTE.text);
    y += name_height + 8.0;
    let subtitle = theme::fit(
        ui,
        &top.subtitle,
        theme::regular(words),
        PALETTE.secondary,
        room,
        1,
    );
    ui.painter().galley(
        pos2(left, y + (line_height - subtitle.size().y) / 2.0),
        subtitle,
        PALETTE.secondary,
    );
    y += line_height + 12.0;

    let mut pressed = false;
    if !buttons.is_empty() {
        // Sharing up to 290 of the room, 16 apart.
        let count = buttons.len() as f32;
        let each = (room.min(290.0) - 16.0 * (count - 1.0)) / count;
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_size(pos2(left, y), vec2(room, 36.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        row.spacing_mut().item_spacing.x = 16.0;
        for button in buttons {
            let style = if button.filled {
                Pill::Filled
            } else {
                Pill::Outline(Color32::from_rgb(0xf1, 0xf1, 0xf1))
            };
            let icon = match button.icon.as_deref() {
                Some("MUSIC_SHUFFLE") => Some(Icon::Shuffle),
                Some("MIX") => Some(Icon::Mix),
                Some("PLAY_ARROW") => Some(Icon::Play),
                _ => None,
            };
            if theme::pill_sized(&mut row, icon, &button.text, style, Some(each)).clicked() {
                pressed = true;
                if let Some(target) = button.target.clone() {
                    app.act(Action::Play(target, None));
                }
            }
        }
    }

    // The small arrow at the top right opens the match's page.
    if let Some(target) = &open {
        let at = Rect::from_min_size(
            pos2(half.right() - 32.0, rect.top() + 3.5),
            Vec2::splat(24.0),
        );
        let mut corner = ui.new_child(UiBuilder::new().max_rect(at));
        if theme::round_button(
            &mut corner,
            Icon::Forward,
            24.0,
            16.0,
            Round::Plain,
            PALETTE.text,
            "Open",
        )
        .clicked()
        {
            pressed = true;
            app.act(Action::Open(target.clone(), None));
        }
    }

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // A click elsewhere on the left half: a song plays (and its radio),
    // anything else opens.
    if response.clicked() && !pressed {
        match best {
            Item::Track(track) => {
                app.act(play_song(route, std::slice::from_ref(track), 0));
            }
            Item::Card(card) => {
                if let Some(target) = card.open.clone().or_else(|| card.play.clone()) {
                    app.act(Action::Open(target, card.song()));
                }
            }
        }
    }
    match best {
        Item::Track(track) => {
            theme::context_menu(&response)
                .show(|ui| widgets::song_menu(app, ui, track, widgets::Place::Page));
        }
        Item::Card(card) => {
            theme::context_menu(&response).show(|ui| widgets::card_menu(app, ui, card));
        }
    }

    // The songs on the right: rows 56 high, 16 apart, centred down, 16 in.
    if songs.is_empty() {
        return;
    }
    let count = songs.len() as f32;
    let column = 56.0 * count + 16.0 * (count - 1.0);
    let top_y = rect.center().y - column / 2.0;
    let mut list = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(half.right() + 16.0, top_y),
                pos2(rect.right() - 16.0, rect.bottom()),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    list.spacing_mut().item_spacing.y = 0.0;
    let playing = app
        .playback
        .entry
        .as_ref()
        .map(|e| e.track.video_id.as_str());
    for (index, track) in songs.iter().enumerate() {
        let is_playing = playing == Some(track.video_id.as_str());
        widgets::track_row(
            app,
            &mut list,
            Row::TOP_RESULT,
            track,
            None,
            is_playing,
            || play_song(route, &songs, index),
        );
    }
}

/// Songs per column in a sideways shelf (Quick picks).
const GRID_ROWS: usize = 4;

/// Where a shelf that scrolls sideways is, for its arrows.
#[derive(Clone, Copy, Debug, Default)]
struct Shelf {
    offset: f32,
    /// The furthest it scrolls.
    max: f32,
    /// How much of it shows.
    view: f32,
    /// Turning a page: when it began, from where, to where.
    glide: Option<(f64, f32, f32)>,
}

/// How long turning a shelf's page takes, in seconds.
const GLIDE: f64 = 0.3;

/// The two round arrows in a shelf's title row, which turn its pages.
/// Drawn right to left: forward, then back.
fn arrows(ui: &mut egui::Ui, id: egui::Id) {
    let mut shelf: Shelf = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    if shelf.max <= 1.0 {
        return;
    }
    let now = ui.input(|i| i.time);
    // Where it is going, when it is on its way.
    let at = shelf.glide.map_or(shelf.offset, |(_, _, to)| to);
    let mut turn = |ui: &mut egui::Ui, icon: Icon, tip: &str, by: f32| {
        let to = (at + by).clamp(0.0, shelf.max);
        let enabled = (to - at).abs() > 1.0;
        ui.add_enabled_ui(enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tip));
            let lit = response.hovered() && enabled;
            if lit {
                ui.painter()
                    .circle_filled(rect.center(), 18.0, PALETTE.surface);
            }
            // A ring of white@0.20; one that cannot turn is dimmed whole,
            // its icon `#717171`.
            let dim = if enabled { 1.0 } else { 0.4 };
            ui.painter().circle_stroke(
                rect.center(),
                17.5,
                egui::Stroke::new(1.0, PALETTE.surface_hover.gamma_multiply(dim)),
            );
            let tint = if enabled {
                PALETTE.text
            } else {
                PALETTE.disabled.gamma_multiply(dim)
            };
            theme::paint_icon(ui, icon, rect, 18.7, tint);
            if response.clicked() {
                shelf.glide = Some((now, shelf.offset, to));
            }
        });
    };
    let page = (shelf.view * 0.9).max(120.0);
    turn(ui, Icon::Forward, "Next", page);
    turn(ui, Icon::Back, "Previous", -page);
    ui.data_mut(|d| d.insert_temp(id, shelf));
}

/// A row that scrolls sideways without a scroll bar, as YouTube Music's
/// shelves do; the arrows in its title row turn its pages.
fn carousel(ui: &mut egui::Ui, id: egui::Id, add: impl FnOnce(&mut egui::Ui)) {
    let mut shelf: Shelf = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let mut area = egui::ScrollArea::horizontal()
        .id_salt(id)
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if let Some((began, from, to)) = shelf.glide {
        let now = ui.input(|i| i.time);
        let t = (((now - began) / GLIDE) as f32).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        area = area.horizontal_scroll_offset(from + (to - from) * eased);
        if t >= 1.0 {
            shelf.glide = None;
        } else {
            ui.ctx().request_repaint();
        }
    }
    let output = area.show(ui, |ui| {
        ui.horizontal_top(|ui| add(ui));
    });
    shelf.offset = output.state.offset.x;
    shelf.view = output.inner_rect.width();
    shelf.max = (output.content_size.x - shelf.view).max(0.0);
    ui.data_mut(|d| d.insert_temp(id, shelf));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mood_button_opens_the_same_page_with_another_chosen() {
        let home = |params: &str| Route::browse("FEmusic_home".into(), Some(params.into()));
        // Home and its moods (or Podcasts), whichever way round.
        assert!(same_page(&Route::Home, &home("energize")));
        assert!(same_page(&home("energize"), &Route::Home));
        assert!(same_page(&home("energize"), &home("relax")));
        // Liked Music and its filters.
        let filter = Route::browse("VLLM".into(), Some("pop".into()));
        assert!(same_page(&Route::Liked, &filter));
        // Not the page itself, nor another page.
        assert!(!same_page(&Route::Home, &Route::Home));
        assert!(!same_page(&Route::Home, &Route::Explore));
        let album = |id: &str| Route::browse(id.into(), None);
        assert!(!same_page(&album("MPREb_1"), &album("MPREb_2")));
    }
}
