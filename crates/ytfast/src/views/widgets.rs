//! The pieces pages are made of, sized as YouTube Music sizes them: a
//! cover picture, a song row, a card.

use egui::{Color32, CornerRadius, Rect, Sense, Vec2, pos2, vec2};
use ytfast_core::read::{Card, Target, Thumb, Track, TrackKind};

use crate::app::{Action, App};
use crate::theme::{self, Icon, PALETTE};

/// Draws a cover in `rect`: the picture when it has arrived, a quiet
/// placeholder until then. Round for artists.
pub fn cover(app: &App, ui: &egui::Ui, rect: Rect, thumb: Option<&Thumb>, round: bool) {
    let radius = if round {
        CornerRadius::same(255)
    } else if rect.width() > 200.0 {
        CornerRadius::same(12)
    } else if rect.width() > 64.0 {
        CornerRadius::same(6)
    } else {
        CornerRadius::same(4)
    };
    cover_with(app, ui, rect, thumb, radius);
}

/// [`cover`], with the corners' rounding chosen.
pub fn cover_with(
    app: &App,
    ui: &egui::Ui,
    rect: Rect,
    thumb: Option<&Thumb>,
    radius: CornerRadius,
) {
    let pixels = (rect.width() * ui.ctx().pixels_per_point()).ceil() as u32;
    // Ask for a size the server makes, not every size drawn.
    let size = [60, 120, 226, 360, 544]
        .into_iter()
        .find(|s| *s >= pixels)
        .unwrap_or(544);
    let texture = thumb.and_then(|t| app.picture(&t.sized(size)));
    match texture {
        Some(texture) => {
            egui::Image::new((texture.id(), rect.size()))
                .corner_radius(radius)
                .paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, radius, PALETTE.surface);
            let icon = rect.width() * 0.35;
            theme::paint_icon(ui, Icon::Music, rect, icon, PALETTE.faint);
        }
    }
}

/// How a song row is drawn. The sizes are YouTube Music's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub height: f32,
    /// The space under the row, before the next.
    pub gap: f32,
    /// The cover's side (or the number's box).
    pub art: f32,
    /// The space left and right, inside the row.
    pub pad: f32,
    /// A hairline under the row.
    pub line: bool,
    /// The artist and the album in columns of their own, beside the title
    /// (an artist's top songs), not under it.
    pub columns: bool,
    /// What the line under the title says.
    pub under: Under,
    /// The song's length, at the right.
    pub length: bool,
}

/// What a song row says under the song's title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Under {
    /// "Artist".
    Artists,
    /// "Artist • Album".
    ArtistsAlbum,
    /// "Song • Artist • Album", as search results say what each row is.
    Kind,
}

impl Row {
    /// A playlist's or an album's songs.
    pub const LIST: Self = Self {
        height: 68.0,
        gap: 8.0,
        art: 48.0,
        pad: 16.0,
        line: false,
        columns: false,
        under: Under::Artists,
        length: true,
    };
    /// A shelf of songs on a page (an artist's top songs).
    pub const SHELF: Self = Self {
        height: 48.0,
        gap: 0.0,
        art: 32.0,
        pad: 8.0,
        line: true,
        columns: true,
        under: Under::Artists,
        length: true,
    };
    /// Search results.
    pub const SEARCH: Self = Self {
        height: 80.0,
        gap: 0.0,
        art: 56.0,
        pad: 8.0,
        line: false,
        columns: false,
        under: Under::Kind,
        length: true,
    };
    /// Songs in columns that scroll sideways (Quick picks, "You might
    /// also like").
    pub const GRID: Self = Self {
        height: 48.0,
        gap: 16.0,
        art: 48.0,
        pad: 8.0,
        line: false,
        columns: false,
        under: Under::ArtistsAlbum,
        length: false,
    };
    /// Up next.
    pub const QUEUE: Self = Self {
        height: 56.0,
        gap: 0.0,
        art: 32.0,
        pad: 8.0,
        line: true,
        columns: false,
        under: Under::Artists,
        length: true,
    };
}

/// How long the pointer rests on a song before it is found ahead of time.
const WARM_AFTER: f32 = 0.35;

