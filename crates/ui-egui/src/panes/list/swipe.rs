//! A sideways swipe over a message-list row on a trackpad: the row slides with the fingers over
//! the colour, icon and name of what the swipe does. Let go past [`RUN_AT`], that runs on the
//! message (as the row's own buttons run theirs, `list.rowAction`); short of it, the row slides
//! back. Toward the start of a line (to the left, in a language written left to right) and toward
//! its end each do what File › Options › Mail says. With motion off the row doesn't slide: the
//! action runs as the swipe reaches the distance.
//!
//! A scroll swipes only when it's sideways from its start: one that starts up or down is the
//! list's to its end. A mouse's wheel never swipes (tilted, or with Shift), nor the momentum the
//! system adds after the fingers lift.

use std::time::Duration;

use egui::emath::TSTransform;
use egui::layers::ShapeIdx;
use egui::{Align2, Event, FontId, Id, MouseWheelSource, MouseWheelUnit, TouchPhase, Vec2};
use mailcraft_engine::SwipeAction;
use mailcraft_engine::model::MessageKey;

use super::*;

/// How far a row is swiped before letting go runs what the swipe does.
pub(super) const RUN_AT: f32 = 96.0;
/// How far a scroll goes before it's taken as sideways or not.
const DECIDE_AT: f32 = 6.0;
/// Where the system doesn't say when the fingers lift (a Mac does), the swipe ends when no scroll
/// has come for this long.
const QUIET_SECS: f64 = 0.2;
/// The slide back, let go short of the distance (or after a mark, which leaves the row).
const BACK_SECS: f32 = 0.15;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Way {
    Undecided,
    Sideways,
    Upright,
}

/// One scroll on the trackpad, from the fingers touching it to their lifting.
#[derive(Clone, Debug)]
struct Gesture {
    /// The row it's over, once it's sideways.
    key: Option<MessageKey>,
    way: Way,
    /// How far it's gone, in points, the way the content moves (right and down positive).
    dx: f32,
    dy: f32,
    /// The system said when the fingers touched: it says when they lift.
    phased: bool,
    /// The last scroll's time.
    last: f64,
    /// When it ended: the row slides back from then.
    ended: Option<f64>,
    /// What it does ran (at the distance with motion off, else on letting go).
    ran: bool,
}

/// The swipe while the list's rows are drawn in a frame.
pub(super) struct Swipe {
    g: Option<Gesture>,
    /// It ended this frame.
    ended_now: bool,
    now: f64,
}

fn state_id() -> Id {
    Id::unique("list-swipe")
}

/// What a swipe the content's way `dx` does: toward the start of a line is to the left, or to
/// the right in a language written right to left.
fn action_of(s: &Session, dx: f32) -> SwipeAction {
    let toward_start = (dx < 0.0) != crate::i18n::current().rtl();
    if toward_start { s.prefs.swipe_start } else { s.prefs.swipe_end }
}

