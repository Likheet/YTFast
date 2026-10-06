//! The bar along the bottom: what is playing, the controls, the progress
//! bar, volume, and the Up next button.

use egui::{Align, CornerRadius, Frame, Layout, Margin, Rect, Sense, UiBuilder, Vec2, pos2, vec2};

use crate::app::{Action, App, PlayState, Repeat};
use crate::theme::{self, Icon, PALETTE};
use crate::views::widgets;

pub fn show(app: &App, ui: &mut egui::Ui) {
    egui::Panel::bottom("player-bar")
        .exact_size(theme::PLAYER_BAR_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(PALETTE.panel)
                .stroke(egui::Stroke::new(1.0, PALETTE.outline))
                .corner_radius(egui::CornerRadius::same(theme::PANEL_RADIUS))
                .outer_margin(Margin {
                    left: theme::GAP,
                    right: theme::GAP,
                    top: theme::GAP,
                    bottom: theme::GAP,
                })
                .inner_margin(Margin::symmetric(16, 0)),
        )
        .show(ui, |ui| {
            let rect = ui.max_rect();
            // The progress line along the top edge, as in YouTube Music.
            progress_line(
                app,
                ui,
                Rect::from_min_max(
                    rect.min + vec2(-2.0, 1.0),
                    pos2(rect.right() + 2.0, rect.top() + 5.0),
                ),
            );

            let side = (rect.width() * 0.32).clamp(220.0, 420.0);
            let left = Rect::from_min_max(rect.min, pos2(rect.left() + side, rect.bottom()));
            let center = Rect::from_min_max(
                pos2(rect.left() + side, rect.top()),
                pos2(rect.right() - side, rect.bottom()),
            );
            let right = Rect::from_min_max(pos2(rect.right() - side, rect.top()), rect.max);

            now_playing(app, ui, left);
            controls(app, ui, center);
            extras(app, ui, right);
        });
}

fn progress_line(app: &App, ui: &mut egui::Ui, rect: Rect) {
    let status = &app.audio_status;
    let length = status.length.max(0.0);
    let fraction = if length > 0.0 {
        (status.position / length).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let response = ui.interact(
        rect.expand2(vec2(0.0, 4.0)),
        ui.id().with("progress"),
        Sense::click_and_drag(),
    );
    let hovered = response.hovered() || response.dragged();
    let bar = if hovered {
        rect.expand2(vec2(0.0, 1.0))
    } else {
        rect.with_min_y(rect.top() + 1.0)
            .with_max_y(rect.top() + 3.0)
    };
    ui.painter().rect_filled(bar, 0.0, PALETTE.outline);
    let filled = Rect::from_min_max(
        bar.min,
        pos2(bar.left() + bar.width() * fraction, bar.bottom()),
    );
    ui.painter().rect_filled(filled, 0.0, app.accent());
    if hovered && length > 0.0 {
        ui.painter()
            .circle_filled(pos2(filled.right(), bar.center().y), 6.0, app.accent());
    }
    if length > 0.0
        && (response.clicked() || response.drag_stopped())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let at = ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64 * length;
        app.act(Action::Seek(at));
    }
}

fn now_playing(app: &App, ui: &mut egui::Ui, band: Rect) {
    let Some(entry) = &app.playback.entry else {
        return;
    };
    let art = Rect::from_min_size(pos2(band.left(), band.center().y - 26.0), Vec2::splat(52.0));
    // The cover and title open the player page.
    let opener = Rect::from_min_max(art.min, pos2(band.right() - 92.0, art.bottom()));
    let open = ui.interact(opener, ui.id().with("open-player"), Sense::click());
    if open.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if open.clicked() {
        app.act(Action::ToggleNowPlaying);
    }
    widgets::cover(app, ui, art, entry.track.thumbnail.as_ref(), false);
    let mut likes = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(band.right() - 88.0, band.top()),
                band.max,
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    widgets::like_buttons(app, &mut likes, &entry.track);
    let mut text = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(art.right() + 12.0, band.center().y - 20.0),
                pos2(band.right() - 96.0, band.bottom()),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    text.spacing_mut().item_spacing.y = 2.0;
    theme::label(
        &mut text,
        &entry.track.title,
        theme::medium(14.5),
        PALETTE.text,
    );
    let line = match &app.playback.state {
        PlayState::Preparing => "Getting the song ready...".to_string(),
        PlayState::Failed(message) => message.clone(),
        _ => entry.track.artists.clone(),
    };
    let color = if matches!(app.playback.state, PlayState::Failed(_)) {
        PALETTE.danger
    } else {
        PALETTE.secondary
    };
    let hint = if app.playback.format.is_empty() {
        line.clone()
    } else {
        format!("{line}\n{}", app.playback.format)
    };
    theme::label(&mut text, &line, theme::regular(13.0), color).on_hover_text(hint);
}

