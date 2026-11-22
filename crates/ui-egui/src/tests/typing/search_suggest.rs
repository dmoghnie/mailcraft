//! Suggestions as a search is typed, in the window: each named with its group for a screen
//! reader, chosen with the arrows and Enter or a click, a chip standing for its syntax, and
//! Backspace in an empty box takes the chip out.

use super::*;

const SCREEN: egui::Vec2 = egui::vec2(1400.0, 860.0);

fn names_of(ctx: &egui::Context, a: &mut MailApp) -> Vec<String> {
    named(ctx, a, SCREEN).iter().filter_map(Named::name).map(str::to_string).collect()
}

/// Into the search box, as a click puts the keyboard there.
fn into_the_box(ctx: &egui::Context, a: &mut MailApp) {
    for _ in 0..2 {
        frame(ctx, a, vec![]);
    }
    let r = ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::unique("mailcraft-top-search-input-rect"))).expect("the search box drawn");
    click(ctx, a, r.center());
    frame(ctx, a, vec![]);
}

fn press(ctx: &egui::Context, a: &mut MailApp, k: Key) {
    frame(ctx, a, vec![key(k, Modifiers::NONE)]);
    frame(ctx, a, vec![]);
}

#[test]
fn a_suggestion_chosen_becomes_a_chip() {
    let _screen = windows::layout::Dressed::new(SCREEN, "");
    let (mut a, _) = app("search-suggest");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    a.run("folder.open", json!({"account": "sample", "folder": "INBOX"}));
    into_the_box(&ctx, &mut a);
    frame(&ctx, &mut a, vec![Event::Text("ana".into())]);
    frame(&ctx, &mut a, vec![]);
    assert_eq!(a.ui.search_drop, crate::SearchDrop::Recent, "open as it's typed");
    let ana = "People: Ana Lima <ana@soundlab.example>";
    assert!(names_of(&ctx, &mut a).iter().any(|n| n == ana), "{:?}", names_of(&ctx, &mut a));
    // Down to it, and Enter: a chip, searched at once.
    press(&ctx, &mut a, Key::ArrowDown);
    press(&ctx, &mut a, Key::Enter);
    assert_eq!(a.ui.search_text, "from:ana@soundlab.example");
    assert_eq!(a.session.search.as_ref().map(|(q, _)| q.as_str()), Some("from:ana@soundlab.example"));
    let names = names_of(&ctx, &mut a);
    assert!(names.iter().any(|n| n == "Remove From: Ana Lima from the search"), "{names:?}");
    assert!(a.ui.search_focused, "the box keeps the keyboard");
    // Words typed after it go with it.
    frame(&ctx, &mut a, vec![Event::Text("mix".into())]);
    frame(&ctx, &mut a, vec![]);
    assert_eq!(a.ui.search_text, "from:ana@soundlab.example mix");
    // Backspace takes the letters, then the chip.
    for _ in 0..3 {
        press(&ctx, &mut a, Key::Backspace);
    }
    assert_eq!((a.ui.search_text.as_str(), a.ui.search_chips.len()), ("from:ana@soundlab.example", 1));
    press(&ctx, &mut a, Key::Backspace);
    assert_eq!((a.ui.search_text.as_str(), a.ui.search_chips.len()), ("", 0));
}

#[test]
fn a_date_clicked_becomes_a_chip_in_the_window_language() {
    let _screen = windows::layout::Dressed::new(SCREEN, "");
    let (mut a, _) = app("search-suggest-date");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    into_the_box(&ctx, &mut a);
    frame(&ctx, &mut a, vec![Event::Text("budget last w".into())]);
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Dates: Last week");
    frame(&ctx, &mut a, vec![]);
    assert_eq!(a.ui.search_text, "received:\"last week\" budget");
    assert_eq!(a.ui.search_chips.first().map(|c| c.text.as_str()), Some("Last week"));
    // Typed by hand instead, it stays words in the box.
    a.ui.search_text = "budget".into();
    frame(&ctx, &mut a, vec![]);
    assert!(a.ui.search_chips.is_empty());
}
