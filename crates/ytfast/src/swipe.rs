//! Back and Forward by a sideways swipe of two fingers on a touchpad, as
//! Chrome does (the owner's word, 11 October 2026): where nothing under the
//! pointer scrolls sideways, or it is at its end, the swipe pulls an arrow
//! in from the page's side; let go once it is full, the page goes back
//! (swiped right) or forward (swiped left). A swipe a shelf takes as it
//! starts stays the shelf's, and one that starts up or down never pulls,
//! as in a browser. A wheel turned sideways by Shift is a wheel, not a
//! swipe.

use egui::{Color32, Context, Event, MouseWheelUnit, Rect, TouchPhase, Vec2, pos2, vec2};

use crate::theme::{self, Icon, PALETTE};

/// How far a swipe pulls, in points of scrolling, before letting go goes
/// back or forward.
pub const PULL: f32 = 160.0;
/// A pause this long, in seconds, ends a gesture where the platform does
/// not say when the fingers lift (Windows).
const GAP: f64 = 0.2;
/// A first movement this much wider than it is tall starts a sideways
/// swipe.
const SIDEWAYS: f32 = 1.5;

/// Where a swipe goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Way {
    Back,
    Forward,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum State {
    #[default]
    Idle,
    /// A sideways gesture of the page's own: how far it has pulled (more
    /// than 0 back, less forward), and whether any of it has been offered
    /// to the window yet (to tell a shelf taking it as it starts).
    Pulling { pull: f32, offered: bool },
    /// A gesture that is not the page's (up and down, or a shelf's), or
    /// one that has ended while more of it still comes (a Mac's glide):
    /// ignored until the next pause.
    Ignored,
}

/// The swipe in progress, one per window.
#[derive(Debug, Default)]
pub struct Swipe {
    state: State,
    /// Input time of the latest movement.
    last: f64,
    /// The sideways scrolling offered to the window this frame, before it
    /// drew.
    offered: f32,
    /// The fingers lifted this frame (where the platform says so).
    lifted: bool,
}

impl Swipe {
    /// Before the window draws: this frame's movement, and how much
    /// sideways scrolling it is offered.
    pub fn before(&mut self, ctx: &Context) {
        let options = ctx.options(|o| o.input_options);
        let (now, moved, lifted, offered) = ctx.input(|i| {
            let (mut moved, mut lifted) = (Vec2::ZERO, false);
            for event in &i.events {
                if let Event::MouseWheel {
                    unit,
                    delta,
                    phase,
                    modifiers,
                } = event
                {
                    lifted |= matches!(phase, TouchPhase::End | TouchPhase::Cancel);
                    // Turned sideways by a modifier: a wheel, not a swipe.
                    if modifiers.matches_any(options.horizontal_scroll_modifier) {
                        continue;
                    }
                    let points = match unit {
                        MouseWheelUnit::Point => 1.0,
                        MouseWheelUnit::Line => options.line_scroll_speed,
                        MouseWheelUnit::Page => i.content_rect().width(),
                    };
                    moved += *delta * points;
                }
            }
            (i.time, moved, lifted, i.smooth_scroll_delta.x)
        });
        self.start(now, moved, lifted, offered);
    }

    fn start(&mut self, now: f64, moved: Vec2, lifted: bool, offered: f32) {
        self.offered = offered;
        self.lifted = lifted;
        if moved == Vec2::ZERO {
            return;
        }
        let quiet = now - self.last > GAP;
        self.last = now;
        // A new gesture: sideways, or not the page's.
        if quiet || self.state == State::Idle {
            self.state = if moved.x.abs() > moved.y.abs() * SIDEWAYS {
                State::Pulling {
                    pull: 0.0,
                    offered: false,
                }
            } else {
                State::Ignored
            };
        }
    }

