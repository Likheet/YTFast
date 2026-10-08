//! The bar across the bottom, as YouTube Music's: what is playing on the
//! left, the controls in the middle (shuffle, previous, play, next,
//! repeat), and on the right the time, like and dislike, the volume and
//! the song's menu. The progress line runs along its top edge.

use egui::{Align, Color32, CornerRadius, Frame, Layout, Rect, Sense, UiBuilder, Vec2, pos2, vec2};

use crate::app::{Action, App, LikeState, PlayState, Repeat};
use crate::queue::Entry;
use crate::theme::{self, Icon, PALETTE, Round};
use crate::views::widgets;

pub fn show(app: &App, ui: &mut egui::Ui) {
    egui::Panel::bottom("player-bar")
        .exact_size(theme::PLAYER_BAR_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(PALETTE.panel))
        .show(ui, |ui| {
            let bar = ui.max_rect();
            let Some(entry) = &app.playback.entry else {
                return;
            };
            // The five buttons, 48 each with 16 between, in the middle.
            let controls_width = 5.0 * 48.0 + 4.0 * 16.0;
            let middle = Rect::from_center_size(bar.center(), vec2(controls_width, 48.0));
            let narrow = bar.width() < 900.0;
            let left = Rect::from_min_max(bar.min, pos2(middle.left() - 24.0, bar.bottom()));
            let right = Rect::from_min_max(pos2(middle.right() + 24.0, bar.top()), bar.max);

            now_playing(app, ui, left, entry);
            controls(app, ui, middle);
            extras(app, ui, right, entry, narrow);
            // Last, so it is over the bar's own things.
            progress_line(app, ui, bar);
        });
}

/// The line along the top edge: red up to where the song is. Click or
/// drag to move in the song.
fn progress_line(app: &App, ui: &mut egui::Ui, bar: Rect) {
    let length = app.audio_status.length.max(0.0);
    let fraction = if length > 0.0 {
        (app.shown_position() / length).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let track = Rect::from_min_max(bar.left_top(), pos2(bar.right(), bar.top() + 3.0));
    let response = ui.interact(
        track.expand2(vec2(0.0, 5.0)),
        ui.id().with("progress"),
        Sense::click_and_drag(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::slider(true, f64::from(fraction), "Position in the song")
    });
    let held = response.hovered() || response.dragged();
    let grow = ui
        .ctx()
        .animate_bool_with_time(response.id.with("grow"), held, 0.1);
    let line = track.with_max_y(track.bottom() + 2.0 * grow);
    // While dragging, the line follows the pointer; the song moves when
    // the button is let go.
    let shown = match response.interact_pointer_pos() {
        Some(pointer) if response.dragged() => {
            ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0)
        }
        _ => fraction,
    };
    // Over everything, so the knob can stand above the bar's edge.
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        ui.id().with("progress-line"),
    ));
    painter.rect_filled(line, 0.0, Color32::from_white_alpha(51));
    let played = Rect::from_min_max(
        line.min,
        pos2(line.left() + line.width() * shown, line.bottom()),
    );
    painter.rect_filled(played, 0.0, PALETTE.accent);
    if grow > 0.0 {
        painter.circle_filled(
            pos2(played.right(), line.center().y),
            6.0 * grow,
            PALETTE.accent,
        );
    }
    if held {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if length > 0.0
        && (response.clicked() || response.drag_stopped())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let at = ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64 * length;
        app.act(Action::Seek(at));
    }
}

