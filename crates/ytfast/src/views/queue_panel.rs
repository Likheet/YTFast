//! "Up next": the queue, as the player page's first tab lists it (YouTube
//! Music's `ytmusic-player-queue`): what it plays from, every song in it
//! (those already played too, the playing one marked), then the Autoplay
//! switch. A song is moved by dragging its row, as on YouTube Music.

use egui::{Color32, CornerRadius, Rect, Sense, pos2, vec2};

use crate::app::{Action, App, Setting};
use crate::queue::Entry;
use crate::theme::{self, PALETTE};
use crate::views::widgets::{self, Row};

/// The queue as a list.
pub fn list(app: &App, ui: &mut egui::Ui) {
    let entries = app.queue.entries();
    if entries.is_empty() {
        ui.add_space(16.0);
        theme::label(
            ui,
            "Play something to fill the queue.",
            theme::regular(14.0),
            PALETTE.secondary,
        );
        return;
    }
    let current = app.queue.current_index().unwrap_or(0);
    let header = if app.queue.title.is_some() {
        16.0 + 16.8 + theme::text_size(ui) * 1.2 + 16.0
    } else {
        16.0
    };
    // The list opens at the playing song, the one before it still in
    // sight (or the header, for the first two), and moves on with it.
    let id = ui.id().with("queue-list");
    let playing = entries.get(current).map(|e| e.id);
    let shown: Option<Option<u64>> = ui.data(|d| d.get_temp(id));
    let mut area = egui::ScrollArea::vertical()
        .id_salt("queue-list")
        .auto_shrink([false, false]);
    if shown != Some(playing) {
        let queue = Row::QUEUE.themed();
        let step = queue.height + queue.gap;
        let offset = if current < 2 {
            0.0
        } else {
            header + (current - 1) as f32 * step
        };
        area = area.vertical_scroll_offset(offset);
        ui.data_mut(|d| d.insert_temp(id, playing));
    }
    // The entry whose row is being dragged, while the button is held.
    let drag_id = ui.id().with("queue-drag");
    let mut dragging: Option<u64> = ui.data(|d| d.get_temp(drag_id));
    area.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(16.0);
        if let Some(title) = &app.queue.title {
            playing_from(ui, title);
            ui.add_space(16.0);
        }
        let list = (ui.cursor().top(), ui.max_rect().x_range());
        widgets::rows(ui, Row::QUEUE, entries.len(), |ui, index| {
            let entry = &entries[index];
            let id = entry.id;
            let row = widgets::track_row_in(
                app,
                ui,
                Row::QUEUE,
                &entry.track,
                None,
                index == current,
                widgets::Place::Queue(id),
                || Action::JumpTo(id),
            );
            if let Some(row) = row {
                if row.drag_started() {
                    dragging = Some(id);
                }
                // Dimmed in its place while it is carried.
                if dragging == Some(id) {
                    ui.painter()
                        .rect_filled(row.rect, 0.0, Color32::from_black_alpha(160));
                }
            }
        });
        if let Some(id) = dragging {
            dragging = carry(app, ui, entries, id, list);
        }
        autoplay(app, ui);
        ui.add_space(24.0);
    });
    ui.data_mut(|d| match dragging {
        Some(id) => {
            d.insert_temp(drag_id, id);
        }
        None => d.remove::<u64>(drag_id),
    });
}

