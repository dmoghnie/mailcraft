//! Mail from a mailing list: Unsubscribe over its text, asked first; in Junk Email, Block sender
//! in its place.

use super::*;

const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);

fn names(ctx: &egui::Context, a: &mut MailApp) -> Vec<Named> {
    named(ctx, a, SCREEN)
}

fn shows(ctx: &egui::Context, a: &mut MailApp, label: &str) -> bool {
    names(ctx, a).iter().any(|n| n.name() == Some(label))
}

/// Click the button called `label`, other than one at `not` (the bar's, under a dialog). Where
/// it was.
fn click_button(ctx: &egui::Context, a: &mut MailApp, label: &str, not: Option<egui::Pos2>) -> egui::Pos2 {
    let all = names(ctx, a);
    let at = all
        .iter()
        .filter(|n| n.name() == Some(label) && n.role == egui::accesskit::Role::Button)
        .filter_map(Named::centre)
        .find(|p| not.is_none_or(|q| p.distance(q) > 2.0));
    let at = at.unwrap_or_else(|| panic!("no {label} to click: {:?}", all.iter().filter_map(Named::name).collect::<Vec<_>>()));
    click(ctx, a, at);
    at
}

fn opened(ctx: &egui::Context, a: &mut MailApp, folder: &str, subject: &str) {
    a.run("folder.open", json!({"account": "sample", "folder": folder}));
    let key = a.session.list.iter().find(|m| m.subject == subject).map(|m| m.key.clone()).unwrap();
    a.run("list.open", json!({"key": key.to_id()}));
    for _ in 0..3 {
        frame(ctx, a, vec![]);
    }
}

#[test]
fn unsubscribing_asks_first_then_sends_the_lists_message() {
    let _screen = windows::layout::Dressed::new(SCREEN, "");
    let (mut a, _) = app("unsubscribe-bar");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frame(&ctx, &mut a, vec![]);
    a.run("folder.open", json!({"account": "sample", "folder": "Sent"}));
    let before = a.session.list.len();
    opened(&ctx, &mut a, "INBOX", "Open call: winter group show");
    assert!(shows(&ctx, &mut a, "From a mailing list."));
    let bar = click_button(&ctx, &mut a, "Unsubscribe", None);
    frame(&ctx, &mut a, vec![]);
    assert!(matches!(a.ui.dialog, Some(Dialog::Unsubscribe { .. })), "not asked first");
    let texts: Vec<String> = drawn_text(&ctx, &mut a, SCREEN).into_iter().map(|d| d.text).collect();
    assert!(texts.iter().any(|t| t.contains("leave@gallery.example")), "where it goes isn't said: {texts:?}");
    assert!(a.session.notices.is_empty(), "something happened before it was answered");
    // Answered: the list's message goes, from the account.
    click_button(&ctx, &mut a, "Unsubscribe", Some(bar));
    frame(&ctx, &mut a, vec![]);
    assert!(a.ui.dialog.is_none());
    a.run("folder.open", json!({"account": "sample", "folder": "Sent"}));
    assert_eq!(a.session.list.len(), before + 1, "the list's message wasn't sent");
}

#[test]
fn junk_mail_from_a_list_offers_block_sender_instead() {
    let _screen = windows::layout::Dressed::new(SCREEN, "");
    let (mut a, _) = app("unsubscribe-junk-bar");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frame(&ctx, &mut a, vec![]);
    opened(&ctx, &mut a, "Junk", "You may have won a camera");
    assert!(shows(&ctx, &mut a, "From a mailing list."));
    assert!(!shows(&ctx, &mut a, "Unsubscribe"), "offered in Junk Email");
    assert!(shows(&ctx, &mut a, "Block sender"));
}

#[test]
fn found_by_its_name_it_still_asks_first() {
    let (mut a, _) = app("unsubscribe-palette");
    let ctx = egui::Context::default();
    frame(&ctx, &mut a, vec![]);
    opened(&ctx, &mut a, "INBOX", "Open call: winter group show");
    a.ui.dialog = Some(Dialog::commands("unsubscribe"));
    frame(&ctx, &mut a, vec![]);
    frame(&ctx, &mut a, vec![key(Key::Enter, Modifiers::NONE)]);
    frame(&ctx, &mut a, vec![]);
    assert!(matches!(a.ui.dialog, Some(Dialog::Unsubscribe { .. })), "{:?}", a.ui.dialog.as_ref().map(std::mem::discriminant));
}
