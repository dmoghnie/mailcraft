//! Needs me in the window: mail arriving read by the model for quick decisions once it's turned
//! on in File › Options › AI, a tag on its row (and in its name, to a screen reader), taken off
//! from the row's menu.

use super::*;
use mailcraft_engine::model::{AiAnswer, AiDone};

const ASKS: &[u8] = b"From: Zoe Park <zoe@studio.example>\r\nTo: riley@studio.example\r\nSubject: Can you sign the contract?\r\nMessage-ID: <sign-1@studio.example>\r\n\r\nCould you sign the contract by Friday?\r\n";

fn names(ctx: &egui::Context, a: &mut MailApp) -> Vec<String> {
    let size = windows::layout::SIZE.with(std::cell::Cell::get);
    named(ctx, a, size).iter().filter_map(Named::name).map(str::to_string).collect()
}

#[test]
fn mail_that_needs_me_is_tagged_on_its_row() {
    // Tall enough for the whole Options page.
    let _tall = windows::layout::Dressed::new(egui::vec2(1280.0, 8000.0), "");
    let (mut a, _) = app("needs-me-ui");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    a.run("ai.allow", json!({"account": "sample", "reach": "local"}));
    a.run("ai.saveProfile", json!({"name": "Local", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small"}));
    // Turned on from File › Options › AI.
    a.ui.backstage = Some(crate::backstage::Backstage::new(crate::backstage::Page::Options));
    for _ in 0..3 {
        frame(&ctx, &mut a, vec![]);
    }
    click_named(&ctx, &mut a, "Mark mail that needs me");
    assert!(a.session.prefs.ai.needs_me, "turned on");
    a.ui.backstage = None;
    let key = arrive(&mut a, 9970, ASKS);
    let id = (0..64).find(|id| a.session.ai.is_waiting(*id)).expect("its question");
    let answer = AiAnswer { text: "YES".into(), ..AiAnswer::default() };
    a.session.apply(MailEvent::Ai { done: AiDone::Answered { id, answer: Some(answer), error: None } });
    a.run("folder.open", json!({"account": "sample", "folder": "INBOX"}));
    for _ in 0..3 {
        frame(&ctx, &mut a, vec![]);
    }
    let size = windows::layout::SIZE.with(std::cell::Cell::get);
    assert!(has(&drawn_strings(&ctx, &mut a, size), "Needs me"), "the tag");
    let spoken = names(&ctx, &mut a);
    assert!(spoken.iter().any(|n| n.contains("Can you sign the contract?") && n.contains("Needs me")), "{spoken:?}");
    a.run("mail.notNeeded", json!({"key": key.to_id()}));
    frame(&ctx, &mut a, vec![]);
    assert!(!has(&drawn_strings(&ctx, &mut a, size), "Needs me"), "taken off");
}