/// The cover, the title and "Artist • Album". Clicking opens the player
/// page (or closes it).
fn now_playing(app: &App, ui: &mut egui::Ui, band: Rect, entry: &Entry) {
    let track = &entry.track;
    let art = Rect::from_min_size(
        pos2(band.left() + 16.0, band.center().y - 24.0),
        Vec2::splat(48.0),
    );
    let text_left = art.right() + 16.0;
    let text_width = (band.right() - text_left).max(0.0);

    let open = ui.interact(band, ui.id().with("open-player"), Sense::click());
    open.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Open the player page")
    });
    if open.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if open.clicked() {
        app.act(Action::ToggleNowPlaying);
    }
    widgets::cover_with(
        app,
        ui,
        art,
        track.thumbnail.as_ref(),
        CornerRadius::same(2),
    );

    let title_color = Color32::from_rgb(0xf1, 0xf1, 0xf1);
    theme::paint_line(
        ui,
        pos2(text_left, band.top() + 17.0),
        &track.title,
        theme::regular(14.0),
        title_color,
        text_width,
    );
    let (line, color) = match &app.playback.state {
        PlayState::Preparing => ("Getting the song ready...".to_string(), PALETTE.dim),
        PlayState::Failed(message) => (message.clone(), PALETTE.danger),
        _ => (byline(track), PALETTE.dim),
    };
    let at = pos2(text_left, band.top() + 37.0);
    let size = theme::paint_line(ui, at, &line, theme::regular(14.0), color, text_width);
    // How the song was found, for whoever rests the pointer on the line.
    let hint = if app.playback.format.is_empty() {
        line
    } else {
        format!("{line}\n{}", app.playback.format)
    };
    ui.interact(
        Rect::from_min_size(at, size),
        ui.id().with("byline"),
        Sense::hover(),
    )
    .on_hover_text(hint);
}

/// "Artist • Album".
fn byline(track: &ytfast_core::read::Track) -> String {
    match &track.album {
        Some(album) if !album.is_empty() && !track.artists.is_empty() => {
            format!("{} \u{2022} {album}", track.artists)
        }
        Some(album) if !album.is_empty() => album.clone(),
        _ => track.artists.clone(),
    }
}

fn controls(app: &App, ui: &mut egui::Ui, row: Rect) {
    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(row)
            .layout(Layout::left_to_right(Align::Center)),
    );
    ui.spacing_mut().item_spacing.x = 16.0;
    let icon = |ui: &mut egui::Ui, icon: Icon, style: Round, tip: &str| {
        theme::round_button(ui, icon, 48.0, 24.0, style, PALETTE.text, tip)
    };

    ui.add_enabled_ui(app.queue.remaining() > 1, |ui| {
        if icon(
            ui,
            Icon::Shuffle,
            Round::Plain,
            "Shuffle the songs coming up",
        )
        .clicked()
        {
            app.act(Action::ShuffleQueue);
        }
    });
    if icon(&mut ui, Icon::SkipBack, Round::Plain, "Previous").clicked() {
        app.act(Action::Previous);
    }
    let playing = app.playback.state == PlayState::Playing && !app.audio_status.paused;
    if app.playback.state == PlayState::Preparing {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 24.0, PALETTE.surface);
        ui.put(
            Rect::from_center_size(rect.center(), Vec2::splat(24.0)),
            egui::Spinner::new().size(24.0).color(PALETTE.text),
        );
    } else {
        let (glyph, tip) = if playing {
            (Icon::Pause, "Pause")
        } else {
            (Icon::Play, "Play")
        };
        if icon(&mut ui, glyph, Round::Tonal, tip).clicked() {
            app.act(Action::TogglePause);
        }
    }
    if icon(&mut ui, Icon::SkipForward, Round::Plain, "Next").clicked() {
        app.act(Action::Next);
    }
    // Lit (on a disc) when repeating.
    let (glyph, style, tip) = match app.settings.repeat {
        Repeat::Off => (Icon::Repeat, Round::Plain, "Repeat is off"),
        Repeat::All => (Icon::Repeat, Round::Tonal, "Repeating the queue"),
        Repeat::One => (Icon::RepeatOne, Round::Tonal, "Repeating this song"),
    };
    if icon(&mut ui, glyph, style, tip).clicked() {
        app.act(Action::CycleRepeat);
    }
}

