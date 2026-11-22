//! UI tests: a sideways swipe over a message-list row on a trackpad runs what File › Options ›
//! Mail says for that way on that message: once let go past the distance, or as it gets there
//! with motion off. A scroll up or down, a short swipe and a mouse's wheel do nothing; in a
//! language written right to left the ways are mirrored.

use egui::accesskit::Role;
use egui::{MouseWheelSource, MouseWheelUnit, TouchPhase};
use mailcraft_engine::model::MessageKey;

use super::*;

const SIZE: egui::Vec2 = egui::vec2(1280.0, 800.0);

/// The message rows as a screen reader is told of them, from the top, with their messages.
fn rows(ctx: &egui::Context, a: &mut MailApp) -> Vec<(MessageKey, egui::Rect)> {
    let mut rects: Vec<egui::Rect> = named(ctx, a, SIZE).into_iter().filter(|n| n.role == Role::ListItem).filter_map(|n| n.rect).collect();
    rects.sort_by(|x, y| x.min.y.total_cmp(&y.min.y));
    a.session.row_keys().into_iter().zip(rects).collect()
}

fn scroll(dx: f32, dy: f32, phase: TouchPhase, source: MouseWheelSource, unit: MouseWheelUnit) -> Event {
    Event::MouseWheel { unit, delta: egui::vec2(dx, dy), phase, source, modifiers: Modifiers::NONE }
}

/// Fingers on a trackpad at `at`, moving by each of `moves` a frame, then lifted; then the frames
/// for the row to settle.
fn swipe(ctx: &egui::Context, a: &mut MailApp, at: egui::Pos2, moves: &[(f32, f32)]) {
    let pad = |dx, dy, phase| scroll(dx, dy, phase, MouseWheelSource::Trackpad, MouseWheelUnit::Point);
    frame(ctx, a, vec![Event::PointerMoved(at), pad(0.0, 0.0, TouchPhase::Start)]);
    for (dx, dy) in moves {
        frame(ctx, a, vec![pad(*dx, *dy, TouchPhase::Move)]);
    }
    frame(ctx, a, vec![pad(0.0, 0.0, TouchPhase::End)]);
    for _ in 0..12 {
        frame(ctx, a, vec![]);
    }
}

fn seen(a: &MailApp, k: &MessageKey) -> bool {
    a.session.summary(k).is_some_and(|m| m.flags.seen)
}

fn started(name: &str) -> (MailApp, egui::Context) {
    let (mut a, _) = app(name);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frame(&ctx, &mut a, vec![]);
    frame(&ctx, &mut a, vec![]);
    (a, ctx)
}

#[test]
fn a_swipe_left_archives_and_right_marks_read() {
    let (mut a, ctx) = started("swipe-archive");
    let before = a.session.list.len();
    let (k, r) = rows(&ctx, &mut a).get(1).cloned().expect("a second row");
    swipe(&ctx, &mut a, r.center(), &[(-40.0, 1.0), (-40.0, 0.0), (-40.0, -1.0)]);
    assert!(a.session.list.iter().all(|m| m.key != k), "archived");
    assert_eq!(a.session.list.len(), before - 1);
    a.run("edit.undo", json!({}));
    assert_eq!(a.session.list.len(), before, "back with Undo");
    frame(&ctx, &mut a, vec![]);

    let (k, r) = rows(&ctx, &mut a).first().cloned().expect("a row");
    let was = seen(&a, &k);
    swipe(&ctx, &mut a, r.center(), &[(50.0, 0.0), (50.0, 0.0), (20.0, 0.0)]);
    assert_ne!(seen(&a, &k), was, "marked");
    assert_eq!(a.session.list.len(), before);
}

