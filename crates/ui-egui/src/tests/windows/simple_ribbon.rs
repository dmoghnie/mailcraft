//! UI tests: the simplified ribbon is Home in one row of the buttons chosen for it, what doesn't
//! fit in More commands; an order made in Customize the ribbon survives a restart; key tips reach
//! every button, on the row or in More commands.

use egui::accesskit::Role;
use egui::os::OperatingSystem;

use super::*;

const WIDE: egui::Vec2 = egui::vec2(1600.0, 800.0);
const NARROW: egui::Vec2 = egui::vec2(700.0, 800.0);
/// The simplified ribbon's buttons as it starts, as a screen reader hears them.
const EIGHT: [&str; 8] = ["New mail", "Delete", "Archive", "Move to", "Reply", "Reply all", "Forward", "Flag"];

/// The app with the simplified ribbon (or the full one), its frames drawn at `size` (kept while
/// the returned guard is).
fn ribbon_app(name: &str, size: egui::Vec2, simple: bool) -> (MailApp, egui::Context, layout::Dressed) {
    let dressed = layout::Dressed::new(size, "");
    let (mut a, _) = app(name);
    a.session.prefs.simple_ribbon = simple;
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.set_os(OperatingSystem::Windows);
    settle(&ctx, &mut a);
    (a, ctx, dressed)
}

/// A few frames: the row's widths are measured, and it's laid out again with them.
fn settle(ctx: &egui::Context, a: &mut MailApp) {
    for _ in 0..4 {
        frame(ctx, a, vec![]);
    }
}

/// The buttons on the ribbon, by name, from its start.
fn on_ribbon(ctx: &egui::Context, a: &mut MailApp, size: egui::Vec2) -> Vec<(String, egui::Rect)> {
    let nodes = named(ctx, a, size);
    let ribbon = a.ui.ribbon_rect.expect("the ribbon is drawn");
    let mut on: Vec<(String, egui::Rect)> = nodes
        .iter()
        .filter(|n| n.role == Role::Button)
        .filter_map(|n| Some((n.name()?.to_string(), n.rect?)))
        .filter(|(_, r)| ribbon.contains(r.center()))
        .collect();
    on.sort_by(|a, b| a.1.left().total_cmp(&b.1.left()));
    on
}

fn names(row: &[(String, egui::Rect)]) -> Vec<&str> {
    row.iter().map(|(n, _)| n.as_str()).collect()
}

/// A click on what a screen reader names `label` in Customize the ribbon: below its title and
/// as wide as the dialog (the ribbon behind it has buttons of the same names).
fn click_in_dialog(ctx: &egui::Context, a: &mut MailApp, label: &str) {
    // The dialog centres itself again once a change sizes it anew.
    settle(ctx, a);
    let nodes = named(ctx, a, layout::SIZE.with(std::cell::Cell::get));
    let title =
        nodes.iter().find(|n| n.role == Role::Label && n.name() == Some("Customize the ribbon")).and_then(|n| n.rect).expect("the dialog is open");
    let dialog = egui::Rect::from_min_size(title.left_top() - egui::vec2(4.0, 0.0), egui::vec2(480.0, 2000.0));
    let at = nodes.iter().filter(|n| n.name() == Some(label) && !n.disabled).filter_map(Named::centre).find(|c| dialog.contains(*c));
    let at = at.unwrap_or_else(|| panic!("no {label} in the dialog: {:?}", nodes.iter().filter_map(Named::name).collect::<Vec<_>>()));
    click(ctx, a, at);
}

fn alt(pressed: bool) -> Event {
    Event::Key { key: Key::AltLeft, physical_key: None, pressed, repeat: false, modifiers: if pressed { Modifiers::ALT } else { Modifiers::NONE } }
}

/// Alt, then `letters`, each pressed and let go in a frame of its own (a key not let go is held:
/// pressed again, it repeats).
fn key_tips(ctx: &egui::Context, a: &mut MailApp, letters: &[Key]) {
    frame(ctx, a, vec![alt(true), alt(false)]);
    for k in letters {
        let up = Event::Key { key: *k, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE };
        frame(ctx, a, vec![key(*k, Modifiers::NONE), up]);
    }
    frame(ctx, a, vec![]);
}

#[test]
fn buttons_that_dont_fit_are_in_more_commands() {
    let (mut a, ctx, wide) = ribbon_app("simple-ribbon-fit", WIDE, true);
    let row = on_ribbon(&ctx, &mut a, WIDE);
    let labelled: Vec<&str> = names(&row).into_iter().filter(|n| EIGHT.contains(n) || *n == "More commands").collect();
    assert_eq!(labelled, [&EIGHT[..], &["More commands"]].concat(), "{:?}", names(&row));
    drop(wide);
    let _narrow = layout::Dressed::new(NARROW, "");
    settle(&ctx, &mut a);
    let row = on_ribbon(&ctx, &mut a, NARROW);
    let fitted: Vec<&str> = EIGHT.iter().copied().filter(|n| names(&row).contains(n)).collect();
    assert!((2..EIGHT.len()).contains(&fitted.len()), "{:?}", names(&row));
    assert_eq!(fitted, EIGHT[..fitted.len()], "the first ones, in order");
    assert_eq!(names(&row).last(), Some(&"More commands"));
    assert!(row.iter().all(|(_, r)| r.right() <= NARROW.x), "nothing past the window's edge: {row:?}");
    // More commands: those that didn't fit, then Home's others.
    click_named(&ctx, &mut a, "More commands");
    frame(&ctx, &mut a, vec![]);
    let menu: Vec<String> = named(&ctx, &mut a, NARROW).iter().filter_map(Named::name).map(str::to_string).collect();
    for name in EIGHT[fitted.len()..].iter().chain(&["Report junk", "Quick steps", "Categorize", "Pin", "Snooze", "Filter"]) {
        assert!(menu.iter().any(|m| m == name), "{name}: {menu:?}");
    }
}

