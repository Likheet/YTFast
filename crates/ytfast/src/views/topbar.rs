//! The bar across the top, as YouTube Music's: the menu button and the
//! app's mark, the search box, and the account. Back and forward sit just
//! before the search box (a web browser has its own; this window has not).

use std::time::Duration;

use egui::{Align2, CornerRadius, Frame, Margin, Rect, Sense, UiBuilder, Vec2, pos2, vec2};

use crate::app::{Action, App, Auth};
use crate::backend::Route;
use crate::theme::{self, Icon, PALETTE, Round};
use crate::views::{backdrop, page, sidebar};

pub fn show(app: &App, ui: &mut egui::Ui) {
    egui::Panel::top("top-bar")
        .exact_size(theme::TOP_BAR_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new())
        .show(ui, |ui| {
            let bar = ui.max_rect();
            let guide = sidebar::width(app);
            // An album's or playlist's page is washed with its cover's
            // colour from the top of the window: this bar's part of it.
            if let Some(color) = page::wash_color(app, bar.width() - guide) {
                let below = if app.playback.entry.is_some() {
                    theme::PLAYER_BAR_HEIGHT
                } else {
                    0.0
                };
                let page_height = ui.ctx().content_rect().height() - bar.height() - below;
                let reach = bar.top() + page::wash_reach(page_height);
                let part = Rect::from_min_max(pos2(bar.left() + guide, bar.top()), bar.max);
                backdrop::wash(ui, part, color, (bar.top()..=reach).into());
            }
            if !app.settings.mini_guide {
                // The hairline between the menu and the page carries on
                // through the bar.
                ui.painter().vline(
                    bar.left() + guide - 0.5,
                    bar.y_range(),
                    egui::Stroke::new(1.0, PALETTE.outline),
                );
            }

            // The menu button and the mark.
            let button =
                Rect::from_center_size(pos2(bar.left() + 32.0, bar.center().y), Vec2::splat(40.0));
            let mut left = ui.new_child(UiBuilder::new().max_rect(button));
            if theme::round_button(
                &mut left,
                Icon::Menu,
                40.0,
                24.0,
                Round::Plain,
                PALETTE.text,
                "Menu",
            )
            .clicked()
            {
                app.act(Action::ToggleGuide);
            }
            let mark = Rect::from_min_size(
                pos2(bar.left() + 68.0, bar.center().y - 12.0),
                Vec2::splat(24.0),
            );
            theme::paint_logo(ui, mark);
            let name = ui.painter().text(
                pos2(mark.right() + 5.0, bar.center().y),
                Align2::LEFT_CENTER,
                "YTFast",
                theme::bold(19.0),
                PALETTE.text,
            );
            let home = ui.interact(mark.union(name), ui.id().with("home"), Sense::click());
            if home.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if home.clicked() {
                app.act(Action::Navigate(Route::Home));
            }

            // The page's own margins, so the search box and the account
            // line up with what is under them.
            let margin = theme::page_margin(bar.width() - guide);
            let arrows = 88.0;
            let search_left = (bar.left() + guide + margin).max(name.right() + 12.0 + arrows);
            let account_right = bar.right() - margin;

            let mut nav = ui.new_child(
                UiBuilder::new()
                    .max_rect(Rect::from_min_size(
                        pos2(search_left - arrows, bar.center().y - 20.0),
                        vec2(arrows, 40.0),
                    ))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            nav.spacing_mut().item_spacing.x = 0.0;
            nav.add_enabled_ui(app.can_go_back(), |ui| {
                if theme::round_button(
                    ui,
                    Icon::Back,
                    40.0,
                    22.0,
                    Round::Plain,
                    PALETTE.text,
                    "Back",
                )
                .clicked()
                {
                    app.act(Action::Back);
                }
            });
            nav.add_enabled_ui(app.can_go_forward(), |ui| {
                if theme::round_button(
                    ui,
                    Icon::Forward,
                    40.0,
                    22.0,
                    Round::Plain,
                    PALETTE.text,
                    "Forward",
                )
                .clicked()
                {
                    app.act(Action::Forward);
                }
            });

            let avatar = Rect::from_min_size(
                pos2(account_right - 32.0, bar.center().y - 16.0),
                Vec2::splat(32.0),
            );
            account(app, ui, avatar);

            let width = (avatar.left() - 24.0 - search_left).clamp(120.0, 480.0);
            let field =
                Rect::from_min_size(pos2(search_left, bar.center().y - 20.0), vec2(width, 40.0));
            search_box(app, ui, field);
        });
}

/// The account's round button, and its menu: History, Settings, signing
/// out.
fn account(app: &App, ui: &mut egui::Ui, rect: Rect) {
    let Auth::SignedIn { name } = &app.auth else {
        return;
    };
    let response = ui.interact(rect, ui.id().with("account"), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Account"));
    let fill = egui::Color32::from_rgb(0x5f, 0x6b, 0x7a);
    ui.painter().circle_filled(rect.center(), 16.0, fill);
    let initial: String = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        initial,
        theme::medium(15.0),
        PALETTE.text,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    egui::Popup::menu(&response).gap(8.0).show(|ui| {
        theme::menu(ui);
        // The account's name, in line with the menu's words.
        let (who, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
        let under = if app.demo { "Demo" } else { "YouTube Music" };
        theme::paint_line(
            ui,
            who.left_top() + vec2(16.0, 7.0),
            name,
            theme::medium(16.0),
            PALETTE.text,
            who.width() - 32.0,
        );
        theme::paint_line(
            ui,
            who.left_top() + vec2(16.0, 28.0),
            under,
            theme::regular(13.0),
            PALETTE.dim,
            who.width() - 32.0,
        );
        ui.separator();
        let go = |ui: &mut egui::Ui, text: &str, route: Route| {
            if ui.button(text).clicked() {
                app.act(Action::Navigate(route));
                ui.close();
            }
        };
        go(ui, "History", Route::History);
        go(ui, "Settings", Route::Settings);
        if !app.demo {
            ui.separator();
            if ui.button("Sign out").clicked() {
                app.act(Action::SignOut);
                ui.close();
            }
        }
    });
}

fn search_box(app: &App, ui: &mut egui::Ui, rect: Rect) {
    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), PALETTE.field);
    let icon = Rect::from_min_size(rect.min + vec2(16.0, 8.0), Vec2::splat(24.0));
    theme::paint_icon(ui, Icon::Search, icon, 20.0, PALETTE.secondary);

    let id = ui.id().with("search");
    let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default();
    let field_rect = Rect::from_min_max(rect.min + vec2(53.0, 9.0), rect.max - vec2(12.0, 8.0));
    let hint = egui::RichText::new("Search songs, albums, artists, podcasts")
        .color(egui::Color32::from_white_alpha(128));
    let edit = egui::TextEdit::singleline(&mut text)
        .id(super::SEARCH_BOX.into())
        .hint_text(hint)
        .font(theme::regular(16.0))
        .frame(Frame::NONE)
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
    rect: Rect,
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
    let popup_rect: Option<Rect> = ui.data(|d| d.get_temp(id.with("rect")));
    let over_popup = popup_rect.is_some_and(|r| ui.rect_contains_pointer(r));
    let show = (field.has_focus() || over_popup)
        && !typed.is_empty()
        && *for_text == typed
        && !found.is_empty();
    if !show {
        ui.data_mut(|d| d.remove::<Rect>(id.with("rect")));
        return;
    }
    let area = egui::Area::new(id.with("popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.left_bottom() + vec2(0.0, 4.0))
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(PALETTE.panel)
                .stroke(egui::Stroke::new(1.0, PALETTE.outline))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(0, 8))
                .show(ui, |ui| {
                    ui.set_width(rect.width());
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for suggestion in found.iter().take(7) {
                        let (row, response) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(row, 0.0, PALETTE.surface);
                        }
                        let icon =
                            Rect::from_min_size(row.min + vec2(16.0, 8.0), Vec2::splat(24.0));
                        theme::paint_icon(ui, Icon::Search, icon, 18.0, PALETTE.dim);
                        theme::paint_line(
                            ui,
                            row.left_top() + vec2(53.0, 10.0),
                            suggestion,
                            theme::medium(16.0),
                            PALETTE.text,
                            row.width() - 69.0,
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
