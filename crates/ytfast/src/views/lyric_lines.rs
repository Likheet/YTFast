//! What the lyrics share in every look: the words of the line being sung
//! lit as they are sung, the line in Latin letters and in English under
//! it (Translate), and the Translate chip above the lyrics.

use std::sync::Arc;

use egui::{Align, Color32, Galley, Layout, vec2};

use crate::app::{Action, App, Setting};
use crate::lyrics::{Lyrics, TRANSLATE_TO, Translation, Word};
use crate::theme::{self, PALETTE};
use ytfast_core::translate::Translated;

/// The space between a line and what is under it (in Latin letters, in
/// English), and between those two.
pub(super) const UNDER_GAP: f32 = 4.0;

/// The line `galley` (laid out from the line's text, which `words` are
/// places in) as it is sung at `position`: each word lit from its left as
/// it is sung, `lit` where it is, `unsung` where not yet, the edge
/// between them soft over `feather` points. What lies between words takes
/// the word before it.
pub(super) fn sung(
    galley: &Arc<Galley>,
    words: &[Word],
    position: f64,
    lit: Color32,
    unsung: Color32,
    feather: f32,
) -> Arc<Galley> {
    let feather = feather.max(1.0);
    let mut sung = (**galley).clone();
    for placed in &mut sung.rows {
        let row = Arc::make_mut(&mut placed.row);
        // Each letter's word, and where each word lies on this row.
        let of_glyph: Vec<Option<usize>> = row
            .glyphs
            .iter()
            .map(|glyph| {
                let at = glyph.cluster as usize;
                words.iter().position(|w| (w.from..w.to).contains(&at))
            })
            .collect();
        let mut spans: Vec<(usize, f32, f32)> = Vec::new();
        for (glyph, word) in row.glyphs.iter().zip(&of_glyph) {
            let Some(word) = *word else {
                continue;
            };
            match spans.iter_mut().find(|span| span.0 == word) {
                Some(span) => {
                    span.1 = span.1.min(glyph.pos.x);
                    span.2 = span.2.max(glyph.max_x());
                }
                None => spans.push((word, glyph.pos.x, glyph.max_x())),
            }
        }
        let letters = row.visuals.glyph_vertex_range.clone();
        let mut before = 0.0;
        for (k, glyph) in row.glyphs.iter().enumerate() {
            let from = (glyph.first_vertex as usize).max(letters.start);
            let to = row
                .glyphs
                .get(k + 1)
                .map_or(letters.end, |next| next.first_vertex as usize)
                .min(letters.end);
            // How much of its word is sung, and where the lit part ends.
            let (done, edge) = match of_glyph[k] {
                Some(word) => {
                    let done = words[word].sung(position);
                    before = done;
                    let edge = spans
                        .iter()
                        .find(|span| span.0 == word)
                        .map(|(_, left, right)| left + done * (right - left + feather));
                    (done, edge)
                }
                None => (before, None),
            };
            for vertex in row
                .visuals
                .mesh
                .vertices
                .get_mut(from..to)
                .unwrap_or_default()
            {
                let share = match edge {
                    _ if done >= 1.0 => 1.0,
                    Some(edge) if done > 0.0 => ((edge - vertex.pos.x) / feather).clamp(0.0, 1.0),
                    _ => 0.0,
                };
                vertex.color = unsung.lerp_to_gamma(lit, share);
            }
        }
    }
    Arc::new(sung)
}

/// The lyrics' translation when Translate is on and it is in, a line for
/// each of theirs.
pub(super) fn translation<'a>(
    app: &'a App,
    lyrics: &Lyrics,
    video_id: &str,
) -> Option<&'a Translated> {
    if !app.settings.translate_lyrics {
        return None;
    }
    match app.translations.get(video_id) {
        Some(Translation::Ready(translated))
            if translated.lines.len() == lyrics.lines.len()
                && translated.latin.len() == lyrics.lines.len() =>
        {
            Some(translated)
        }
        _ => None,
    }
}

/// What shows under line `i`: it in Latin letters, then in English.
pub(super) fn under(translated: Option<&Translated>, i: usize) -> impl Iterator<Item = &str> {
    translated
        .into_iter()
        .flat_map(move |t| [t.latin[i].as_deref(), t.lines[i].as_deref()])
        .flatten()
}

