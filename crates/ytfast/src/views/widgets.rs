//! The pieces pages are made of: a cover picture, a song row, a card.

use egui::{Align, Color32, CornerRadius, Layout, Rect, Sense, UiBuilder, Vec2, pos2, vec2};
use ytfast_core::read::{Card, Target, Thumb, Track};

use crate::app::{Action, App};
use crate::theme::{self, Icon, PALETTE};

/// Draws a cover in `rect`: the picture when it has arrived, a quiet
/// placeholder until then. Round for artists.
pub fn cover(app: &App, ui: &egui::Ui, rect: Rect, thumb: Option<&Thumb>, round: bool) {
    let radius = if round {
        CornerRadius::same(255)
    } else {
        CornerRadius::same((rect.width() * 0.06).clamp(4.0, 12.0) as u8)
    };
    let pixels = (rect.width() * ui.ctx().pixels_per_point()).ceil() as u32;
    // Ask for a size the server makes, not every size drawn.
    let size = [60, 120, 226, 360, 544]
        .into_iter()
        .find(|s| *s >= pixels)
        .unwrap_or(544);
    let texture = thumb.and_then(|t| app.picture(&t.sized(size)));
    match texture {
        Some(texture) => {
            egui::Image::new((texture.id(), rect.size()))
                .corner_radius(radius)
                .paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, radius, PALETTE.surface);
            let icon = rect.width() * 0.35;
            theme::paint_icon(ui, Icon::Music, rect, icon, PALETTE.dim);
        }
    }
}

/// How long the pointer rests on a song before it is found ahead of time.
const WARM_AFTER: f32 = 0.35;

/// One song in a list. `number` (on an album's page) is shown instead of
/// the cover; `playing` marks the song playing now.
pub fn track_row(
    app: &App,
    ui: &mut egui::Ui,
    track: &Track,
    number: Option<usize>,
    playing: bool,
    on_click: impl FnOnce() -> Action,
) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, theme::ROW_HEIGHT), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let lit = ui
        .ctx()
        .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    if lit > 0.0 {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(8),
            PALETTE.surface.gamma_multiply(lit),
        );
    }
    if response.hovered() {
        // A song the pointer rests on is found ahead of time, so a click
        // starts it at once.
        let resting = ui.input(|i| i.pointer.time_since_last_movement());
        if resting >= WARM_AFTER {
            app.warm(&track.video_id);
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f32(
                    WARM_AFTER - resting + 0.05,
                ));
        }
    }
    let art = Rect::from_min_size(
        pos2(rect.left() + 8.0, rect.center().y - 20.0),
        Vec2::splat(40.0),
    );
    match number {
        Some(number) => {
            if playing {
                theme::paint_icon(ui, Icon::Volume, art, 18.0, app.accent());
            } else if response.hovered() {
                theme::paint_icon(ui, Icon::Play, art, 16.0, PALETTE.text);
            } else {
                ui.painter().text(
                    art.center(),
                    egui::Align2::CENTER_CENTER,
                    number.to_string(),
                    theme::regular(14.0),
                    PALETTE.secondary,
                );
            }
        }
        None => {
            cover(app, ui, art, track.thumbnail.as_ref(), false);
            if playing {
                ui.painter().rect_filled(
                    art,
                    CornerRadius::same(4),
                    Color32::from_black_alpha(140),
                );
                theme::paint_icon(ui, Icon::Volume, art, 18.0, PALETTE.text);
            } else if response.hovered() {
                ui.painter().rect_filled(
                    art,
                    CornerRadius::same(4),
                    Color32::from_black_alpha(120),
                );
                theme::paint_icon(ui, Icon::Play, art, 16.0, PALETTE.text);
            }
        }
    }

    let duration = track
        .duration_seconds
        .map(|s| theme::clock(f64::from(s)))
        .unwrap_or_default();
    let text_left = art.right() + 14.0;
    let text_right = rect.right() - 72.0;
    let album_width = ((text_right - text_left) * 0.35).clamp(0.0, 280.0);
    let show_album = rect.width() > 640.0 && track.album.is_some();
    let main_right = if show_album {
        text_right - album_width - 16.0
    } else {
        text_right
    };

    let mut text_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(text_left, rect.top() + 9.0),
                pos2(main_right, rect.bottom()),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    text_ui.spacing_mut().item_spacing.y = 2.0;
    let title_color = if playing { app.accent() } else { PALETTE.text };
    theme::label(&mut text_ui, &track.title, theme::medium(14.5), title_color);
    theme::label(
        &mut text_ui,
        &track.artists,
        theme::regular(13.0),
        PALETTE.secondary,
    );

    if show_album && let Some(album) = &track.album {
        let mut album_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    pos2(text_right - album_width, rect.top()),
                    pos2(text_right, rect.bottom()),
                ))
                .layout(Layout::left_to_right(Align::Center)),
        );
        theme::label(
            &mut album_ui,
            album,
            theme::regular(13.0),
            PALETTE.secondary,
        );
    }
    ui.painter().text(
        pos2(rect.right() - 16.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        duration,
        theme::regular(13.0),
        PALETTE.secondary,
    );
    if response.clicked() {
        app.act(on_click());
    }
    response.context_menu(|ui| song_menu(app, ui, track));
}

