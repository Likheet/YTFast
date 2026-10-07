//! The player page, laid out as YouTube Music's: the playing song's cover
//! large on the left, and on the right a panel with the tabs Up next,
//! Lyrics and Related. The page lies over the cover's own colours, and
//! lyrics follow the song in the Even Better Lyrics Plus way: the line
//! being sung lit, the rest dimmed, scrolling smoothly; click a line to
//! jump there.

use egui::{
    Align, Align2, Color32, CornerRadius, Layout, Rect, Sense, UiBuilder, Vec2, pos2, vec2,
};

use crate::app::{Action, App, NpTab};
use crate::lyrics::{Lyrics, State};
use crate::queue::Entry;
use crate::theme::{self, PALETTE};
use crate::views::{backdrop, page, queue_panel, widgets};

pub fn show(app: &App, ui: &mut egui::Ui) {
    let area = ui.max_rect();
    backdrop::cover(app, ui, area);
    // Keep lyrics and the backdrop moving while music plays.
    if app.audio_status.entry.is_some() && !app.audio_status.paused {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }
    let Some(entry) = &app.playback.entry else {
        ui.painter().text(
            area.center(),
            Align2::CENTER_CENTER,
            "Play something to see it here.",
            theme::regular(16.0),
            PALETTE.secondary,
        );
        return;
    };

    // YouTube Music's own spacing: 56 at the sides, 32 above, and 56
    // between the cover and the panel.
    let wide = area.width() >= 840.0;
    let side = if wide { 56.0 } else { 24.0 };
    let inner = Rect::from_min_max(
        pos2(area.left() + side, area.top() + 32.0),
        pos2(area.right() - side, area.bottom()),
    );
    let panel_width = if wide {
        (inner.width() * 0.36).clamp(320.0, 480.0)
    } else {
        inner.width()
    };
    let panel = Rect::from_min_max(pos2(inner.right() - panel_width, inner.top()), inner.max);
    if wide {
        let main = Rect::from_min_max(inner.min, pos2(panel.left() - 56.0, inner.bottom() - 32.0));
        let side = main.width().min(main.height()).max(120.0);
        let art = Rect::from_center_size(main.center(), Vec2::splat(side));
        widgets::cover_with(
            app,
            ui,
            art,
            entry.track.thumbnail.as_ref(),
            CornerRadius::same(8),
        );
    }

    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(panel)
            .layout(Layout::top_down(Align::Min)),
    );
    ui.spacing_mut().item_spacing.y = 0.0;
    tabs(app, &mut ui);
    match app.np_tab {
        NpTab::UpNext => queue_panel::list(app, &mut ui),
        NpTab::Lyrics => lyrics(app, &mut ui, entry),
        NpTab::Related => related(app, &mut ui, entry),
    }
}

/// UP NEXT, LYRICS, RELATED: words in capitals over a hairline, the
/// chosen one white with a line under it.
fn tabs(app: &App, ui: &mut egui::Ui) {
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
    ui.painter().hline(
        bar.x_range(),
        bar.bottom() - 0.5,
        egui::Stroke::new(1.0, PALETTE.outline),
    );
    let mut x = bar.left();
    for (tab, name) in [
        (NpTab::UpNext, "UP NEXT"),
        (NpTab::Lyrics, "LYRICS"),
        (NpTab::Related, "RELATED"),
    ] {
        let chosen = app.np_tab == tab;
        let color = if chosen {
            PALETTE.text
        } else {
            PALETTE.secondary
        };
        let words = ui
            .painter()
            .layout_no_wrap(name.to_string(), theme::medium(14.0), color);
        let rect = Rect::from_min_size(pos2(x, bar.top()), vec2(words.size().x + 24.0, 48.0));
        x = rect.right();
        let response = ui.interact(rect, ui.id().with(("tab", name)), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, chosen, name)
        });
        let color = if response.hovered() {
            PALETTE.text
        } else {
            color
        };
        let at = rect.center() - words.size() / 2.0;
        ui.painter()
            .galley_with_override_text_color(at, words, color);
        if chosen {
            ui.painter().hline(
                rect.x_range(),
                bar.bottom() - 1.0,
                egui::Stroke::new(2.0, PALETTE.text),
            );
        }
        if response.clicked() {
            app.act(Action::NowPlayingTab(tab));
        }
    }
}

fn lyrics(app: &App, ui: &mut egui::Ui, entry: &Entry) {
    match app.lyrics.get(&entry.track.video_id) {
        None | Some(State::Loading) => {
            ui.add_space(64.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(28.0).color(PALETTE.secondary));
            });
        }
        Some(State::Missing) => {
            ui.add_space(64.0);
            ui.vertical_centered(|ui| {
                theme::label(
                    ui,
                    "Lyrics not available",
                    theme::regular(16.0),
                    PALETTE.secondary,
                );
            });
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
        let width = ui.available_width() - 16.0;
        // Lines that follow the song are large and bold; plain ones are
        // set as YouTube Music sets them.
        let (font, gap) = if lyrics.synced {
            (theme::bold(24.0), 16.0)
        } else {
            (theme::regular(14.0), 3.0)
        };
        for (i, line) in lyrics.lines.iter().enumerate() {
            let goal = match current {
                Some(c) if c == i => LIT,
                Some(c) if i < c => SUNG,
                _ if lyrics.synced => COMING,
                _ => 1.0,
            };
            let bright = ui
                .ctx()
                .animate_value_with_time(id.with(("line", i)), goal, 0.35);
            let text = if line.text.trim().is_empty() {
                if lyrics.synced { "♪" } else { " " }
            } else {
                line.text.as_str()
            };
            let color = Color32::from_white_alpha((bright * 255.0) as u8);
            ui.set_max_width(width);
            let response = ui.add(
                egui::Label::new(egui::RichText::new(text).font(font.clone()).color(color))
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
            ui.add_space(gap);
        }
        ui.add_space(16.0);
        theme::label(
            ui,
            &format!("Source: {}", lyrics.source),
            theme::regular(14.0),
            PALETTE.dim,
        );
        ui.add_space(if lyrics.synced { viewport * 0.6 } else { 32.0 });
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
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    page::sections(app, ui, &app.route, found, &page::Look::PANEL);
                    ui.add_space(32.0);
                });
        }
        Some(crate::app::Loadable::Failed(message)) => {
            ui.add_space(64.0);
            ui.vertical_centered(|ui| {
                theme::label(ui, message, theme::regular(16.0), PALETTE.secondary);
            });
        }
        _ => {
            ui.add_space(64.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(28.0).color(PALETTE.secondary));
            });
        }
    }
}