/// A line's name for screen readers (and the tests): a button when a
/// click on it jumps there (`timed`), words otherwise.
pub(super) fn name(response: &egui::Response, text: &str, timed: bool) {
    let kind = if timed {
        egui::WidgetType::Button
    } else {
        egui::WidgetType::Label
    };
    response.widget_info(|| egui::WidgetInfo::labeled(kind, true, text));
}

/// `text` laid out in `font`, white, wrapped at `width`, on lines
/// `line_height` apart when given.
pub(super) fn layout(
    ui: &egui::Ui,
    text: &str,
    font: &egui::FontId,
    width: f32,
    line_height: Option<f32>,
) -> Arc<Galley> {
    let mut job =
        egui::text::LayoutJob::simple(text.to_string(), font.clone(), Color32::WHITE, width);
    job.sections[0].format.line_height = line_height;
    ui.fonts_mut(|f| f.layout_job(job))
}

/// Above the lyrics, 40 high: the Translate chip, `margin` from the right,
/// and before it what it is doing (translating, or why nothing shows).
pub(super) fn translate_bar(app: &App, ui: &mut egui::Ui, video_id: &str, margin: f32) {
    let on = app.settings.translate_lyrics;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 40.0),
        Layout::right_to_left(Align::Center),
        |ui| {
            ui.set_min_height(40.0);
            ui.add_space(margin);
            let chip = theme::chip(ui, "Translate", on, 32.0)
                .on_hover_text("Each line in English, and in Latin letters");
            if chip.clicked() {
                app.act(Action::Toggle(Setting::TranslateLyrics));
            }
            if !on {
                return;
            }
            ui.add_space(12.0);
            let note = match app.translations.get(video_id) {
                None | Some(Translation::Loading) => {
                    ui.add(egui::Spinner::new().size(16.0).color(PALETTE.secondary));
                    return;
                }
                Some(Translation::Missing) => "Could not translate",
                Some(Translation::Ready(t)) if t.is_empty() => {
                    if t.language.as_deref() == Some(TRANSLATE_TO) {
                        "Already in English"
                    } else {
                        "Nothing to translate"
                    }
                }
                Some(Translation::Ready(_)) => return,
            };
            theme::label(ui, note, theme::regular(14.0), PALETTE.secondary);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line being sung: its first word lit, the next lit to half way
    /// (dim beyond the soft edge), the last not yet.
    #[test]
    fn a_line_lights_word_by_word() {
        let ctx = egui::Context::default();
        let mut galley = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            galley = Some(layout(
                ui,
                "one two six",
                &egui::FontId::proportional(20.0),
                500.0,
                None,
            ));
        });
        // No window takes the frame's new textures here.
        output.textures_delta.clear();
        let galley = galley.unwrap();
        let word = |start: f64, from: usize, to: usize| Word {
            start,
            end: start + 1.0,
            from,
            to,
        };
        let words = [word(0.0, 0, 3), word(1.0, 4, 7), word(2.0, 8, 11)];
        let (lit, unsung) = (Color32::WHITE, Color32::from_white_alpha(100));
        // Half way through "two" (the lead is a tenth of a second).
        let sung = sung(&galley, &words, 1.4, lit, unsung, 4.0);
        let row = &sung.rows[0];
        let colors_of = |k: usize| -> Vec<Color32> {
            let from = row.glyphs[k].first_vertex as usize;
            let to = row
                .glyphs
                .get(k + 1)
                .map_or(row.visuals.glyph_vertex_range.end, |g| {
                    g.first_vertex as usize
                });
            row.visuals.mesh.vertices[from..to]
                .iter()
                .map(|v| v.color)
                .collect()
        };
        assert!(colors_of(0).iter().all(|&c| c == lit), "{:?}", colors_of(0));
        // "t" lit, "o" not.
        assert!(colors_of(4).iter().all(|&c| c == lit), "{:?}", colors_of(4));
        assert!(
            colors_of(6).iter().all(|&c| c == unsung),
            "{:?}",
            colors_of(6)
        );
        assert!(colors_of(9).iter().all(|&c| c == unsung));
        // The galley laid out is left as it was.
        assert!(
            galley.rows[0]
                .visuals
                .mesh
                .vertices
                .iter()
                .all(|v| v.color == Color32::WHITE)
        );
    }
}
