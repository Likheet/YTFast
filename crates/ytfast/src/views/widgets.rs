//! The pieces pages are made of, sized as YouTube Music sizes them: a
//! cover picture, a song row, a card.

use egui::{Color32, CornerRadius, Rect, Sense, Vec2, pos2, vec2};
use ytfast_core::read::{Card, Target, Thumb, Track, TrackKind};

use crate::app::{Action, App, PlayState};
use crate::theme::{self, Icon, PALETTE};

/// Draws a cover in `rect`: the picture when it has arrived, a quiet
/// placeholder until then. Round for artists.
pub fn cover(app: &App, ui: &egui::Ui, rect: Rect, thumb: Option<&Thumb>, round: bool) {
    cover_with(app, ui, rect, thumb, cover_radius(rect.width(), round));
}

/// How round a cover's corners are, by its width.
fn cover_radius(width: f32, round: bool) -> CornerRadius {
    if round {
        CornerRadius::same(255)
    } else if width > 200.0 {
        CornerRadius::same(12)
    } else if width > 64.0 {
        CornerRadius::same(6)
    } else {
        CornerRadius::same(4)
    }
}

/// [`cover`], with the corners' rounding chosen.
pub fn cover_with(
    app: &App,
    ui: &egui::Ui,
    rect: Rect,
    thumb: Option<&Thumb>,
    radius: CornerRadius,
) {
    if !picture(app, ui, rect, thumb, radius) {
        ui.painter().rect_filled(rect, radius, PALETTE.surface);
        let icon = rect.width() * 0.35;
        theme::paint_icon(ui, Icon::Music, rect, icon, PALETTE.faint);
    }
}

/// The picture `thumb` in `rect`, once it has arrived (nothing before);
/// whether it was drawn.
pub fn picture(
    app: &App,
    ui: &egui::Ui,
    rect: Rect,
    thumb: Option<&Thumb>,
    radius: CornerRadius,
) -> bool {
    let pixels = (rect.width() * ui.ctx().pixels_per_point()).ceil() as u32;
    // Ask for a size the server makes, not every size drawn.
    let size = [60, 120, 226, 360, 544]
        .into_iter()
        .find(|s| *s >= pixels)
        .unwrap_or(544);
    let Some(texture) = thumb.and_then(|t| app.picture(&t.sized(size))) else {
        return false;
    };
    egui::Image::new((texture.id(), rect.size()))
        .corner_radius(radius)
        .paint_at(ui, rect);
    true
}

/// How a song row is drawn. The sizes are YouTube Music's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub height: f32,
    /// The space under the row, before the next.
    pub gap: f32,
    /// The cover's side (or the number's box).
    pub art: f32,
    /// The cover's corners.
    pub corner: u8,
    /// The space after the cover, before the words.
    pub art_gap: f32,
    /// The space left and right, inside the row.
    pub pad: f32,
    /// A hairline between rows (`rows` draws it, not under the last).
    pub line: bool,
    /// The artist, the plays and the album in columns of their own, beside
    /// the title (an artist's top songs), not under it.
    pub columns: bool,
    /// What the line under the title says.
    pub under: Under,
    /// The song's length, at the right.
    pub length: bool,
    /// The space between the title and the line under it: YouTube
    /// Music's own, list by list.
    pub text_gap: f32,
    /// The fill under the pointer (none on an album's or playlist's
    /// songs).
    pub hover: Color32,
    /// An album's or a playlist's own songs: the playing one filled r 8,
    /// covers r 2.
    pub album: bool,
    /// Like, dislike and ⋮ under the pointer, 36 each, their left edges
    /// this far from the row's right (YouTube Music's places); the words
    /// then end 16 before like.
    pub buttons: Option<[f32; 3]>,
    /// Their room is kept even while they are hidden (an artist's top
    /// songs); else the length shows there.
    pub keep_room: bool,
    /// The colour of the line under the title and of the length.
    pub quiet: Color32,
    /// Under the pointer the length gives its place to ⋮ (Up next).
    pub menu_for_length: bool,
}

/// White at 5%: a list's row under the pointer.
const ROW_HOVER: Color32 = Color32::from_rgba_premultiplied(13, 13, 13, 13);

/// What a song row says under the song's title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Under {
    /// "Artist".
    Artists,
    /// "Artist • Album".
    ArtistsAlbum,
    /// "Song • Artist", as search results say what each row is.
    Kind,
    /// "Song • 3:37", as the songs on search's top result card.
    KindLength,
    /// "Artists • 7.6M plays", as an album's songs.
    ArtistsCount,
}