#[test]
fn up_and_down_short_swipes_and_a_wheel_do_nothing() {
    let (mut a, ctx) = started("swipe-nothing");
    let marks = |a: &MailApp| a.session.list.iter().map(|m| (m.key.clone(), m.flags.seen, m.flags.flagged)).collect::<Vec<_>>();
    let before = marks(&a);
    let (_, r) = rows(&ctx, &mut a).get(1).cloned().expect("a second row");
    // Up and down first, sideways after: the list's scroll to its end.
    swipe(&ctx, &mut a, r.center(), &[(-2.0, -20.0), (-80.0, 0.0), (-80.0, 0.0)]);
    // Sideways, short of the distance.
    swipe(&ctx, &mut a, r.center(), &[(-30.0, 0.0), (-30.0, 0.0)]);
    // A mouse's wheel tilted, in lines.
    let wheel = |dx| scroll(dx, 0.0, TouchPhase::Move, MouseWheelSource::Wheel, MouseWheelUnit::Line);
    frame(&ctx, &mut a, vec![Event::PointerMoved(r.center()), wheel(-10.0)]);
    for _ in 0..20 {
        frame(&ctx, &mut a, vec![wheel(-10.0)]);
    }
    assert_eq!(marks(&a), before, "nothing done");
    // The momentum after fingers lift isn't a swipe either.
    let coasting = |dx| scroll(dx, 0.0, TouchPhase::Move, MouseWheelSource::Momentum, MouseWheelUnit::Point);
    for _ in 0..20 {
        frame(&ctx, &mut a, vec![coasting(-30.0)]);
    }
    assert_eq!(marks(&a), before, "nothing done");
}

/// With motion off the row doesn't slide, and the action runs as the swipe reaches the distance,
/// once; where the system doesn't say when the fingers lift, the swipe ends when the scrolls stop.
#[test]
fn with_motion_off_it_runs_at_the_distance() {
    let (mut a, ctx) = started("swipe-still");
    a.run("view.animations", json!({"on": false}));
    a.run("view.swipe", json!({"start": "flag"}));
    let (k, r) = rows(&ctx, &mut a).first().cloned().expect("a row");
    let flagged = |a: &MailApp| a.session.summary(&k).is_some_and(|m| m.flags.flagged);
    let was = flagged(&a);
    let pad = |dx| scroll(dx, 0.0, TouchPhase::Move, MouseWheelSource::Unknown, MouseWheelUnit::Point);
    frame(&ctx, &mut a, vec![Event::PointerMoved(r.center()), pad(-60.0)]);
    assert_eq!(flagged(&a), was, "short of the distance");
    frame(&ctx, &mut a, vec![pad(-60.0)]);
    assert_ne!(flagged(&a), was, "at the distance, before the fingers lift");
    for _ in 0..4 {
        frame(&ctx, &mut a, vec![pad(-30.0)]);
    }
    for _ in 0..30 {
        frame(&ctx, &mut a, vec![]);
    }
    assert_ne!(flagged(&a), was, "once");
}

#[test]
fn right_to_left_the_ways_are_mirrored() {
    let (mut a, ctx) = started("swipe-rtl");
    a.ui.language = "he".into();
    frame(&ctx, &mut a, vec![]);
    frame(&ctx, &mut a, vec![]);
    let (k, r) = rows(&ctx, &mut a).first().cloned().expect("a row");
    let was = seen(&a, &k);
    // To the left: toward the end of a Hebrew line, Mark as read.
    swipe(&ctx, &mut a, r.center(), &[(-60.0, 0.0), (-60.0, 0.0)]);
    assert_ne!(seen(&a, &k), was, "marked");
    // To the right: toward its start, Archive.
    swipe(&ctx, &mut a, r.center(), &[(60.0, 0.0), (60.0, 0.0)]);
    assert!(a.session.list.iter().all(|m| m.key != k), "archived");
}

