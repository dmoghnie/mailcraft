//! UI tests: a message kept as a template from its More options, then a new message started from
//! it (Home's New mail ▾ › From a template) and it put into a message being written (More
//! options › Templates).

use super::*;

const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);

fn frames(ctx: &egui::Context, a: &mut MailApp, events: Vec<Event>) {
    sized_frame(ctx, a, SCREEN, events);
    sized_frame(ctx, a, SCREEN, vec![]);
}

/// A click on what a screen reader names `label`, then the frames for what it opens.
fn choose(ctx: &egui::Context, a: &mut MailApp, label: &str) {
    click_named(ctx, a, label);
    frames(ctx, a, vec![]);
}

#[test]
fn a_template_is_saved_and_put_into_messages() {
    let (mut a, _) = app("templates-ui");
    a.session.prefs.new_mail_window = false;
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frames(&ctx, &mut a, vec![]);
    a.run("mail.new", json!({}));
    a.run("compose.update", json!({"subject": "Weekly numbers", "body": "The numbers for this week are in."}));
    frames(&ctx, &mut a, vec![]);
    // More options › Templates › Save as template…: a name, then Enter.
    choose(&ctx, &mut a, "More options");
    choose(&ctx, &mut a, "Templates");
    choose(&ctx, &mut a, "Save as template…");
    assert!(matches!(a.ui.dialog, Some(Dialog::TemplateName(_))), "the dialog asks for a name");
    frames(&ctx, &mut a, vec![Event::Text("Weekly".into())]);
    frames(&ctx, &mut a, vec![key(Key::Enter, Modifiers::NONE)]);
    assert!(a.ui.dialog.is_none());
    assert_eq!(a.session.template("Weekly").map(|t| t.body.as_str()), Some("The numbers for this week are in."));
    a.run("compose.discard", json!({}));
    frames(&ctx, &mut a, vec![]);
    // New mail ▾ › From a template › Weekly: a new message from it.
    choose(&ctx, &mut a, "New items");
    choose(&ctx, &mut a, "From a template");
    choose(&ctx, &mut a, "Weekly");
    let c = a.session.compose.as_ref().expect("a new message");
    assert_eq!(c.subject, "Weekly numbers");
    assert!(c.body.starts_with("The numbers for this week are in."), "{:?}", c.body);
    // More options › Templates › Weekly: into the message being written.
    a.run("compose.update", json!({"body": "Hello,\n"}));
    frames(&ctx, &mut a, vec![]);
    choose(&ctx, &mut a, "More options");
    choose(&ctx, &mut a, "Templates");
    choose(&ctx, &mut a, "Weekly");
    let body = a.session.compose.as_ref().map(|c| c.body.clone()).unwrap_or_default();
    assert!(body.contains("The numbers for this week are in.") && body.contains("Hello,"), "{body:?}");
}