/// From the right edge: the song's menu, the volume, like and dislike,
/// and the time.
fn extras(app: &App, ui: &mut egui::Ui, band: Rect, entry: &Entry, narrow: bool) {
    let row = Rect::from_min_max(
        pos2(band.left(), band.center().y - 16.0),
        pos2(band.right() - 16.0, band.center().y + 16.0),
    );
    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(row)
            .layout(Layout::right_to_left(Align::Center)),
    );
    ui.spacing_mut().item_spacing.x = 8.0;

    let more = theme::round_button(
        &mut ui,
        Icon::MoreVertical,
        32.0,
        18.0,
        Round::Tonal,
        PALETTE.text,
        "More",
    );
    egui::Popup::menu(&more)
        .gap(8.0)
        .show(|ui| widgets::song_menu(app, ui, &entry.track, widgets::Place::Playing));

    // The volume bar opens no wider than leaves room for the like pill.
    let bar_room = ui.available_width() - (32.0 + 8.0) - (8.0 + LIKE_WIDTH);
    volume(app, &mut ui, bar_room.clamp(0.0, 96.0));
    like_pill(app, &mut ui, &entry.track);

    if narrow {
        return;
    }
    if app.audio_status.length > 0.0 {
        let time = format!(
            "{} / {}",
            theme::clock(app.shown_position()),
            theme::clock(app.audio_status.length)
        );
        // Whole or not at all: in a narrow window the open volume bar
        // leaves no room for it.
        let galley = ui
            .painter()
            .layout_no_wrap(time.clone(), theme::medium(14.0), PALETTE.dim);
        if galley.size().x + 16.0 <= ui.available_width() {
            ui.add_space(16.0);
            theme::label(&mut ui, &time, theme::medium(14.0), PALETTE.dim);
        }
    } else {
        ui.add_space(16.0);
    }
    if app.demo {
        theme::label(&mut ui, "Demo", theme::medium(12.0), PALETTE.faint);
    }
    if let Some(problem) = &app.audio_status.problem {
        theme::label(&mut ui, problem, theme::regular(12.0), PALETTE.danger).on_hover_text(problem);
    }
}

/// How wide the like and dislike pill is.
const LIKE_WIDTH: f32 = 96.0;

/// Like and dislike in one pill, as YouTube Music draws them.
fn like_pill(app: &App, ui: &mut egui::Ui, track: &ytfast_core::read::Track) {
    let state = app
        .likes
        .get(&track.video_id)
        .copied()
        .unwrap_or(LikeState::Neutral);
    let (rect, _) = ui.allocate_exact_size(vec2(LIKE_WIDTH, 32.0), Sense::hover());
    let like = Rect::from_min_max(rect.min, pos2(rect.center().x, rect.bottom()));
    let dislike = Rect::from_min_max(pos2(rect.center().x, rect.top()), rect.max);
    let liked = state == LikeState::Liked;
    let disliked = state == LikeState::Disliked;
    let like_tip = if liked {
        "Remove from Liked Music"
    } else {
        "Like"
    };
    let dislike_tip = if disliked {
        "Remove dislike"
    } else {
        "Dislike"
    };
    let id = ui.id().with(("like", &track.video_id));
    let like_response = ui
        .interact(like, id.with("up"), Sense::click())
        .on_hover_text(like_tip);
    let dislike_response = ui
        .interact(dislike, id.with("down"), Sense::click())
        .on_hover_text(dislike_tip);
    like_response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, liked, "Like"));
    dislike_response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, disliked, "Dislike")
    });

    ui.painter()
        .rect_filled(rect, CornerRadius::same(16), PALETTE.surface);
    let lit = |half: Rect, left: bool| {
        let radius = if left {
            CornerRadius {
                nw: 16,
                sw: 16,
                ne: 0,
                se: 0,
            }
        } else {
            CornerRadius {
                nw: 0,
                sw: 0,
                ne: 16,
                se: 16,
            }
        };
        ui.painter().rect_filled(half, radius, PALETTE.surface);
    };
    if like_response.hovered() {
        lit(like, true);
    }
    if dislike_response.hovered() {
        lit(dislike, false);
    }
    ui.painter().vline(
        rect.center().x,
        (rect.top() + 6.0)..=(rect.bottom() - 6.0),
        egui::Stroke::new(1.0, PALETTE.surface_hover),
    );
    let up = if liked {
        Icon::ThumbsUpFilled
    } else {
        Icon::ThumbsUp
    };
    let down = if disliked {
        Icon::ThumbsDownFilled
    } else {
        Icon::ThumbsDown
    };
    theme::paint_icon(ui, up, like, 18.0, PALETTE.text);
    theme::paint_icon(ui, down, dislike, 18.0, PALETTE.text);

    if like_response.clicked() {
        let next = if liked {
            LikeState::Neutral
        } else {
            LikeState::Liked
        };
        app.act(Action::Rate(track.video_id.clone(), next));
    }
    if dislike_response.clicked() {
        let next = if disliked {
            LikeState::Neutral
        } else {
            LikeState::Disliked
        };
        app.act(Action::Rate(track.video_id.clone(), next));
    }
}

