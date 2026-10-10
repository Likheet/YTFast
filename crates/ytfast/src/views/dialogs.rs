//! Small windows that ask one thing: a new playlist's name, a new name,
//! or "really delete?".

use crate::app::{Action, App, Dialog};
use ytfast_core::library::Privacy;

use crate::backend::Edit;
use crate::theme::{self, Icon, PALETTE};

pub fn show(app: &App, ui: &egui::Ui) {
    let mut dialog = app.dialog.borrow_mut();
    let Some(current) = dialog.as_mut() else {
        return;
    };
    let mut close = false;
    let window = ui.ctx().content_rect().width();
    // The first field takes the keyboard once, as the dialog opens.
    let fresh = app.dialog_fresh.replace(false);
    let premium = theme::premium();
    let dynamic = theme::dynamic();
    let frame = match current {
        // Dynamic Background: glass, corners 24, the theme's shadow.
        _ if dynamic => crate::dynamic::dialog_frame(),
        // Premium: one frame for every dialog, corners 16.
        _ if premium => egui::Frame::new()
            .fill(PALETTE.panel)
            .stroke(egui::Stroke::new(1.0, PALETTE.outline))
            .corner_radius(egui::CornerRadius::same(16))
            .shadow(SHADOW),
        // `ytmusic-add-to-playlist-renderer`.
        Dialog::SaveToPlaylist { .. } => egui::Frame::new()
            .fill(PALETTE.panel)
            .stroke(egui::Stroke::new(1.0, PALETTE.outline))
            .corner_radius(egui::CornerRadius::same(2)),
        // A confirmation (`yt-confirm-dialog-renderer`): `#282828`, an
        // outline of `#4e4e4e`.
        Dialog::Delete { .. } => egui::Frame::new()
            .fill(egui::Color32::from_rgb(0x28, 0x28, 0x28))
            .stroke(egui::Stroke::new(
                1.0,
                egui::Color32::from_rgb(0x4e, 0x4e, 0x4e),
            ))
            .corner_radius(egui::CornerRadius::same(2))
            .shadow(SHADOW),
        // YTFast's own question before closing: rounder, as it is no
        // YouTube Music dialog.
        Dialog::ConfirmClose { .. } => egui::Frame::new()
            .fill(PALETTE.panel)
            .stroke(egui::Stroke::new(1.0, PALETTE.outline))
            .corner_radius(egui::CornerRadius::same(12))
            .shadow(SHADOW),
        // `ytmusic-dialog` around a form: `#212121`, a white@0.10 border,
        // r 3; its parts set their own padding.
        _ => egui::Frame::new()
            .fill(PALETTE.panel)
            .stroke(egui::Stroke::new(1.0, PALETTE.outline))
            .corner_radius(egui::CornerRadius::same(3))
            .shadow(SHADOW),
    };
    let modal = egui::Modal::new(egui::Id::new("dialog"))
        .frame(frame)
        // YouTube Music dims the window behind a dialog by 30% (measured
        // signed in).
        .backdrop_color(egui::Color32::from_black_alpha(if premium { 140 } else { 77 }))
        .show(ui.ctx(), |ui| {
            if dynamic {
                crate::dynamic::glass_behind(ui, crate::dynamic::RADIUS_PANEL_LG);
            }
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            match current {
                Dialog::SaveToPlaylist { video_ids } => {
                    close = save_to_playlist(app, ui, video_ids);
                }
                Dialog::NewPlaylist {
                    name,
                    description,
                    privacy,
                    songs,
                } => {
                    let done = form(
                        ui,
                        (window, fresh),
                        "New playlist",
                        (name, description, privacy),
                        "Create",
                    );
                    if done == Some(true) && !name.trim().is_empty() {
                        app.act(Action::Edit(Edit::CreatePlaylist {
                            title: name.trim().to_string(),
                            description: description.trim().to_string(),
                            privacy: *privacy,
                            video_ids: songs.clone(),
                        }));
                    }
                    close = done.is_some();
                }
                Dialog::Rename {
                    playlist_id,
                    name,
                    description,
                    privacy,
                } => {
                    let done = form(
                        ui,
                        (window, fresh),
                        "Edit playlist",
                        (name, description, privacy),
                        "Save",
                    );
                    if done == Some(true) && !name.trim().is_empty() {
                        app.act(Action::RenamePlaylist {
                            playlist_id: playlist_id.clone(),
                            name: name.trim().to_string(),
                            description: description.trim().to_string(),
                            privacy: *privacy,
                        });
                    }
                    close = done.is_some();
                }
                Dialog::Delete { playlist_id, title } => {
                    let text = format!(
                        "\u{201c}{title}\u{201d} will be deleted from your account. This cannot be undone."
                    );
                    match confirm(ui, "Delete this playlist?", &text, "Delete") {
                        Some(true) => {
                            app.act(Action::DeletePlaylist(playlist_id.clone()));
                            close = true;
                        }
                        Some(false) => close = true,
                        None => {}
                    }
                }
                Dialog::ConfirmClose { dont_ask } => match close_question(app, ui, dont_ask) {
                    Some(true) => {
                        app.act(Action::ConfirmClose {
                            dont_ask: *dont_ask,
                        });
                        close = true;
                    }
                    Some(false) => close = true,
                    None => {}
                },
                Dialog::Shortcuts => close = shortcuts(ui, window),
                Dialog::Update => close = update_window(app, ui),
                // The whole description (its look is not measured yet).
                Dialog::Description { title, text } => {
                    ui.set_width(dialog_width(window));
                    egui::Frame::new()
                        .inner_margin(egui::Margin::same(24))
                        .show(ui, |ui| {
                            theme::label(ui, title, theme::bold(24.0), PALETTE.text);
                            ui.add_space(16.0);
                            egui::ScrollArea::vertical()
                                .max_height(ui.ctx().content_rect().height() * 0.6)
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(text.as_str())
                                            .font(theme::regular(14.0))
                                            .color(PALETTE.secondary)
                                            .line_height(Some(19.6)),
                                    );
                                });
                            ui.add_space(16.0);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if theme::pill(ui, None, "Close", theme::Pill::Filled)
                                        .clicked()
                                    {
                                        close = true;
                                    }
                                },
                            );
                        });
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
    if modal.should_close() || close {
        *dialog = None;
    }
}

