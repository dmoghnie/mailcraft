//! Send again in a message's More actions: there for one the user sent, opening it as a new
//! message; not there for one someone else sent.

use super::*;

const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);

fn shows(ctx: &egui::Context, a: &mut MailApp, label: &str) -> bool {
    named(ctx, a, SCREEN).iter().any(|n| n.name() == Some(label))
}

fn click_last(ctx: &egui::Context, a: &mut MailApp, label: &str) {
    let all = named(ctx, a, SCREEN);
    let at = all.iter().filter(|n| n.name() == Some(label)).filter_map(Named::centre).next_back();
    let at = at.unwrap_or_else(|| panic!("no {label} to click: {:?}", all.iter().filter_map(Named::name).collect::<Vec<_>>()));
    click(ctx, a, at);
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
fn a_message_you_sent_can_be_sent_again_from_more_actions() {
    let _screen = windows::layout::Dressed::new(SCREEN, "");
    let (mut a, _) = app("send-again-menu");
    a.session.prefs.new_mail_window = false;
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frame(&ctx, &mut a, vec![]);

    opened(&ctx, &mut a, "INBOX", "Open call: winter group show");
    click_last(&ctx, &mut a, "More actions");
    assert!(shows(&ctx, &mut a, "Forward as attachment"), "the menu is open");
    assert!(!shows(&ctx, &mut a, "Send again"), "offered for a message someone else sent");
    frame(&ctx, &mut a, vec![key(Key::Escape, Modifiers::NONE)]);

    opened(&ctx, &mut a, "Sent", "Festival poster order");
    click_last(&ctx, &mut a, "More actions");
    click_last(&ctx, &mut a, "Send again");
    frame(&ctx, &mut a, vec![]);
    let c = a.session.compose.as_ref().expect("a new message");
    assert_eq!(c.subject, "Festival poster order");
    assert!(c.to.contains("orders@printshop.example"), "{}", c.to);
}