impl Row {
    /// An album's songs.
    pub const LIST: Self = Self {
        height: 68.0,
        gap: 8.0,
        art: 48.0,
        corner: 2,
        art_gap: 16.0,
        pad: 16.0,
        line: false,
        columns: false,
        under: Under::ArtistsCount,
        length: true,
        text_gap: 8.0,
        hover: Color32::TRANSPARENT,
        album: true,
        // Measured at 1280: like 200.5 from the right, dislike 2.5 after
        // it, ⋮ at 110.
        buttons: Some([200.5, 162.0, 110.0]),
        keep_room: false,
        quiet: PALETTE.secondary,
        menu_for_length: false,
    };
    /// A playlist's songs (and Liked Music's): as an album's, saying
    /// "Artist • Album".
    pub const PLAYLIST: Self = Self {
        under: Under::ArtistsAlbum,
        ..Self::LIST
    };
    /// A shelf of songs on a page (an artist's top songs): 48 high and a
    /// hairline, cover 32 r 4 with 24 after it.
    pub const SHELF: Self = Self {
        height: 48.0,
        gap: 1.0,
        art: 32.0,
        corner: 4,
        art_gap: 24.0,
        pad: 8.0,
        line: true,
        columns: true,
        under: Under::Artists,
        length: true,
        text_gap: 3.0,
        hover: ROW_HOVER,
        album: false,
        // Measured at 1280: like at 134.5 from the right, dislike 2.5
        // after it, then 8, ⋮ at 44.
        buttons: Some([134.5, 96.0, 44.0]),
        keep_room: true,
        quiet: PALETTE.secondary,
        menu_for_length: false,
    };
    /// Search results.
    pub const SEARCH: Self = Self {
        height: 80.0,
        gap: 0.0,
        art: 56.0,
        corner: 4,
        art_gap: 16.0,
        pad: 8.0,
        line: false,
        columns: false,
        under: Under::Kind,
        // Search's rows have no length column.
        length: false,
        text_gap: 4.0,
        hover: ROW_HOVER,
        album: false,
        buttons: None,
        keep_room: false,
        quiet: PALETTE.secondary,
        menu_for_length: false,
    };
    /// Search results narrowed to one kind: a hairline between rows.
    pub const SEARCH_ONLY: Self = Self {
        gap: 1.0,
        line: true,
        ..Self::SEARCH
    };
    /// Songs in columns that scroll sideways (Quick picks, "You might
    /// also like").
    pub const GRID: Self = Self {
        height: 48.0,
        gap: 16.0,
        art: 48.0,
        corner: 4,
        art_gap: 16.0,
        pad: 8.0,
        line: false,
        columns: false,
        under: Under::ArtistsAlbum,
        length: false,
        text_gap: 3.0,
        hover: ROW_HOVER,
        album: false,
        buttons: None,
        keep_room: false,
        quiet: PALETTE.secondary,
        menu_for_length: false,
    };
    /// Up next: 56 high and a hairline (57 apart), the artists and the
    /// length `#aaa`, ⋮ in the length's place under the pointer.
    pub const QUEUE: Self = Self {
        height: 56.0,
        gap: 1.0,
        art: 32.0,
        corner: 2,
        art_gap: 16.0,
        pad: 8.0,
        line: true,
        columns: false,
        under: Under::Artists,
        length: true,
        text_gap: 4.0,
        hover: ROW_HOVER,
        album: false,
        buttons: None,
        keep_room: false,
        quiet: PALETTE.dim,
        menu_for_length: true,
    };
    /// The songs on the right of search's top result card.
    pub const TOP_RESULT: Self = Self {
        height: 56.0,
        gap: 16.0,
        art: 56.0,
        corner: 4,
        art_gap: 16.0,
        pad: 0.0,
        line: false,
        columns: false,
        under: Under::KindLength,
        length: false,
        text_gap: 4.0,
        hover: ROW_HOVER,
        album: false,
        buttons: None,
        keep_room: false,
        quiet: PALETTE.secondary,
        menu_for_length: false,
    };

    /// Where the title's line and the line under it start, from the top:
    /// the two lines (and the gap between them) centred in the row.
    fn text_lines(&self, row: Rect) -> (f32, f32) {
        self.text_lines_sized(row, LINE, LINE)
    }

    /// As `text_lines`, for lines `title` and `under` high.
    fn text_lines_sized(&self, row: Rect, title: f32, under: f32) -> (f32, f32) {
        let top = row.center().y - (title + self.text_gap + under) / 2.0;
        (top, top + title + self.text_gap)
    }
}

/// The height of a line of 14 point text on YouTube Music (its line
/// height is 1.2).
const LINE: f32 = 16.8;

/// Paints one line of text in a line 1.2 times the font's size high (`LINE`
/// for 14) whose top is `top`, centred in it as a browser centres text in
/// its line.
fn line_at(
    ui: &egui::Ui,
    left: f32,
    top: f32,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
) {
    let line = font_line(&font);
    let galley = theme::fit(ui, text, font, color, width, 1);
    let at = pos2(left, top + (line - galley.size().y) / 2.0);
    ui.painter().galley(at, galley, color);
}

/// A line's height for `font`, as YouTube Music's (1.2 times the size).
fn font_line(font: &egui::FontId) -> f32 {
    font.size * 1.2
}

/// How long the pointer rests on a song before it is found ahead of time.
const WARM_AFTER: f32 = 0.35;

/// Draws `count` song rows in `style` from the cursor down, but only those
/// on screen (and one more at each end, so the keyboard can move on to
/// them); the rest is empty space of the same height. A list of thousands
/// of songs then costs no more each frame than a short one.
///
/// Each row must add exactly one widget of its own (`track_row_in` does):
/// the rows not drawn are counted as one each, so every row keeps the same
/// ID (its open menu, its keyboard focus) wherever the list is scrolled.
pub fn rows(
    ui: &mut egui::Ui,
    style: Row,
    count: usize,
    mut row: impl FnMut(&mut egui::Ui, usize),
) {
    let step = style.height + style.gap + ui.spacing().item_spacing.y;
    let top = ui.cursor().top();
    let clip = ui.clip_rect();
    let index = |y: f32| (((y - top) / step).max(0.0) as usize).min(count);
    let first = index(clip.top()).saturating_sub(1);
    let last = (index(clip.bottom()) + 2).min(count);
    let (first, last) = (first.min(last), last);
    ui.add_space(first as f32 * step);
    ui.skip_ahead_auto_ids(first);
    for index in first..last {
        let across = ui.available_rect_before_wrap().x_range();
        row(ui, index);
        // A hairline between rows, in the row's last point (or its gap),
        // none under the last.
        if style.line && index + 1 < count {
            let y = ui.cursor().top() - ui.spacing().item_spacing.y - 0.5;
            ui.painter()
                .hline(across, y, egui::Stroke::new(1.0, PALETTE.outline));
        }
    }
    ui.add_space((count - last) as f32 * step);
    ui.skip_ahead_auto_ids(count - last);
}

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
    track_row_in(
        app,
        ui,
        style,
        track,
        number,
        playing,
        Place::Page,
        on_click,
    );
}

/// Where a song's menu was opened, which decides what it offers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// A row on the open page: on the account's own playlist, the song can
    /// be taken out of it.
    Page,
    /// A song in Up next (its entry), whose menu edits the queue.
    Queue(u64),
    /// The song playing, in the player bar or at the top of Up next.
    Playing,
}

