//! The video mode on the player page, as YouTube Music's Song and Video
//! switch: the switch above the cover, and the song's music video in the
//! cover's place, 16:9, as large as the cover's room allows. The video
//! follows the music ([`App::clock`]): the window is drawn again when its
//! next picture is due, and only while the music plays.

use std::time::Duration;

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Vec2, pos2, vec2};

use crate::app::{Action, App, PlayState, VideoState};
use crate::theme::{self, PALETTE};

/// The switch's height, and the room it takes above the picture with the
/// space under it.
pub const SWITCH_HEIGHT: f32 = 36.0;
pub const SWITCH_ROOM: f32 = SWITCH_HEIGHT + 16.0;

/// Each half of the switch, Song and Video.
const HALF: f32 = 88.0;

/// Whether the player page shows the video's place instead of the cover:
/// in the video mode, for a song with a video (or not yet known to have
/// none), while it gets ready or plays.
pub fn shown(app: &App) -> bool {
    let video = matches!(
        app.video.as_ref().map(|v| &v.state),
        Some(
            VideoState::Finding
                | VideoState::Coming
                | VideoState::Loading
                | VideoState::Playing { .. }
                | VideoState::Demo
        )
    );
    video
        && matches!(
            app.playback.state,
            PlayState::Preparing | PlayState::Playing
        )
}

/// The video's size in `room` under the switch: 16:9, at most `widest`
/// wide and `tallest` high.
pub fn size_in(room: Rect, widest: f32, tallest: f32) -> Vec2 {
    let width = room
        .width()
        .min((room.height() - SWITCH_ROOM).max(0.0) * 16.0 / 9.0)
        .min(widest)
        .min(tallest * 16.0 / 9.0)
        .max(0.0);
    vec2(width, width * 9.0 / 16.0)
}

/// The switch's row and, under it, a picture of `size`, together centred
/// in `room`.
pub fn stack(room: Rect, size: Vec2) -> (Rect, Rect) {
    let top = room.center().y - (SWITCH_ROOM + size.y) / 2.0;
    let row = Rect::from_min_size(pos2(room.left(), top), vec2(room.width(), SWITCH_HEIGHT));
    let picture = Rect::from_min_size(
        pos2(room.center().x - size.x / 2.0, top + SWITCH_ROOM),
        size,
    );
    (row, picture)
}

/// The switch's row over `picture` (the cover or the video).
pub fn row_above(picture: Rect) -> Rect {
    Rect::from_min_size(
        pos2(picture.left(), picture.top() - SWITCH_ROOM),
        vec2(picture.width(), SWITCH_HEIGHT),
    )
}

/// The Song and Video switch, centred in `row`: a capsule 36 high of
/// white@0.10 (Premium: clear glass) holding two halves 88 wide, the
/// chosen one white@0.20 (Premium: glass with a rim of light) with white
/// words (14, medium), the other's words dimmer. Each half is a button
/// named for screen readers. A song without a video has Video greyed out
/// and not to be pressed (the video mode stays on for the songs after it,
/// which play as their videos).
pub fn switch(app: &App, ui: &egui::Ui, row: Rect) {
    let premium = theme::premium();
    let bar = Rect::from_center_size(row.center(), vec2(2.0 * HALF + 8.0, SWITCH_HEIGHT));
    if premium {
        super::now_playing::glass(ui.painter(), bar, 0.05, 0.0);
    } else {
        ui.painter()
            .rect_filled(bar, CornerRadius::same(18), PALETTE.surface);
    }
    let offered = app.has_video() != Some(false);
    for (index, (name, video)) in [("Song", false), ("Video", true)].into_iter().enumerate() {
        let enabled = offered || !video;
        let chosen = (app.video_mode && offered) == video;
        let rect = Rect::from_min_size(
            pos2(bar.left() + 4.0 + index as f32 * HALF, bar.top() + 4.0),
            vec2(HALF, SWITCH_HEIGHT - 8.0),
        );
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui.interact(rect, ui.id().with(("song or video", name)), sense);
        theme::pointing_or_not(ui, &response, enabled);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, chosen, name)
        });
        let corners = CornerRadius::same(14);
        let white = Color32::from_white_alpha;
        match (chosen, response.hovered() && enabled, premium) {
            (true, _, true) => super::now_playing::glass(ui.painter(), rect, 0.06, 0.32),
            (true, _, false) => {
                ui.painter().rect_filled(rect, corners, white(51));
            }
            (false, true, true) => super::now_playing::glass(ui.painter(), rect, 0.05, 0.0),
            (false, true, false) => {
                ui.painter().rect_filled(rect, corners, white(20));
            }
            (false, false, _) => {}
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                corners,
                egui::Stroke::new(1.0, PALETTE.accent),
                egui::StrokeKind::Inside,
            );
        }
        let color = if chosen {
            PALETTE.text
        } else if enabled {
            PALETTE.secondary
        } else {
            PALETTE.disabled
        };
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            name,
            theme::medium(14.0),
            color,
        );
        if enabled && response.clicked() && !chosen {
            app.act(Action::VideoMode(video));
        }
    }
}