/// What a right-click on a song offers.
pub fn song_menu(app: &App, ui: &mut egui::Ui, track: &Track) {
    ui.set_min_width(180.0);
    if ui.button("Play next").clicked() {
        app.act(Action::PlayNext(track.clone()));
        ui.close();
    }
    if ui.button("Add to queue").clicked() {
        app.act(Action::AddToQueue(track.clone()));
        ui.close();
    }
    if ui.button("Start radio").clicked() {
        let radio = Target::Watch {
            video_id: Some(track.video_id.clone()),
            playlist_id: None,
        };
        app.act(Action::Play(radio, Some(track.clone())));
        ui.close();
    }
}

/// A card: a cover with a title and subtitle under it. Clicking opens it;
/// the round button over the cover (when hovered) plays it.
pub fn card(app: &App, ui: &mut egui::Ui, card: &Card) {
    let size = theme::CARD_SIZE;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size + 54.0), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let art = Rect::from_min_size(rect.min, Vec2::splat(size));
    cover(app, ui, art, card.thumbnail.as_ref(), card.round);
    // Hovered by where the pointer is, not by which widget is on top: the
    // play button over the cover must not make the card think the pointer
    // left (that made it flicker).
    let hovered = ui.rect_contains_pointer(rect);
    let lit = ui
        .ctx()
        .animate_bool_with_time(response.id.with("hover"), hovered, 0.15);
    let mut play_clicked = false;
    if lit > 0.0 && card.play.is_some() {
        let radius = if card.round {
            CornerRadius::same(255)
        } else {
            CornerRadius::same(10)
        };
        ui.painter()
            .rect_filled(art, radius, Color32::from_black_alpha((70.0 * lit) as u8));
        let button =
            Rect::from_center_size(art.right_bottom() - vec2(30.0, 30.0), Vec2::splat(44.0));
        let over_button = hovered
            && ui
                .interact(button, response.id.with("play"), Sense::click())
                .clicked();
        let pointer_on_button = ui.rect_contains_pointer(button);
        let scale = 0.85 + 0.15 * lit;
        let fill = if pointer_on_button {
            app.accent()
        } else {
            Color32::from_white_alpha((235.0 * lit) as u8)
        };
        ui.painter()
            .circle_filled(button.center(), 22.0 * scale, fill.gamma_multiply(lit));
        theme::paint_icon(
            ui,
            Icon::Play,
            button,
            18.0 * scale,
            Color32::from_black_alpha((255.0 * lit) as u8),
        );
        play_clicked = over_button;
    }
    let mut text_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(rect.left(), art.bottom() + 8.0),
                rect.max,
            ))
            .layout(Layout::top_down(if card.round {
                Align::Center
            } else {
                Align::Min
            })),
    );
    text_ui.spacing_mut().item_spacing.y = 2.0;
    theme::label(&mut text_ui, &card.title, theme::medium(14.5), PALETTE.text);
    if !card.subtitle.is_empty() {
        theme::label(
            &mut text_ui,
            &card.subtitle,
            theme::regular(13.0),
            PALETTE.secondary,
        );
    }
    if play_clicked {
        if let Some(target) = card.play.clone() {
            app.act(Action::Play(target, None));
        }
    } else if response.clicked()
        && let Some(target) = card.open.clone().or_else(|| card.play.clone())
    {
        app.act(Action::Open(target, None));
    }
}

