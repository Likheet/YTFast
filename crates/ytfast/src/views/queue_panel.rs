//! "Up next": the queue, on the right, with the song playing now on top.

use egui::{Frame, Margin};

use crate::app::{Action, App};
use crate::theme::{self, PALETTE};
use crate::views::widgets;

pub fn show(app: &App, ui: &mut egui::Ui) {
    egui::Panel::right("queue")
        .exact_size(theme::QUEUE_WIDTH)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(PALETTE.panel)
                .inner_margin(Margin::symmetric(12, 0)),
        )
        .show(ui, |ui| {
            ui.add_space(20.0);
            theme::label(ui, "Up next", theme::bold(20.0), PALETTE.text);
            ui.add_space(10.0);
            let entries = app.queue.entries();
            if entries.is_empty() {
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
                    for (index, entry) in entries.iter().enumerate().skip(current) {
                        let playing = index == current;
                        let id = entry.id;
                        widgets::track_row(app, ui, &entry.track, None, playing, || {
                            Action::JumpTo(id)
                        });
                    }
                    ui.add_space(12.0);
                    if app.queue.remaining() == 0 {
                        theme::label(
                            ui,
                            "More songs like these will follow.",
                            theme::regular(13.0),
                            PALETTE.dim,
                        );
                    }
                });
        });
}
