//! What the window draws, laid out as YouTube Music lays its page out: a
//! bar across the top, the menu on the left, the page, and the player bar
//! across the bottom once something plays. Each view reads [`App`] and
//! pushes actions.

mod backdrop;
mod dialogs;
mod now_playing;
mod page;
mod player_bar;
mod queue_panel;
mod selection_bar;
mod settings;
mod sidebar;
mod signin;
mod topbar;
mod widgets;
mod window_frame;

use egui::{Frame, Margin};

use crate::app::{App, Auth};
use crate::theme::{self, PALETTE};

/// The search box's ID, to focus it from a shortcut.
pub const SEARCH_BOX: &str = "ytfast-search-box";

pub fn show(app: &App, ui: &mut egui::Ui) {
    let (album_slot, wash_slot) = backdrop::paint(ui, ui.max_rect());
    app.backdrop_slot.set(Some(album_slot));
    app.page_backdrop.set(false);
    // Over everything: the edges that resize YTFast's own window frame.
    window_frame::edges(ui.ctx());
    if !matches!(app.auth, Auth::SignedIn { .. }) {
        // Without the top bar, a strip of its own moves the window and
        // holds its buttons.
        if window_frame::OWN_FRAME {
            egui::Panel::top("title-strip")
                .exact_size(32.0)
                .resizable(false)
                .show_separator_line(false)
                .frame(Frame::new())
                .show(ui, |ui| {
                    let strip = ui.max_rect();
                    window_frame::drag_area(ui, strip);
                    window_frame::buttons(ui, strip);
                });
        }
        // Signed out by YouTube while a song plays: it can still be paused.
        if app.playback.entry.is_some() {
            player_bar::show(app, ui);
        }
        egui::CentralPanel::default()
            .frame(Frame::new())
            .show(ui, |ui| signin::show(app, ui));
        return;
    }
    // Panels first, in the order they take their space: the two bars span
    // the window, the menu sits between them, and the page fills the rest.
    if app.playback.entry.is_some() {
        player_bar::show(app, ui);
    }
    topbar::show(app, ui);
    sidebar::show(app, ui);
    egui::CentralPanel::default()
        .frame(Frame::new())
        .show(ui, |ui| {
            // The player page slides up over the page, and back down, in
            // 0.3 s (`cubic-bezier(.2,0,.6,1)`, as YouTube Music's).
            let open = ui.ctx().animate_bool_with_time(
                egui::Id::new("player-page-open"),
                app.now_playing,
                0.3,
            );
            if open < 1.0 {
                page::show(app, ui);
            }
            // Premium: the playing song's colours, softly across the top
            // of a page without a background of its own, and all over
            // behind the player page; both under the menu and the bars.
            let premium = theme::premium();
            let wash = premium.then(|| app.listening_wash(ui.ctx())).flatten();
            let screen = ui.ctx().content_rect();
            let mut behind = Vec::new();
            if premium && open < 1.0 && !app.page_backdrop.get() {
                behind.push(backdrop::ambient(screen, wash, 1.0 - open));
            }
            if open > 0.0 {
                let area = ui.max_rect();
                let down = area.height() * (1.0 - theme::bezier(0.2, 0.0, 0.6, 1.0, open));
                let rect = area.translate(egui::vec2(0.0, down));
                let mut player = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                player.set_clip_rect(area);
                // Premium: the playing song's colours behind the whole
                // window (the menu and the bars let them through), and
                // rising with the page over what it covers.
                if let Some(texture) = wash {
                    behind.push(backdrop::listening(screen, texture, open));
                    let rising = screen.translate(egui::vec2(0.0, down));
                    player
                        .painter()
                        .add(backdrop::listening(rising, texture, 1.0));
                } else {
                    player.painter().rect_filled(rect, 0.0, PALETTE.window);
                }
                now_playing::show(app, &mut player);
            }
            if !behind.is_empty() {
                ui.ctx()
                    .layer_painter(egui::LayerId::background())
                    .set(wash_slot, egui::Shape::Vec(behind));
            }
        });
    notice(app, ui);
    selection_bar::show(app, ui);
    dialogs::show(app, ui);
}

/// A short message ("Added to the queue") at the bottom left, just above
/// the player bar, as YouTube Music shows its own.
fn notice(app: &App, ui: &egui::Ui) {
    let Some((text, at)) = &app.notice else {
        return;
    };
    // In over 0.3 s (rising 100 as it fades in), 3 s in all, out the same
    // way; drawn again only while it moves.
    let elapsed = at.elapsed().as_secs_f32();
    let coming = (elapsed / 0.3).min(1.0);
    let going = ((elapsed - 3.0) / 0.3).clamp(0.0, 1.0);
    if elapsed < 0.3 || elapsed > 3.0 {
        ui.ctx().request_repaint();
    } else {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f32(3.0 - elapsed));
    }
    let shown = theme::bezier(0.25, 0.1, 0.25, 1.0, coming) * (1.0 - going);
    if shown <= 0.0 {
        return;
    }
    // On the player bar's top edge (12 above the window's foot without
    // one), 12 from the left.
    let lift = if app.playback.entry.is_some() {
        theme::player_bar_height()
    } else {
        12.0
    };
    let drop = 100.0 * (1.0 - shown);
    egui::Area::new(egui::Id::new("notice"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(12.0, -lift + drop))
        .interactable(false)
        .show(ui.ctx(), |ui| {
            ui.set_opacity(shown);
            // YouTube Music's toast: `#f1f1f1`, words `#030303` 14,
            // padding 16 24, r 8, at least 288 wide and as wide as its
            // words (`tp-yt-paper-toast` has no greater limit than the
            // window, 12 from each side).
            let widest = (ui.ctx().content_rect().width() - 24.0 - 48.0).max(240.0);
            let words = theme::fit(ui, text, theme::regular(14.0), PALETTE.window, widest, 3);
            let width = words.size().x.max(240.0);
            Frame::new()
                .fill(PALETTE.button)
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(Margin::symmetric(24, 16))
                .shadow(egui::Shadow {
                    offset: [0, 2],
                    blur: 5,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(66),
                })
                .show(ui, |ui| {
                    ui.set_width(width);
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(width, words.size().y.max(16.8)),
                        egui::Sense::hover(),
                    );
                    ui.painter().galley(rect.min, words, PALETTE.window);
                });
        });
}