/// A song row in Up next (`place` is its entry), whose menu edits the
/// queue; there the row can be dragged (its response says so; `None` while
/// it is out of sight).
#[allow(clippy::too_many_arguments)]
pub fn track_row_in(
    app: &App,
    ui: &mut egui::Ui,
    style: Row,
    track: &Track,
    number: Option<usize>,
    playing: bool,
    place: Place,
    on_click: impl FnOnce() -> Action,
) -> Option<egui::Response> {
    let queued = matches!(place, Place::Queue(_));
    // The playing song's menu does not edit the queue.
    let place = if playing && queued {
        Place::Playing
    } else {
        place
    };
    let width = ui.available_width();
    let (slot, placed) =
        ui.allocate_exact_size(vec2(width, style.height + style.gap), Sense::hover());
    let rect = Rect::from_min_size(slot.min, vec2(width, style.height));
    // A row just out of sight can still take the keyboard's focus, and is
    // then scrolled into view.
    let near = rect.expand2(vec2(0.0, style.height + style.gap));
    if !ui.is_rect_visible(near) {
        return None;
    }
    let id = placed.id.with("row");
    // Up next's rows move when dragged (the pointer shows it), as YouTube
    // Music's (`cursor: move`).
    let sense = if queued {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let response = ui.interact(rect, id, sense);
    if queued && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
    }
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &track.title));
    if response.gained_focus() {
        response.scroll_to_me(None);
    }
    if !ui.is_rect_visible(rect) {
        return Some(response);
    }
    // Shift+F10 opens the menu of the row the keyboard is on.
    if response.has_focus()
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::F10))
    {
        egui::Popup::open_id(ui.ctx(), id.with("menu-popup"));
    }
    let menu_open = egui::Popup::is_id_open(ui.ctx(), id.with("menu-popup"));
    let hovered = ui.rect_contains_pointer(rect) || menu_open;
    // The song playing is marked white@0.10 (a list's rows r 8); the row
    // under the pointer gets the list's own fill.
    let radius = if style.album { 8 } else { 0 };
    if playing {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(radius), PALETTE.surface);
    } else if hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(radius), style.hover);
    }
    // A song YouTube no longer offers is shown at 60%, without its cover,
    // and does not play.
    let playable = track.playable;
    let shown = if playable { 1.0 } else { 0.6 };
    let (title_color, line_color) = (
        PALETTE.text.gamma_multiply(shown),
        style.quiet.gamma_multiply(shown),
    );
    // From a window 1364 wide an album's or playlist's words are 16 (an
    // album's second line stays 14).
    // Every row's words are 16 from a window 1364 wide
    // (`--ytmusic-responsive-font-size`).
    let big = theme::text_size(ui) > 14.0;
    let size = if big { 16.0 } else { 14.0 };
    let under_size = if big && style.under != Under::ArtistsCount {
        16.0
    } else {
        14.0
    };
    if response.hovered() && playable {
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
    // YouTube Music's small play button: a 24 icon (its 32 spot centred).
    // The playing song: bars while it sounds (still, so a playing list
    // costs nothing), pause under the pointer, play while paused.
    let sounding = app.playback.state == crate::app::PlayState::Playing && !app.audio_status.paused;
    let mark = |ui: &egui::Ui| {
        if !sounding {
            theme::paint_icon(ui, Icon::Play, art, 24.0, PALETTE.text);
        } else if hovered {
            theme::paint_icon(ui, Icon::Pause, art, 24.0, PALETTE.text);
        } else {
            paint_bars(ui, art.center());
        }
    };
    match number {
        Some(_) if playing => mark(ui),
        Some(_) if hovered && playable => {
            theme::paint_icon(ui, Icon::Play, art, 24.0, PALETTE.text);
        }
        Some(number) => {
            ui.painter().text(
                art.center(),
                egui::Align2::CENTER_CENTER,
                number.to_string(),
                theme::medium(size),
                PALETTE.dim.gamma_multiply(shown),
            );
        }
        None if !playable => {}
        None => {
            let radius = CornerRadius::same(style.corner);
            cover_with(app, ui, art, track.thumbnail.as_ref(), radius);
            // Darkened by black at 80% (measured), with the play icon 24.
            if playing || (hovered && playable) {
                ui.painter()
                    .rect_filled(art, radius, Color32::from_black_alpha(204));
                if playing {
                    mark(ui);
                } else {
                    theme::paint_icon(ui, Icon::Play, art, 24.0, PALETTE.text);
                }
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
    let mut clicked_menu = false;
    if let Some(places) = style.buttons {
        // Like, dislike and ⋮ under the pointer. An album's or playlist's
        // row shows its length in their place otherwise; an artist's top
        // songs keep their room, the length (when known) before it.
        if hovered {
            row_actions(app, ui, rect, id, track, place, places, &mut clicked_menu);
        }
        if style.keep_room || hovered {
            right = rect.right() - places[0];
        }
        // An album's or playlist's rows have a tick box at their right end,
        // under the pointer, and on every row while some are ticked (the
        // length then makes way for it), as YouTube Music's.
        let tickable = !style.keep_room && place == Place::Page;
        let picking = tickable && !app.selected.is_empty();
        if tickable && (hovered || picking) {
            tick_box(app, ui, rect, id, track, &mut clicked_menu);
        }
        if !duration.is_empty() && (style.keep_room || !hovered) && !picking {
            let end = if style.keep_room { right - 16.0 } else { right };
            let galley =
                ui.painter()
                    .layout_no_wrap(duration.clone(), theme::regular(size), line_color);
            let at = pos2(
                end - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            );
            right = at.x - 4.0;
            ui.painter().galley(at, galley, line_color);
        }
    } else if style.menu_for_length {
        // Up next: the length at the right edge; under the pointer ⋮
        // (`#909090`) in its place, 16 after the words.
        if hovered {
            let more = Rect::from_min_size(
                pos2(right - 36.0, rect.center().y - 18.0),
                Vec2::splat(36.0),
            );
            let response = ui.interact(more, id.with("menu"), Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "More actions")
            });
            if response.hovered() {
                ui.painter()
                    .circle_filled(more.center(), 18.0, PALETTE.surface);
            }
            theme::paint_icon(ui, Icon::MoreVertical, more, 24.0, PALETTE.quiet);
            clicked_menu = response.clicked();
            theme::menu_popup(&response)
                .id(id.with("menu-popup"))
                .show(|ui| song_menu(app, ui, track, place));
            right = more.left();
        } else if !duration.is_empty() {
            let galley = ui
                .painter()
                .layout_no_wrap(duration, theme::regular(size), line_color);
            let at = pos2(
                right - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            );
            // The words end 8 before it.
            right = at.x + 8.0;
            ui.painter().galley(at, galley, line_color);
        }
    } else if !duration.is_empty() {
        let galley = ui
            .painter()
            .layout_no_wrap(duration, theme::regular(size), PALETTE.secondary);
        let at = pos2(
            right - 32.0 - galley.size().x,
            rect.center().y - galley.size().y / 2.0,
        );
        right = at.x;
        ui.painter().galley(at, galley, PALETTE.secondary);
    } else {
        right -= 32.0;
    }
    if hovered && style.buttons.is_none() && !style.menu_for_length {
        let menu_response = ui.interact(menu, id.with("menu"), Sense::click());
        menu_response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "More actions")
        });
        if menu_response.hovered() {
            ui.painter()
                .circle_filled(menu.center(), 16.0, PALETTE.surface);
        }
        // Grey, as YouTube Music's rows draw it (`--ytmusic-menu-renderer-overflow-button-color`).
        theme::paint_icon(ui, Icon::MoreVertical, menu, 18.0, PALETTE.dim);
        clicked_menu = menu_response.clicked();
        theme::menu_popup(&menu_response)
            .id(id.with("menu-popup"))
            .show(|ui| song_menu(app, ui, track, place));
    }

    // A chart's place: 4 after the cover, a 52 column (the number in its
    // last 24, 14/500 white, 16 from a window 1364 wide), then 16.
    let mut text_left = art.right() + style.art_gap;
    if let Some(rank) = track.rank() {
        let column = Rect::from_min_size(
            pos2(art.right() + 4.0, rect.center().y - 8.4),
            vec2(52.0, 16.8),
        );
        ui.painter().text(
            pos2(column.right() - 12.0, column.center().y),
            egui::Align2::CENTER_CENTER,
            rank,
            theme::medium(size),
            PALETTE.text,
        );
        text_left = column.right() + 16.0;
    }
    let text_right = right - 16.0;
    let space = (text_right - text_left).max(0.0);
    let title_font = theme::medium(size);
    let line = theme::regular(under_size);
    let single = rect.center().y - font_line(&title_font) / 2.0;
    if style.columns && space > 420.0 {
        // YouTube Music's flex columns: the title 6 parts of 15, then the
        // artist, the plays (when said) and the album sharing the other 9,
        // 16 before each.
        let title_width = space * 6.0 / 15.0;
        // An explicit song's "E" at the end of its title's column.
        let badge = if track.explicit() { 32.0 } else { 0.0 };
        line_at(
            ui,
            text_left,
            single,
            &track.title,
            title_font,
            title_color,
            title_width - badge,
        );
        if badge > 0.0 {
            explicit_badge(
                ui,
                pos2(text_left + title_width - 16.0, rect.center().y - 8.0),
            );
        }
        let others: Vec<&str> = [
            Some(track.artists.as_str()),
            track.count(),
            Some(track.album.as_deref().unwrap_or_default()),
        ]
        .into_iter()
        .flatten()
        .collect();
        let each = space * 9.0 / 15.0 / others.len() as f32 - 16.0;
        for (index, text) in others.into_iter().enumerate() {
            let x = text_left + title_width + 16.0 + index as f32 * (each + 16.0);
            line_at(ui, x, single, text, line.clone(), line_color, each);
        }
    } else {
        let under = described(track, style.under);
        if under.is_empty() {
            line_at(
                ui,
                text_left,
                single,
                &track.title,
                title_font,
                title_color,
                space,
            );
        } else {
            let (title_top, under_top) =
                style.text_lines_sized(rect, font_line(&title_font), font_line(&line));
            line_at(
                ui,
                text_left,
                title_top,
                &track.title,
                title_font,
                title_color,
                space,
            );
            // An explicit song's "E" (16, then 4) before the line under.
            let badge = if track.explicit() { 20.0 } else { 0.0 };
            if badge > 0.0 {
                let height = font_line(&line);
                explicit_badge(ui, pos2(text_left, under_top + (height - 16.0) / 2.0));
            }
            line_at(
                ui,
                text_left + badge,
                under_top,
                &under,
                line,
                line_color,
                space - badge,
            );
        }
    }
    if response.clicked() && !clicked_menu {
        app.act(on_click());
    }
    theme::context_menu(&response).show(|ui| song_menu(app, ui, track, place));
    Some(response)
}

