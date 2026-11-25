//! The digest in the window: Home's Digest asks for it, in the window's language; over the
//! Inbox it shows what's written as it comes, the model, and Close; made by itself, it's
//! announced.

use super::*;
use mailcraft_engine::model::{AiAnswer, AiDone};

#[test]
fn the_digest_is_asked_from_home_and_shown_over_the_inbox() {
    let (mut a, _) = app("digest-ui");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let size = windows::layout::SIZE.with(std::cell::Cell::get);
    a.run("ai.allow", json!({"account": "sample", "reach": "local"}));
    a.run("ai.saveProfile", json!({"name": "Local", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small"}));
    a.run("folder.open", json!({"account": "sample", "folder": "INBOX"}));
    for _ in 0..3 {
        frame(&ctx, &mut a, vec![]);
    }
    click_named(&ctx, &mut a, "Digest");
    let id = a.session.digest().and_then(|d| d.asking).expect("asked");
    frame(&ctx, &mut a, vec![]);
    assert!(has(&drawn_strings(&ctx, &mut a, size), "Writing the digest…"));
    let answer = AiAnswer { text: "A quiet day.\n\nNeeds an answer from you\n- Zoe: the proof".into(), ..AiAnswer::default() };
    a.session.apply(MailEvent::Ai { done: AiDone::Answered { id, answer: Some(answer), error: None } });
    frame(&ctx, &mut a, vec![]);
    let t = drawn_strings(&ctx, &mut a, size);
    assert!(has(&t, "small at localhost:11434") && t.iter().any(|s| s.contains("A quiet day.")), "{t:?}");
    click_named(&ctx, &mut a, "Close the digest");
    frame(&ctx, &mut a, vec![]);
    assert!(!drawn_strings(&ctx, &mut a, size).iter().any(|s| s.contains("A quiet day.")), "closed");
}
