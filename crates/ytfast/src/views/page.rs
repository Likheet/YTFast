//! The page in the middle: Home, Explore, Library, search results, an
//! album, a playlist or an artist. All of them are a header and sections,
//! laid out as YouTube Music lays them out: an album or playlist with its
//! cover and buttons on the left and its songs on the right, an artist
//! under a wide picture, everything else as shelves under one another.

use egui::{Align, CornerRadius, Layout, Rect, Sense, UiBuilder, Vec2, pos2, vec2};
use ytfast_core::read::{Card, Header, Item, Page, Section, Shape, Target, Track};

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
    /// The side of a card's cover.
    pub card: f32,
    /// Song rows in lists.
    pub rows: Row,
    /// The width of one column of songs in a shelf that scrolls sideways.
    pub column: f32,
}

impl Look {
    /// For a page `width` wide (without its margins) that fits `across`
    /// cards side by side.
    fn page(route: &Route, width: f32, across: f32) -> Self {
        let card = ((width - (across - 1.0) * CARD_GAP) / across).clamp(120.0, 226.0);
        let rows = match route {
            Route::Search(_) | Route::SearchOnly(..) => Row::SEARCH,
            Route::Liked => Row::LIST,
            Route::Browse { id, .. } if id.starts_with("VL") => Row::LIST,
            _ if is_album(route) => Row::LIST,
            _ => Row::SHELF,
        };
        Self {
            title: 28.0,
            card,
            rows,
            // Three columns of songs across, as in YouTube Music's Quick
            // picks.
            column: ((width - 2.0 * CARD_GAP) / 3.0).clamp(240.0, 440.0),
        }
    }

    /// In the player page's side panel.
    pub const PANEL: Self = Self {
        title: 24.0,
        card: 160.0,
        rows: Row::GRID,
        column: 232.0,
    };
}

/// The space between cards.
const CARD_GAP: f32 = 24.0;

pub fn show(app: &App, ui: &mut egui::Ui) {
    let route = app.route.clone();
    let area = ui.max_rect();
    let margin = theme::page_margin(area.width());
    if route == Route::Settings {
        let inner = area.shrink2(vec2(margin, 0.0));
        let mut ui = ui.new_child(UiBuilder::new().max_rect(inner));
        crate::views::settings::show(app, &mut ui);
        return;
    }
    match app.pages.get(&route) {
        None | Some(Loadable::Loading) => {
            ui.add_space(96.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(32.0).color(PALETTE.secondary));
            });
        }
        Some(Loadable::Failed(message)) => {
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
        Some(Loadable::Ready(page)) => match &page.header {
            // An album or a playlist, in a window wide enough for both
            // columns.
            Some(header) if in_two_columns(header, area.width()) => {
                two_columns(app, ui, &route, page, header);
            }
            _ => {
                egui::ScrollArea::vertical()
                    .id_salt(("page", &route))
                    .auto_shrink([false, false])
                    .show(ui, |ui| one_column(app, ui, &route, page, margin));
            }
        },
    }
}

