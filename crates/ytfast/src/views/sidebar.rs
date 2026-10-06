//! The sidebar: Home, Explore, Library, Liked Music, and the playlists in
//! the library, as in YouTube Music.

use egui::{Align, CornerRadius, Frame, Layout, Margin, Rect, Sense, Vec2, pos2, vec2};
use ytfast_core::read::{Item, Target};

use crate::app::{Action, App, Auth, Loadable};
use crate::backend::Route;
use crate::theme::{self, Icon, PALETTE};

pub fn show(app: &App, ui: &mut egui::Ui) {
    egui::Panel::left("sidebar")
        .exact_size(theme::SIDEBAR_WIDTH)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(PALETTE.panel)
                .stroke(egui::Stroke::new(1.0, PALETTE.outline))
                .corner_radius(egui::CornerRadius::same(theme::PANEL_RADIUS))
                .outer_margin(Margin {
                    left: theme::GAP,
                    right: 0,
                    top: theme::GAP,
                    bottom: 0,
                })
                .inner_margin(Margin::symmetric(12, 0)),
        )
        .show(ui, |ui| {
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                theme::paint_logo(ui, rect);
                ui.add_space(2.0);
                theme::label(ui, "YTFast", theme::bold(20.0), PALETTE.text);
            });
            ui.add_space(18.0);

            nav_item(app, ui, Icon::Home, "Home", Route::Home);
            nav_item(app, ui, Icon::Explore, "Explore", Route::Explore);
            nav_item(app, ui, Icon::Library, "Library", Route::Library);
            nav_item(app, ui, Icon::History, "History", Route::History);

            ui.add_space(10.0);
            ui.painter().hline(
                ui.max_rect().x_range(),
                ui.cursor().top(),
                egui::Stroke::new(1.0, PALETTE.outline),
            );
            ui.add_space(12.0);

            let bottom = 48.0;
            egui::ScrollArea::vertical()
                .id_salt("sidebar-playlists")
                .max_height((ui.available_height() - bottom).max(0.0))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    playlist_item(app, ui, "Liked Music", "Auto playlist", Route::Liked);
                    if let Some(Loadable::Ready(page)) = app.pages.get(&Route::Library) {
                        for section in &page.sections {
                            for item in &section.items {
                                let Item::Card(card) = item else { continue };
                                let Some(Target::Browse { id, params, .. }) = &card.open else {
                                    continue;
                                };
                                // Liked Music is already listed first.
                                if id == "VLLM" {
                                    continue;
                                }
                                let route = Route::browse(id.clone(), params.clone());
                                playlist_item(app, ui, &card.title, &card.subtitle, route);
                            }
                        }
                    }
                });

            // The account, and signing out, at the bottom.
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if let Auth::SignedIn { name } = &app.auth {
                        let (icon, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                        theme::paint_icon(ui, Icon::User, icon, 16.0, PALETTE.secondary);
                        let width = ui.available_width() - 40.0;
                        ui.allocate_ui(vec2(width, 24.0), |ui| {
                            theme::label(ui, name, theme::regular(13.0), PALETTE.secondary);
                        });
                    }
                    let color = if app.route == Route::Settings {
                        PALETTE.text
                    } else {
                        PALETTE.secondary
                    };
                    if theme::icon_button(ui, Icon::Settings, 17.0, color, "Settings").clicked() {
                        app.act(Action::Navigate(Route::Settings));
                    }
                });
            });
        });
}

fn nav_item(app: &App, ui: &mut egui::Ui, icon: Icon, text: &str, route: Route) {
    let active = app.route == route;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    if active || response.hovered() {
        let fill = if active {
            PALETTE.surface
        } else {
            PALETTE.surface.gamma_multiply(0.6)
        };
        ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    }
    let icon_rect = Rect::from_min_size(
        pos2(rect.left() + 12.0, rect.center().y - 12.0),
        Vec2::splat(24.0),
    );
    theme::paint_icon(ui, icon, icon_rect, 22.0, PALETTE.text);
    ui.painter().text(
        pos2(icon_rect.right() + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        theme::medium(15.0),
        PALETTE.text,
    );
    if response.clicked() {
        app.act(Action::Navigate(route));
    }
}

fn playlist_item(app: &App, ui: &mut egui::Ui, title: &str, subtitle: &str, route: Route) {
    let active = app.route == route;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click());
    if active || response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(8),
            PALETTE
                .surface
                .gamma_multiply(if active { 1.0 } else { 0.6 }),
        );
    }
    let left = rect.left() + 12.0;
    let width = rect.width() - 24.0;
    let painter = ui.painter().with_clip_rect(rect.shrink2(vec2(10.0, 0.0)));
    let title_galley = painter.layout_no_wrap(title.to_string(), theme::medium(14.0), PALETTE.text);
    painter.galley(
        pos2(left, rect.top() + 6.0),
        elide(ui, title_galley, width),
        PALETTE.text,
    );
    let subtitle_galley = painter.layout_no_wrap(
        subtitle.to_string(),
        theme::regular(12.5),
        PALETTE.secondary,
    );
    painter.galley(
        pos2(left, rect.top() + 26.0),
        elide(ui, subtitle_galley, width),
        PALETTE.secondary,
    );
    if response.clicked() {
        app.act(Action::Navigate(route));
    }
}

/// Cuts a laid-out line short with "…" when it is wider than `width`.
fn elide(
    ui: &egui::Ui,
    galley: std::sync::Arc<egui::Galley>,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    if galley.size().x <= width {
        return galley;
    }
    let text = galley.text().to_string();
    let format = galley
        .job
        .sections
        .first()
        .map(|s| s.format.clone())
        .unwrap_or_default();
    let mut job = egui::text::LayoutJob::single_section(text, format);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    ui.fonts_mut(|f| f.layout_job(job))
}