/// The keyboard's shortcuts, as YouTube Music lists them ("?"), and
/// YTFast's own. True when closed.
fn shortcuts(ui: &mut egui::Ui, window: f32) -> bool {
    const KEYS: [(&str, &str); 20] = [
        ("Play or pause", "Space or ;"),
        ("Next song", "j or Shift+N"),
        ("Previous song", "k or Shift+P"),
        ("Forward 10 seconds", "l, → or Shift+→"),
        ("Back 10 seconds", "h, ← or Shift+←"),
        ("Forward 1 second", "Shift+L or Ctrl+Shift+→"),
        ("Back 1 second", "Shift+H or Ctrl+Shift+←"),
        ("Shuffle on or off", "s"),
        ("Repeat", "r"),
        ("Volume up or down", "= or ↑ / - or ↓"),
        ("Mute", "m"),
        ("Open or close the player page", "q (Esc closes)"),
        ("Full screen on or off", "f (Esc leaves)"),
        ("Like or dislike the song playing", "+ / _"),
        ("Home", "g then h"),
        ("Explore", "g then e"),
        ("Library", "g then l"),
        ("Settings", "g then ,"),
        ("Search", "/ or Ctrl+F"),
        ("This list", "?"),
    ];
    let mut close = false;
    ui.set_width(dialog_width(window));
    egui::Frame::new()
        .inner_margin(egui::Margin::same(24))
        .show(ui, |ui| {
            theme::label(ui, "Keyboard shortcuts", theme::bold(24.0), PALETTE.text);
            ui.add_space(16.0);
            egui::ScrollArea::vertical()
                .max_height(ui.ctx().content_rect().height() * 0.6)
                .show(ui, |ui| {
                    for (does, keys) in KEYS {
                        let (row, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 32.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().text(
                            egui::pos2(row.left(), row.center().y),
                            egui::Align2::LEFT_CENTER,
                            does,
                            theme::regular(14.0),
                            PALETTE.text,
                        );
                        ui.painter().text(
                            egui::pos2(row.right(), row.center().y),
                            egui::Align2::RIGHT_CENTER,
                            keys,
                            theme::medium(14.0),
                            PALETTE.dim,
                        );
                    }
                });
            ui.add_space(16.0);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 36.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if theme::pill(ui, None, "Close", theme::Pill::Filled).clicked() {
                        close = true;
                    }
                },
            );
        });
    close
}