    /// After the window drew: what no part of it took of a sideways swipe
    /// pulls, toward a page there is (`back`, `forward`), while the page
    /// shows (`open`: no player page or dialog over it). Where to go, once
    /// a full pull is let go.
    pub fn after(&mut self, ctx: &Context, open: bool, back: bool, forward: bool) -> Option<Way> {
        let (now, left) = ctx.input(|i| (i.time, i.smooth_scroll_delta.x));
        let way = self.pull(now, left, open, back, forward);
        if matches!(self.state, State::Pulling { .. }) {
            // The next frame, to see the fingers lift (Windows says nothing
            // when they do).
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(GAP / 2.0));
        }
        way
    }

    fn pull(&mut self, now: f64, left: f32, open: bool, back: bool, forward: bool) -> Option<Way> {
        let State::Pulling { pull, offered } = self.state else {
            return None;
        };
        let mut pull = pull;
        // The first sideways movement offered, all taken (a shelf that can
        // scroll that way): the gesture is the shelf's.
        let taken = !offered && self.offered != 0.0 && left == 0.0;
        if !open || taken {
            self.state = State::Ignored;
            return None;
        }
        pull = (pull + left).clamp(-1.5 * PULL, 1.5 * PULL);
        // Only toward a page there is.
        if (pull > 0.0 && !back) || (pull < 0.0 && !forward) {
            pull = 0.0;
        }
        self.state = State::Pulling {
            pull,
            offered: offered || self.offered != 0.0,
        };
        if !self.lifted && now - self.last <= GAP {
            return None;
        }
        // Let go: a full pull goes there; what follows of the gesture is
        // ignored.
        self.state = if self.lifted {
            State::Ignored
        } else {
            State::Idle
        };
        (pull.abs() >= PULL).then_some(if pull > 0.0 { Way::Back } else { Way::Forward })
    }

    /// How far the swipe has pulled, from 0 to 1 (full: letting go goes
    /// there), and which way; `None` while nothing pulls.
    fn shown(&self) -> Option<(Way, f32)> {
        match self.state {
            State::Pulling { pull, .. } if pull.abs() >= 1.0 => {
                let way = if pull > 0.0 { Way::Back } else { Way::Forward };
                Some((way, (pull.abs() / PULL).min(1.0)))
            }
            _ => None,
        }
    }

    /// The arrow being pulled in at the side of `page`: in from beyond its
    /// edge as the swipe goes, a white disc once letting go would go there
    /// (as YouTube Music's white buttons), named for screen readers.
    pub fn paint(&self, ui: &egui::Ui, page: Rect) {
        let Some((way, pulled)) = self.shown() else {
            return;
        };
        const RADIUS: f32 = 20.0;
        let travel = RADIUS * 2.0 + 16.0;
        let x = match way {
            Way::Back => page.left() - RADIUS + travel * pulled,
            Way::Forward => page.right() + RADIUS - travel * pulled,
        };
        let center = pos2(x, page.center().y);
        let spot = Rect::from_center_size(center, vec2(RADIUS * 2.0, RADIUS * 2.0));
        let full = pulled >= 1.0;
        let (disc, arrow) = if full {
            (PALETTE.button, PALETTE.window)
        } else {
            (Color32::from_gray(48), PALETTE.text)
        };
        let fade = 0.4 + 0.6 * pulled;
        let painter = ui.painter().with_clip_rect(page);
        painter.circle_filled(center, RADIUS, disc.gamma_multiply(fade));
        let icon = match way {
            Way::Back => Icon::NavBack,
            Way::Forward => Icon::NavForward,
        };
        theme::paint_icon(ui, icon, spot, 24.0, arrow.gamma_multiply(fade));
        let name = match way {
            Way::Back => "Swiping back",
            Way::Forward => "Swiping forward",
        };
        ui.interact(spot, egui::Id::new("swipe"), egui::Sense::hover())
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A swipe: `steps` movements of `dx` (points, +: right) 0.05 s
    /// apart, each wholly left over by the window (nothing scrolls
    /// sideways), then a pause; where it went.
    fn swipe(
        swipe: &mut Swipe,
        from: f64,
        dx: f32,
        steps: usize,
        back: bool,
        forward: bool,
    ) -> Option<Way> {
        let mut went = None;
        for step in 0..steps {
            let now = from + step as f64 * 0.05;
            swipe.start(now, vec2(dx, 0.0), false, dx);
            went = went.or(swipe.pull(now, dx, true, back, forward));
        }
        // The pause that ends it.
        let end = from + steps as f64 * 0.05 + 0.3;
        swipe.start(end, Vec2::ZERO, false, 0.0);
        went.or(swipe.pull(end, 0.0, true, back, forward))
    }

    #[test]
    fn a_full_swipe_right_goes_back_and_left_forward() {
        let mut s = Swipe::default();
        assert_eq!(swipe(&mut s, 0.0, 40.0, 6, true, true), Some(Way::Back));
        assert_eq!(
            swipe(&mut s, 10.0, -40.0, 6, true, true),
            Some(Way::Forward)
        );
    }

    #[test]
    fn a_short_swipe_or_none_to_go_to_goes_nowhere() {
        let mut s = Swipe::default();
        // 120 points: not full.
        assert_eq!(swipe(&mut s, 0.0, 40.0, 3, true, true), None);
        // Full, but there is no page back.
        assert_eq!(swipe(&mut s, 10.0, 40.0, 6, false, true), None);
        assert_eq!(s.shown(), None);
    }

    /// While the fingers move the arrow shows how far; it does not go until
    /// they lift, and taken back (swiped the other way) it goes nowhere.
    #[test]
    fn it_goes_only_when_let_go_full() {
        let mut s = Swipe::default();
        for step in 0..6 {
            let now = step as f64 * 0.05;
            s.start(now, vec2(40.0, 0.0), false, 40.0);
            assert_eq!(s.pull(now, 40.0, true, true, true), None);
        }
        assert_eq!(s.shown(), Some((Way::Back, 1.0)));
        // Back the other way before letting go.
        for step in 6..12 {
            let now = step as f64 * 0.05;
            s.start(now, vec2(-40.0, 0.0), false, -40.0);
            assert_eq!(s.pull(now, -40.0, true, true, false), None);
        }
        s.start(1.0, Vec2::ZERO, false, 0.0);
        assert_eq!(s.pull(1.0, 0.0, true, true, false), None);
    }

    /// A shelf that scrolls sideways takes a swipe that starts over it;
    /// one that starts up or down never pulls; nor one while the player
    /// page is open.
    #[test]
    fn a_swipe_a_shelf_takes_or_that_starts_up_or_down_goes_nowhere() {
        let mut s = Swipe::default();
        let mut went = None;
        for step in 0..6 {
            let now = step as f64 * 0.05;
            s.start(now, vec2(40.0, 0.0), false, 40.0);
            // The shelf took all of it.
            went = went.or(s.pull(now, 0.0, true, true, true));
        }
        assert_eq!(went, None);
        assert_eq!(s.state, State::Ignored);
        // Up and down, a little sideways.
        let mut went = None;
        for step in 0..12 {
            let now = 10.0 + step as f64 * 0.05;
            s.start(now, vec2(30.0, -60.0), false, 30.0);
            went = went.or(s.pull(now, 30.0, true, true, true));
        }
        assert_eq!(went, None);
        // The player page open.
        let mut went = None;
        for step in 0..6 {
            let now = 20.0 + step as f64 * 0.05;
            s.start(now, vec2(40.0, 0.0), false, 40.0);
            went = went.or(s.pull(now, 40.0, false, true, true));
        }
        assert_eq!(went, None);
    }

    /// Where the platform says the fingers lifted (a Mac), it goes then,
    /// and the glide that follows does not pull again.
    #[test]
    fn a_lift_goes_at_once_and_its_glide_is_ignored() {
        let mut s = Swipe::default();
        for step in 0..6 {
            let now = step as f64 * 0.02;
            s.start(now, vec2(40.0, 0.0), false, 40.0);
            assert_eq!(s.pull(now, 40.0, true, true, true), None);
        }
        s.start(0.12, Vec2::ZERO, true, 0.0);
        assert_eq!(s.pull(0.12, 0.0, true, true, true), Some(Way::Back));
        // The glide: more of the same, at once.
        for step in 0..20 {
            let now = 0.13 + step as f64 * 0.02;
            s.start(now, vec2(30.0, 0.0), false, 30.0);
            assert_eq!(s.pull(now, 30.0, true, true, true), None);
        }
    }
}
