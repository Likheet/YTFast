//! The player page: the playing song large, over its own colours, with
//! Up next, Lyrics and Related beside it. Lyrics follow the song in the
//! Even Better Lyrics Plus way: big bold lines, the one being sung lit,
//! the rest dimmed, scrolling smoothly; click a line to jump there.

use egui::{Align, Color32, Layout, Rect, Sense, UiBuilder, Vec2, pos2, vec2};

use crate::app::{Action, App, NpTab};
use crate::lyrics::{Lyrics, State};
use crate::queue::Entry;
use crate::theme::{self, Icon, PALETTE};
use crate::views::{page, queue_panel, widgets};

pub fn show(app: &App, ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    // Keep lyrics and the backdrop moving while music plays.
    if app.audio_status.entry.is_some() && !app.audio_status.paused {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }
    let close = Rect::from_min_size(rect.min + vec2(4.0, 8.0), Vec2::splat(40.0));
    let mut top = ui.new_child(UiBuilder::new().max_rect(close));
    if theme::icon_button(&mut top, Icon::Collapse, 24.0, PALETTE.text, "Close (Esc)").clicked() {
        app.act(Action::CloseNowPlaying);
    }

    let Some(entry) = &app.playback.entry else {
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::centered_and_justified(egui::Direction::TopDown)),
        );
        theme::label(
            &mut ui,
            "Play something to see it here.",
            theme::regular(16.0),
            PALETTE.secondary,
        );
        return;
    };

    let wide = rect.width() > 820.0;
    let left_width = if wide {
        (rect.width() * 0.42).clamp(300.0, 560.0)
    } else {
        rect.width()
    };
    let left = Rect::from_min_max(
        rect.min + vec2(0.0, 56.0),
        pos2(rect.left() + left_width, rect.bottom() - 12.0),
    );
    song(app, ui, left, entry, wide);
    if wide {
        let right = Rect::from_min_max(
            pos2(left.right() + 32.0, rect.top() + 12.0),
            rect.max - vec2(8.0, 12.0),
        );
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(right)
                .layout(Layout::top_down(Align::Min)),
        );
        tabs(app, &mut ui);
        ui.add_space(12.0);
        match app.np_tab {
            NpTab::UpNext => queue_panel::list(app, &mut ui),
            NpTab::Lyrics => lyrics(app, &mut ui, entry),
            NpTab::Related => related(app, &mut ui, entry),
        }
    }
}

/// The cover, title, artist and album, and the like buttons.
fn song(app: &App, ui: &mut egui::Ui, band: Rect, entry: &Entry, wide: bool) {
    let side = (band.width() - 48.0)
        .min(band.height() - if wide { 190.0 } else { 260.0 })
        .clamp(160.0, 520.0);
    let art = Rect::from_center_size(
        pos2(band.center().x, band.top() + side / 2.0 + 8.0),
        Vec2::splat(side),
    );
    // A soft shadow under the cover.
    for (grow, alpha) in [(18.0, 18u8), (10.0, 30), (4.0, 46)] {
        ui.painter().rect_filled(
            art.expand(grow).translate(vec2(0.0, grow * 0.6)),
            egui::CornerRadius::same((16.0 + grow) as u8),
            Color32::from_black_alpha(alpha),
        );
    }
    widgets::cover(app, ui, art, entry.track.thumbnail.as_ref(), false);

    let text = Rect::from_min_max(
        pos2(band.left() + 16.0, art.bottom() + 20.0),
        pos2(band.right() - 16.0, band.bottom()),
    );
    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(text)
            .layout(Layout::top_down(Align::Center)),
    );
    ui.spacing_mut().item_spacing.y = 4.0;
    ui.add(
        egui::Label::new(
            egui::RichText::new(&entry.track.title)
                .font(theme::bold(26.0))
                .color(PALETTE.text),
        )
        .wrap()
        .selectable(false),
    );
    theme::label(
        &mut ui,
        &entry.track.artists,
        theme::medium(17.0),
        PALETTE.secondary,
    );
    if let Some(album) = &entry.track.album {
        theme::label(&mut ui, album, theme::regular(15.0), PALETTE.dim);
    }
    ui.add_space(10.0);
    ui.allocate_ui_with_layout(
        vec2(84.0, 36.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            widgets::like_buttons(app, ui, &entry.track);
        },
    );
}

fn tabs(app: &App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (tab, name) in [
            (NpTab::UpNext, "Up next"),
            (NpTab::Lyrics, "Lyrics"),
            (NpTab::Related, "Related"),
        ] {
            let chosen = app.np_tab == tab;
            let text = egui::RichText::new(name)
                .font(theme::medium(14.5))
                .color(if chosen { Color32::BLACK } else { PALETTE.text });
            let button = egui::Button::new(text)
                .fill(if chosen {
                    PALETTE.text
                } else {
                    PALETTE.surface
                })
                .corner_radius(egui::CornerRadius::same(18))
                .min_size(vec2(84.0, 34.0));
            if ui.add(button).clicked() {
                app.act(Action::NowPlayingTab(tab));
            }
        }
    });
}