/// A dialog's shadow: YouTube Music's three layers
/// (`0 16px 24px 2px`, `0 6px 30px 5px`, `0 8px 10px -5px`), as one.
const SHADOW: egui::Shadow = egui::Shadow {
    offset: [0, 12],
    blur: 28,
    spread: 3,
    color: egui::Color32::from_black_alpha(90),
};

/// A form dialog's width (`--ytmusic-dialog-width`): 560, 640 from a window
/// 1364 wide.
/// The update window, as Spotifast's: "Update YTFast" and a close button,
/// the versions ("0.6.0 → 0.6.1"), then what the update is doing: its
/// download with a bar and the megabytes, "Ready to install" with a word
/// that music stops on the restart and Restart to update, or why it could
/// not be downloaded and Retry. 420 wide. True when it should close.
fn update_window(app: &App, ui: &mut egui::Ui) -> bool {
    use crate::update::State;
    let mut close = false;
    let state = app
        .updates
        .as_ref()
        .map(crate::update::Updates::state)
        .unwrap_or_default();
    let found = app.update_found.clone().unwrap_or_default();
    ui.set_width(420.0_f32.min((ui.ctx().content_rect().width() - 64.0).max(240.0)));
    egui::Frame::new()
        .inner_margin(egui::Margin::same(24))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                theme::label(ui, "Update YTFast", theme::bold(20.0), PALETTE.text);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close |= theme::round_button(
                        ui,
                        Icon::Close,
                        32.0,
                        20.0,
                        theme::Round::Plain,
                        PALETTE.secondary,
                        "Close update",
                    )
                    .clicked();
                });
            });
            ui.add_space(4.0);
            theme::label(
                ui,
                &format!("{} → {found}", env!("CARGO_PKG_VERSION")),
                theme::regular(14.0),
                PALETTE.secondary,
            );
            ui.add_space(20.0);
            let mut action = None;
            match state {
                State::Downloading {
                    received, total, ..
                } => {
                    let checking = total > 0 && received >= total;
                    let words = if checking {
                        "Checking download..."
                    } else {
                        "Downloading update..."
                    };
                    theme::label(ui, words, theme::medium(14.0), PALETTE.text);
                    ui.add_space(8.0);
                    let (bar, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 6.0),
                        egui::Sense::hover(),
                    );
                    let corners = egui::CornerRadius::same(3);
                    ui.painter().rect_filled(bar, corners, PALETTE.surface);
                    let part = if total > 0 {
                        (received as f32 / total as f32).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let done = egui::Rect::from_min_size(
                        bar.min,
                        egui::vec2(bar.width() * part, bar.height()),
                    );
                    ui.painter().rect_filled(done, corners, PALETTE.switch);
                    ui.add_space(6.0);
                    if total > 0 {
                        let megabytes = |bytes: u64| bytes as f64 / 1_000_000.0;
                        theme::label(
                            ui,
                            &format!("{:.1} of {:.1} MB", megabytes(received), megabytes(total)),
                            theme::regular(12.0),
                            PALETTE.secondary,
                        );
                    }
                }
                State::Ready { .. } => {
                    theme::label(ui, "Ready to install", theme::medium(14.0), PALETTE.text);
                    ui.add_space(6.0);
                    wrapped(
                        ui,
                        "Music playing on this computer will stop when YTFast restarts.",
                    );
                    action = Some(("Restart to update", Action::RestartToUpdate));
                }
                State::Restarting => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(16.0).color(PALETTE.switch));
                        ui.add_space(8.0);
                        theme::label(
                            ui,
                            "Preparing to restart...",
                            theme::regular(14.0),
                            PALETTE.text,
                        );
                    });
                }
                State::Available {
                    note: Some(note), ..
                } => {
                    wrapped(ui, &note);
                    // A copy that cannot install it (every Mac copy): its
                    // page is a click away.
                    action = Some(("Open download page", Action::OpenDownloadPage));
                }
                State::Available { note: None, .. } => {
                    action = Some(("Download update", Action::InstallUpdate));
                }
                State::Failed(message) => {
                    wrapped(ui, &message);
                    action = Some(("Retry download", Action::InstallUpdate));
                }
                State::Idle | State::Checking | State::UpToDate => {
                    wrapped(ui, "YTFast is up to date.");
                }
            }
            if let Some((words, action)) = action {
                ui.add_space(20.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if theme::pill_button(ui, words, true).clicked() {
                        app.act(action);
                    }
                });
            }
        });
    close
}