/// The row slides with the fingers over what the swipe does, and back once they lift short of
/// the distance.
#[test]
fn the_row_slides_over_what_the_swipe_does_and_back() {
    let (mut a, ctx) = started("swipe-slide");
    a.run("view.swipe", json!({"start": "read"}));
    let (k, r) = rows(&ctx, &mut a).first().cloned().expect("a row");
    // The row's band, past the panes on its left: slid, its words leave its sides.
    let in_row = |d: &Drawn| (r.top()..=r.bottom()).contains(&d.rect.center().y) && d.rect.right() > r.left();
    let words = |ctx: &egui::Context, a: &mut MailApp, events| drawn_with(ctx, a, SIZE, events).into_iter().filter(in_row).collect::<Vec<_>>();
    let before = words(&ctx, &mut a, vec![]);
    // Its longest words, the preview line: far enough from its start to stay in view.
    let first = before
        .iter()
        .filter(|d| r.contains(d.rect.center()))
        .max_by_key(|d| d.text.len())
        .map(|d| (d.text.clone(), d.rect.left()))
        .expect("the row's words");
    let left_of = |drawn: &[Drawn]| drawn.iter().find(|d| d.text == first.0).map(|d| d.rect.left());
    let pad = |dx, phase| scroll(dx, 0.0, phase, MouseWheelSource::Trackpad, MouseWheelUnit::Point);
    frame(&ctx, &mut a, vec![Event::PointerMoved(r.center()), pad(0.0, TouchPhase::Start)]);
    frame(&ctx, &mut a, vec![pad(-30.0, TouchPhase::Move)]);
    let mid = words(&ctx, &mut a, vec![pad(-40.0, TouchPhase::Move)]);
    let label = if seen(&a, &k) { "Mark as unread" } else { "Mark as read" };
    assert!(mid.iter().any(|d| d.text == label), "what it does, under the row: {:?}", mid.iter().map(|d| &d.text).collect::<Vec<_>>());
    let slid = left_of(&mid).expect("the row's words, slid");
    assert!((slid - (first.1 - 70.0)).abs() < 1.0, "slid with the fingers: {slid} from {}", first.1);
    frame(&ctx, &mut a, vec![pad(0.0, TouchPhase::End)]);
    for _ in 0..15 {
        frame(&ctx, &mut a, vec![]);
    }
    let after = words(&ctx, &mut a, vec![]);
    assert_eq!(left_of(&after), Some(first.1), "back where it was");
    assert!(after.iter().all(|d| d.text != label));
}

/// File › Options › Mail says what each way does, left and right, and sets it.
#[test]
fn options_set_what_each_way_does() {
    let tall = egui::vec2(1280.0, 2000.0);
    let _tall = crate::tests::windows::layout::Dressed::new(tall, "");
    let (mut a, ctx) = started("swipe-options");
    a.ui.backstage = Some(crate::backstage::Backstage::new(crate::backstage::Page::Options));
    frame(&ctx, &mut a, vec![]);
    // In the order they're drawn, top to bottom: AccessKit's list comes from a hash map, its
    // order not the page's (it changed as File › Options grew).
    let boxes = |ctx: &egui::Context, a: &mut MailApp| {
        let mut b: Vec<Named> =
            named(ctx, a, tall).into_iter().filter(|n| n.role == Role::ComboBox && n.name().is_some_and(|l| l.starts_with("Swipe "))).collect();
        b.sort_by(|x, y| x.rect.map(|r| (r.min.y, r.min.x)).partial_cmp(&y.rect.map(|r| (r.min.y, r.min.x))).unwrap_or(std::cmp::Ordering::Equal));
        b
    };
    let shown = boxes(&ctx, &mut a);
    assert_eq!(shown.iter().filter_map(Named::name).collect::<Vec<_>>(), ["Swipe left", "Swipe right"]);
    let left = shown.first().and_then(Named::centre).expect("Swipe left");
    click(&ctx, &mut a, left);
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Flag or unflag");
    frame(&ctx, &mut a, vec![]);
    assert_eq!(a.session.prefs.swipe_start, mailcraft_engine::SwipeAction::Flag);
    assert_eq!(a.session.prefs.swipe_end, mailcraft_engine::SwipeAction::Read, "the other way kept");
}