/// One song in a list. `number` (on an album's page) is shown instead of
/// the cover; `playing` marks the song playing now.
pub fn track_row(
    app: &App,
    ui: &mut egui::Ui,
    style: Row,
    track: &Track,
    number: Option<usize>,
    playing: bool,
    on_click: impl FnOnce() -> Action,
) {
    track_row_in(app, ui, style, track, number, playing, None, on_click);
}

/// A song row in Up next (`queued` is its entry), whose menu edits the
/// queue.
#[allow(clippy::too_many_arguments)]
pub fn track_row_in(
    app: &App,
    ui: &mut egui::Ui,
    style: Row,
    track: &Track,
    number: Option<usize>,
    playing: bool,
    queued: Option<u64>,
    on_click: impl FnOnce() -> Action,
) {
    let width = ui.available_width();
    let (slot, placed) =
        ui.allocate_exact_size(vec2(width, style.height + style.gap), Sense::hover());
    let rect = Rect::from_min_size(slot.min, vec2(width, style.height));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let id = placed.id.with("row");
    let response = ui.interact(rect, id, Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &track.title));
    let menu_open = egui::Popup::is_id_open(ui.ctx(), id.with("menu-popup"));
    let hovered = ui.rect_contains_pointer(rect) || menu_open;
    let radius = if style.line { 0 } else { 8 };
    // The queue marks the song playing; every list marks the row pointed at.
    if (playing && queued.is_some()) || hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(radius), PALETTE.surface);
    }
    if style.line {
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(1.0, PALETTE.outline),
        );
    }
    if response.hovered() {
        // A song the pointer rests on is found ahead of time, so a click
        // starts it at once. Rows scrolled under a still pointer are only
        // passing: resting counts from when this row came under it.
        let resting = app.resting_on(ui.ctx(), &track.video_id);
        if resting >= WARM_AFTER {
            app.warm(&track.video_id);
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f32(
                    WARM_AFTER - resting + 0.05,
                ));
        }
    }

    let art = Rect::from_min_size(
        pos2(rect.left() + style.pad, rect.center().y - style.art / 2.0),
        Vec2::splat(style.art),
    );
    let glyph = (style.art * 0.5).clamp(16.0, 24.0);
    match number {
        Some(_) if playing => theme::paint_icon(ui, Icon::Volume, art, glyph, PALETTE.text),
        Some(_) if hovered => theme::paint_icon(ui, Icon::Play, art, glyph * 0.8, PALETTE.text),
        Some(number) => {
            ui.painter().text(
                art.center(),
                egui::Align2::CENTER_CENTER,
                number.to_string(),
                theme::medium(14.0),
                PALETTE.dim,
            );
        }
        None => {
            let radius = CornerRadius::same(if style.art > 40.0 { 4 } else { 2 });
            cover_with(app, ui, art, track.thumbnail.as_ref(), radius);
            if playing || hovered {
                ui.painter()
                    .rect_filled(art, radius, Color32::from_black_alpha(150));
                let icon = if playing { Icon::Volume } else { Icon::Play };
                let size = if playing { glyph } else { glyph * 0.8 };
                theme::paint_icon(ui, icon, art, size, PALETTE.text);
            }
        }
    }

    // From the right: the menu button (when pointed at), then the length.
    let duration = track
        .duration_seconds
        .filter(|_| style.length)
        .map(|s| theme::clock(f64::from(s)))
        .unwrap_or_default();
    let mut right = rect.right() - style.pad;
    let menu = Rect::from_center_size(pos2(right - 12.0, rect.center().y), Vec2::splat(32.0));
    if !duration.is_empty() {
        let galley = ui
            .painter()
            .layout_no_wrap(duration, theme::regular(14.0), PALETTE.secondary);
        let at = pos2(
            right - 32.0 - galley.size().x,
            rect.center().y - galley.size().y / 2.0,
        );
        right = at.x;
        ui.painter().galley(at, galley, PALETTE.secondary);
    } else {
        right -= 32.0;
    }
    let mut clicked_menu = false;
    if hovered {
        let menu_response = ui.interact(menu, id.with("menu"), Sense::click());
        if menu_response.hovered() {
            ui.painter()
                .circle_filled(menu.center(), 16.0, PALETTE.surface);
        }
        theme::paint_icon(ui, Icon::MoreVertical, menu, 18.0, PALETTE.text);
        clicked_menu = menu_response.clicked();
        egui::Popup::menu(&menu_response)
            .id(id.with("menu-popup"))
            .show(|ui| song_menu(app, ui, track, queued.filter(|_| !playing)));
    }

    let text_left = art.right() + 16.0;
    let text_right = right - 16.0;
    let space = (text_right - text_left).max(0.0);
    let title_font = theme::medium(14.0);
    let line = theme::regular(14.0);
    if style.columns && space > 420.0 {
        // Title | artist | album, each a third.
        let column = space / 3.0;
        let y = rect.center().y - 9.0;
        theme::paint_line(
            ui,
            pos2(text_left, y),
            &track.title,
            title_font,
            PALETTE.text,
            column - 16.0,
        );
        theme::paint_line(
            ui,
            pos2(text_left + column, y),
            &track.artists,
            line.clone(),
            PALETTE.secondary,
            column - 16.0,
        );
        theme::paint_line(
            ui,
            pos2(text_left + 2.0 * column, y),
            track.album.as_deref().unwrap_or_default(),
            line,
            PALETTE.secondary,
            column - 16.0,
        );
    } else {
        let under = described(track, style.under);
        if under.is_empty() {
            theme::paint_line(
                ui,
                pos2(text_left, rect.center().y - 9.0),
                &track.title,
                title_font,
                PALETTE.text,
                space,
            );
        } else {
            theme::paint_line(
                ui,
                pos2(text_left, rect.center().y - 20.0),
                &track.title,
                title_font,
                PALETTE.text,
                space,
            );
            theme::paint_line(
                ui,
                pos2(text_left, rect.center().y + 3.0),
                &under,
                line,
                PALETTE.secondary,
                space,
            );
        }
    }
    if response.clicked() && !clicked_menu {
        app.act(on_click());
    }
    response.context_menu(|ui| song_menu(app, ui, track, queued.filter(|_| !playing)));
}