/// A row's tick box (YouTube Music's, 24, 18 from the row's right end): an
/// outline `#aaa`; white with a black tick once ticked. Ticked songs bring
/// up the bar of what can be done with them (`selection_bar`).
fn tick_box(app: &App, ui: &egui::Ui, rect: Rect, id: egui::Id, track: &Track, clicked: &mut bool) {
    let ticked = app
        .selected
        .iter()
        .any(|t| t.video_id == track.video_id && t.set_video_id == track.set_video_id);
    let spot = Rect::from_min_size(
        pos2(rect.right() - 42.0, rect.center().y - 12.0),
        Vec2::splat(24.0),
    );
    let response = ui.interact(spot, id.with("tick"), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            ticked,
            format!("Tick {}", track.title),
        )
    });
    let square = Rect::from_center_size(spot.center(), Vec2::splat(18.0));
    if ticked {
        ui.painter()
            .rect_filled(square, CornerRadius::same(2), PALETTE.text);
        theme::paint_icon(ui, Icon::Check, square, 16.0, PALETTE.window);
    } else {
        ui.painter().rect_stroke(
            square,
            CornerRadius::same(2),
            egui::Stroke::new(2.0, PALETTE.dim),
            egui::StrokeKind::Inside,
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        app.act(Action::ToggleSelected(track.clone()));
        *clicked = true;
    }
}

/// YouTube's "E" for explicit: 16 across, white@0.70, `at` its top left.
fn explicit_badge(ui: &egui::Ui, at: egui::Pos2) {
    let spot = Rect::from_min_size(at, Vec2::splat(16.0));
    theme::paint_icon(ui, Icon::Explicit, spot, 16.0, PALETTE.secondary);
}

