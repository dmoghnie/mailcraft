//! The ribbon's tabs in Calendar, People, Tasks and Notes.

use super::*;

const SIZE: egui::Vec2 = egui::vec2(1280.0, 800.0);

/// Issue #142: in Calendar, People, Tasks and Notes, Send / Receive and View changed nothing, the
/// module's Home buttons always shown. Each tab now shows its own, as Outlook's: Home the
/// module's, Send / Receive the same as in Mail, View how the module shows what it holds, with
/// the folder pane.
#[test]
fn each_modules_tabs_show_their_own_buttons() {
    let (mut a, _) = app("module-ribbon");
    let ctx = egui::Context::default();
    // The words drawn on the ribbon (its buttons, under the tabs).
    let shown = |a: &mut MailApp, tab| {
        a.ui.tab = tab;
        for _ in 0..3 {
            frame(&ctx, a, vec![]);
        }
        let texts = drawn_text(&ctx, a, SIZE);
        let ribbon = a.ui.ribbon_rect.expect("the ribbon drawn");
        texts.into_iter().filter(|d| ribbon.contains(d.rect.center())).map(|d| d.text).collect::<Vec<String>>()
    };
    let is = |texts: &[String], what: &str| texts.iter().any(|t| t == what);
    for (module, home, view) in [
        ("calendar", "New event", "Month"),
        ("tasks", "New task", "Hide completed"),
        ("notes", "New note", "Arrange by"),
        ("people", "New contact", "Cards"),
    ] {
        a.run("view.module", json!({"name": module}));
        let h = shown(&mut a, crate::RibbonTab::Home);
        assert!(is(&h, home) && !is(&h, "Send / Receive all folders"), "{module} Home: {h:?}");
        let v = shown(&mut a, crate::RibbonTab::View);
        assert!(is(&v, view) && is(&v, "Folder pane") && !is(&v, home), "{module} View: {v:?}");
        let sr = shown(&mut a, crate::RibbonTab::SendReceive);
        assert!(is(&sr, "Send / Receive all folders") && is(&sr, "Update folder") && !is(&sr, home), "{module} Send / Receive: {sr:?}");
    }
}

/// Today, the views, New event and Side by side were each shown twice in Calendar's week (the
/// ribbon, and the header or the side pane), which looked like two things. Each is shown once:
/// Today and the views in the header, New event and Side by side on the ribbon, or in the side
/// pane while the ribbon is hidden.
#[test]
fn each_calendar_control_is_shown_once() {
    let (mut a, _) = app("calendar-once");
    let ctx = egui::Context::default();
    a.run("view.module", json!({"name": "calendar"}));
    a.run("calendar.view", json!({"mode": "week"}));
    let counts = |a: &mut MailApp| {
        for _ in 0..3 {
            frame(&ctx, a, vec![]);
        }
        let texts: Vec<String> = drawn_text(&ctx, a, SIZE).into_iter().map(|d| d.text).collect();
        let n = |what: &str| texts.iter().filter(|t| *t == what).count();
        ["Today", "Day", "Work week", "Week", "Month", "Schedule", "New event", "Side by side"].map(|w| (w, n(w)))
    };
    for (what, n) in counts(&mut a) {
        assert_eq!(n, 1, "{what} shown {n} times");
    }
    a.run("view.hideRibbon", json!({}));
    assert!(a.session.prefs.ribbon_hidden);
    for (what, n) in counts(&mut a) {
        assert_eq!(n, 1, "with the ribbon hidden, {what} shown {n} times");
    }
}