/// Words in the quieter grey, 14, wrapped to the dialog.
fn wrapped(ui: &mut egui::Ui, words: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(words)
                .font(theme::regular(14.0))
                .color(PALETTE.secondary),
        )
        .wrap(),
    );
}

fn dialog_width(window: f32) -> f32 {
    if window >= 1364.0 { 640.0 } else { 560.0 }
}

/// A playlist's form, as YouTube Music's (`ytmusic-playlist-form`, measured
/// signed in): the title (bold 24, 20 under 1150) 24 in; 32 under it the
/// Title field, 32 under that the Description, 40 under that the Privacy;
/// at the right Cancel (words) and `main` (white), 8 apart, 16 24 24 around
/// them. Enter in the Title takes it. `Some(true)` when taken, `Some(false)`
/// when cancelled. (Its Collaborate switch is left out.)
fn form(
    ui: &mut egui::Ui,
    (window, fresh): (f32, bool),
    heading: &str,
    fields: (&mut String, &mut String, &mut Privacy),
    main: &str,
) -> Option<bool> {
    let (name, description, privacy) = fields;
    let width = dialog_width(window);
    ui.set_width(width);
    let mut done = None;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 24,
            right: 24,
            top: 24,
            bottom: 0,
        })
        .show(ui, |ui| {
            let size = if window >= 1150.0 { 24.0 } else { 20.0 };
            theme::label(ui, heading, theme::bold(size), PALETTE.text);
        });
    // Premium: boxed fields, closer together.
    let premium = theme::premium();
    let (above, between, before_privacy) = if premium {
        (24, 20.0, 20.0)
    } else {
        (32, 32.0, 40.0)
    };
    let field = if premium { premium_field } else { field };
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(24, above))
        .show(ui, |ui| {
            ui.set_width(width - 48.0);
            if field(ui, "Title", name, false, fresh) {
                done = Some(true);
            }
            ui.add_space(between);
            field(ui, "Description", description, true, false);
            ui.add_space(before_privacy);
            privacy_menu(ui, privacy);
        });
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 24,
            right: 24,
            top: 16,
            bottom: 24,
        })
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 36.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let ready = !name.trim().is_empty();
                    ui.add_enabled_ui(ready, |ui| {
                        if theme::pill(ui, None, main, theme::Pill::Filled).clicked() {
                            done = Some(true);
                        }
                    });
                    if theme::pill(ui, None, "Cancel", theme::Pill::Plain).clicked() {
                        done = Some(false);
                    }
                },
            );
        });
    if done == Some(true) && name.trim().is_empty() {
        done = None;
    }
    done
}

/// Premium's text field: its label (13) above a box of the field colour,
/// r 10, outlined in the accent while focused. True when Enter is pressed
/// in a one-line field.
fn premium_field(
    ui: &mut egui::Ui,
    label: &str,
    text: &mut String,
    lines: bool,
    first: bool,
) -> bool {
    theme::label(ui, label, theme::medium(13.0), PALETTE.secondary);
    ui.add_space(8.0);
    let width = ui.available_width();
    let rows = if lines {
        text.lines().count().clamp(3, 5)
    } else {
        1
    };
    let id = ui.id().with(("playlist-field", label));
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    let edge = if focused {
        PALETTE.accent
    } else {
        PALETTE.outline
    };
    let frame = egui::Frame::new()
        .fill(PALETTE.field)
        .stroke(egui::Stroke::new(1.0, edge))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(12, 12));
    let edit = if lines {
        egui::TextEdit::multiline(text).desired_rows(rows)
    } else {
        egui::TextEdit::singleline(text)
    };
    let response = ui.add(
        edit.id(id)
            .frame(frame)
            .font(theme::regular(15.0))
            .text_color(PALETTE.text)
            .desired_width(width),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, label));
    // Asked before anything takes the focus back.
    let enter = !lines && response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if first {
        response.request_focus();
    }
    enter
}