fn lyrics(app: &App, ui: &mut egui::Ui, entry: &Entry) {
    match app.lyrics.get(&entry.track.video_id) {
        None | Some(State::Loading) => {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(26.0).color(PALETTE.secondary));
            });
        }
        Some(State::Missing) => {
            ui.add_space(60.0);
            theme::label(
                ui,
                "No lyrics for this song.",
                theme::medium(18.0),
                PALETTE.secondary,
            );
        }
        Some(State::Ready(lyrics)) => lines(app, ui, lyrics, &entry.track.video_id),
    }
}

/// How bright a line is: the one being sung, those already sung, and
/// those still to come.
const LIT: f32 = 1.0;
const SUNG: f32 = 0.34;
const COMING: f32 = 0.5;

fn lines(app: &App, ui: &mut egui::Ui, lyrics: &Lyrics, video_id: &str) {
    let id = ui.id().with(("lyrics", video_id));
    let now = ui.input(|i| i.time);
    let current = lyrics.current(app.audio_status.position);
    let viewport = ui.available_height();
    // Line positions from the last frame, to scroll the sung line into
    // view (a third of the way down).
    let tops: Vec<f32> = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let manual_until: f64 = ui.data(|d| d.get_temp(id.with("manual"))).unwrap_or(0.0);
    let follow = lyrics.synced && now > manual_until;
    let target = current
        .and_then(|i| tops.get(i))
        .map_or(0.0, |top| (top - viewport * 0.3).max(0.0));
    let offset = ui
        .ctx()
        .animate_value_with_time(id.with("scroll"), target, 0.55);

    let mut area = egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if follow {
        area = area.vertical_scroll_offset(offset);
    }
    let output = area.show(ui, |ui| {
        let origin = ui.min_rect().top();
        ui.add_space(24.0);
        let mut new_tops = Vec::with_capacity(lyrics.lines.len());
        let width = ui.available_width() - 24.0;
        for (i, line) in lyrics.lines.iter().enumerate() {
            let goal = match current {
                Some(c) if c == i => LIT,
                Some(c) if i < c => SUNG,
                _ if lyrics.synced => COMING,
                _ => 0.82,
            };
            let bright = ui
                .ctx()
                .animate_value_with_time(id.with(("line", i)), goal, 0.35);
            let text = if line.text.trim().is_empty() {
                "♪"
            } else {
                line.text.as_str()
            };
            let size = if lyrics.synced { 30.0 } else { 24.0 };
            let color = Color32::from_white_alpha((bright * 255.0) as u8);
            ui.set_max_width(width);
            let response = ui.add(
                egui::Label::new(
                    egui::RichText::new(text)
                        .font(theme::bold(size))
                        .color(color),
                )
                .wrap()
                .selectable(false)
                .sense(if line.start.is_some() {
                    Sense::click()
                } else {
                    Sense::hover()
                }),
            );
            new_tops.push(response.rect.top() - origin);
            if let Some(start) = line.start {
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    app.act(Action::Seek(start));
                    ui.data_mut(|d| d.insert_temp(id.with("manual"), 0.0f64));
                }
            }
            ui.add_space(if lyrics.synced { 18.0 } else { 10.0 });
        }
        ui.add_space(24.0);
        theme::label(
            ui,
            &format!("Lyrics from {}", lyrics.source),
            theme::regular(13.0),
            PALETTE.dim,
        );
        ui.add_space(viewport * 0.6);
        ui.data_mut(|d| d.insert_temp(id, new_tops));
    });
    // Scrolling by hand pauses following for a few seconds.
    let scrolled = ui.input(|i| i.smooth_scroll_delta.y != 0.0);
    if scrolled && ui.rect_contains_pointer(output.inner_rect) {
        ui.data_mut(|d| d.insert_temp(id.with("manual"), now + 4.0));
    }
}

fn related(app: &App, ui: &mut egui::Ui, entry: &Entry) {
    match app.related.get(&entry.track.video_id) {
        Some(crate::app::Loadable::Ready(found)) => {
            egui::ScrollArea::vertical()
                .id_salt(("related", &entry.track.video_id))
                .auto_shrink([false, false])
                .show(ui, |ui| page::sections(app, ui, &app.route, found));
        }
        Some(crate::app::Loadable::Failed(message)) => {
            theme::label(ui, message, theme::regular(15.0), PALETTE.secondary);
        }
        _ => {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(26.0).color(PALETTE.secondary));
            });
        }
    }
}
