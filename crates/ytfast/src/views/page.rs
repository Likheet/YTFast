//! The page in the middle: Home, Explore, Library, search results, an
//! album, a playlist or an artist. All of them are a header and sections.

use egui::{Align, Layout, Sense, Vec2, vec2};
use ytfast_core::read::{Card, Header, Item, Page, Section, Shape, Track};

use crate::app::{Action, App, Loadable};
use crate::backend::Route;
use crate::theme::{self, Icon, PALETTE};
use crate::views::widgets;

pub fn show(app: &App, ui: &mut egui::Ui) {
    let route = app.route.clone();
    if route == Route::Settings {
        crate::views::settings::show(app, ui);
        return;
    }
    match app.pages.get(&route) {
        None | Some(Loadable::Loading) => {
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(28.0).color(PALETTE.secondary));
            });
        }
        Some(Loadable::Failed(message)) => {
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                theme::label(
                    ui,
                    "This page could not load",
                    theme::bold(20.0),
                    PALETTE.text,
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(message)
                        .font(theme::regular(14.0))
                        .color(PALETTE.secondary),
                );
                ui.add_space(14.0);
                if theme::pill_button(ui, "Try again", true).clicked() {
                    app.act(Action::Retry(route.clone()));
                }
            });
        }
        Some(Loadable::Ready(page)) => {
            egui::ScrollArea::vertical()
                .id_salt(("page", &route))
                .auto_shrink([false, false])
                .show(ui, |ui| content(app, ui, &route, page));
        }
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

/// An album's page, whose songs are numbered.
fn is_album(route: &Route) -> bool {
    matches!(route, Route::Browse { id, .. } if id.starts_with("MPRE"))
}

fn songs(section: &Section) -> impl Iterator<Item = &Track> {
    section.items.iter().filter_map(|i| match i {
        Item::Track(t) => Some(t),
        Item::Card(_) => None,
    })
}

fn content(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page) {
    ui.add_space(20.0);
    match (&page.header, route) {
        (Some(header), _) => header_block(app, ui, route, page, header),
        (None, Route::Search(query)) => title(ui, &format!("Results for \u{201c}{query}\u{201d}")),
        (
            None,
            Route::Library | Route::LibrarySongs | Route::LibraryAlbums | Route::LibraryArtists,
        ) => {
            title(ui, "Library");
            library_tabs(app, ui, route);
        }
        (None, Route::History) => title(ui, "History"),
        (None, Route::Explore) => title(ui, "Explore"),
        _ => {}
    }
    if page.sections.is_empty() {
        ui.add_space(24.0);
        theme::label(
            ui,
            "Nothing here yet.",
            theme::regular(15.0),
            PALETTE.secondary,
        );
    }
    sections(app, ui, route, page);
    ui.add_space(32.0);
}

/// A page's sections, without its header.
pub fn sections(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page) {
    for (index, section) in page.sections.iter().enumerate() {
        section_block(app, ui, route, section, index);
    }
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
            let chosen = *route == tab;
            let text = egui::RichText::new(name)
                .font(theme::medium(14.0))
                .color(if chosen {
                    egui::Color32::BLACK
                } else {
                    PALETTE.text
                });
            let button = egui::Button::new(text)
                .fill(if chosen {
                    PALETTE.text
                } else {
                    PALETTE.surface
                })
                .corner_radius(egui::CornerRadius::same(16))
                .min_size(vec2(72.0, 32.0));
            if ui.add(button).clicked() {
                app.act(Action::Navigate(tab));
            }
        }
        if *route == Route::Library {
            ui.add_space(8.0);
            if theme::pill_button(ui, "+ New playlist", false).clicked() {
                app.act(Action::OpenDialog(crate::app::Dialog::NewPlaylist {
                    name: String::new(),
                    song: None,
                }));
            }
        }
    });
    ui.add_space(14.0);
}

fn title(ui: &mut egui::Ui, text: &str) {
    theme::label(ui, text, theme::bold(30.0), PALETTE.text);
    ui.add_space(12.0);
}

fn header_block(app: &App, ui: &mut egui::Ui, route: &Route, page: &Page, header: &Header) {
    let lines: Vec<&String> = [&header.subtitle, &header.owner, &header.detail]
        .into_iter()
        .filter(|l| !l.is_empty())
        .collect();
    // Counted every frame; copied only when a button is pressed.
    let count = page
        .sections
        .iter()
        .map(|s| songs(s).count())
        .sum::<usize>();
    let text = |ui: &mut egui::Ui| {
        theme::label(ui, &header.title, theme::bold(34.0), PALETTE.text);
        ui.add_space(4.0);
        for line in &lines {
            theme::label(ui, line, theme::regular(15.0), PALETTE.secondary);
        }
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if count > 0 && theme::pill_button(ui, "Play", true).clicked() {
                app.act(Action::PlayTracks {
                    tracks: page.tracks(),
                    start: 0,
                    source: source(route),
                });
            }
            if count > 1 && theme::pill_button(ui, "Shuffle", false).clicked() {
                app.act(Action::Shuffle {
                    tracks: page.tracks(),
                    source: source(route),
                });
            }
            header_actions(app, ui, route, header);
        });
    };
    if header.thumbnail.is_none() {
        // A mood or genre: only words.
        text(ui);
        ui.add_space(20.0);
        return;
    }
    let side = 200.0;
    // The text's height, to center it beside the cover.
    let height = 44.0 + 22.0 * lines.len() as f32 + 50.0;
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
        widgets::cover(app, ui, rect, header.thumbnail.as_ref(), header.round);
        ui.add_space(24.0);
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), side),
            Layout::top_down(Align::Min),
            |ui| {
                ui.add_space(((side - height) / 2.0).max(0.0));
                text(ui);
            },
        );
    });
    ui.add_space(24.0);
}