/// A text field as YouTube Music's (`tp-yt-paper-input`): its label 14
/// white@0.70 where the words go until there are some, then 12 above them
/// (`#aaa`, blue while focused); the words 14 on 19.6 lines, 20 down; a line
/// 42.6 down, `#606060`, blue and 2 thick while focused; a blue caret.
/// `first` takes the keyboard (as the dialog opens). True when Enter is
/// pressed in a one-line field.
fn field(ui: &mut egui::Ui, label: &str, text: &mut String, lines: bool, first: bool) -> bool {
    use egui::{Sense, pos2, vec2};
    let width = ui.available_width();
    let shown_rows = if lines {
        text.lines().count().clamp(1, 5) as f32
    } else {
        1.0
    };
    let height = 20.0 + 19.6 * shown_rows;
    let (block, _) = ui.allocate_exact_size(vec2(width, height + 3.0), Sense::hover());
    let input = egui::Rect::from_min_size(
        pos2(block.left(), block.top() + 20.0),
        vec2(width, 19.6 * shown_rows),
    );
    let mut field_ui = ui.new_child(egui::UiBuilder::new().max_rect(input));
    field_ui.visuals_mut().text_cursor.stroke.color = PALETTE.switch;
    field_ui.visuals_mut().selection.bg_fill = PALETTE.switch.gamma_multiply(0.4);
    let edit = if lines {
        egui::TextEdit::multiline(text).desired_rows(1)
    } else {
        egui::TextEdit::singleline(text)
    };
    let response = field_ui.add(
        edit.frame(egui::Frame::NONE)
            .font(theme::regular(14.0))
            .text_color(PALETTE.text)
            .margin(egui::Margin::ZERO)
            .desired_width(width),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, label));
    // Asked before anything takes the focus back.
    let enter = !lines && response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if first {
        response.request_focus();
    }
    let focused = response.has_focus();
    let risen = ui
        .ctx()
        .animate_bool_with_time(response.id.with("label"), !text.is_empty(), 0.15);
    let size = 14.0 - 2.0 * risen;
    let y = input.top() + (2.0 - 20.0) * risen;
    let color = if risen > 0.0 && focused {
        PALETTE.switch
    } else if risen > 0.0 {
        PALETTE.dim
    } else {
        PALETTE.secondary
    };
    ui.painter().text(
        pos2(block.left(), y),
        egui::Align2::LEFT_TOP,
        label,
        theme::regular(size),
        color,
    );
    let (line, thick) = if focused {
        (PALETTE.switch, 2.0)
    } else {
        (PALETTE.thumb, 1.0)
    };
    ui.painter().line_segment(
        [
            pos2(block.left(), input.bottom() + 3.0),
            pos2(block.right(), input.bottom() + 3.0),
        ],
        egui::Stroke::new(thick, line),
    );
    enter
}