/// The video in `rect`, on black (a spinner while it comes), the window
/// drawn again when its next picture is due while the music plays.
pub fn paint(app: &App, ui: &egui::Ui, rect: Rect, corners: CornerRadius) {
    let Some(show) = &app.video else {
        return;
    };
    ui.painter().rect_filled(rect, corners, Color32::BLACK);
    let running = app.audio_status.running && app.audio_status.entry == Some(show.entry);
    let clock = app.clock();
    match &show.state {
        VideoState::Playing { video, shown } => {
            app.video_drawn.set(true);
            let now = video.at(clock);
            let mut kept = shown.borrow_mut();
            if let Some(picture) = &now.picture
                && kept
                    .as_ref()
                    .is_none_or(|(at, _)| at.to_bits() != picture.at.to_bits())
            {
                // Opaque, so its colours are already as egui keeps them.
                let image = egui::ColorImage::from_rgba_premultiplied(
                    [picture.width, picture.height],
                    &picture.rgba,
                );
                match kept.as_mut() {
                    Some((at, texture)) => {
                        texture.set(image, egui::TextureOptions::LINEAR);
                        *at = picture.at;
                    }
                    None => {
                        let texture =
                            ui.ctx()
                                .load_texture("video", image, egui::TextureOptions::LINEAR);
                        *kept = Some((picture.at, texture));
                    }
                }
            }
            match kept.as_ref() {
                Some((_, texture)) => {
                    let size = texture.size_vec2();
                    let fitted = fit(rect, size.x / size.y.max(1.0));
                    let corners = if fitted.size() == rect.size() {
                        corners
                    } else {
                        CornerRadius::ZERO
                    };
                    egui::Image::new((texture.id(), fitted.size()))
                        .corner_radius(corners)
                        .paint_at(ui, fitted);
                }
                None if video.failure().is_some() => {
                    words_over(ui, rect, "This video could not be shown.");
                }
                None => spinner(ui, rect),
            }
            if running {
                let wait = now
                    .next
                    .map_or(0.04, |next| (next - clock).clamp(0.004, 0.1));
                ui.ctx()
                    .request_repaint_after(Duration::from_secs_f64(wait));
            }
        }
        VideoState::Demo => {
            stand_in(ui, rect, clock);
            if running {
                ui.ctx()
                    .request_repaint_after(Duration::from_secs_f64(1.0 / 30.0));
            }
        }
        _ => spinner(ui, rect),
    }
}

