//! The Inbox's No answer yet in the window: sent mail nobody answered, over the list, to dismiss
//! or follow up.

use super::*;

const SCREEN: egui::Vec2 = egui::vec2(1400.0, 860.0);

fn names_of(ctx: &egui::Context, a: &mut MailApp) -> Vec<String> {
    named(ctx, a, SCREEN).iter().filter_map(Named::name).map(str::to_string).collect()
}

/// Click the first button called `label`.
fn click_button(ctx: &egui::Context, a: &mut MailApp, label: &str) {
    let all = named(ctx, a, SCREEN);
    let at = all.iter().filter(|n| n.name() == Some(label) && n.role == egui::accesskit::Role::Button).find_map(Named::centre);
    let at = at.unwrap_or_else(|| panic!("no {label} to click: {:?}", all.iter().filter_map(Named::name).collect::<Vec<_>>()));
    click(ctx, a, at);
}

#[test]
fn unanswered_mail_comes_back_over_the_inbox() {
    let _screen = windows::layout::Dressed::new(SCREEN, "");
    let (mut a, _) = app("no-answer-ui");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    for (to, subject) in [("ana@soundlab.example", "Budget for the mix?"), ("zoe@studio.example", "Dates for the screening?")] {
        a.run("mail.new", json!({}));
        a.run("compose.update", json!({"to": to, "subject": subject}));
        a.run("compose.send", json!({}));
    }
    a.run("folder.open", json!({"account": "sample", "folder": "INBOX"}));
    frame(&ctx, &mut a, vec![]);
    assert!(!names_of(&ctx, &mut a).iter().any(|n| n.starts_with("No answer yet")), "just sent");
    // Three days on (the window takes the time from its services each frame).
    a.services = Box::new(NullServices(NOW + 3 * 86_400));
    for _ in 0..60 {
        frame(&ctx, &mut a, vec![]);
    }
    // Those two, and the sample's own order to the print shop (its answer doesn't say so).
    let names = names_of(&ctx, &mut a);
    assert!(names.iter().any(|n| n == "No answer yet (3)"), "{names:?}");
    // Dismissed: two are left.
    click_button(&ctx, &mut a, "Dismiss");
    frame(&ctx, &mut a, vec![]);
    let names = names_of(&ctx, &mut a);
    assert!(names.iter().any(|n| n == "No answer yet (2)"), "{names:?}");
    assert_eq!(a.session.prefs.no_answer_dismissed.len(), 1);
    // Followed up: a reply to it is begun.
    click_button(&ctx, &mut a, "Write a follow-up");
    frame(&ctx, &mut a, vec![]);
    let subject = a.session.compose.as_ref().map(|c| c.subject.clone()).unwrap_or_default();
    assert!(subject.starts_with("Re: "), "{subject:?}");
}