/// The Privacy choice, as YouTube Music's (`ytmusic-dropdown-renderer`):
/// "Privacy" small `#aaa` above; its icon 24 (a globe, a link, a lock),
/// the choice 14/500 white, an arrow at the right, a `#606060` line under,
/// 183 wide; a click lists the three with what each means.
fn privacy_menu(ui: &mut egui::Ui, privacy: &mut Privacy) {
    use egui::{Sense, pos2, vec2};
    let icon = |p: Privacy| match p {
        Privacy::Public => Icon::Public,
        Privacy::Unlisted => Icon::Link,
        Privacy::Private => Icon::Lock,
    };
    // Premium: a box 220 by 42 under its label.
    let premium = theme::premium();
    let size = if premium {
        vec2(220.0, 66.0)
    } else {
        vec2(183.3, 46.0)
    };
    let (block, response) = ui.allocate_exact_size(size, Sense::click());
    let (name, _) = privacy.words();
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Privacy: {name}"))
    });
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.painter().text(
        block.left_top(),
        egui::Align2::LEFT_TOP,
        "Privacy",
        theme::regular(12.0),
        PALETTE.dim,
    );
    if premium {
        let row =
            egui::Rect::from_min_size(pos2(block.left(), block.top() + 24.0), vec2(220.0, 42.0));
        let edge = if response.has_focus() {
            PALETTE.accent
        } else {
            PALETTE.outline
        };
        ui.painter()
            .rect_filled(row, egui::CornerRadius::same(10), PALETTE.field);
        ui.painter().rect_stroke(
            row,
            egui::CornerRadius::same(10),
            egui::Stroke::new(1.0, edge),
            egui::StrokeKind::Inside,
        );
        let mark = egui::Rect::from_min_size(
            pos2(row.left() + 10.0, row.center().y - 12.0),
            egui::Vec2::splat(24.0),
        );
        theme::paint_icon(ui, icon(*privacy), mark, 24.0, PALETTE.text);
        theme::paint_line(
            ui,
            pos2(row.left() + 44.0, row.center().y - 8.4),
            name,
            theme::medium(14.0),
            PALETTE.text,
            110.0,
        );
        let arrow = egui::Rect::from_min_size(
            pos2(row.right() - 32.0, row.center().y - 12.0),
            egui::Vec2::splat(24.0),
        );
        theme::paint_icon(ui, Icon::DropDown, arrow, 24.0, PALETTE.text);
    } else {
        let row =
            egui::Rect::from_min_size(pos2(block.left(), block.top() + 20.0), vec2(183.3, 24.0));
        let mark =
            egui::Rect::from_min_size(row.left_top() + vec2(4.0, 0.0), egui::Vec2::splat(24.0));
        theme::paint_icon(ui, icon(*privacy), mark, 24.0, PALETTE.text);
        theme::paint_line(
            ui,
            pos2(row.left() + 35.0, row.center().y - 8.4),
            name,
            theme::medium(14.0),
            PALETTE.text,
            110.0,
        );
        let arrow =
            egui::Rect::from_min_size(pos2(row.right() - 24.0, row.top()), egui::Vec2::splat(24.0));
        theme::paint_icon(ui, Icon::DropDown, arrow, 24.0, PALETTE.text);
        ui.painter().hline(
            block.x_range(),
            block.bottom() - 1.0,
            egui::Stroke::new(1.0, PALETTE.thumb),
        );
    }
    theme::menu_popup(&response).width(288.0).show(|ui| {
        theme::menu(ui);
        for choice in Privacy::ALL {
            let (title, meaning) = choice.words();
            let (spot, pick) =
                ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
            pick.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Button,
                    true,
                    choice == *privacy,
                    title,
                )
            });
            if pick.hovered() || choice == *privacy {
                ui.painter().rect_filled(spot, 0.0, PALETTE.surface);
            }
            let mark = egui::Rect::from_min_size(
                pos2(spot.left() + 16.0, spot.center().y - 12.0),
                egui::Vec2::splat(24.0),
            );
            theme::paint_icon(ui, icon(choice), mark, 24.0, PALETTE.text);
            theme::paint_line(
                ui,
                pos2(spot.left() + 56.0, spot.top() + 10.0),
                title,
                theme::regular(14.0),
                PALETTE.text,
                spot.width() - 72.0,
            );
            theme::paint_line(
                ui,
                pos2(spot.left() + 56.0, spot.top() + 29.0),
                meaning,
                theme::regular(12.0),
                PALETTE.secondary,
                spot.width() - 72.0,
            );
            if pick.clicked() {
                *privacy = choice;
                ui.close();
            }
        }
    });
}

/// A confirmation, as YouTube Music's (`yt-confirm-dialog-renderer`): the
/// question 16/400 white 24 from the top and 24 in, 16 under it the words
/// 14 `#aaa`, then Cancel and `main` as blue words at the right (8 8 8 24
/// around them). `Some(true)` when confirmed, `Some(false)` when cancelled.
fn confirm(ui: &mut egui::Ui, question: &str, text: &str, main: &str) -> Option<bool> {
    let mut done = None;
    ui.set_max_width(400.0);
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 24,
            right: 24,
            top: 24,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.set_width(352.0);
            ui.label(
                egui::RichText::new(question)
                    .font(theme::regular(16.0))
                    .color(PALETTE.text)
                    .line_height(Some(22.0)),
            );
            ui.add_space(16.0);
            ui.label(
                egui::RichText::new(text)
                    .font(theme::regular(14.0))
                    .color(PALETTE.dim)
                    .line_height(Some(20.0)),
            );
        });
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 24,
            right: 8,
            top: 8,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(368.0);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 36.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if theme::pill(ui, None, main, theme::Pill::Link).clicked() {
                        done = Some(true);
                    }
                    if theme::pill(ui, None, "Cancel", theme::Pill::Link).clicked() {
                        done = Some(false);
                    }
                },
            );
        });
    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        done = Some(true);
    }
    done
}