/// The line under a song's title: "Artist", "Artist • Album" or
/// "Song • Artist • Album".
fn described(track: &Track, under: Under) -> String {
    let kind = match (under, &track.kind) {
        (Under::Kind, TrackKind::Song) => "Song",
        (Under::Kind, TrackKind::MusicVideo) => "Video",
        _ => "",
    };
    let album = match under {
        Under::Artists => "",
        Under::ArtistsAlbum | Under::Kind => track.album.as_deref().unwrap_or_default(),
    };
    [kind, &track.artists, album]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" \u{2022} ")
}

/// What a right-click on a song (or its ⋮ button) offers.
pub fn song_menu(app: &App, ui: &mut egui::Ui, track: &Track, queued: Option<u64>) {
    theme::menu(ui);
    let item = |ui: &mut egui::Ui, text: &str, action: Action| {
        if ui.button(text).clicked() {
            app.act(action);
            ui.close();
        }
    };
    if ui.button("Start radio").clicked() {
        let radio = Target::Watch {
            video_id: Some(track.video_id.clone()),
            playlist_id: None,
        };
        app.act(Action::Play(radio, Some(track.clone())));
        ui.close();
    }
    match queued {
        // A song in Up next.
        Some(id) => {
            item(ui, "Play next", Action::MoveNextInQueue(id));
            item(ui, "Move up", Action::ShiftInQueue(id, true));
            item(ui, "Move down", Action::ShiftInQueue(id, false));
            item(ui, "Remove from queue", Action::RemoveFromQueue(id));
        }
        None => {
            item(ui, "Play next", Action::PlayNext(track.clone()));
            item(ui, "Add to queue", Action::AddToQueue(track.clone()));
        }
    }
    ui.separator();
    // As chosen in this run or as YouTube said; else every song on Liked
    // Music is liked.
    let liked = app
        .likes
        .get(&track.video_id)
        .map_or(app.route == crate::backend::Route::Liked, |like| {
            *like == crate::app::LikeState::Liked
        });
    if liked {
        item(
            ui,
            "Remove from liked songs",
            Action::Rate(track.video_id.clone(), crate::app::LikeState::Neutral),
        );
    } else {
        item(
            ui,
            "Add to liked songs",
            Action::Rate(track.video_id.clone(), crate::app::LikeState::Liked),
        );
    }
    crate::views::playlists_menu(app, ui, track);
    if queued.is_none()
        && let Some(set_video_id) = &track.set_video_id
        && let Some(playlist_id) = app.editable_playlist()
    {
        item(
            ui,
            "Remove from playlist",
            Action::RemoveFromPlaylist {
                playlist_id,
                video_id: track.video_id.clone(),
                set_video_id: set_video_id.clone(),
            },
        );
    }
    if let Some(album) = &track.album_id {
        item(
            ui,
            "Go to album",
            Action::Navigate(crate::backend::Route::browse(album.clone(), None)),
        );
    }
    if let Some(artist) = &track.artist_id {
        item(
            ui,
            "Go to artist",
            Action::Navigate(crate::backend::Route::browse(artist.clone(), None)),
        );
    }
    if ui.button("Copy link").clicked() {
        ui.ctx().copy_text(format!(
            "https://music.youtube.com/watch?v={}",
            track.video_id
        ));
        app.act(Action::Notify("Link copied to clipboard".into()));
        ui.close();
    }
}