/// A row being dragged (entry `id`): a copy of it follows the pointer, a
/// white line shows where it will land, and the list scrolls when the
/// pointer nears its top or bottom. Let go, it moves there (Escape puts it
/// back). `list` is the first row's top and the rows' width. `None` once
/// it is dropped.
fn carry(
    app: &App,
    ui: &mut egui::Ui,
    entries: &[Entry],
    id: u64,
    list: (f32, egui::Rangef),
) -> Option<u64> {
    let (top, across) = list;
    let from = entries.iter().position(|e| e.id == id)?;
    let pointer = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()))?;
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        return None;
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    // Between which rows it would land.
    let queue = Row::QUEUE.themed();
    let step = queue.height + queue.gap;
    let gap = ((pointer.y - top) / step + 0.5)
        .floor()
        .clamp(0.0, entries.len() as f32) as usize;
    ui.painter().hline(
        across,
        top + gap as f32 * step - 1.0,
        egui::Stroke::new(2.0, PALETTE.text),
    );
    // The copy, over everything: its cover and words on `#212121`.
    let rect = Rect::from_min_size(
        pos2(across.min, pointer.y - queue.height / 2.0),
        vec2(across.span(), queue.height),
    );
    let layer = egui::LayerId::new(egui::Order::Tooltip, ui.id().with("queue-carried"));
    let ghost = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(rect));
    if theme::dynamic() {
        crate::dynamic::glass(ghost.painter(), rect, CornerRadius::same(8));
    } else {
        ghost
            .painter()
            .rect_filled(rect, CornerRadius::same(4), PALETTE.panel);
    }
    let track = &entries[from].track;
    let art = Rect::from_min_size(
        pos2(rect.left() + 8.0, rect.center().y - 16.0),
        egui::Vec2::splat(32.0),
    );
    widgets::cover_with(
        app,
        &ghost,
        art,
        track.thumbnail.as_ref(),
        CornerRadius::same(2),
    );
    let size = theme::text_size(&ghost);
    let words = rect.right() - 16.0 - (art.right() + 16.0);
    let first = rect.center().y - size * 1.2;
    theme::paint_line(
        &ghost,
        pos2(art.right() + 16.0, first),
        &track.title,
        theme::medium(size),
        PALETTE.text,
        words,
    );
    theme::paint_line(
        &ghost,
        pos2(art.right() + 16.0, first + size * 1.2),
        &track.artists,
        theme::regular(size),
        PALETTE.secondary,
        words,
    );
    // Near the list's top or bottom, it scrolls that way.
    let view = ui.clip_rect();
    let edge = 48.0;
    let speed = if pointer.y < view.top() + edge {
        8.0
    } else if pointer.y > view.bottom() - edge {
        -8.0
    } else {
        0.0
    };
    if speed != 0.0 {
        ui.scroll_with_delta(vec2(0.0, speed));
        ui.ctx().request_repaint();
    }
    // Let go: it moves there.
    if ui.input(|i| !i.pointer.primary_down()) {
        let to = if gap > from { gap - 1 } else { gap };
        if to != from {
            app.act(Action::MoveInQueue(id, to));
        }
        return None;
    }
    Some(id)
}

/// "Playing from" (12, white@0.70, on a 16.8 line) and under it what the
/// queue plays from (14/500 white, 16 from a window 1364 wide)
/// (`ytmusic-queue-header-renderer`).
fn playing_from(ui: &mut egui::Ui, title: &str) {
    let width = ui.available_width();
    let size = theme::text_size(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 16.8 + size * 1.2), Sense::hover());
    theme::paint_line(
        ui,
        rect.min,
        "Playing from",
        theme::regular(12.0),
        PALETTE.secondary,
        width,
    );
    theme::paint_line(
        ui,
        pos2(rect.left(), rect.top() + 16.8),
        title,
        theme::medium(size),
        PALETTE.text,
        width,
    );
}

/// After the queue's songs, as YouTube Music: "Autoplay" (14/500, 16 from
/// a window 1364 wide), 4 under it "Add similar content to the end of the
/// queue" (12, `#aaa`), and its switch at the right
/// (`.autoplay.ytmusic-tab-renderer`: margin 16 16 12 8, padding 4 0). The
/// same setting as in Settings.
fn autoplay(app: &App, ui: &mut egui::Ui) {
    ui.add_space(16.0 + 4.0);
    let width = ui.available_width();
    let size = theme::text_size(ui);
    let title_line = size * 1.2;
    let (row, _) = ui.allocate_exact_size(vec2(width, title_line + 4.0 + 14.4), Sense::hover());
    let words = Rect::from_min_max(
        pos2(row.left() + 8.0, row.top()),
        pos2(row.right() - 16.0 - 36.0 - 16.0, row.bottom()),
    );
    theme::paint_line(
        ui,
        words.min,
        "Autoplay",
        theme::medium(size),
        PALETTE.text,
        words.width(),
    );
    theme::paint_line(
        ui,
        pos2(words.left(), words.top() + title_line + 4.0),
        "Add similar content to the end of the queue",
        theme::regular(12.0),
        PALETTE.dim,
        words.width(),
    );
    let switch = Rect::from_min_size(
        pos2(row.right() - 16.0 + 6.0 - 36.0, row.center().y - 10.0),
        vec2(36.0, 20.0),
    );
    let mut place = ui.new_child(egui::UiBuilder::new().max_rect(switch));
    if theme::toggle(&mut place, app.settings.autoplay, "Autoplay").clicked() {
        app.act(Action::Toggle(Setting::Autoplay));
    }
    ui.add_space(4.0 + 12.0);
}
