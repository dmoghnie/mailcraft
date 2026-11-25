//! MailCraft's AI on the message being written, in the window: the bar's Rewrite with AI ›
//! Shorter asks the model, the card over the text shows what's written, Replace puts it in place
//! of the text (the quoted original kept), Undo takes it back, Close lets it go; Write it from a
//! few words asks for them over the text.

use super::*;
use mailcraft_engine::RewriteHow;
use mailcraft_engine::mime::rich::{self, Styles};
use mailcraft_engine::model::{AiAnswer, AiDone};

const OWN: &str = "Thank you so much for sending this over. I had a good look and it is approved.";

/// A reply in the reading pane with a model the sample's mail may go to, `own` typed above the
/// original.
fn replying(name: &str, own: &str) -> (MailApp, egui::Context) {
    let (mut a, _) = app(name);
    a.session.prefs.new_mail_window = false;
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    a.run("ai.allow", json!({"account": "sample", "reach": "local"}));
    a.run("ai.saveProfile", json!({"name": "Local", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small"}));
    frame(&ctx, &mut a, vec![]);
    let k = a.session.list.first().map(|m| m.key.clone()).unwrap();
    a.run("list.open", json!({"key": k.to_id()}));
    a.run("mail.reply", json!({}));
    let mut c = a.session.compose.take().unwrap();
    rich::splice(&mut c.body, &mut c.styles, 0..0, own, &Styles::plain(own));
    a.session.compose = Some(c);
    for _ in 0..3 {
        frame(&ctx, &mut a, vec![]);
    }
    (a, ctx)
}

fn size() -> egui::Vec2 {
    windows::layout::SIZE.with(std::cell::Cell::get)
}

/// The last control named `label`, clicked (a menu's item is drawn over the rest).
fn click_last(ctx: &egui::Context, a: &mut MailApp, label: &str) {
    let all = named(ctx, a, size());
    let at = all.iter().filter(|n| n.name() == Some(label)).filter_map(Named::centre).next_back();
    let at = at.unwrap_or_else(|| panic!("no {label} to click: {:?}", all.iter().filter_map(Named::name).collect::<Vec<_>>()));
    click(ctx, a, at);
}

/// The model answers the rewrite asked for with `text`.
fn answers(a: &mut MailApp, text: &str) {
    let id = a.session.rewrite().and_then(|r| r.asking).expect("a rewrite on its way");
    let answer = AiAnswer { text: text.into(), ..AiAnswer::default() };
    a.session.apply(MailEvent::Ai { done: AiDone::Answered { id, answer: Some(answer), error: None } });
}

fn body(a: &MailApp) -> String {
    a.session.compose.as_ref().map(|c| c.body.clone()).unwrap_or_default()
}

#[test]
fn the_text_is_rewritten_from_the_bar_and_put_in_from_its_card() {
    let (mut a, ctx) = replying("rewrite-bar", OWN);
    let before = body(&a);
    click_last(&ctx, &mut a, "Rewrite with AI");
    click_last(&ctx, &mut a, "Shorter");
    assert!(a.session.rewrite().is_some_and(|r| r.how == RewriteHow::Shorter && r.asking.is_some()), "asked");
    let t = drawn_strings(&ctx, &mut a, size());
    assert!(has(&t, "Rewriting…") && has(&t, "small at localhost:11434"), "{t:?}");
    answers(&mut a, "Thanks, it's approved.");
    assert!(has(&drawn_strings(&ctx, &mut a, size()), "Thanks, it's approved."), "what's written, shown");
    assert_eq!(body(&a), before, "nothing changes by itself");
    click_last(&ctx, &mut a, "Replace");
    frame(&ctx, &mut a, vec![]);
    let after = body(&a);
    assert!(after.starts_with("Thanks, it's approved.") && !after.contains(OWN), "{after}");
    click_last(&ctx, &mut a, "Undo");
    frame(&ctx, &mut a, vec![]);
    assert_eq!(body(&a), before, "undone");
    click_last(&ctx, &mut a, "Close the rewrite");
    frame(&ctx, &mut a, vec![]);
    assert!(a.session.rewrite().is_none(), "closed");
}

#[test]
fn a_reply_is_written_from_a_few_words_typed_over_it() {
    let (mut a, ctx) = replying("rewrite-words", "");
    click_last(&ctx, &mut a, "Rewrite with AI");
    click_last(&ctx, &mut a, "Write it from a few words…");
    frame(&ctx, &mut a, vec![]);
    frame(&ctx, &mut a, vec![egui::Event::Text("say yes, but Thursday".into())]);
    frame(&ctx, &mut a, vec![key(Key::Enter, Modifiers::NONE)]);
    frame(&ctx, &mut a, vec![]);
    assert!(a.session.rewrite().is_some_and(|r| r.how == RewriteHow::FromWords && r.asking.is_some()), "asked from the words typed");
    assert!(a.ui.rewrite_words.is_none(), "the box goes once they're sent");
}