/// The playlist songs on this page come from, for more songs later.
fn source(route: &Route) -> Option<String> {
    match route {
        Route::Liked => Some("LM".into()),
        Route::Browse { id, .. } => id.strip_prefix("VL").map(str::to_string),
        _ => None,
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
fn in_two_columns(header: &Header, width: f32) -> bool {
    !is_artist(header) && header.thumbnail.is_some() && width >= 780.0
}

/// The colour the page showing is washed with at its top, when it is an
/// album or a playlist in two columns: the top bar carries the wash on.
pub fn wash_color(app: &App, width: f32) -> Option<egui::Color32> {
    if app.now_playing {
        return None;
    }
    let Some(Loadable::Ready(page)) = app.pages.get(&app.route) else {
        return None;
    };
    let header = page.header.as_ref()?;
    if !in_two_columns(header, width) {
        return None;
    }
    app.cover_color(&header.thumbnail.as_ref()?.sized(120))
}

/// How far down a page of this height its wash reaches, from the top of
/// the window.
pub fn wash_reach(page_height: f32) -> f32 {
    theme::TOP_BAR_HEIGHT + page_height * 0.7
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
fn one_column(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page, margin: f32) {
    let full = ui.available_rect_before_wrap();
    // YouTube Music fits about five cards across a 1280-wide window, and
    // more across a wider one.
    let window = ui.ctx().content_rect().width();
    let across = (window / 256.0).round().clamp(2.0, 7.0);
    let look = Look::page(route, full.width() - 2.0 * margin, across);
    if let Some(header) = page.header.as_ref().filter(|h| is_artist(h)) {
        artist_header(app, ui, route, page, header, margin);
    }
    let inner = Rect::from_min_max(
        pos2(full.left() + margin, ui.cursor().top()),
        pos2(full.right() - margin, full.bottom()),
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
                (Some(header), _) => stacked_header(app, ui, route, page, header),
                (None, Route::Search(query) | Route::SearchOnly(query, _)) => {
                    ui.add_space(8.0);
                    search_kinds(app, ui, route, query);
                }
                (
                    None,
                    Route::Library
                    | Route::LibrarySongs
                    | Route::LibraryAlbums
                    | Route::LibraryArtists,
                ) => {
                    ui.add_space(16.0);
                    library_tabs(app, ui, route);
                }
                (None, Route::History) => title(ui, "History"),
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
            ui.add_space(48.0);
        },
    );
}

/// A page's sections, without its header.
pub fn sections(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page, look: &Look) {
    for (index, section) in page.sections.iter().enumerate() {
        section_block(app, ui, route, section, index, look);
    }
}

/// All, Songs, Albums, Artists, Playlists.
fn search_kinds(app: &App, ui: &mut egui::Ui, route: &Route, query: &str) {
    use crate::backend::SearchKind;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let all = std::iter::once((Route::Search(query.to_string()), "All"));
        let kinds = SearchKind::ALL
            .into_iter()
            .map(|k| (Route::SearchOnly(query.to_string(), k), k.label()));
        for (tab, name) in all.chain(kinds) {
            if theme::chip(ui, name, *route == tab, 32.0).clicked() {
                app.act(Action::Navigate(tab));
            }
        }
    });
    ui.add_space(8.0);
}

/// Playlists, Songs, Albums, Artists.
fn library_tabs(app: &App, ui: &mut egui::Ui, route: &Route) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (tab, name) in [
            (Route::Library, "Playlists"),
            (Route::LibrarySongs, "Songs"),
            (Route::LibraryAlbums, "Albums"),
            (Route::LibraryArtists, "Artists"),
        ] {
            if theme::chip(ui, name, *route == tab, 36.0).clicked() {
                app.act(Action::Navigate(tab));
            }
        }
    });
    ui.add_space(8.0);
}

fn title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(24.0);
    theme::label(ui, text, theme::bold(34.0), PALETTE.text);
    ui.add_space(8.0);
}

// ---- An album or a playlist ----

/// The cover, the words and the buttons on the left, standing still; the
/// songs (and whatever follows them) on the right, scrolling.
fn two_columns(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page, header: &Header) {
    let area = ui.max_rect();
    if let Some(color) = wash_color(app, area.width()) {
        let reach = area.top() - theme::TOP_BAR_HEIGHT + wash_reach(area.height());
        let top = Rect::from_min_max(area.min, pos2(area.right(), reach.min(area.bottom())));
        let whole = (area.top() - theme::TOP_BAR_HEIGHT)..=reach;
        backdrop::wash(ui, top, color, whole.into());
    }
    // Both columns together, in the middle of the page.
    let side = (theme::page_margin(area.width()) * 0.75).max(24.0);
    let block = (area.width() - 2.0 * side).min(312.0 + 24.0 + 960.0);
    let left = area.center().x - block / 2.0;
    let first = Rect::from_min_max(
        pos2(left + 16.0, area.top()),
        pos2(left + 296.0, area.bottom()),
    );
    // The songs scroll with their bar at the window's edge.
    let second = Rect::from_min_max(pos2(left + 336.0, area.top()), area.max);
    let second_width = block - 336.0;

    let mut first_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(first)
            .layout(Layout::top_down(Align::Center)),
    );
    egui::ScrollArea::vertical()
        .id_salt(("page-header", route))
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(&mut first_ui, |ui| {
            ui.add_space(64.0);
            header_column(app, ui, route, page, header);
            ui.add_space(32.0);
        });

    let across = (second_width / 170.0).floor().clamp(2.0, 7.0);
    let look = Look::page(route, second_width, across);
    let mut second_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(second)
            .layout(Layout::top_down(Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt(("page", route))
        .auto_shrink([false, false])
        .show(&mut second_ui, |ui| {
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
            ui.add_space(48.0);
        });
}

/// The same header above the songs, for a narrow window.
fn stacked_header(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page, header: &Header) {
    if header.thumbnail.is_none() {
        // A mood or genre: only words.
        title(ui, &header.title);
        return;
    }
    ui.add_space(32.0);
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 0.0),
        Layout::top_down(Align::Center),
        |ui| header_column(app, ui, route, page, header),
    );
    ui.add_space(24.0);
}