/// Three still bars, as YouTube Music's equaliser marks the playing song
/// (3 wide, 2 apart).
fn paint_bars(ui: &egui::Ui, centre: egui::Pos2) {
    for (index, height) in [10.0_f32, 16.0, 7.0].into_iter().enumerate() {
        let x = centre.x - 6.5 + index as f32 * 5.0;
        let bar = Rect::from_min_max(
            pos2(x, centre.y + 8.0 - height),
            pos2(x + 3.0, centre.y + 8.0),
        );
        ui.painter().rect_filled(bar, 0.0, PALETTE.text);
    }
}

/// Like, dislike and ⋮ on a row under the pointer, their left edges
/// `places` from the row's right (`Row::buttons`), 36 each, their icons 24
/// `#f1f1f1`. A greyed-out song has only ⋮.
#[allow(clippy::too_many_arguments)]
fn row_actions(
    app: &App,
    ui: &mut egui::Ui,
    rect: Rect,
    id: egui::Id,
    track: &Track,
    place: Place,
    places: [f32; 3],
    clicked_menu: &mut bool,
) {
    use crate::app::LikeState;
    let soft = PALETTE.button;
    let spot = |from_right: f32| {
        Rect::from_min_size(
            pos2(rect.right() - from_right, rect.center().y - 18.0),
            Vec2::splat(36.0),
        )
    };
    let button = |ui: &mut egui::Ui, at: Rect, icon: Icon, name: &str| {
        let response = ui.interact(at, id.with(name), Sense::click());
        if response.hovered() {
            ui.painter()
                .circle_filled(at.center(), 18.0, PALETTE.surface_hover);
        }
        theme::paint_icon(ui, icon, at, 24.0, soft);
        response
    };
    let state = app
        .likes
        .get(&track.video_id)
        .copied()
        .unwrap_or(LikeState::Neutral);
    let liked = state == LikeState::Liked;
    let disliked = state == LikeState::Disliked;
    let up = if liked {
        Icon::ThumbsUpFilled
    } else {
        Icon::ThumbsUp
    };
    let more_at = spot(places[2]);
    if !track.playable {
        let more = button(ui, more_at, Icon::MoreVertical, "menu");
        more.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "More actions")
        });
        *clicked_menu |= more.clicked();
        theme::menu_popup(&more)
            .id(id.with("menu-popup"))
            .show(|ui| song_menu(app, ui, track, place));
        return;
    }
    let like = button(ui, spot(places[0]), up, "like");
    like.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, liked, "Like"));
    if like.clicked() {
        *clicked_menu = true;
        let next = if liked {
            LikeState::Neutral
        } else {
            LikeState::Liked
        };
        app.act(Action::Rate(track.video_id.clone(), next));
    }
    let down = if disliked {
        Icon::ThumbsDownFilled
    } else {
        Icon::ThumbsDown
    };
    let dislike = button(ui, spot(places[1]), down, "dislike");
    dislike.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, disliked, "Dislike")
    });
    if dislike.clicked() {
        *clicked_menu = true;
        let next = if disliked {
            LikeState::Neutral
        } else {
            LikeState::Disliked
        };
        app.act(Action::Rate(track.video_id.clone(), next));
    }
    let more = button(ui, more_at, Icon::MoreVertical, "menu");
    more.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "More actions"));
    if more.clicked() {
        *clicked_menu = true;
    }
    theme::menu_popup(&more)
        .id(id.with("menu-popup"))
        .show(|ui| song_menu(app, ui, track, place));
}

/// The line under a song's title: "Artist", "Artist • Album",
/// "Song • Artist" or "Song • 3:37".
fn described(track: &Track, under: Under) -> String {
    let kind = match (under, &track.kind) {
        (Under::Kind | Under::KindLength, TrackKind::Song) => "Song",
        (Under::Kind | Under::KindLength, TrackKind::MusicVideo) => "Video",
        _ => "",
    };
    let parts = match under {
        Under::Artists => [String::new(), track.artists.clone(), String::new()],
        // The album, else (a chart's songs) the views.
        Under::ArtistsAlbum => [
            String::new(),
            track.artists.clone(),
            track
                .album
                .clone()
                .or(track.count().map(str::to_string))
                .unwrap_or_default(),
        ],
        Under::Kind => [
            kind.to_string(),
            track.artists.clone(),
            track.count().unwrap_or_default().to_string(),
        ],
        Under::KindLength => [
            kind.to_string(),
            track
                .duration_seconds
                .map(|s| theme::clock(f64::from(s)))
                .unwrap_or_default(),
            track.count().unwrap_or_default().to_string(),
        ],
        Under::ArtistsCount => [
            String::new(),
            track.artists.clone(),
            track.count().unwrap_or_default().to_string(),
        ],
    };
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" \u{2022} ")
}