/// Over the cover, in the video mode, when its video could not be shown:
/// why. (A song without a video says so by its greyed-out switch.)
pub fn note(app: &App, ui: &egui::Ui, art: Rect) {
    let Some(VideoState::Failed(message)) = app.video.as_ref().map(|v| &v.state) else {
        return;
    };
    if app.video_mode {
        words_over(ui, art, message);
    }
}

/// `text` in a dark capsule near the bottom of `rect`.
fn words_over(ui: &egui::Ui, rect: Rect, text: &str) {
    let galley = ui.painter().layout(
        text.to_string(),
        theme::regular(14.0),
        Color32::WHITE,
        (rect.width() - 64.0).max(80.0),
    );
    let size = galley.size() + vec2(32.0, 16.0);
    let place = Rect::from_min_size(
        pos2(
            rect.center().x - size.x / 2.0,
            rect.bottom() - 16.0 - size.y,
        ),
        size,
    );
    ui.painter().rect_filled(
        place,
        CornerRadius::same(18),
        Color32::from_black_alpha(178),
    );
    ui.painter()
        .galley(place.min + vec2(16.0, 8.0), galley, Color32::WHITE);
}

fn spinner(ui: &egui::Ui, rect: Rect) {
    egui::Spinner::new()
        .size(28.0)
        .color(PALETTE.secondary)
        .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(28.0)));
}

/// The largest place of `aspect` (width over height) inside `rect`,
/// centred: a video not 16:9 shows whole, with black beside it.
fn fit(rect: Rect, aspect: f32) -> Rect {
    let width = rect.width().min(rect.height() * aspect);
    Rect::from_center_size(rect.center(), vec2(width, width / aspect.max(0.01)))
}

/// The demo's stand-in for a video (it has none): colours drifting with
/// the music, and where it is.
fn stand_in(ui: &egui::Ui, rect: Rect, clock: f64) {
    use egui::ecolor::Hsva;
    let hue = (clock * 0.02).fract() as f32;
    let color =
        |shift: f32, value: f32| Color32::from(Hsva::new((hue + shift).fract(), 0.55, value, 1.0));
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), color(0.0, 0.55));
    mesh.colored_vertex(rect.right_top(), color(0.25, 0.4));
    mesh.colored_vertex(rect.right_bottom(), color(0.5, 0.25));
    mesh.colored_vertex(rect.left_bottom(), color(0.75, 0.4));
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(mesh);
    ui.painter().text(
        rect.left_bottom() + vec2(16.0, -16.0),
        Align2::LEFT_BOTTOM,
        format!("Demo video, {:.1} s", clock),
        theme::regular(14.0),
        Color32::from_white_alpha(200),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_video_is_16_by_9_under_the_switch_and_inside_its_room() {
        for (width, height) in [
            (692.0, 620.0),
            (566.0, 632.0),
            (1200.0, 500.0),
            (300.0, 900.0),
        ] {
            let room = Rect::from_min_size(pos2(56.0, 96.0), vec2(width, height));
            let size = size_in(room, 1280.0, 900.0);
            assert!((size.x / size.y - 16.0 / 9.0).abs() < 0.001);
            let (row, picture) = stack(room, size);
            assert!(room.contains_rect(row), "{width} by {height}");
            assert!(
                room.contains_rect(picture.shrink(0.01)),
                "{width} by {height}"
            );
            assert!((picture.top() - row.bottom() - 16.0).abs() < 0.01);
            assert!((picture.center().x - room.center().x).abs() < 0.01);
        }
    }

    #[test]
    fn a_video_not_16_by_9_shows_whole() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 360.0));
        // Upright (a short): as high as the place, centred.
        let upright = fit(rect, 9.0 / 16.0);
        assert!((upright.height() - 360.0).abs() < 0.01);
        assert!((upright.center().x - 320.0).abs() < 0.01);
        // Wider than 16:9: as wide as the place.
        let wide = fit(rect, 2.39);
        assert!((wide.width() - 640.0).abs() < 0.01);
        assert!(wide.height() < 360.0);
    }
}
