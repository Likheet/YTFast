//! "Your stats": the listener's own numbers, from Radar's record
//! ([`ytfast_core::stats`]), opened from Radar's page. A period's chips
//! (this week, this month, all time), its numbers as tiles, the genres as
//! bars, and lists of songs and artists: a song's row plays it, an
//! artist's opens their page. YTFast's own page (YouTube Music has none),
//! in the look chosen: one column in the middle, as Settings'.

use egui::{Align, CornerRadius, Layout, Rect, Sense, UiBuilder, pos2, vec2};
use ytfast_core::read::Track;
use ytfast_core::stats::{ArtistCount, Counted, Period, Stats, time_heard};

use crate::app::{Action, App};
use crate::backend::Route;
use crate::theme::{self, PALETTE};

/// What the page is, under its title.
pub const STATS_WORDS: &str = "From what you play in YTFast and on your other devices.";
/// Under the chips, for all time.
pub const ALL_TIME_WORDS: &str = "Since Radar began noting your plays. Older plays have no time.";
/// Under the chips, when some plays were on other devices.
fn elsewhere_words(plays: u32) -> String {
    let these = if plays == 1 {
        "1 of these plays was".to_string()
    } else {
        format!("{plays} of these plays were")
    };
    format!("{these} on your other devices, from your YouTube history.")
}
/// With nothing played in the period.
pub const NOTHING_YET: &str =
    "Nothing played in this time yet. Play some music, and your numbers show here.";

/// The page's widest, and from how wide its lists stand two by two.
const MOST_WIDE: f32 = 880.0;
const TWO_LISTS: f32 = 640.0;
/// A list's row.
const ROW: f32 = 52.0;

pub fn show(app: &App, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .id_salt("stats")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let full = ui.max_rect();
            // At least 24 clear of the menu and the window's edge.
            let width = (full.width() - 48.0).clamp(0.0, MOST_WIDE);
            let column = Rect::from_min_size(
                pos2(full.center().x - width / 2.0, ui.cursor().top()),
                vec2(width, full.height()),
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(column)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    ui.set_width(width);
                    contents(app, ui);
                    ui.add_space(48.0);
                },
            );
        });
}

fn contents(app: &App, ui: &mut egui::Ui) {
    let (above, size) = if theme::premium() {
        (32.0, 36.0)
    } else {
        (20.0, 30.0)
    };
    ui.add_space(above);
    theme::label(ui, "Your stats", theme::bold(size), PALETTE.text);
    ui.add_space(6.0);
    theme::label(ui, STATS_WORDS, theme::regular(14.0), PALETTE.secondary);
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        for period in Period::ALL {
            if theme::chip(ui, period.words(), app.stats_period == period, 32.0).clicked() {
                app.act(Action::StatsPeriod(period));
            }
        }
    });
    if app.stats_period == Period::All {
        ui.add_space(10.0);
        theme::label(ui, ALL_TIME_WORDS, theme::regular(13.0), PALETTE.secondary);
    }
    let stats = app.stats();
    if stats.elsewhere > 0 {
        ui.add_space(10.0);
        theme::label(
            ui,
            &elsewhere_words(stats.elsewhere),
            theme::regular(13.0),
            PALETTE.secondary,
        );
    }
    ui.add_space(24.0);
    if stats.plays == 0 {
        theme::label(ui, NOTHING_YET, theme::regular(16.0), PALETTE.secondary);
        return;
    }
    tiles(ui, &stats);
    if !stats.genres.is_empty() {
        heading(ui, "Your genres");
        genres(ui, &stats.genres);
    }
    let width = ui.available_width();
    let lists: Vec<(&str, Vec<Line>)> = [
        ("Most played", songs(&stats.most_played, "play")),
        ("Top artists", artists(&stats.top_artists)),
        ("Skipped most", songs(&stats.skipped_most, "skip")),
        ("Liked", plain(&stats.liked)),
        ("Disliked", plain(&stats.disliked)),
    ]
    .into_iter()
    .filter(|(_, lines)| !lines.is_empty())
    .collect();
    if width >= TWO_LISTS {
        for pair in lists.chunks(2) {
            ui.add_space(8.0);
            ui.columns(2, |columns| {
                for (column, (title, lines)) in columns.iter_mut().zip(pair) {
                    list(app, column, title, lines);
                }
            });
        }
    } else {
        for (title, lines) in &lists {
            ui.add_space(8.0);
            list(app, ui, title, lines);
        }
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(32.0);
    theme::label(ui, text, theme::bold(20.0), PALETTE.text);
    ui.add_space(12.0);
}