/// A card as a row (an album or artist in search results): a small cover,
/// the title and the subtitle. Clicking opens it.
pub fn card_row(app: &App, ui: &mut egui::Ui, card: &Card) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, theme::ROW_HEIGHT), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), PALETTE.surface);
    }
    let art = Rect::from_min_size(
        pos2(rect.left() + 8.0, rect.center().y - 20.0),
        Vec2::splat(40.0),
    );
    cover(app, ui, art, card.thumbnail.as_ref(), card.round);
    let mut text_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(art.right() + 14.0, rect.top() + 9.0),
                pos2(rect.right() - 16.0, rect.bottom()),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    text_ui.spacing_mut().item_spacing.y = 2.0;
    theme::label(&mut text_ui, &card.title, theme::medium(14.5), PALETTE.text);
    theme::label(
        &mut text_ui,
        &card.subtitle,
        theme::regular(13.0),
        PALETTE.secondary,
    );
    if response.clicked()
        && let Some(target) = card.open.clone().or_else(|| card.play.clone())
    {
        app.act(Action::Open(target, None));
    }
}

/// A mood or genre button (a card without a picture).
pub fn chip(app: &App, ui: &mut egui::Ui, card: &Card) {
    let text = egui::RichText::new(&card.title)
        .font(theme::medium(14.0))
        .color(PALETTE.text);
    let button = egui::Button::new(text)
        .fill(PALETTE.surface)
        .corner_radius(CornerRadius::same(6))
        .min_size(vec2(160.0, 44.0));
    if ui.add(button).clicked()
        && let Some(target) = card.open.clone()
    {
        app.act(Action::Open(target, None));
    }
}

/// Like and dislike, as in YouTube Music's player.
pub fn like_buttons(app: &App, ui: &mut egui::Ui, track: &Track) {
    use crate::app::LikeState;
    let state = app
        .likes
        .get(&track.video_id)
        .copied()
        .unwrap_or(LikeState::Neutral);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (icon, tip) = if state == LikeState::Disliked {
            (Icon::ThumbsDownFilled, "Remove dislike")
        } else {
            (Icon::ThumbsDown, "Dislike")
        };
        if theme::icon_button(ui, icon, 20.0, PALETTE.secondary, tip).clicked() {
            let next = if state == LikeState::Disliked {
                LikeState::Neutral
            } else {
                LikeState::Disliked
            };
            app.act(Action::Rate(track.video_id.clone(), next));
        }
        let (icon, tip, color) = if state == LikeState::Liked {
            (
                Icon::ThumbsUpFilled,
                "Remove from Liked Music",
                PALETTE.text,
            )
        } else {
            (Icon::ThumbsUp, "Like", PALETTE.secondary)
        };
        if theme::icon_button(ui, icon, 20.0, color, tip).clicked() {
            let next = if state == LikeState::Liked {
                LikeState::Neutral
            } else {
                LikeState::Liked
            };
            app.act(Action::Rate(track.video_id.clone(), next));
        }
    });
}