/// The height of a card whose cover is `size` across: the cover, then up
/// to two lines of title and two of subtitle.
pub fn card_height(size: f32) -> f32 {
    size + 84.0
}

/// A card: a cover with a title and subtitle under it. Clicking opens it;
/// the round button over the cover (when pointed at) plays it.
pub fn card(app: &App, ui: &mut egui::Ui, card: &Card, size: f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(size, card_height(size)), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &card.title));
    let art = Rect::from_min_size(rect.min, Vec2::splat(size));
    cover(app, ui, art, card.thumbnail.as_ref(), card.round);
    // Hovered by where the pointer is, not by which widget is on top: the
    // play button over the cover must not make the card think the pointer
    // left (that made it flicker).
    let hovered = ui.rect_contains_pointer(rect);
    let lit = ui
        .ctx()
        .animate_bool_with_time(response.id.with("hover"), hovered, 0.15);
    let mut play_clicked = false;
    if lit > 0.0 && card.play.is_some() {
        let radius = if card.round {
            CornerRadius::same(255)
        } else {
            CornerRadius::same(6)
        };
        ui.painter()
            .rect_filled(art, radius, Color32::from_black_alpha((60.0 * lit) as u8));
        let button =
            Rect::from_center_size(art.right_bottom() - vec2(32.0, 32.0), Vec2::splat(40.0));
        let over_button = hovered
            && ui
                .interact(button, response.id.with("play"), Sense::click())
                .clicked();
        let grown = if ui.rect_contains_pointer(button) {
            1.15
        } else {
            1.0
        };
        ui.painter().circle_filled(
            button.center(),
            20.0 * grown,
            Color32::from_black_alpha((205.0 * lit) as u8),
        );
        theme::paint_icon(
            ui,
            Icon::Play,
            button,
            18.0 * grown,
            Color32::from_white_alpha((255.0 * lit) as u8),
        );
        play_clicked = over_button;
    }

    let center = card.round;
    let title = theme::fit(ui, &card.title, theme::medium(14.0), PALETTE.text, size, 2);
    let place = |galley: &egui::Galley, top: f32| {
        let x = if center {
            rect.center().x - galley.size().x / 2.0
        } else {
            rect.left()
        };
        pos2(x, top)
    };
    let mut top = art.bottom() + 12.0;
    let at = place(&title, top);
    top += title.size().y + 4.0;
    ui.painter().galley(at, title, PALETTE.text);
    if !card.subtitle.is_empty() {
        let subtitle = theme::fit(
            ui,
            &card.subtitle,
            theme::regular(14.0),
            PALETTE.secondary,
            size,
            2,
        );
        let at = place(&subtitle, top);
        ui.painter().galley(at, subtitle, PALETTE.secondary);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.context_menu(|ui| card_menu(app, ui, card));
    if play_clicked {
        if let Some(target) = card.play.clone() {
            app.act(Action::Play(target, card.song()));
        }
    } else if response.clicked()
        && let Some(target) = card.open.clone().or_else(|| card.play.clone())
    {
        app.act(Action::Open(target, card.song()));
    }
}