/// The numbers, as tiles: four across (two in a narrow window), each its
/// name above its number. A screen reader reads each as "Plays: 214".
fn tiles(ui: &mut egui::Ui, stats: &Stats) {
    let mut numbers: Vec<(&str, String)> = vec![("Plays", stats.plays.to_string())];
    if let Some(seconds) = stats.seconds {
        numbers.push(("Time in YTFast", time_heard(seconds)));
    }
    numbers.extend([
        ("Songs", stats.songs.to_string()),
        ("Artists", stats.artists.to_string()),
        ("Replays", stats.replays.to_string()),
        ("Skips", stats.skips.to_string()),
        ("Likes", stats.likes.to_string()),
        ("Dislikes", stats.dislikes.to_string()),
    ]);
    let width = ui.available_width();
    let across = if width >= 560.0 { 4 } else { 2 };
    let gap = 12.0;
    let tile = vec2((width - gap * (across - 1) as f32) / across as f32, 84.0);
    let corners = CornerRadius::same(if theme::premium() { 12 } else { 8 });
    for row in numbers.chunks(across) {
        let (area, _) = ui.allocate_exact_size(vec2(width, tile.y), Sense::hover());
        for (i, (name, number)) in row.iter().enumerate() {
            let rect = Rect::from_min_size(
                pos2(area.left() + i as f32 * (tile.x + gap), area.top()),
                tile,
            );
            let response = ui.interact(rect, ui.id().with(("tile", name)), Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Label,
                    true,
                    format!("{name}: {number}"),
                )
            });
            if !ui.is_rect_visible(rect) {
                continue;
            }
            ui.painter().rect_filled(rect, corners, PALETTE.surface);
            let inner = rect.shrink2(vec2(16.0, 14.0));
            ui.painter().text(
                inner.left_top(),
                egui::Align2::LEFT_TOP,
                *name,
                theme::regular(14.0),
                PALETTE.secondary,
            );
            ui.painter().text(
                inner.left_bottom(),
                egui::Align2::LEFT_BOTTOM,
                number,
                theme::bold(28.0),
                PALETTE.text,
            );
        }
        ui.add_space(gap);
    }
}

/// The genres as bars: each one's name, its share as a bar, and as a
/// number. The first six.
fn genres(ui: &mut egui::Ui, genres: &[(String, f64)]) {
    let width = ui.available_width();
    let name_width = 132.0;
    let number_width = 48.0;
    let fill = if theme::premium() {
        PALETTE.accent
    } else {
        PALETTE.text
    };
    for (genre, share) in genres.iter().take(6) {
        let (rect, response) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
        let percent = (share * 100.0).round();
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Label,
                true,
                format!("{genre}: {percent}%"),
            )
        });
        if !ui.is_rect_visible(rect) {
            continue;
        }
        let painter = ui.painter();
        painter.text(
            pos2(rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            genre,
            theme::regular(14.0),
            PALETTE.text,
        );
        let track = Rect::from_min_max(
            pos2(rect.left() + name_width, rect.center().y - 4.0),
            pos2(rect.right() - number_width, rect.center().y + 4.0),
        );
        painter.rect_filled(track, CornerRadius::same(4), PALETTE.surface);
        let mut filled = track;
        filled.set_width((track.width() * *share as f32).max(4.0));
        painter.rect_filled(filled, CornerRadius::same(4), fill);
        painter.text(
            pos2(rect.right(), rect.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("{percent}%"),
            theme::regular(14.0),
            PALETTE.secondary,
        );
    }
}