fn section_block(app: &App, ui: &mut egui::Ui, route: &Route, section: &Section, index: usize) {
    let cards: Vec<&Card> = section
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Card(c) => Some(c),
            Item::Track(_) => None,
        })
        .collect();

    if !section.title.is_empty() {
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            theme::label(ui, &section.title, theme::bold(22.0), PALETTE.text);
            if let Some(more) = &section.more {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let more_button = egui::Button::new(
                        egui::RichText::new("More")
                            .font(theme::medium(13.0))
                            .color(PALETTE.text),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::new(1.0, PALETTE.outline))
                    .corner_radius(egui::CornerRadius::same(14));
                    if ui.add(more_button).clicked() {
                        app.act(Action::Open(more.clone(), None));
                    }
                });
            }
        });
        ui.add_space(10.0);
    }

    if !cards.is_empty() {
        let pictures = cards.iter().any(|c| c.thumbnail.is_some());
        match section.shape {
            // Moods and genres: buttons, wrapped.
            _ if !pictures => {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                    for card in &cards {
                        widgets::chip(app, ui, card);
                    }
                });
            }
            Shape::Carousel => {
                egui::ScrollArea::horizontal()
                    .id_salt(("carousel", route, index))
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                    )
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 18.0;
                            for card in &cards {
                                widgets::card(app, ui, card);
                            }
                        });
                        ui.add_space(6.0);
                    });
            }
            Shape::Grid => {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(18.0, 18.0);
                    for card in &cards {
                        widgets::card(app, ui, card);
                    }
                });
            }
            // Search results: one under another.
            Shape::List => {
                ui.spacing_mut().item_spacing.y = 0.0;
                for card in &cards {
                    widgets::card_row(app, ui, card);
                }
                ui.spacing_mut().item_spacing.y = 6.0;
            }
        }
        ui.add_space(8.0);
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
    let tracks: Vec<&Track> = songs(section).collect();
    if section.shape == Shape::Carousel && tracks.len() > GRID_ROWS {
        // Quick picks: columns of four songs that scroll sideways.
        let width = (ui.available_width() / 2.2).clamp(300.0, 440.0);
        egui::ScrollArea::horizontal()
            .id_salt(("songs", route, index))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    for (column, chunk) in tracks.chunks(GRID_ROWS).enumerate() {
                        ui.allocate_ui_with_layout(
                            vec2(width, theme::ROW_HEIGHT * GRID_ROWS as f32),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.set_width(width);
                                ui.spacing_mut().item_spacing.y = 0.0;
                                for (row, track) in chunk.iter().enumerate() {
                                    let start = column * GRID_ROWS + row;
                                    let is_playing = playing == Some(track.video_id.as_str());
                                    widgets::track_row(app, ui, track, None, is_playing, || {
                                        play_from(start)
                                    });
                                }
                            },
                        );
                    }
                });
                ui.add_space(6.0);
            });
        return;
    }
    let numbered = is_album(route) && section.title.is_empty();
    ui.spacing_mut().item_spacing.y = 0.0;
    for (start, track) in tracks.into_iter().enumerate() {
        let is_playing = playing == Some(track.video_id.as_str());
        let number = numbered.then_some(start + 1);
        widgets::track_row(app, ui, track, number, is_playing, || play_from(start));
    }
    ui.spacing_mut().item_spacing.y = 6.0;
}

/// Songs per column in a sideways shelf (Quick picks).
const GRID_ROWS: usize = 4;

/// Save to library, Subscribe, and Rename and Delete for the account's own
/// playlists.
fn header_actions(app: &App, ui: &mut egui::Ui, route: &Route, header: &Header) {
    if let Some(id) = header.library_id.as_ref().filter(|_| !header.editable) {
        let saved = app.saved.get(id).copied().or(header.saved).unwrap_or(false);
        let label = if saved {
            "In your library"
        } else {
            "Save to library"
        };
        if theme::pill_button(ui, label, false).clicked() {
            app.act(Action::ToggleSave {
                playlist_id: id.clone(),
                save: !saved,
            });
        }
    }
    if let Some(channel) = &header.channel_id {
        let subscribed = app
            .subscribed
            .get(channel)
            .copied()
            .or(header.subscribed)
            .unwrap_or(false);
        let label = if subscribed {
            "Subscribed"
        } else {
            "Subscribe"
        };
        if theme::pill_button(ui, label, !subscribed).clicked() {
            app.act(Action::ToggleSubscribe {
                channel_id: channel.clone(),
                subscribe: !subscribed,
            });
        }
    }
    let own = match route {
        Route::Browse { id, .. } if header.editable => {
            Some(id.strip_prefix("VL").unwrap_or(id).to_string())
        }
        _ => None,
    };
    if let Some(playlist_id) = own {
        if theme::icon_button(ui, Icon::Pencil, 18.0, PALETTE.secondary, "Rename").clicked() {
            app.act(Action::OpenDialog(crate::app::Dialog::Rename {
                playlist_id: playlist_id.clone(),
                name: header.title.clone(),
            }));
        }
        if theme::icon_button(ui, Icon::Trash, 18.0, PALETTE.secondary, "Delete").clicked() {
            app.act(Action::OpenDialog(crate::app::Dialog::Delete {
                playlist_id,
                title: header.title.clone(),
            }));
        }
    }
}