/// The volume button (click to mute), and the bar (up to `bar_width`
/// wide) that opens beside it while the pointer is over either.
fn volume(app: &App, ui: &mut egui::Ui, bar_width: f32) {
    let id = ui.id().with("volume");
    let was_open: bool = ui.data(|d| d.get_temp(id)).unwrap_or(false);
    let open = ui
        .ctx()
        .animate_bool_with_time(id.with("open"), was_open, 0.15);

    let mut volume = app.settings.volume;
    let muted = volume <= 0.001;
    let (icon, name) = if muted {
        (Icon::Muted, "Unmute")
    } else {
        (Icon::Loud, "Mute")
    };
    let button = theme::round_button(ui, icon, 32.0, 18.0, Round::Tonal, PALETTE.text, name);
    if button.clicked() {
        app.act(Action::ToggleMute);
    }
    let mut region = button.rect;
    let mut dragging = false;
    if open > 0.0 {
        let (rect, mut response) =
            ui.allocate_exact_size(vec2(bar_width * open, 32.0), Sense::click_and_drag());
        region = region.union(rect);
        dragging = response.dragged();
        let rail = Rect::from_center_size(rect.center(), vec2((rect.width() - 12.0).max(1.0), 4.0));
        if let Some(pointer) = response.interact_pointer_pos()
            && (response.is_pointer_button_down_on() || response.clicked())
        {
            let value = ((pointer.x - rail.left()) / rail.width()).clamp(0.0, 1.0);
            if value != volume {
                volume = value;
                response.mark_changed();
            }
        }
        let shown = volume;
        response.widget_info(|| egui::WidgetInfo::slider(true, f64::from(shown), "Volume"));
        if rect.width() > 24.0 {
            let x = rail.left() + rail.width() * volume;
            ui.painter()
                .rect_filled(rail, CornerRadius::same(2), PALETTE.faint);
            let filled = Rect::from_min_max(rail.min, pos2(x, rail.bottom()));
            ui.painter()
                .rect_filled(filled, CornerRadius::same(2), PALETTE.text);
            ui.painter()
                .circle_filled(pos2(x, rail.center().y), 6.0, PALETTE.text);
        }
        if response.changed() {
            app.act(Action::SetVolume(volume));
        }
    }
    let over = ui.rect_contains_pointer(region.expand(6.0)) || dragging;
    if over != was_open {
        ui.data_mut(|d| d.insert_temp(id, over));
        ui.ctx().request_repaint();
    }
    // Scrolling over it turns the volume, too.
    if over {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let next = (volume + scroll * 0.002).clamp(0.0, 1.0);
            app.act(Action::SetVolume(next));
        }
    }
}