impl Swipe {
    /// This frame's scrolls from fingers on a trackpad; while a row is swiped, the list doesn't
    /// scroll up or down with them.
    pub(super) fn begin(ui: &Ui) -> Swipe {
        let now = ui.input(|i| i.time);
        let mut g: Option<Gesture> = ui.data(|d| d.get_temp(state_id()));
        let (mut moved, mut touched, mut lifted, mut any) = (Vec2::ZERO, false, false, false);
        ui.input(|i| {
            for e in &i.events {
                // Points, from fingers: a wheel's lines aren't a swipe, nor the momentum after the
                // fingers lift, nor a scroll with Ctrl (zooming) or another key held.
                let Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta,
                    phase,
                    source: MouseWheelSource::Trackpad | MouseWheelSource::Unknown,
                    modifiers,
                } = e
                else {
                    continue;
                };
                if !modifiers.is_none() {
                    continue;
                }
                match phase {
                    TouchPhase::Start => touched = true,
                    TouchPhase::End | TouchPhase::Cancel => lifted = true,
                    TouchPhase::Move => {}
                }
                moved += *delta;
                any = true;
            }
        });
        // A new one: the fingers touched, or scrolls came after the last one ended.
        if touched || (any && g.as_ref().is_none_or(|g| g.ended.is_some())) {
            g = Some(Gesture { key: None, way: Way::Undecided, dx: 0.0, dy: 0.0, phased: touched, last: now, ended: None, ran: false });
        }
        let mut ended_now = false;
        if let Some(g) = g.as_mut().filter(|g| g.ended.is_none()) {
            g.dx += moved.x;
            g.dy += moved.y;
            if any {
                g.last = now;
            }
            if g.way == Way::Undecided && g.dx.abs().max(g.dy.abs()) >= DECIDE_AT {
                g.way = if g.dx.abs() > 2.0 * g.dy.abs() { Way::Sideways } else { Way::Upright };
            }
            if lifted || (!g.phased && now - g.last > QUIET_SECS) {
                g.ended = Some(now);
                ended_now = true;
            }
        }
        if g.as_ref().is_some_and(|g| g.way == Way::Sideways && g.ended.is_none()) {
            ui.ctx().input_mut(|i| i.smooth_scroll_delta.y = 0.0);
        }
        Swipe { g, ended_now, now }
    }

    /// A message row, slid by the swipe over it (if any) over what the swipe does.
    pub(super) fn row(&mut self, s: &Session, t: &theme::Tokens, ui: &mut Ui, of: RowOf<'_>, acts: &mut Vec<Action>) -> egui::Response {
        let r = of.r;
        let Some(g) = self.g.as_mut() else { return message_row(s, t, ui, of, acts) };
        // The row under the pointer as the scroll turned out sideways.
        if g.key.is_none() && g.way == Way::Sideways && g.ended.is_none() && ui.rect_contains_pointer(r) {
            g.key = Some(of.m.key.clone());
        }
        if g.key.as_ref() != Some(&of.m.key) {
            return message_row(s, t, ui, of, acts);
        }
        let action = action_of(s, g.dx);
        let still = !crate::motion::enabled(ui.ctx());
        let reach = r.width() * 0.75;
        let swiping = g.ended.is_none() && !still && action != SwipeAction::Off;
        let target = if swiping { g.dx.clamp(-reach, reach) } else { 0.0 };
        let secs = if swiping || still { 0.0 } else { BACK_SECS };
        let dx = ui.ctx().animate_value_with_time(Id::unique(("list-swipe", &of.m.key)), target, secs);
        if dx.abs() < 0.5 {
            return message_row(s, t, ui, of, acts);
        }
        let clip = r.intersect(ui.clip_rect());
        let p = ui.painter().with_clip_rect(clip);
        behind(&p, t, r, dx, action, of.m);
        p.rect_filled(r.translate(vec2(dx, 0.0)), 0.0, t.surface);
        // The row drawn where it is, then its shapes moved over by the swipe (inside its own
        // room): its buttons and what a screen reader is told stay where the row is.
        let layer = ui.layer_id();
        let from = ui.ctx().graphics(|gr| gr.get(layer).map_or(0, |l| l.next_idx().0));
        let resp = message_row(s, t, ui, of, acts);
        let slide = TSTransform::from_translation(vec2(dx, 0.0));
        ui.ctx().graphics_mut(|gr| {
            let list = gr.entry(layer);
            for i in from..list.next_idx().0 {
                list.mutate_shape(ShapeIdx(i), |c| {
                    let kept = c.clip_rect.intersect(clip);
                    c.transform(slide);
                    c.clip_rect = kept;
                });
            }
        });
        resp
    }

    /// After the rows: what the swipe does runs once it's let go past the distance (with motion
    /// off, as it gets there), and the swipe is kept for the next frame until the row is back.
    pub(super) fn end(self, s: &Session, ui: &Ui, acts: &mut Vec<Action>) {
        let still = !crate::motion::enabled(ui.ctx());
        let Swipe { mut g, ended_now, now } = self;
        if let Some(g) = g.as_mut()
            && let Some(key) = g.key.clone()
            && g.way == Way::Sideways
            && !g.ran
            && g.dx.abs() >= RUN_AT
            && (ended_now || (still && g.ended.is_none()))
        {
            g.ran = true;
            let action = action_of(s, g.dx);
            if action != SwipeAction::Off {
                acts.push(Action::Run("list.rowAction", json!({"key": key.to_id(), "action": action.code()})));
            }
        }
        let g = g.filter(|g| g.ended.is_none_or(|at| now - at <= f64::from(BACK_SECS)));
        match &g {
            Some(g) if g.ended.is_some() => ui.ctx().request_repaint(),
            // Its end is when the scrolls stop.
            Some(g) if !g.phased => ui.ctx().request_repaint_after(Duration::from_secs_f64(QUIET_SECS * 1.5)),
            _ => {}
        }
        ui.data_mut(|d| match g {
            Some(g) => {
                d.insert_temp(state_id(), g);
            }
            None => d.remove::<Gesture>(state_id()),
        });
    }
}

/// What the swipe does, in the room the row leaves: its colour, faded short of the distance (let
/// go there, nothing happens), with its icon and name at the row's edge.
fn behind(p: &egui::Painter, t: &theme::Tokens, r: Rect, dx: f32, action: SwipeAction, m: &Summary) {
    let (fill, icon, name) = match action {
        SwipeAction::Archive => (t.ok, Icon::Archive, tl!("Archive")),
        SwipeAction::Delete => (t.danger, Icon::Delete, tl!("Delete")),
        SwipeAction::Read => (t.accent, Icon::Read, if m.flags.seen { tl!("Mark as unread") } else { tl!("Mark as read") }),
        SwipeAction::Flag => (t.flag, Icon::Flag, if m.flags.flagged { tl!("Unflag") } else { tl!("Flag") }),
        SwipeAction::Off => return,
    };
    let fill = if dx.abs() >= RUN_AT { fill } else { fill.gamma_multiply(0.45) };
    p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 6.0, fill);
    let x = if dx < 0.0 { r.max.x - 36.0 } else { r.min.x + 36.0 };
    let icon_r = Rect::from_center_size(pos2(x, r.center().y - 8.0), vec2(18.0, 18.0));
    icons::paint(p, icon_r, icon, t.on_accent);
    p.text(pos2(x, icon_r.max.y + 3.0), Align2::CENTER_TOP, name, FontId::proportional(11.0), t.on_accent);
}