/// A row of a list: its words, how many times (when counted), and what
/// pressing it does.
struct Line {
    title: String,
    under: String,
    times: String,
    action: Option<Action>,
    name: String,
}

/// Songs counted ("3 plays", "1 skip"): a row plays its song.
fn songs(counted: &[Counted], what: &str) -> Vec<Line> {
    counted
        .iter()
        .map(|c| {
            let times = if c.times == 1 {
                format!("1 {what}")
            } else {
                format!("{} {what}s", c.times)
            };
            song_line(&c.track, times)
        })
        .collect()
}

/// Songs liked or disliked.
fn plain(tracks: &[Track]) -> Vec<Line> {
    tracks
        .iter()
        .filter(|t| !t.title.is_empty())
        .map(|t| song_line(t, String::new()))
        .collect()
}

fn song_line(track: &Track, times: String) -> Line {
    Line {
        title: track.title.clone(),
        under: track.artists.clone(),
        times,
        name: format!("Play {}", track.title),
        action: Some(Action::PlayTracks {
            tracks: vec![track.clone()],
            start: 0,
            source: None,
        }),
    }
}

/// Artists by their plays: a row opens their page, when known.
fn artists(counted: &[ArtistCount]) -> Vec<Line> {
    counted
        .iter()
        .map(|a| Line {
            title: a.name.clone(),
            under: String::new(),
            times: if a.plays == 1 {
                "1 play".into()
            } else {
                format!("{} plays", a.plays)
            },
            name: format!("Open {}", a.name),
            action: a.id.as_ref().map(|id| {
                Action::Navigate(Route::Browse {
                    id: id.clone(),
                    params: None,
                })
            }),
        })
        .collect()
}

/// A list: its title, then its rows, numbered.
fn list(app: &App, ui: &mut egui::Ui, title: &str, lines: &[Line]) {
    ui.add_space(24.0);
    theme::label(ui, title, theme::bold(20.0), PALETTE.text);
    ui.add_space(8.0);
    for (place, line) in lines.iter().enumerate() {
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &line.name));
        if line.action.is_some() {
            theme::pointing(ui, &response);
        }
        if response.clicked()
            && let Some(action) = &line.action
        {
            app.act(action.clone());
        }
        if !ui.is_rect_visible(rect) {
            continue;
        }
        let painter = ui.painter();
        if response.hovered() && line.action.is_some() {
            painter.rect_filled(rect, CornerRadius::same(8), PALETTE.surface);
        }
        painter.text(
            pos2(rect.left() + 12.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            (place + 1).to_string(),
            theme::regular(14.0),
            PALETTE.secondary,
        );
        let times_width = if line.times.is_empty() { 0.0 } else { 72.0 };
        let words = Rect::from_min_max(
            pos2(rect.left() + 44.0, rect.top()),
            pos2(rect.right() - 12.0 - times_width, rect.bottom()),
        );
        let title = theme::fit(
            ui,
            &line.title,
            theme::medium(14.0),
            PALETTE.text,
            words.width(),
            1,
        );
        let under = (!line.under.is_empty()).then(|| {
            theme::fit(
                ui,
                &line.under,
                theme::regular(13.0),
                PALETTE.secondary,
                words.width(),
                1,
            )
        });
        let height = title.size().y + under.as_ref().map_or(0.0, |u| u.size().y + 2.0);
        let mut at = pos2(words.left(), rect.center().y - height / 2.0);
        let title_height = title.size().y;
        painter.galley(at, title, PALETTE.text);
        if let Some(under) = under {
            at.y += title_height + 2.0;
            painter.galley(at, under, PALETTE.secondary);
        }
        if !line.times.is_empty() {
            painter.text(
                pos2(rect.right() - 12.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                &line.times,
                theme::regular(13.0),
                PALETTE.secondary,
            );
        }
    }
}
