//! What the lyrics share in every look: the words of the line being sung
//! lit as they are sung, and the line in Latin letters and in English
//! under it (Settings, "Translate lyrics").

use std::sync::Arc;

use egui::{Color32, Galley};

use crate::app::App;
use crate::lyrics::{Lyrics, Translation, Word};
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

/// How quickly lyrics that follow the song move to a new place, in
/// seconds: most of the way in about twice this, settled in about three
/// times it (a spring's smooth time).
const GLIDE: f32 = 0.2;
/// The longest step of time one frame moves them by: the first frame after
/// the window rested starts the move gently.
const LONGEST_STEP: f32 = 0.05;

/// Where lyrics that follow the song are scrolled. They move to each new
/// place (the line being sung a third of the way down) as a spring
/// settles, easing in and out, and a new place met on the way changes
/// their course without a jolt. Following again after a scroll by hand (or
/// a click on a line) they start from where they were drawn, so a line
/// clicked glides into place without first jumping back to the line that
/// was being sung.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Follower {
    /// Where the spring is, while following.
    position: Option<f32>,
    /// How fast it moves, in points a second.
    speed: f32,
    /// When it last moved (the input's time).
    at: f64,
    /// Following when last drawn, not scrolled by hand.
    following: bool,
    /// Where the scroll area last drew the lyrics.
    drawn: Option<f32>,
}

impl Follower {
    /// Where to scroll at `now` for `target`, or `None` while the lyrics
    /// are scrolled by hand (`follow` false).
    pub(super) fn step(&mut self, now: f64, target: f32, follow: bool) -> Option<f32> {
        let dt = ((now - self.at) as f32).clamp(0.0, LONGEST_STEP);
        self.at = now;
        if !follow {
            self.following = false;
            self.speed = 0.0;
            return None;
        }
        let from = match (self.following, self.position, self.drawn) {
            (true, Some(position), _) => position,
            // Following again: from where they were drawn, at rest.
            (false, _, Some(drawn)) => drawn,
            (false, Some(position), None) => position,
            // Drawn for the first time: in place at once.
            _ => target,
        };
        if !self.following {
            self.speed = 0.0;
            self.following = true;
        }
        // A critically damped spring, stepped exactly for any frame rate
        // (as Game Programming Gems 4's SmoothDamp).
        let omega = 2.0 / GLIDE;
        let x = omega * dt;
        let decay = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
        let change = from - target;
        let pull = (self.speed + omega * change) * dt;
        self.speed = (self.speed - omega * pull) * decay;
        let mut position = target + (change + pull) * decay;
        if (position - target).abs() < 0.5 && self.speed.abs() < 5.0 {
            position = target;
            self.speed = 0.0;
        }
        self.position = Some(position);
        Some(position)
    }

    /// Still on its way: the window must be drawn again.
    pub(super) fn moving(&self) -> bool {
        self.following && self.speed != 0.0
    }

    /// Where the scroll area drew the lyrics this frame (where they were
    /// scrolled by hand, or where it could follow to).
    pub(super) fn drawn_at(&mut self, offset: f32) {
        self.drawn = Some(offset);
    }
}

/// [`Follower::step`] for the lyrics `id`, kept between frames; the window
/// is drawn again while they move. `target` is `None` while it is not
/// known (the lines not yet placed, drawn for the first time): then they
/// are in place at once the frame after, not moving there from the top.
/// Call [`drawn`] after the scroll area.
pub(super) fn follow(
    ui: &egui::Ui,
    id: egui::Id,
    target: Option<f32>,
    follow: bool,
) -> Option<f32> {
    let target = target?;
    let now = ui.input(|i| i.time);
    let mut follower: Follower = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let offset = follower.step(now, target, follow);
    if follower.moving() {
        crate::theme::repaint_in(ui.ctx(), lyric_frame());
    }
    ui.data_mut(|d| d.insert_temp(id, follower));
    offset
}

/// The time between the lyrics' frames while they change.
pub(super) fn lyric_frame() -> std::time::Duration {
    std::time::Duration::from_secs_f64(crate::lyrics::WORD_FRAME)
}

/// A value easing to `target` over `seconds` as egui's
/// `animate_value_with_time` does (from where it was, in a straight line),
/// drawn at the lyrics' frame rate: egui's own asks for every frame the
/// screen shows (60 a second) while it moves. The first value is `target`
/// at once.
pub(super) fn ease(ctx: &egui::Context, id: egui::Id, target: f32, seconds: f32) -> f32 {
    /// Where an ease started, where it goes, and when it started.
    #[derive(Clone, Copy)]
    struct Ease {
        from: f32,
        to: f32,
        since: f64,
    }
    let now = ctx.input(|i| i.time);
    let at = |ease: Ease| {
        let t = ((now - ease.since) as f32 / seconds.max(f32::EPSILON)).clamp(0.0, 1.0);
        ease.from + (ease.to - ease.from) * t
    };
    let ease = match ctx.data(|d| d.get_temp::<Ease>(id)) {
        None => Ease {
            from: target,
            to: target,
            since: now,
        },
        Some(ease) if ease.to != target => Ease {
            from: at(ease),
            to: target,
            since: now,
        },
        Some(ease) => ease,
    };
    ctx.data_mut(|d| d.insert_temp(id, ease));
    let value = at(ease);
    if value != target {
        crate::theme::repaint_in(ctx, lyric_frame());
    }
    value
}