/// What an album's or playlist's header holds, centred: who it is by, the
/// cover, the title, what it is, and the buttons.
fn header_column(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page, header: &Header) {
    let width = ui.available_width().min(280.0);
    ui.spacing_mut().item_spacing.y = 0.0;
    let album = is_album(route);
    let line = |ui: &mut egui::Ui, text: &str, font: egui::FontId, color, rows: usize| {
        let galley = theme::fit_centered(ui, text, font, color, width, rows);
        let (rect, _) = ui.allocate_exact_size(vec2(width, galley.size().y), Sense::hover());
        ui.painter()
            .galley(pos2(rect.center().x, rect.top()), galley, color);
    };
    // An album says whose it is above its cover.
    if album && !header.owner.is_empty() {
        line(ui, &header.owner, theme::regular(14.0), PALETTE.text, 1);
        ui.add_space(15.0);
    }
    let (art, _) = ui.allocate_exact_size(Vec2::splat(240.0), Sense::hover());
    widgets::cover_with(
        app,
        ui,
        art,
        header.thumbnail.as_ref(),
        CornerRadius::same(12),
    );
    ui.add_space(16.0);
    line(ui, &header.title, theme::bold(28.0), PALETTE.text, 2);
    ui.add_space(10.0);
    if !album && !header.owner.is_empty() {
        line(ui, &header.owner, theme::medium(14.0), PALETTE.text, 1);
        ui.add_space(6.0);
    }
    for text in [&header.subtitle, &header.detail] {
        if !text.is_empty() {
            line(ui, text, theme::regular(14.0), PALETTE.secondary, 1);
        }
    }
    ui.add_space(24.0);
    header_buttons(app, ui, route, page, header);
}