/// "Do you really want to close? There's a song playing.": the playing
/// song's cover (64, r 8) beside the question (18, bold) and what plays
/// (14), then "Do not ask again" (ticked as it opens) and No and Yes at
/// the right, 24 in all round, 440 wide. Enter is Yes, Escape No.
/// `Some(true)` for Yes.
fn close_question(app: &App, ui: &mut egui::Ui, dont_ask: &mut bool) -> Option<bool> {
    let mut answer = None;
    ui.set_width(440.0);
    egui::Frame::new()
        .inner_margin(egui::Margin::same(24))
        .show(ui, |ui| {
            ui.set_width(392.0);
            ui.horizontal_top(|ui| {
                let (cover, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(64.0), egui::Sense::hover());
                if let Some(entry) = &app.playback.entry {
                    super::widgets::cover_with(
                        app,
                        ui,
                        cover,
                        entry.track.thumbnail.as_ref(),
                        egui::CornerRadius::same(8),
                    );
                }
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.set_width(312.0);
                    ui.label(
                        egui::RichText::new("Do you really want to close?")
                            .font(theme::bold(18.0))
                            .color(PALETTE.text),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("There's a song playing.")
                            .font(theme::regular(14.0))
                            .color(PALETTE.secondary),
                    );
                    if let Some(entry) = &app.playback.entry {
                        ui.add_space(2.0);
                        let playing = if entry.track.artists.is_empty() {
                            entry.track.title.clone()
                        } else {
                            format!("{} \u{2022} {}", entry.track.title, entry.track.artists)
                        };
                        theme::label(ui, &playing, theme::medium(14.0), PALETTE.text);
                    }
                });
            });
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                if not_again(ui, *dont_ask).clicked() {
                    *dont_ask = !*dont_ask;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if theme::pill(ui, None, "Yes", theme::Pill::Filled).clicked() {
                        answer = Some(true);
                    }
                    if theme::pill(ui, None, "No", theme::Pill::Tonal).clicked() {
                        answer = Some(false);
                    }
                });
            });
        });
    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        answer = Some(true);
    }
    answer
}

/// The close question's "Do not ask again": a tick box (18, as a song
/// row's) and its words, pressed as one.
fn not_again(ui: &mut egui::Ui, ticked: bool) -> egui::Response {
    let words = "Do not ask again";
    let galley = ui
        .painter()
        .layout_no_wrap(words.to_string(), theme::regular(14.0), PALETTE.text);
    let size = egui::vec2(18.0 + 10.0 + galley.size().x, 36.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, ticked, words)
    });
    theme::pointing(ui, &response);
    let square = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - 9.0),
        egui::Vec2::splat(18.0),
    );
    if theme::dynamic() {
        super::dynamic::tick_box_look(ui, square, ticked);
    } else if ticked {
        ui.painter()
            .rect_filled(square, egui::CornerRadius::same(3), PALETTE.text);
        theme::paint_icon(ui, Icon::Check, square, 16.0, PALETTE.window);
    } else {
        ui.painter().rect_stroke(
            square,
            egui::CornerRadius::same(3),
            egui::Stroke::new(2.0, PALETTE.dim),
            egui::StrokeKind::Inside,
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            square.expand(3.0),
            egui::CornerRadius::same(5),
            egui::Stroke::new(1.0, PALETTE.accent),
            egui::StrokeKind::Outside,
        );
    }
    ui.painter().galley(
        egui::pos2(
            square.right() + 10.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        PALETTE.text,
    );
    response
}