/// What a right-click on a song (or its ⋮ button) offers, in YouTube
/// Music's order and words (measured): Start mix, Play next, Add to queue,
/// Add to liked songs, Save to playlist, Remove from queue, Go to album,
/// Go to artist, Share. (Download, Report and Stats for nerds are left
/// out.)
pub fn song_menu(app: &App, ui: &mut egui::Ui, track: &Track, place: Place) {
    theme::menu(ui);
    let item = |ui: &mut egui::Ui, icon: Icon, text: &str, action: Action| {
        if theme::menu_item(ui, icon, text).clicked() {
            app.act(action);
            ui.close();
        }
    };
    // A song YouTube no longer offers can only be taken out of a playlist
    // (and unliked, or added elsewhere).
    if track.playable {
        let mix = Target::Watch {
            video_id: Some(track.video_id.clone()),
            playlist_id: None,
        };
        item(
            ui,
            Icon::Mix,
            "Start mix",
            Action::Play(mix, Some(track.clone())),
        );
    }
    match place {
        Place::Queue(id) => {
            // (Moved elsewhere by dragging its row, as on YouTube Music.)
            item(ui, Icon::PlayNext, "Play next", Action::MoveNextInQueue(id));
        }
        Place::Page | Place::Playing if track.playable => {
            item(
                ui,
                Icon::PlayNext,
                "Play next",
                Action::PlayNext(track.clone()),
            );
            item(
                ui,
                Icon::AddToQueue,
                "Add to queue",
                Action::AddToQueue(track.clone()),
            );
        }
        Place::Page | Place::Playing => {}
    }
    // As chosen in this run or as YouTube said; else every song on Liked
    // Music's own page is liked.
    let on_liked = place == Place::Page && app.route == crate::backend::Route::Liked;
    let liked = app
        .likes
        .get(&track.video_id)
        .map_or(on_liked, |like| *like == crate::app::LikeState::Liked);
    if liked {
        item(
            ui,
            Icon::ThumbsUpFilled,
            "Remove from liked songs",
            Action::Rate(track.video_id.clone(), crate::app::LikeState::Neutral),
        );
    } else {
        item(
            ui,
            Icon::ThumbsUp,
            "Add to liked songs",
            Action::Rate(track.video_id.clone(), crate::app::LikeState::Liked),
        );
    }
    item(
        ui,
        Icon::SaveToPlaylist,
        "Save to playlist",
        Action::OpenDialog(crate::app::Dialog::SaveToPlaylist {
            video_ids: vec![track.video_id.clone()],
        }),
    );
    if let Place::Queue(id) = place {
        item(
            ui,
            Icon::RemoveFromQueue,
            "Remove from queue",
            Action::RemoveFromQueue(id),
        );
    }
    // Only from the playlist's own page: the playing song may have come
    // from anywhere.
    if place == Place::Page
        && let Some(set_video_id) = &track.set_video_id
        && let Some(playlist_id) = app.editable_playlist()
    {
        item(
            ui,
            Icon::RemoveFromPlaylist,
            "Remove from playlist",
            Action::RemoveFromPlaylist {
                playlist_id,
                songs: vec![(track.video_id.clone(), set_video_id.clone())],
            },
        );
    }
    if let Some(album) = &track.album_id {
        item(
            ui,
            Icon::Album,
            "Go to album",
            Action::Navigate(crate::backend::Route::browse(album.clone(), None)),
        );
    }
    if let Some(artist) = &track.artist_id {
        item(
            ui,
            Icon::Artist,
            "Go to artist",
            Action::Navigate(crate::backend::Route::browse(artist.clone(), None)),
        );
    }
    share(
        app,
        ui,
        format!("https://music.youtube.com/watch?v={}", track.video_id),
    );
}

/// "Share": copies the link, and says so.
pub fn share(app: &App, ui: &mut egui::Ui, link: String) {
    if theme::menu_item(ui, Icon::Share, "Share").clicked() {
        ui.ctx().copy_text(link);
        app.act(Action::Notify("Link copied to clipboard".into()));
        ui.close();
    }
}

/// A card, as YouTube Music's (`ytmusic-two-row-item-renderer`): a cover
/// `size` across (corners 8, or round for an artist), 16 under it the
/// title (14/500, up to 2 lines), 3 under that the subtitle (14/400
/// white@0.70, up to 2 lines); as tall as its words. Under the pointer the
/// cover darkens at its top (black 50% to clear at a third of the way
/// down), a round play button (40, black 60%) shows 32 in from its right
/// and bottom, and a ⋮ button (36) 4 from its right and 8 from its top;
/// they fade in over 0.2 s. Clicking opens it; the play button plays it.
/// Whether the queue plays from this card's album or playlist.
fn plays_from(app: &App, card: &Card) -> bool {
    let Some(Target::Watch {
        playlist_id: Some(list),
        ..
    }) = &card.play
    else {
        return false;
    };
    let bare = |id: &str| id.strip_prefix("VL").unwrap_or(id).to_string();
    app.playback.entry.is_some() && app.queue.source.as_deref().map(bare) == Some(bare(list))
}

/// A card: its picture `size` across, its words 14 (16 from a window 1364
/// wide, [`theme::responsive`]).
pub fn card(app: &App, ui: &mut egui::Ui, card: &Card, size: f32) {
    card_with_text(app, ui, card, size, theme::text_size(ui));
}