/// Save (or rename, for the account's own playlist), the big Play, and
/// the menu.
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
    let width = 40.0 + 32.0 + 64.0 + 32.0 + 40.0;
    ui.allocate_ui_with_layout(
        vec2(width, 64.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 32.0;
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
                if small(ui, Icon::Pencil, "Edit playlist").clicked() {
                    app.act(Action::OpenDialog(Dialog::Rename {
                        playlist_id: playlist_id.clone(),
                        name: header.title.clone(),
                    }));
                }
            } else if let Some(id) = save {
                let saved = app.saved.get(id).copied().or(header.saved).unwrap_or(false);
                let (icon, tip) = if saved {
                    (Icon::Check, "Remove from library")
                } else {
                    (Icon::Plus, "Save to library")
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

            ui.add_enabled_ui(count > 0, |ui| {
                if theme::round_button(
                    ui,
                    Icon::Play,
                    64.0,
                    28.0,
                    Round::Filled,
                    PALETTE.text,
                    "Play",
                )
                .clicked()
                {
                    app.act(Action::PlayTracks {
                        tracks: page.tracks(),
                        start: 0,
                        source: source(route),
                    });
                }
            });

            let more = small(ui, Icon::MoreVertical, "More");
            egui::Popup::menu(&more).gap(8.0).show(|ui| {
                theme::menu(ui);
                let item = |ui: &mut egui::Ui, text: &str, action: Action| {
                    if ui.button(text).clicked() {
                        app.act(action);
                        ui.close();
                    }
                };
                if count > 1 {
                    item(
                        ui,
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
                        "Play next",
                        Action::QueuePlaylist(id.clone(), QueueMode::Next),
                    );
                    item(
                        ui,
                        "Add to queue",
                        Action::QueuePlaylist(id.clone(), QueueMode::End),
                    );
                }
                if let Some(playlist_id) = &own {
                    ui.separator();
                    item(
                        ui,
                        "Edit playlist",
                        Action::OpenDialog(Dialog::Rename {
                            playlist_id: playlist_id.clone(),
                            name: header.title.clone(),
                        }),
                    );
                    item(
                        ui,
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

/// How tall an artist's header is: the picture, with the name, the
/// listeners and the buttons over its foot.
const ARTIST_HEADER: f32 = 428.0;

/// An artist's page begins under a wide picture that fades into the
/// page, with the name and Shuffle, Radio and Subscribe over it.
fn artist_header(
    app: &App,
    ui: &mut egui::Ui,
    route: &Route,
    page: &Page,
    header: &Header,
    margin: f32,
) {
    let full = ui.available_rect_before_wrap();
    let (rect, _) = ui.allocate_exact_size(vec2(full.width(), ARTIST_HEADER), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    // The picture, cropped to fill.
    let texture = header
        .thumbnail
        .as_ref()
        .and_then(|t| app.picture(&t.wide(1280, 534)));
    if let Some(texture) = texture {
        let [w, h] = texture.size();
        let picture = w as f32 / h.max(1) as f32;
        let frame = rect.width() / rect.height();
        let uv = if picture > frame {
            // Wider than its frame: the sides go.
            let keep = frame / picture;
            Rect::from_min_max(pos2((1.0 - keep) / 2.0, 0.0), pos2((1.0 + keep) / 2.0, 1.0))
        } else {
            // Taller: keep the top, where faces are.
            let keep = picture / frame;
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, keep))
        };
        ui.painter()
            .image(texture.id(), rect, uv, egui::Color32::WHITE);
    }
    // Darker to the foot, where it meets the page.
    let fade = |from: f32, to: f32, top: u8, bottom: u8| {
        let mut mesh = egui::Mesh::default();
        let (a, b) = (
            rect.top() + rect.height() * from,
            rect.top() + rect.height() * to,
        );
        let color = |alpha: u8| {
            let [r, g, bl, _] = PALETTE.window.to_array();
            egui::Color32::from_rgba_unmultiplied(r, g, bl, alpha)
        };
        mesh.colored_vertex(pos2(rect.left(), a), color(top));
        mesh.colored_vertex(pos2(rect.right(), a), color(top));
        mesh.colored_vertex(pos2(rect.right(), b), color(bottom));
        mesh.colored_vertex(pos2(rect.left(), b), color(bottom));
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        ui.painter().add(egui::Shape::mesh(mesh));
    };
    // Dark where it meets the top bar, so no edge shows there.
    fade(0.0, 0.2, 255, 70);
    fade(0.2, 0.45, 70, 90);
    fade(0.45, 1.0, 90, 255);

    let left = rect.left() + margin;
    let width = rect.width() - 2.0 * margin;
    let buttons_top = rect.bottom() - 48.0;
    let mut y = buttons_top - 24.0;
    if !header.subtitle.is_empty() {
        y -= 17.0;
        theme::paint_line(
            ui,
            pos2(left, y),
            &header.subtitle,
            theme::regular(14.0),
            PALETTE.secondary,
            width,
        );
        y -= 8.0;
    }
    let size = if rect.width() > 1100.0 { 45.0 } else { 34.0 };
    let name = theme::fit(ui, &header.title, theme::bold(size), PALETTE.text, width, 1);
    y -= name.size().y;
    ui.painter().galley(pos2(left, y), name, PALETTE.text);

    let count = page
        .sections
        .iter()
        .map(|s| songs(s).count())
        .sum::<usize>();
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(
                pos2(left, buttons_top),
                vec2(width, 40.0),
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 8.0;
    if count > 1 && theme::pill(&mut row, Some(Icon::Shuffle), "Shuffle", Pill::Filled).clicked() {
        app.act(Action::Shuffle {
            tracks: page.tracks(),
            source: source(route),
        });
    }
    // An artist's radio: their best-known song, then songs like it.
    if count > 0
        && theme::pill(&mut row, Some(Icon::Radio), "Radio", Pill::Filled).clicked()
        && let Some(first) = page.tracks().into_iter().next()
    {
        let radio = Target::Watch {
            video_id: Some(first.video_id.clone()),
            playlist_id: None,
        };
        app.act(Action::Play(radio, Some(first)));
    }
    if let Some(channel) = &header.channel_id {
        let subscribed = app
            .subscribed
            .get(channel)
            .copied()
            .or(header.subscribed)
            .unwrap_or(false);
        let (label, style) = if subscribed {
            ("Subscribed", Pill::Tonal)
        } else {
            ("Subscribe", Pill::Outline(PALETTE.subscribe))
        };
        if theme::pill(&mut row, None, label, style).clicked() {
            app.act(Action::ToggleSubscribe {
                channel_id: channel.clone(),
                subscribe: !subscribed,
            });
        }
    }
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
    let card_shelf = section.shape == Shape::Carousel && pictures;
    let shelf = ui.id().with(("shelf", route, index));
    // Shelves that scroll sideways and grids have large titles; lists
    // (search results, an artist's top songs) smaller ones.
    let title_size = if section.shape == Shape::List {
        look.title.min(24.0)
    } else {
        look.title
    };

    if !section.title.is_empty() {
        ui.add_space(32.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let room = ui.available_width() - 180.0;
            let title = theme::fit(
                ui,
                &section.title,
                theme::bold(title_size),
                PALETTE.text,
                room.max(120.0),
                2,
            );
            let (rect, _) = ui.allocate_exact_size(title.size(), Sense::hover());
            ui.painter().galley(rect.min, title, PALETTE.text);
            ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                if card_shelf || song_shelf {
                    arrows(ui, shelf);
                }
                if let Some(more) = &section.more
                    && theme::pill(ui, None, "More", Pill::Outline(PALETTE.text)).clicked()
                {
                    app.act(Action::Open(more.clone(), None));
                }
            });
        });
        ui.add_space(16.0);
    } else if index > 0 {
        ui.add_space(24.0);
    }

    // The first of all results, when it stands alone: drawn large.
    if index == 0
        && matches!(route, Route::Search(_))
        && section.items.len() == 1
        && let Some(item) = section.items.first()
    {
        top_result(app, ui, route, item);
        return;
    }

    if !cards.is_empty() {
        match section.shape {
            // Moods and genres: buttons. One row that scrolls sideways at
            // the top of Home, wrapped elsewhere.
            _ if !pictures => {
                if *route == Route::Home {
                    carousel(ui, shelf.with("chips"), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        for card in &cards {
                            widgets::chip(app, ui, card);
                        }
                    });
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                        for card in &cards {
                            widgets::chip(app, ui, card);
                        }
                    });
                }
            }
            Shape::Carousel => {
                carousel(ui, shelf, |ui| {
                    ui.spacing_mut().item_spacing.x = CARD_GAP;
                    for card in &cards {
                        widgets::card(app, ui, card, look.card);
                    }
                });
            }
            Shape::Grid => {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(CARD_GAP, 16.0);
                    for card in &cards {
                        widgets::card(app, ui, card, look.card);
                    }
                });
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
    let play_from = |start: usize| Action::PlayTracks {
        tracks: section.tracks(),
        start,
        source: source(route),
    };
    if song_shelf {
        // Quick picks: columns of four songs that scroll sideways.
        let width = look.column;
        carousel(ui, shelf, |ui| {
            ui.spacing_mut().item_spacing.x = CARD_GAP;
            for (column, chunk) in tracks.chunks(GRID_ROWS).enumerate() {
                ui.allocate_ui_with_layout(
                    vec2(width, (Row::GRID.height + Row::GRID.gap) * GRID_ROWS as f32),
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
    for (start, track) in tracks.into_iter().enumerate() {
        let is_playing = playing == Some(track.video_id.as_str());
        let number = numbered.then_some(start + 1);
        widgets::track_row(app, ui, style, track, number, is_playing, || {
            play_from(start)
        });
    }
}

/// Search's best match, as YouTube Music's "Top result" card: a large
/// picture, the name, what it is, and Play.
fn top_result(app: &App, ui: &mut egui::Ui, route: &Route, item: &Item) {
    let width = ui.available_width().min(860.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 152.0), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let (title, subtitle, thumbnail, round) = match item {
        Item::Track(track) => {
            let kind = match track.kind {
                ytfast_core::read::TrackKind::MusicVideo => "Video",
                _ => "Song",
            };
            let mut parts = vec![kind.to_string(), track.artists.clone()];
            parts.extend(track.album.clone());
            parts.extend(track.duration_seconds.map(|s| theme::clock(f64::from(s))));
            parts.retain(|part| !part.is_empty());
            (
                track.title.as_str(),
                parts.join(" \u{2022} "),
                track.thumbnail.as_ref(),
                false,
            )
        }
        Item::Card(card) => (
            card.title.as_str(),
            card.subtitle.clone(),
            card.thumbnail.as_ref(),
            card.round,
        ),
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    let fill = if response.hovered() {
        PALETTE.surface_hover
    } else {
        PALETTE.surface
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    let art = Rect::from_min_size(rect.min + vec2(24.0, 24.0), Vec2::splat(104.0));
    if round {
        widgets::cover(app, ui, art, thumbnail, true);
    } else {
        widgets::cover_with(app, ui, art, thumbnail, CornerRadius::same(4));
    }
    let left = art.right() + 24.0;
    let room = (rect.right() - 24.0 - left).max(0.0);
    theme::paint_line(
        ui,
        pos2(left, rect.top() + 22.0),
        title,
        theme::bold(24.0),
        PALETTE.text,
        room,
    );
    theme::paint_line(
        ui,
        pos2(left, rect.top() + 58.0),
        &subtitle,
        theme::regular(14.0),
        PALETTE.secondary,
        room,
    );

    // What the card does, and its buttons.
    let play = || match item {
        Item::Track(track) => Some(Action::PlayTracks {
            tracks: vec![track.clone()],
            start: 0,
            source: source(route),
        }),
        Item::Card(card) => card.play.clone().map(|target| Action::Play(target, None)),
    };
    let open = || match item {
        Item::Track(_) => None,
        Item::Card(card) => card.open.clone().map(|target| Action::Open(target, None)),
    };
    let plays = match item {
        Item::Track(_) => true,
        Item::Card(card) => card.play.is_some(),
    };
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(
                pos2(left, rect.top() + 92.0),
                vec2(room, 36.0),
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 8.0;
    let mut pressed = false;
    if plays && theme::pill(&mut row, Some(Icon::Play), "Play", Pill::Filled).clicked() {
        pressed = true;
        if let Some(action) = play() {
            app.act(action);
        }
    }
    if let Item::Track(track) = item
        && theme::pill(&mut row, Some(Icon::Radio), "Radio", Pill::Tonal).clicked()
    {
        pressed = true;
        let radio = Target::Watch {
            video_id: Some(track.video_id.clone()),
            playlist_id: None,
        };
        app.act(Action::Play(radio, Some(track.clone())));
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // A song plays; anything else opens.
    if response.clicked()
        && !pressed
        && let Some(action) = open().or_else(play)
    {
        app.act(action);
    }
    match item {
        Item::Track(track) => {
            response.context_menu(|ui| widgets::song_menu(app, ui, track, None));
        }
        Item::Card(card) => {
            response.context_menu(|ui| widgets::card_menu(app, ui, card));
        }
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
            ui.painter().circle_stroke(
                rect.center(),
                17.5,
                egui::Stroke::new(1.0, PALETTE.outline),
            );
            let tint = if enabled { PALETTE.text } else { PALETTE.faint };
            theme::paint_icon(ui, icon, rect, 20.0, tint);
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
