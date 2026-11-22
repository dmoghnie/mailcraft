//! Find a command (the command palette): opened from the keyboard or from the search box, a
//! command found by part of its name runs; one that can't run now doesn't.

use super::*;

fn opened(ctx: &egui::Context, a: &mut MailApp) {
    for _ in 0..2 {
        frame(ctx, a, vec![]);
    }
    frame(ctx, a, vec![key(Key::P, Modifiers::COMMAND | Modifiers::SHIFT)]);
    // It takes the keyboard as it opens.
    frame(ctx, a, vec![]);
    assert!(matches!(a.ui.dialog, Some(Dialog::Commands { .. })), "{:?}", a.ui.dialog.as_ref().map(std::mem::discriminant));
}

#[test]
fn a_command_found_by_part_of_its_name_runs() {
    let (mut a, _) = app("palette-runs");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let minimized = a.session.prefs.folder_pane_minimized;
    opened(&ctx, &mut a);
    frame(&ctx, &mut a, vec![Event::Text("minimize the fol".into())]);
    // The box and the row found are named to a screen reader.
    let nodes = named(&ctx, &mut a, egui::vec2(1280.0, 800.0));
    let labelled = |what: &str| nodes.iter().any(|n| n.label.as_deref() == Some(what));
    assert!(nodes.iter().any(|n| n.role == egui::accesskit::Role::TextInput && n.label.as_deref() == Some("Find a command")), "the box isn't named");
    assert!(labelled("Minimize the folder pane"), "the row isn't named");
    frame(&ctx, &mut a, vec![key(Key::Enter, Modifiers::NONE)]);
    frame(&ctx, &mut a, vec![]);
    assert_eq!(a.session.prefs.folder_pane_minimized, !minimized, "the command didn't run");
    assert!(a.ui.dialog.is_none(), "the list stayed open");
}

#[test]
fn a_command_that_cant_run_now_is_shown_and_not_run() {
    let (mut a, _) = app("palette-off");
    let ctx = egui::Context::default();
    opened(&ctx, &mut a);
    // Nothing is being compacted.
    frame(&ctx, &mut a, vec![Event::Text("stop compacting".into())]);
    let texts: Vec<String> = drawn_text(&ctx, &mut a, egui::vec2(1280.0, 800.0)).into_iter().map(|d| d.text).collect();
    // Its row: the name, then why it can't run.
    assert!(texts.iter().any(|t| t.starts_with("Stop compacting") && t.contains("nothing is being compacted")), "{texts:?}");
    frame(&ctx, &mut a, vec![key(Key::Enter, Modifiers::NONE)]);
    frame(&ctx, &mut a, vec![]);
    assert!(matches!(a.ui.dialog, Some(Dialog::Commands { .. })), "it ran, or the list closed");
}

#[test]
fn one_that_needs_something_opens_its_dialog() {
    let (mut a, _) = app("palette-dialog");
    let ctx = egui::Context::default();
    a.run("list.open", json!({"key": "sample:1:1:INBOX"}));
    opened(&ctx, &mut a);
    frame(&ctx, &mut a, vec![Event::Text("move to".into())]);
    frame(&ctx, &mut a, vec![key(Key::Enter, Modifiers::NONE)]);
    frame(&ctx, &mut a, vec![]);
    assert!(matches!(a.ui.dialog, Some(Dialog::MoveTo { copy: false, .. })), "{:?}", a.ui.dialog.as_ref().map(std::mem::discriminant));
}

#[test]
fn a_greater_than_sign_in_the_search_box_finds_a_command() {
    let (mut a, _) = app("palette-search-box");
    let ctx = egui::Context::default();
    for _ in 0..2 {
        frame(&ctx, &mut a, vec![]);
    }
    a.ui.focus_search = true;
    frame(&ctx, &mut a, vec![]);
    frame(&ctx, &mut a, vec![Event::Text(">".into())]);
    frame(&ctx, &mut a, vec![]);
    assert!(matches!(a.ui.dialog, Some(Dialog::Commands { .. })), "{:?}", a.ui.dialog.as_ref().map(std::mem::discriminant));
    assert!(a.ui.search_text.is_empty(), "the sign was searched for: {:?}", a.ui.search_text);
}
