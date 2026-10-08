//! "Up next": the queue, as the player page's first tab lists it (YouTube
//! Music's `ytmusic-player-queue`): what it plays from, every song in it
//! (those already played too, the playing one marked), then the Autoplay
//! switch.

use egui::{Rect, Sense, pos2, vec2};

use crate::app::{Action, App, Setting};
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
        16.0 + 33.6 + 16.0
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
        let step = Row::QUEUE.height + Row::QUEUE.gap;
        let offset = if current < 2 {
            0.0
        } else {
            header + (current - 1) as f32 * step
        };
        area = area.vertical_scroll_offset(offset);
        ui.data_mut(|d| d.insert_temp(id, playing));
    }
    area.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(16.0);
        if let Some(title) = &app.queue.title {
            playing_from(ui, title);
            ui.add_space(16.0);
        }
        widgets::rows(ui, Row::QUEUE, entries.len(), |ui, index| {
            let entry = &entries[index];
            let id = entry.id;
            widgets::track_row_in(
                app,
                ui,
                Row::QUEUE,
                &entry.track,
                None,
                index == current,
                widgets::Place::Queue(id),
                || Action::JumpTo(id),
            );
        });
        autoplay(app, ui);
        ui.add_space(24.0);
    });
}

/// "Playing from" (12, white@0.70) and under it what the queue plays
/// from (14/500 white), 33.6 high in all (`ytmusic-queue-header-renderer`).
fn playing_from(ui: &mut egui::Ui, title: &str) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 33.6), Sense::hover());
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
        theme::medium(14.0),
        PALETTE.text,
        width,
    );
}

/// After the queue's songs, as YouTube Music: "Autoplay" (14/500), 4 under
/// it "Add similar content to the end of the queue" (12, `#aaa`), and its
/// switch at the right (`.autoplay.ytmusic-tab-renderer`: margin 16 16 12
/// 8, padding 4 0). The same setting as in Settings.
fn autoplay(app: &App, ui: &mut egui::Ui) {
    ui.add_space(16.0 + 4.0);
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(vec2(width, 16.8 + 4.0 + 14.4), Sense::hover());
    let words = Rect::from_min_max(
        pos2(row.left() + 8.0, row.top()),
        pos2(row.right() - 16.0 - 36.0 - 16.0, row.bottom()),
    );
    theme::paint_line(
        ui,
        words.min,
        "Autoplay",
        theme::medium(14.0),
        PALETTE.text,
        words.width(),
    );
    theme::paint_line(
        ui,
        pos2(words.left(), words.top() + 16.8 + 4.0),
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