fn controls(app: &App, ui: &mut egui::Ui, band: Rect) {
    let width = 40.0 + 48.0 + 40.0 + 2.0 * 16.0;
    let row = Rect::from_center_size(
        pos2(band.center().x, band.center().y + 2.0),
        vec2(width, 48.0),
    );
    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(row)
            .layout(Layout::left_to_right(Align::Center)),
    );
    ui.spacing_mut().item_spacing.x = 16.0;
    let has_song = app.playback.entry.is_some();
    ui.add_enabled_ui(has_song, |ui| {
        if theme::icon_button(ui, Icon::SkipBack, 22.0, PALETTE.text, "Previous").clicked() {
            app.act(Action::Previous);
        }
        let playing = app.playback.state == PlayState::Playing && !app.audio_status.paused;
        let (icon, tip) = if playing {
            (Icon::Pause, "Pause")
        } else {
            (Icon::Play, "Play")
        };
        if app.playback.state == PlayState::Preparing {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
            ui.put(rect, egui::Spinner::new().size(26.0).color(PALETTE.text));
        } else if theme::round_button(ui, icon, 44.0, tip).clicked() {
            app.act(Action::TogglePause);
        }
        if theme::icon_button(ui, Icon::SkipForward, 22.0, PALETTE.text, "Next").clicked() {
            app.act(Action::Next);
        }
    });
    // The time, under the buttons' row, at the right of the band.
    if has_song && app.audio_status.length > 0.0 {
        let time = format!(
            "{} / {}",
            theme::clock(app.audio_status.position),
            theme::clock(app.audio_status.length)
        );
        ui.painter().text(
            pos2(row.right() + 18.0, band.center().y + 2.0),
            egui::Align2::LEFT_CENTER,
            time,
            theme::regular(12.5),
            PALETTE.secondary,
        );
    }
}

fn extras(app: &App, ui: &mut egui::Ui, band: Rect) {
    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_center_size(
                band.center(),
                vec2(band.width(), 40.0),
            ))
            .layout(Layout::right_to_left(Align::Center)),
    );
    ui.spacing_mut().item_spacing.x = 10.0;
    let queue_color = if app.show_queue {
        PALETTE.text
    } else {
        PALETTE.secondary
    };
    let (icon, tip) = if app.now_playing {
        (Icon::Collapse, "Close the player")
    } else {
        (Icon::Expand, "Open the player")
    };
    if theme::icon_button(&mut ui, icon, 20.0, PALETTE.text, tip).clicked() {
        app.act(Action::ToggleNowPlaying);
    }
    if !app.now_playing
        && theme::icon_button(&mut ui, Icon::ListMusic, 20.0, queue_color, "Up next").clicked()
    {
        app.act(Action::ToggleQueue);
    }
    let has_queue = app.queue.remaining() > 1;
    ui.add_enabled_ui(has_queue, |ui| {
        let tip = "Shuffle the songs coming up";
        if theme::icon_button(ui, Icon::Shuffle, 20.0, PALETTE.secondary, tip).clicked() {
            app.act(Action::ShuffleQueue);
        }
    });
    let (icon, color, tip) = match app.settings.repeat {
        Repeat::Off => (Icon::Repeat, PALETTE.secondary, "Repeat is off"),
        Repeat::All => (Icon::Repeat, PALETTE.text, "Repeating the queue"),
        Repeat::One => (Icon::RepeatOne, PALETTE.text, "Repeating this song"),
    };
    let repeat = theme::icon_button(&mut ui, icon, 20.0, color, tip);
    if app.settings.repeat != Repeat::Off {
        // A dot under the button, as YouTube Music marks it.
        let dot = repeat.rect.center_bottom() - vec2(0.0, 3.0);
        ui.painter().circle_filled(dot, 2.0, PALETTE.text);
    }
    if repeat.clicked() {
        app.act(Action::CycleRepeat);
    }
    ui.add_space(6.0);
    let mut volume = app.settings.volume;
    if volume_slider(&mut ui, &mut volume).changed() {
        app.act(Action::SetVolume(volume));
    }
    let icon = if volume <= 0.001 {
        Icon::Muted
    } else {
        Icon::Loud
    };
    if theme::icon_button(&mut ui, icon, 20.0, PALETTE.secondary, "Mute").clicked() {
        app.act(Action::SetVolume(if volume <= 0.001 { 0.8 } else { 0.0 }));
    }
    if app.demo {
        theme::label(&mut ui, "Demo", theme::medium(12.0), PALETTE.dim);
    }
    if let Some(problem) = &app.audio_status.problem {
        theme::label(&mut ui, problem, theme::regular(12.0), PALETTE.danger).on_hover_text(problem);
    }
}

/// The volume bar: white on grey, with a round knob, as in YouTube Music.
fn volume_slider(ui: &mut egui::Ui, volume: &mut f32) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(96.0, 20.0), Sense::click_and_drag());
    let rail = Rect::from_center_size(rect.center(), vec2(rect.width() - 12.0, 4.0));
    if let Some(pointer) = response.interact_pointer_pos()
        && (response.is_pointer_button_down_on() || response.clicked())
    {
        let value = ((pointer.x - rail.left()) / rail.width()).clamp(0.0, 1.0);
        if value != *volume {
            *volume = value;
            response.mark_changed();
        }
    }
    let shown = *volume;
    response.widget_info(|| egui::WidgetInfo::slider(true, f64::from(shown), "Volume"));
    if ui.is_rect_visible(rect) {
        let x = rail.left() + rail.width() * *volume;
        ui.painter()
            .rect_filled(rail, CornerRadius::same(2), PALETTE.dim);
        let filled = Rect::from_min_max(rail.min, pos2(x, rail.bottom()));
        ui.painter()
            .rect_filled(filled, CornerRadius::same(2), PALETTE.text);
        let knob = if response.hovered() || response.dragged() {
            7.0
        } else {
            6.0
        };
        ui.painter()
            .circle_filled(pos2(x, rail.center().y), knob, PALETTE.text);
    }
    response
}