/// What a right-click on an album or playlist offers.
pub fn card_menu(app: &App, ui: &mut egui::Ui, card: &Card) {
    use crate::app::QueueMode;
    theme::menu(ui);
    let item = |ui: &mut egui::Ui, text: &str, action: Action| {
        if ui.button(text).clicked() {
            app.act(action);
            ui.close();
        }
    };
    let playlist = match &card.play {
        Some(Target::Watch {
            playlist_id: Some(id),
            ..
        }) => Some(id.clone()),
        _ => None,
    };
    if let Some(id) = &playlist {
        item(
            ui,
            "Play",
            Action::QueuePlaylist(id.clone(), QueueMode::Play),
        );
        item(
            ui,
            "Shuffle play",
            Action::QueuePlaylist(id.clone(), QueueMode::Shuffle),
        );
        let radio = Target::Watch {
            video_id: None,
            playlist_id: Some(format!("RDAMPL{id}")),
        };
        item(ui, "Start radio", Action::Play(radio, None));
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
        // Mixes, Liked Music and the account's own playlists are not
        // saved to the library.
        let own = app.own_playlists().iter().any(|(own, _)| own == id);
        if !own && !id.starts_with("RD") && id != "LM" && id != "SE" {
            ui.separator();
            // As chosen in this run; else what the Library shows is saved.
            let in_library = matches!(
                app.route,
                crate::backend::Route::Library | crate::backend::Route::LibraryAlbums
            );
            let saved = app.saved.get(id).copied().unwrap_or(in_library);
            let label = if saved {
                "Remove from library"
            } else {
                "Save to library"
            };
            item(
                ui,
                label,
                Action::ToggleSave {
                    playlist_id: id.clone(),
                    save: !saved,
                },
            );
        }
    }
    let link = match &card.open {
        Some(Target::Browse { id, .. }) if id.starts_with("VL") => Some(format!(
            "https://music.youtube.com/playlist?list={}",
            &id[2..]
        )),
        Some(Target::Browse { id, .. }) if id.starts_with("UC") => {
            Some(format!("https://music.youtube.com/channel/{id}"))
        }
        Some(Target::Browse {
            id, params: None, ..
        }) => Some(format!("https://music.youtube.com/browse/{id}")),
        _ => playlist.map(|id| format!("https://music.youtube.com/playlist?list={id}")),
    };
    if let Some(link) = link
        && ui.button("Copy link").clicked()
    {
        ui.ctx().copy_text(link);
        app.act(Action::Notify("Link copied to clipboard".into()));
        ui.close();
    }
}

/// A card as a row (an album or artist in search results): a cover, the
/// title and the subtitle. Clicking opens it.
pub fn card_row(app: &App, ui: &mut egui::Ui, style: Row, card: &Card) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, style.height), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &card.title));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), PALETTE.surface);
    }
    let art = Rect::from_min_size(
        pos2(rect.left() + style.pad, rect.center().y - style.art / 2.0),
        Vec2::splat(style.art),
    );
    if card.round {
        cover(app, ui, art, card.thumbnail.as_ref(), true);
    } else {
        cover_with(app, ui, art, card.thumbnail.as_ref(), CornerRadius::same(4));
    }
    let left = art.right() + 16.0;
    let space = (rect.right() - style.pad - left).max(0.0);
    theme::paint_line(
        ui,
        pos2(left, rect.center().y - 20.0),
        &card.title,
        theme::medium(14.0),
        PALETTE.text,
        space,
    );
    theme::paint_line(
        ui,
        pos2(left, rect.center().y + 3.0),
        &card.subtitle,
        theme::regular(14.0),
        PALETTE.secondary,
        space,
    );
    response.context_menu(|ui| card_menu(app, ui, card));
    if response.clicked()
        && let Some(target) = card.open.clone().or_else(|| card.play.clone())
    {
        app.act(Action::Open(target, card.song()));
    }
}

/// A mood or genre button (a card without a picture).
pub fn chip(app: &App, ui: &mut egui::Ui, card: &Card) {
    if theme::chip(ui, &card.title, false, 36.0).clicked()
        && let Some(target) = card.open.clone()
    {
        app.act(Action::Open(target, None));
    }
}
