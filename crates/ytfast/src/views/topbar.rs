//! The bar above the page: back and forward, and the search box.

use std::time::Duration;

use egui::{Align, CornerRadius, Frame, Layout, Margin, Sense, Vec2, vec2};

use crate::app::{Action, App};
use crate::theme::{self, Icon, PALETTE};

pub fn show(app: &App, ui: &mut egui::Ui) {
    egui::Panel::top("top-bar")
        .exact_size(theme::TOP_BAR_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().inner_margin(Margin::symmetric(24, 0)))
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
    suggestions(app, ui, &response, rect, &mut text);
    ui.data_mut(|d| d.insert_temp(id, text));
}

/// How long typing pauses before suggestions are asked for, in seconds.
const TYPING_PAUSE: f64 = 0.25;

/// What YouTube Music suggests for the words typed so far, under the box.
fn suggestions(
    app: &App,
    ui: &mut egui::Ui,
    field: &egui::Response,
    rect: egui::Rect,
    text: &mut String,
) {
    let id = ui.id().with("suggest");
    let now = ui.input(|i| i.time);
    let typed = text.trim().to_string();
    // Asked for once typing pauses for a moment.
    let asked: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    if typed != asked {
        let (pending, since): (String, f64) = ui
            .data(|d| d.get_temp(id.with("pending")))
            .unwrap_or_default();
        let waited = now - since;
        if typed.is_empty() {
            // Nothing to ask about.
            ui.data_mut(|d| d.insert_temp(id, typed.clone()));
        } else if pending != typed {
            ui.data_mut(|d| d.insert_temp(id.with("pending"), (typed.clone(), now)));
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(TYPING_PAUSE + 0.01));
        } else if waited >= TYPING_PAUSE {
            app.act(Action::Suggest(typed.clone()));
            ui.data_mut(|d| d.insert_temp(id, typed.clone()));
        } else {
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(TYPING_PAUSE - waited + 0.01));
        }
    }
    let (for_text, found) = &app.suggestions;
    let popup_rect: Option<egui::Rect> = ui.data(|d| d.get_temp(id.with("rect")));
    let over_popup = popup_rect.is_some_and(|r| ui.rect_contains_pointer(r));
    let show = (field.has_focus() || over_popup)
        && !typed.is_empty()
        && *for_text == typed
        && !found.is_empty();
    if !show {
        ui.data_mut(|d| d.remove::<egui::Rect>(id.with("rect")));
        return;
    }
    let area = egui::Area::new(id.with("popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.left_bottom() + vec2(0.0, 6.0))
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(egui::Color32::from_rgba_premultiplied(26, 26, 30, 248))
                .stroke(egui::Stroke::new(1.0, PALETTE.outline))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(6))
                .show(ui, |ui| {
                    ui.set_width(rect.width() - 12.0);
                    for suggestion in found.iter().take(8) {
                        let (row, response) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
                        if response.hovered() {
                            ui.painter()
                                .rect_filled(row, CornerRadius::same(6), PALETTE.surface);
                        }
                        let icon =
                            egui::Rect::from_min_size(row.min + vec2(6.0, 7.0), Vec2::splat(20.0));
                        theme::paint_icon(ui, Icon::Search, icon, 15.0, PALETTE.dim);
                        ui.painter().text(
                            row.left_center() + vec2(34.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            suggestion,
                            theme::regular(14.5),
                            PALETTE.text,
                        );
                        if response.clicked() {
                            *text = suggestion.clone();
                            app.act(Action::Search(suggestion.clone()));
                            ui.memory_mut(|m| m.surrender_focus(field.id));
                        }
                    }
                });
        });
    ui.data_mut(|d| d.insert_temp(id.with("rect"), area.response.rect));
}