/// Where the scroll area drew the lyrics `id` this frame: its offset.
pub(super) fn drawn(ui: &egui::Ui, id: egui::Id, offset: f32) {
    ui.data_mut(|d| d.get_temp_mut_or_default::<Follower>(id).drawn_at(offset));
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

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f64 = 1.0 / 60.0;

    /// The lyrics' fades start where they are, ease in a straight line as
    /// egui's do, and ask for the next frame a 30th of a second on (not
    /// the screen's every frame), until done.
    #[test]
    fn lyrics_fades_ease_at_thirty_frames_a_second() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("fade");
        // One frame at `time` that eases toward `target`: its value, and
        // when the next frame was asked for.
        let frame = |time: f64, target: f32| {
            let input = egui::RawInput {
                time: Some(time),
                predicted_dt: 1.0 / 60.0,
                ..Default::default()
            };
            let mut value = 0.0;
            let mut output = ctx.run_ui(input, |ui| value = ease(ui.ctx(), id, target, 0.35));
            output.textures_delta.clear();
            let next = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
            (value, next)
        };
        // The first value is the target at once.
        for time in [0.0, 0.1, 0.2, 0.3] {
            assert_eq!(frame(time, 1.0).0, 1.0);
        }
        // Then to 0: halfway in half its time, the next frame a 30th on.
        assert_eq!(frame(1.0, 0.0).0, 1.0);
        let (half, next) = frame(1.175, 0.0);
        assert!((half - 0.5).abs() < 0.01, "{half}");
        assert!((next.as_secs_f64() - 1.0 / 30.0).abs() < 0.001, "{next:?}");
        // Done: no frame asked for.
        let (done, next) = frame(1.4, 0.0);
        assert_eq!(done, 0.0);
        assert!(next > std::time::Duration::from_secs(1), "{next:?}");
    }

    /// Steps `follower` a frame at a time toward `target`, drawn where it
    /// says (or at `drawn` when given, as a scroll area that cannot go
    /// that far), and returns where it was each frame.
    fn glide(
        follower: &mut Follower,
        now: &mut f64,
        frames: usize,
        target: f32,
        drawn: Option<f32>,
    ) -> Vec<f32> {
        (0..frames)
            .map(|_| {
                *now += FRAME;
                let at = follower.step(*now, target, true).unwrap();
                follower.drawn_at(drawn.unwrap_or(at));
                at
            })
            .collect()
    }

    /// Scrolled by hand, then a line clicked: the lyrics glide from where
    /// they were to the line clicked, without first jumping back to the
    /// line that was being sung, and come to rest there.
    #[test]
    fn a_line_clicked_after_scrolling_glides_there_from_where_it_was() {
        let mut follower = Follower::default();
        let mut now = 10.0;
        // Following, at rest on the line being sung (its place 200).
        glide(&mut follower, &mut now, 60, 200.0, None);
        assert!(!follower.moving());
        // Scrolled by hand down to 900.
        for k in 1..=30 {
            now += FRAME;
            assert_eq!(follower.step(now, 200.0, false), None);
            follower.drawn_at(200.0 + k as f32 * 700.0 / 30.0);
        }
        // A line clicked: following again, to its place, 600.
        let path = glide(&mut follower, &mut now, 90, 600.0, None);
        assert!((path[0] - 900.0).abs() < 10.0, "first frame at {}", path[0]);
        assert!(path.windows(2).all(|w| w[1] <= w[0]), "{path:?}");
        assert!(path.iter().all(|&at| at >= 600.0), "{path:?}");
        assert_eq!(path.last(), Some(&600.0));
        assert!(!follower.moving(), "it rests, and the window with it");
    }

    /// The next line while moving to this one: the course changes with
    /// no jolt, and the lyrics rest at the new place.
    #[test]
    fn a_new_place_on_the_way_changes_course_smoothly() {
        let mut follower = Follower::default();
        let mut now = 10.0;
        glide(&mut follower, &mut now, 1, 0.0, None);
        let mut path = glide(&mut follower, &mut now, 12, 300.0, None);
        path.extend(glide(&mut follower, &mut now, 90, 500.0, None));
        let steps: Vec<f32> = path.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(
            steps.iter().all(|&step| (0.0..40.0).contains(&step)),
            "{steps:?}"
        );
        // Each frame's step differs from the last by little: no jolt.
        assert!(
            steps.windows(2).all(|w| (w[1] - w[0]).abs() < 8.0),
            "{steps:?}"
        );
        assert_eq!(path.last(), Some(&500.0));
        assert!(!follower.moving());
    }

    /// Drawn for the first time, the lyrics are in place at once; and a
    /// place the scroll area cannot reach (near the end) still lets them
    /// rest, so the window is not drawn again and again.
    #[test]
    fn in_place_at_once_and_at_rest_where_the_area_stops() {
        let mut follower = Follower::default();
        assert_eq!(follower.step(3.0, 350.0, true), Some(350.0));
        assert!(!follower.moving());
        let mut now = 3.0;
        glide(&mut follower, &mut now, 120, 1000.0, Some(800.0));
        assert!(!follower.moving());
    }

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
        // Half way through "two" (the lead is 0.05 s).
        let sung = sung(&galley, &words, 1.45, lit, unsung, 4.0);
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
