//! The reading pane's header, kept at the top while the message's text scrolls (issue #151).

use super::*;

/// A message of 200 lines, put first in the Inbox and opened.
fn long_message(a: &mut MailApp) {
    let lines: String = (1..=200).map(|n| format!("Line {n} of the plan\r\n")).collect();
    let raw = format!(
        "From: Sam Okafor <sam@studio.example>\r\nTo: Riley Moreno <riley@studio.example>\r\nSubject: Quarterly plan\r\n\
         Message-ID: <plan@studio.example>\r\nDate: Tue, 13 Oct 2026 09:00:00 +0000\r\n\r\n{lines}"
    );
    let key = with_raw(a, 9601, raw.as_bytes());
    a.run("list.open", json!({"key": key.to_id()}));
}

/// Where `what` (the start of a text drawn) is in the reading pane, if it can be seen: the last
/// drawn (the message list, drawn first, shows its subject and first line too).
fn seen_at(texts: &[Drawn], what: &str) -> Option<egui::Rect> {
    texts.iter().rev().find(|d| d.text.starts_with(what) && d.seen()).map(|d| d.rect)
}

/// Scrolled down with the mouse wheel over the message, as far as `by` points, and settled.
fn scroll(ctx: &egui::Context, a: &mut MailApp, size: egui::Vec2, over: egui::Pos2, by: f32) {
    let wheel = Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -by),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
    };
    let _ = drawn_with(ctx, a, size, vec![Event::PointerMoved(over), wheel]);
    for _ in 0..30 {
        let _ = drawn_with(ctx, a, size, vec![Event::PointerMoved(over)]);
    }
}

/// Issue #151: the subject, who it's from and its actions stay at the top of the reading pane
/// while the text scrolls under them, as in Outlook; they scrolled away with it. In a pane too
/// short for that, all of it scrolls, as before.
#[test]
fn the_header_stays_at_the_top_as_the_text_scrolls() {
    for (size, pinned) in [(egui::vec2(1280.0, 800.0), true), (egui::vec2(1280.0, 330.0), false)] {
        let (mut a, _) = app(if pinned { "pinned-header" } else { "pinned-header-short" });
        let ctx = egui::Context::default();
        long_message(&mut a);
        for _ in 0..5 {
            let _ = drawn_with(&ctx, &mut a, size, vec![]);
        }
        let before = drawn_with(&ctx, &mut a, size, vec![]);
        let subject = seen_at(&before, "Quarterly plan").expect("the subject");
        let text = seen_at(&before, "Line 1 of the plan").expect("the text");
        scroll(&ctx, &mut a, size, egui::pos2(subject.center().x, text.top() + 20.0), 300.0);
        let after = drawn_with(&ctx, &mut a, size, vec![]);
        let line_one = seen_at(&after, "Line 1 of the plan");
        assert!(line_one.is_none_or(|r| r.top() < text.top() - 100.0), "the text scrolled: {text:?} {line_one:?} ({size:?})");
        // The reading pane's subject, where it starts across (not the list's).
        let subject_after =
            after.iter().find(|d| d.text == "Quarterly plan" && d.seen() && (d.rect.left() - subject.left()).abs() < 1.0).map(|d| d.rect);
        if pinned {
            assert!(subject_after.is_some_and(|r| (r.top() - subject.top()).abs() < 1.0), "the header stays: {subject:?} {subject_after:?}");
            assert!(seen_at(&after, "Sam Okafor").is_some(), "who it's from, still seen");
        } else {
            assert!(subject_after.is_none_or(|r| r.top() < subject.top() - 100.0), "a short pane scrolls it all: {subject_after:?}");
        }
    }
}

/// The open message's Reply, Reply all, Forward, React and More are icons beside who it's from,
/// over its date, as Outlook's, and its text starts close under the header: on a row of their
/// own under the header they put the text below the middle of a short pane. A pane too narrow
/// for them there keeps that row.
#[test]
fn a_messages_actions_are_beside_who_its_from() {
    for (size, beside) in [(egui::vec2(1280.0, 800.0), true), (egui::vec2(820.0, 800.0), false)] {
        let (mut a, _) = app(if beside { "actions-beside" } else { "actions-under" });
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        long_message(&mut a);
        for _ in 0..5 {
            let _ = drawn_with(&ctx, &mut a, size, vec![]);
        }
        // The reading pane's (the ribbon's is higher up).
        let nodes = named(&ctx, &mut a, size);
        let reply_all =
            nodes.iter().filter(|n| n.label.as_deref() == Some("Reply all")).filter_map(|n| n.rect).max_by(|x, y| x.top().total_cmp(&y.top()));
        let drawn = drawn_with(&ctx, &mut a, size, vec![]);
        let from = seen_at(&drawn, "Sam Okafor").expect("who it's from");
        let text = seen_at(&drawn, "Line 1 of the plan").expect("the text");
        let reply_all = reply_all.expect("the reading pane's Reply all");
        if beside {
            assert!(reply_all.left() > from.right() && reply_all.top() < from.bottom(), "beside the sender: {reply_all:?} {from:?}");
            assert!(text.top() - from.bottom() < 110.0, "the text close under the header: {text:?} {from:?}");
        } else {
            assert!(reply_all.top() > from.bottom(), "under the sender in a narrow pane: {reply_all:?} {from:?} ({size:?})");
        }
    }
}
