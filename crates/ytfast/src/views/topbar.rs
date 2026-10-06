//! The bar above the page: back and forward, and the search box.

use egui::{Align, CornerRadius, Frame, Layout, Margin, Sense, Vec2, vec2};

use crate::app::{Action, App};
use crate::theme::{self, Icon, PALETTE};

pub fn show(app: &App, ui: &mut egui::Ui) {
    egui::Panel::top("top-bar")
        .exact_size(theme::TOP_BAR_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(PALETTE.window)
                .inner_margin(Margin::symmetric(24, 0)),
        )
        .show(ui, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add_enabled_ui(app.can_go_back(), |ui| {
                    if theme::icon_button(ui, Icon::Back, 22.0, PALETTE.text, "Back").clicked() {
                        app.act(Action::Back);
                    }
                });
                ui.add_enabled_ui(app.can_go_forward(), |ui| {
                    if theme::icon_button(ui, Icon::Forward, 22.0, PALETTE.text, "Forward")
                        .clicked()
                    {
                        app.act(Action::Forward);
                    }
                });
                ui.add_space(12.0);
                search_box(app, ui);
            });
        });
}

fn search_box(app: &App, ui: &mut egui::Ui) {
    let width = (ui.available_width() * 0.6).clamp(260.0, 560.0);
    let height = 40.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), PALETTE.surface);
    let icon = egui::Rect::from_min_size(rect.min + vec2(10.0, 8.0), Vec2::splat(24.0));
    theme::paint_icon(ui, Icon::Search, icon, 20.0, PALETTE.secondary);

    let id = ui.id().with("search");
    let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default();
    let field_rect =
        egui::Rect::from_min_max(rect.min + vec2(44.0, 9.0), rect.max - vec2(12.0, 8.0));
    let edit = egui::TextEdit::singleline(&mut text)
        .id(super::SEARCH_BOX.into())
        .hint_text(egui::RichText::new("Search songs, albums, artists").color(PALETTE.secondary))
        .font(theme::regular(15.0))
        .frame(egui::Frame::NONE)
        .text_color(PALETTE.text);
    let response = ui.put(field_rect, edit);
    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        app.act(Action::Search(text.clone()));
    }
    ui.data_mut(|d| d.insert_temp(id, text));
}