/// [`card`] with words `text` big (the Library's grid keeps 14).
pub fn card_with_text(app: &App, ui: &mut egui::Ui, card: &Card, size: f32, text: f32) {
    let center = card.round;
    // A video's card is as tall and 16:9 wide.
    let height = size;
    let size = if card.look.wide {
        size * 16.0 / 9.0
    } else {
        size
    };
    let title = theme::fit(ui, &card.title, theme::medium(text), PALETTE.text, size, 2);
    // An explicit card's "E" (16, then 4) begins its subtitle's first line.
    let badge = if card.look.explicit { 20.0 } else { 0.0 };
    let subtitle = (!card.subtitle.is_empty()).then(|| {
        theme::fit_indented(
            ui,
            &card.subtitle,
            theme::regular(text),
            PALETTE.secondary,
            size,
            2,
            badge,
        )
    });
    let words = 16.0 + title.size().y + subtitle.as_ref().map_or(0.0, |s| 3.0 + s.size().y);
    let (rect, response) = ui.allocate_exact_size(vec2(size, height + words), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &card.title));
    let art = Rect::from_min_size(rect.min, vec2(size, height));
    let radius = if card.round {
        CornerRadius::same(255)
    } else {
        CornerRadius::same(8)
    };
    cover_with(app, ui, art, card.thumbnail.as_ref(), radius);
    // Hovered by where the pointer is, not by which widget is on top: the
    // buttons over the cover must not make the card think the pointer
    // left (that made it flicker).
    let hovered = ui.rect_contains_pointer(rect);
    let lit = ui
        .ctx()
        .animate_bool_with_time(response.id.with("hover"), hovered, 0.2);
    // The album or playlist playing keeps its gradient and button without
    // the pointer (YouTube Music's play button in a state other than
    // "default").
    let current = plays_from(app, card);
    let shade = if current { 1.0 } else { lit };
    let mut pressed = false;
    if shade > 0.0 && !card.round {
        // The gradient over the cover's top third.
        let mut mesh = egui::Mesh::default();
        let top = Rect::from_min_size(art.min, vec2(art.width(), art.height() / 3.0));
        let dark = Color32::from_black_alpha((128.0 * shade) as u8);
        mesh.colored_vertex(top.left_top(), dark);
        mesh.colored_vertex(top.right_top(), dark);
        mesh.colored_vertex(top.right_bottom(), Color32::TRANSPARENT);
        mesh.colored_vertex(top.left_bottom(), Color32::TRANSPARENT);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        ui.painter().add(egui::Shape::mesh(mesh));
    }
    if lit > 0.0 && !card.round {
        // ⋮, which opens the card's menu.
        let dots = Rect::from_min_size(
            pos2(art.right() - 4.0 - 36.0, art.top() + 8.0),
            Vec2::splat(36.0),
        );
        let more = ui.interact(dots, response.id.with("more"), Sense::click());
        more.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "More actions")
        });
        if more.hovered() {
            ui.painter().circle_filled(
                dots.center(),
                18.0,
                Color32::from_white_alpha((26.0 * lit) as u8),
            );
        }
        theme::paint_icon(
            ui,
            Icon::MoreVertical,
            dots,
            24.0,
            Color32::from_white_alpha((255.0 * lit) as u8),
        );
        pressed |= more.clicked();
        theme::menu_popup(&more).show(|ui| card_menu(app, ui, card));
    }
    if lit > 0.0 && card.play.is_some() && !card.round && card.look.wide {
        // A video's: a play icon 48 in the middle, without a disc.
        let button = Rect::from_center_size(art.center(), Vec2::splat(48.0));
        let play = ui.interact(button, response.id.with("play"), Sense::click());
        play.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Play {}", card.title),
            )
        });
        theme::paint_icon(
            ui,
            Icon::Play,
            button,
            48.0,
            Color32::from_white_alpha((255.0 * lit) as u8),
        );
        if play.clicked()
            && let Some(target) = card.play.clone()
        {
            pressed = true;
            app.act(Action::Play(target, card.song()));
        }
    } else if shade > 0.0 && card.play.is_some() && !card.round {
        let button =
            Rect::from_center_size(art.right_bottom() - vec2(32.0, 32.0), Vec2::splat(40.0));
        let play = ui.interact(button, response.id.with("play"), Sense::click());
        // Playing: a speaker, and Pause under the pointer; paused: Play.
        let sounding = current
            && matches!(
                app.playback.state,
                PlayState::Playing | PlayState::Preparing
            )
            && !app.audio_status.paused;
        let pointed = ui.rect_contains_pointer(button);
        let (icon, name) = match (sounding, pointed) {
            (true, true) => (Icon::Pause, "Pause"),
            (true, false) => (Icon::Volume, "Pause"),
            (false, _) => (Icon::Play, "Play"),
        };
        play.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("{name} {}", card.title),
            )
        });
        // Under the pointer it turns solid and grows 1.2 times.
        let on = ui
            .ctx()
            .animate_bool_with_time(play.id.with("on"), pointed, 0.2);
        let grown = 1.0 + 0.2 * on;
        let alpha = (153.0 + 102.0 * on) * shade;
        ui.painter().circle_filled(
            button.center(),
            20.0 * grown,
            Color32::from_black_alpha(alpha as u8),
        );
        theme::paint_icon(
            ui,
            icon,
            button,
            24.0 * grown,
            Color32::from_white_alpha((255.0 * shade) as u8),
        );
        if play.clicked() {
            pressed = true;
            if current {
                app.act(Action::TogglePause);
            } else if let Some(target) = card.play.clone() {
                app.act(Action::Play(target, card.song()));
            }
        }
    }

    let place = |galley: &egui::Galley, top: f32| {
        let x = if center {
            rect.center().x - galley.size().x / 2.0
        } else {
            rect.left()
        };
        pos2(x, top)
    };
    let mut top = art.bottom() + 16.0;
    let title_at = place(&title, top);
    // (Wide cards' words are as wide as their picture.)
    let title_rect = Rect::from_min_size(title_at, title.size());
    top += title.size().y + 3.0;
    ui.painter().galley(title_at, title, PALETTE.text);
    // The title is a link: underlined under the pointer.
    if ui.rect_contains_pointer(title_rect) {
        ui.painter().hline(
            title_rect.x_range(),
            title_rect.bottom() - 1.0,
            egui::Stroke::new(1.0, PALETTE.text),
        );
    }
    if let Some(subtitle) = subtitle {
        let at = place(&subtitle, top);
        if badge > 0.0 {
            // Centred on the first line (1.2 times the words' size).
            explicit_badge(ui, pos2(at.x, at.y + (text * 1.2 - 16.0) / 2.0));
        }
        ui.painter().galley(at, subtitle, PALETTE.secondary);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    theme::context_menu(&response).show(|ui| card_menu(app, ui, card));
    if !pressed
        && response.clicked()
        && let Some(target) = card.open.clone().or_else(|| card.play.clone())
    {
        app.act(Action::Open(target, card.song()));
    }
}

