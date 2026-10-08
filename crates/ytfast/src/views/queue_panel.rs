//! "Up next": the queue from the song playing now, as the player page's
//! first tab lists it.

use crate::app::{Action, App};
use crate::theme::{self, PALETTE};
use crate::views::widgets::{self, Row};

/// The queue from the song playing now, as a list.
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
    egui::ScrollArea::vertical()
        .id_salt("queue-list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(16.0);
            let upcoming = &entries[current.min(entries.len())..];
            widgets::rows(ui, Row::QUEUE, upcoming.len(), |ui, index| {
                let entry = &upcoming[index];
                let id = entry.id;
                widgets::track_row_in(
                    app,
                    ui,
                    Row::QUEUE,
                    &entry.track,
                    None,
                    index == 0,
                    widgets::Place::Queue(id),
                    || Action::JumpTo(id),
                );
            });
            ui.add_space(16.0);
            if app.queue.remaining() == 0 && app.settings.autoplay {
                theme::label(
                    ui,
                    "More songs like these will follow.",
                    theme::regular(14.0),
                    PALETTE.dim,
                );
            }
            ui.add_space(24.0);
        });
}