#[test]
fn a_customized_order_survives_a_restart() {
    let (mut a, ctx, _dressed) = ribbon_app("simple-ribbon-arranged", WIDE, false);
    // Right-click on the ribbon: Customize the ribbon.
    let ribbon = a.ui.ribbon_rect.expect("the ribbon is drawn");
    click_at(&ctx, &mut a, ribbon.center(), egui::PointerButton::Secondary, Modifiers::NONE);
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Customize the ribbon…");
    frame(&ctx, &mut a, vec![]);
    assert!(matches!(a.ui.dialog, Some(Dialog::CustomizeRibbon { .. })));
    // Forward first; Delete off; Compact list added from every command.
    click_in_dialog(&ctx, &mut a, "Forward");
    for _ in 0..6 {
        click_in_dialog(&ctx, &mut a, "Move up");
    }
    click_in_dialog(&ctx, &mut a, "Delete");
    click_in_dialog(&ctx, &mut a, "Remove");
    settle(&ctx, &mut a);
    let nodes = named(&ctx, &mut a, WIDE);
    let search = nodes.iter().find(|n| n.role == Role::TextInput && n.name() == Some("Add a command")).and_then(Named::centre).expect("the box");
    click(&ctx, &mut a, search);
    frame(&ctx, &mut a, vec![Event::Text("compact list".into())]);
    frame(&ctx, &mut a, vec![]);
    click_in_dialog(&ctx, &mut a, "Add Compact list");
    click_in_dialog(&ctx, &mut a, "OK");
    frame(&ctx, &mut a, vec![]);
    assert!(a.ui.dialog.is_none());
    let order = ["Forward", "New mail", "Archive", "Move to", "Reply", "Reply all", "Flag", "Compact list"];
    let shown = |a: &mut MailApp| {
        let row = on_ribbon(&ctx, a, WIDE);
        names(&row).into_iter().filter(|n| order.contains(n) || *n == "Delete").map(str::to_string).collect::<Vec<_>>()
    };
    assert_eq!(shown(&mut a), order);
    // Saved, and back after a restart.
    let saved = a.saved_state();
    let (mut b, _) = app("simple-ribbon-restarted");
    b.restore_state(&saved);
    settle(&ctx, &mut b);
    assert!(b.session.prefs.simple_ribbon);
    assert_eq!(shown(&mut b), order);
}

#[test]
fn key_tips_reach_every_button_on_the_row_or_in_more_commands() {
    let (mut a, ctx, _dressed) = ribbon_app("simple-ribbon-keys", NARROW, true);
    // Two commands added: one on the row, one in More commands (past the room).
    let buttons = ["view.compact", "mail.new", "mail.delete", "mail.archive", "mail.move", "mail.forward", "mail.flag", "view.singleLine"];
    a.run("view.arrangeRibbon", json!({ "buttons": buttons }));
    frame(&ctx, &mut a, vec![key(Key::ArrowDown, Modifiers::NONE)]);
    settle(&ctx, &mut a);
    let row = on_ribbon(&ctx, &mut a, NARROW);
    assert!(names(&row).contains(&"Compact list") && !names(&row).contains(&"Single-line list"), "{:?}", names(&row));
    // Alt, H, 1 and 2: the added commands, drawn or not.
    let (compact, single) = (a.session.prefs.compact, a.session.prefs.single_line);
    key_tips(&ctx, &mut a, &[Key::H, Key::Num1]);
    assert_ne!(a.session.prefs.compact, compact);
    key_tips(&ctx, &mut a, &[Key::H, Key::Num2]);
    assert_ne!(a.session.prefs.single_line, single);
    // Alt, H, Y: More commands opens.
    key_tips(&ctx, &mut a, &[Key::H, Key::Y]);
    assert!(named(&ctx, &mut a, NARROW).iter().any(|n| n.name() == Some("Categorize")), "More commands is open");
    frame(&ctx, &mut a, vec![key(Key::Escape, Modifiers::NONE)]);
    // Alt, H, R, P: Reply, off the row, in More commands.
    assert!(!a.session.selection.is_empty());
    key_tips(&ctx, &mut a, &[Key::H, Key::R, Key::P]);
    assert!(a.session.compose.is_some(), "the reply opened");
}