/// What a right-click on an album or playlist offers, in YouTube Music's
/// order and words (measured): Shuffle play, Start mix, Play next, Add to
/// queue, Save album (or playlist) to library, Go to artist, Share. (Save
/// to playlist, for a whole album, and Not interested are left out.)
pub fn card_menu(app: &App, ui: &mut egui::Ui, card: &Card) {
    use crate::app::QueueMode;
    theme::menu(ui);
    let item = |ui: &mut egui::Ui, icon: Icon, text: &str, action: Action| {
        if theme::menu_item(ui, icon, text).clicked() {
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
    let album = matches!(
        &card.open,
        Some(Target::Browse {
            kind: ytfast_core::read::PageKind::Album,
            ..
        })
    );
    if let Some(id) = &playlist {
        item(
            ui,
            Icon::Shuffle,
            "Shuffle play",
            Action::QueuePlaylist(id.clone(), QueueMode::Shuffle),
        );
        let mix = Target::Watch {
            video_id: None,
            playlist_id: Some(format!("RDAMPL{id}")),
        };
        item(ui, Icon::Mix, "Start mix", Action::Play(mix, None));
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
        // Mixes, Liked Music and the account's own playlists are not
        // saved to the library.
        let own = app.own_playlists().iter().any(|(own, _)| own == id);
        if !own && !id.starts_with("RD") && id != "LM" && id != "SE" {
            // As chosen in this run; else what the Library shows is saved.
            let in_library = matches!(
                app.route,
                crate::backend::Route::Library | crate::backend::Route::LibraryAlbums
            );
            let saved = app.saved.get(id).copied().unwrap_or(in_library);
            let what = if album { "album" } else { "playlist" };
            let (icon, label) = if saved {
                (Icon::SavedToLibrary, format!("Remove {what} from library"))
            } else {
                (Icon::Library, format!("Save {what} to library"))
            };
            item(
                ui,
                icon,
                &label,
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
    if let Some(link) = link {
        share(app, ui, link);
    }
}

/// A card as a row (an album or artist in search results): a cover, the
/// title and the subtitle. Clicking opens it.
pub fn card_row(app: &App, ui: &mut egui::Ui, style: Row, card: &Card) {
    let width = ui.available_width();
    let (slot, response) =
        ui.allocate_exact_size(vec2(width, style.height + style.gap), Sense::click());
    let rect = Rect::from_min_size(slot.min, vec2(width, style.height));
    if !ui.is_rect_visible(rect) {
        return;
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &card.title));
    if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, style.hover);
    }
    let art = Rect::from_min_size(
        pos2(rect.left() + style.pad, rect.center().y - style.art / 2.0),
        Vec2::splat(style.art),
    );
    if card.round {
        cover(app, ui, art, card.thumbnail.as_ref(), true);
    } else {
        cover_with(
            app,
            ui,
            art,
            card.thumbnail.as_ref(),
            CornerRadius::same(style.corner),
        );
    }
    let left = art.right() + style.art_gap;
    let space = (rect.right() - style.pad - left).max(0.0);
    let (title_top, under_top) = style.text_lines(rect);
    line_at(
        ui,
        left,
        title_top,
        &card.title,
        theme::medium(14.0),
        PALETTE.text,
        space,
    );
    line_at(
        ui,
        left,
        under_top,
        &card.subtitle,
        theme::regular(14.0),
        PALETTE.secondary,
        space,
    );
    theme::context_menu(&response).show(|ui| card_menu(app, ui, card));
    if response.clicked()
        && let Some(target) = card.open.clone().or_else(|| card.play.clone())
    {
        app.act(Action::Open(target, card.song()));
    }
}

/// A mood or genre button (a card without a picture): a chip, white
/// when it is the one chosen (clicking it again goes back to Home).
pub fn chip(app: &App, ui: &mut egui::Ui, card: &Card) {
    chip_sized(app, ui, card, 36.0);
}

/// [`chip`], `height` high (32: the chips over Liked Music's songs).
pub fn chip_sized(app: &App, ui: &mut egui::Ui, card: &Card, height: f32) {
    if theme::chip(ui, &card.title, card.look.chosen, height).clicked() {
        if card.look.chosen {
            app.act(Action::Navigate(crate::backend::Route::Home));
        } else if let Some(target) = card.open.clone() {
            app.act(Action::Open(target, None));
        }
    }
}

/// One of Explore's big buttons (New releases, Charts, Moods & genres),
/// as YouTube Music's (`ytmusic-navigation-button-renderer`,
/// `STYLE_ICON`): `width` × 56, white@0.15 (`#212121` under the pointer),
/// r 8, padding 8 16; its icon 24 `#aaaaaa`, `gap` after it, its words
/// bold `size`.
pub fn big_button(app: &App, ui: &mut egui::Ui, card: &Card, width: f32, gap: f32, size: f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 56.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &card.title));
    if ui.is_rect_visible(rect) {
        let hover =
            ui.ctx()
                .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.2);
        let fill = lerp_color(PALETTE.field, PALETTE.panel, hover);
        ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
        let icon = match card.look.icon.as_deref() {
            Some("MUSIC_NEW_RELEASE") => Some(Icon::NewReleases),
            Some("TRENDING_UP") => Some(Icon::Charts),
            Some("STICKER_EMOTICON") => Some(Icon::Moods),
            _ => None,
        };
        let mut x = rect.left() + 16.0;
        if let Some(icon) = icon {
            let spot = Rect::from_min_size(pos2(x, rect.center().y - 12.0), Vec2::splat(24.0));
            theme::paint_icon(ui, icon, spot, 24.0, PALETTE.dim);
            x = spot.right() + gap;
        }
        let words = theme::fit(
            ui,
            &card.title,
            theme::bold(size),
            PALETTE.text,
            (rect.right() - 16.0 - x).max(0.0),
            1,
        );
        let at = pos2(x, rect.center().y - words.size().y / 2.0);
        ui.painter().galley(at, words, PALETTE.text);
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    if response.clicked()
        && let Some(target) = card.open.clone()
    {
        app.act(Action::Open(target, None));
    }
}

/// A mood's button in Explore's "Moods & genres", as YouTube Music's
/// (`STYLE_SOLID`): `width` × 48, r 8, white@0.15 (`#212121` under the
/// pointer), a 6 wide stripe of the mood's colour at its left, its words
/// 14/500 white 12 after the stripe (up to 2 lines).
pub fn mood_button(app: &App, ui: &mut egui::Ui, card: &Card, width: f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 48.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &card.title));
    if ui.is_rect_visible(rect) {
        let stripe = card.look.stripe.map_or(PALETTE.text, |rgb| {
            Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
        });
        // The whole button in the stripe's colour, then the rest over it.
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), stripe);
        let rest = Rect::from_min_max(pos2(rect.left() + 6.0, rect.top()), rect.max);
        let corners = CornerRadius {
            nw: 0,
            sw: 0,
            ne: 8,
            se: 8,
        };
        ui.painter().rect_filled(rest, corners, PALETTE.window);
        let fill = if response.hovered() {
            PALETTE.panel
        } else {
            PALETTE.field
        };
        ui.painter().rect_filled(rest, corners, fill);
        let words = theme::fit(
            ui,
            &card.title,
            theme::medium(14.0),
            PALETTE.text,
            (rest.width() - 24.0).max(0.0),
            2,
        );
        let at = pos2(rest.left() + 12.0, rect.center().y - words.size().y / 2.0);
        ui.painter().galley(at, words, PALETTE.text);
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    if response.clicked()
        && let Some(target) = card.open.clone()
    {
        app.act(Action::Open(target, None));
    }
}

/// `a` turning into `b` (`t` from 0 to 1).
fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    Color32::from_rgba_premultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}