/// "Save to playlist", as YouTube Music's (`ytmusic-add-to-playlist-renderer`):
/// a bar with the title and ×, then "All playlists" and the account's
/// playlists, which scroll, and "New playlist" at the bottom right. True
/// when it is done.
fn save_to_playlist(app: &App, ui: &mut egui::Ui, video_ids: &[String]) -> bool {
    use egui::{Align2, Rect, Sense, pos2, vec2};
    let width = 382.0; // 384 with the border
    ui.set_width(width);
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    let mut done = false;

    // The bar: at least 64 high, the title 24 in, × 17 from the right.
    let (bar, _) = ui.allocate_exact_size(vec2(width, 64.0), Sense::hover());
    ui.painter().text(
        pos2(bar.left() + 24.0, bar.center().y),
        Align2::LEFT_CENTER,
        "Save to playlist",
        theme::bold(24.0),
        PALETTE.text,
    );
    let close = Rect::from_center_size(
        pos2(bar.right() - 17.0 - 20.0, bar.center().y),
        egui::Vec2::splat(40.0),
    );
    let mut corner = ui.new_child(egui::UiBuilder::new().max_rect(close));
    if theme::round_button(
        &mut corner,
        theme::Icon::Close,
        40.0,
        24.0,
        theme::Round::Plain,
        PALETTE.dim,
        "Close",
    )
    .clicked()
    {
        done = true;
    }
    ui.painter().hline(
        bar.x_range(),
        bar.bottom() - 0.5,
        egui::Stroke::new(1.0, PALETTE.outline),
    );

    // The list: "All playlists" (20 above it, 7 under), the rows, and 64
    // of room for "New playlist"; at most three quarters of the window
    // high with the bar, then it scrolls.
    let playlists = app.own_playlist_cards();
    let content = 20.0 + 16.8 + 7.0 + 64.0 * playlists.len() as f32 + 64.0;
    let tallest = (ui.ctx().content_rect().height() * 0.75 - 64.0).max(128.0);
    let (list, _) = ui.allocate_exact_size(vec2(width, content.min(tallest)), Sense::hover());
    let mut list_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(list)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt("save-to-playlist")
        .max_height(list.height())
        .auto_shrink([false, false])
        .show(&mut list_ui, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            ui.add_space(20.0);
            let (heading, _) = ui.allocate_exact_size(vec2(width, 16.8), Sense::hover());
            ui.painter().text(
                pos2(heading.left() + 24.0, heading.center().y),
                Align2::LEFT_CENTER,
                "All playlists",
                theme::medium(14.0),
                PALETTE.text,
            );
            ui.add_space(7.0);
            for (playlist_id, card) in playlists {
                if playlist_row(app, ui, &card) {
                    app.act(Action::AddToPlaylist {
                        playlist_id,
                        title: card.title.clone(),
                        video_ids: video_ids.to_vec(),
                    });
                    done = true;
                }
            }
            // Room for "New playlist" over the list's end.
            ui.add_space(64.0);
        });

    // "New playlist", 17 in from the bottom right, over the list.
    let place = Rect::from_min_max(
        pos2(list.left(), list.bottom() - 17.0 - 36.0),
        pos2(list.right() - 17.0, list.bottom() - 17.0),
    );
    let mut corner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(place)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if theme::pill(
        &mut corner,
        Some(theme::Icon::Plus),
        "New playlist",
        theme::Pill::Filled,
    )
    .clicked()
    {
        app.act(Action::OpenDialog(Dialog::NewPlaylist {
            name: String::new(),
            description: String::new(),
            privacy: Privacy::default(),
            songs: video_ids.to_vec(),
        }));
        done = true;
    }
    done
}

/// One playlist in "Save to playlist": 64 high, its cover 44 (r 2) 24 in,
/// its name and the line under it 16 after; white@0.05 under the pointer.
/// True when clicked.
fn playlist_row(app: &App, ui: &mut egui::Ui, card: &ytfast_core::read::Card) -> bool {
    use egui::{Rect, Sense, pos2, vec2};
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &card.title));
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_white_alpha(13));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let cover = Rect::from_min_size(
            pos2(rect.left() + 24.0, rect.center().y - 22.0),
            egui::Vec2::splat(44.0),
        );
        crate::views::widgets::cover_with(
            app,
            ui,
            cover,
            card.thumbnail.as_ref(),
            egui::CornerRadius::same(2),
        );
        let left = cover.right() + 16.0;
        let room = (rect.right() - 24.0 - left).max(0.0);
        // Two lines, 4 apart, centred: 14 (16 from a window 1364 wide) on
        // lines 1.2 times as tall.
        let size = theme::text_size(ui);
        let line_height = size * 1.2;
        let top = rect.center().y - line_height - 2.0;
        let title = theme::fit(ui, &card.title, theme::medium(size), PALETTE.text, room, 1);
        let under = theme::fit(
            ui,
            &card.subtitle,
            theme::regular(size),
            PALETTE.secondary,
            room,
            1,
        );
        ui.painter().galley(
            pos2(left, top + (line_height - title.size().y) / 2.0),
            title,
            PALETTE.text,
        );
        ui.painter().galley(
            pos2(
                left,
                top + line_height + 4.0 + (line_height - under.size().y) / 2.0,
            ),
            under,
            PALETTE.secondary,
        );
    }
    response.clicked()
}
