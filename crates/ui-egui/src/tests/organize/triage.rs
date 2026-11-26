//! Triage in the window: turned on in File › Options › AI; what MailCraft's AI suggests for an
//! arriving message shown on a bar above it, applied or dismissed there; and everything it
//! suggests looked over in a dialog, from the bar or from Options.

use super::*;
use mailcraft_engine::model::{AiAnswer, AiDone, PINNED};

const OVERDUE: &[u8] = b"From: Host Billing <billing@host.example>\r\nTo: riley@studio.example\r\nSubject: Your invoice is overdue\r\nMessage-ID: <overdue-1@host.example>\r\n\r\nYour invoice is two weeks overdue.\r\n";
const REMINDER: &[u8] = b"From: Host Billing <billing@host.example>\r\nTo: riley@studio.example\r\nSubject: Second reminder: your invoice\r\nMessage-ID: <overdue-2@host.example>\r\n\r\nYour invoice is three weeks overdue.\r\n";
const PAPER: &[u8] = b"From: The Weekly Paper <news@paper.example>\r\nTo: riley@studio.example\r\nSubject: This week in print\r\nMessage-ID: <paper-1@paper.example>\r\nList-Unsubscribe: <mailto:leave@paper.example>\r\n\r\nThe week's stories.\r\n";

/// The model's answer to the question that waits.
fn answered(a: &mut MailApp, text: &str) {
    let id = (0..64).find(|id| a.session.ai.is_waiting(*id)).expect("its question");
    let answer = AiAnswer { text: text.into(), ..AiAnswer::default() };
    a.session.apply(MailEvent::Ai { done: AiDone::Answered { id, answer: Some(answer), error: None } });
}

#[test]
fn suggestions_are_shown_above_the_message_and_applied_or_dismissed() {
    // Tall enough for the whole Options page.
    let size = egui::vec2(1280.0, 8000.0);
    let _tall = windows::layout::Dressed::new(size, "");
    let (mut a, _) = app("triage-ui");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    a.run("ai.allow", json!({"account": "sample", "reach": "local"}));
    a.run("ai.saveProfile", json!({"name": "Local", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small"}));
    a.ui.backstage = Some(crate::backstage::Backstage::new(crate::backstage::Page::Options));
    for _ in 0..3 {
        frame(&ctx, &mut a, vec![]);
    }
    click_named(&ctx, &mut a, "Suggest how to sort arriving mail");
    assert!(a.session.prefs.ai.triage, "turned on");
    a.ui.backstage = None;

    let overdue = arrive(&mut a, 9980, OVERDUE);
    answered(&mut a, r#"{"category": "Work", "urgent": true, "group": "transactions"}"#);
    a.run("list.open", json!({"key": overdue.to_id()}));
    for _ in 0..3 {
        frame(&ctx, &mut a, vec![]);
    }
    let texts = drawn_strings(&ctx, &mut a, size);
    assert!(has(&texts, "MailCraft's AI suggests: Category Work · Pin it"), "the bar: {texts:?}");
    assert!(!has(&texts, "Review all…"), "one alone");
    click_named(&ctx, &mut a, "Suggestion: Apply");
    let m = a.session.list.iter().find(|m| m.key == overdue).expect("in the list");
    assert!(m.flags.has("$label2") && m.flags.has(PINNED), "applied");
    frame(&ctx, &mut a, vec![]);
    assert!(!has(&drawn_strings(&ctx, &mut a, size), "MailCraft's AI suggests: Category Work · Pin it"), "gone once applied");

    // Two waiting: looked over from the bar's Review all…, one left out, the other dismissed.
    a.run("view.focused", json!({"on": true}));
    a.run("view.inboxGroups", json!({"on": true}));
    let paper = arrive(&mut a, 9981, PAPER);
    answered(&mut a, r#"{"category": "", "urgent": false, "group": "promotions"}"#);
    let again = arrive(&mut a, 9982, REMINDER);
    answered(&mut a, r#"{"category": "Later", "urgent": false, "group": "transactions"}"#);
    assert_eq!(a.session.triage_suggestions().len(), 2, "{:?}", a.session.triage_suggestions());
    // The paper is sorted in Updates.
    a.run("list.focusTab", json!({"value": "updates"}));
    a.run("list.open", json!({"key": paper.to_id()}));
    for _ in 0..3 {
        frame(&ctx, &mut a, vec![]);
    }
    assert!(has(&drawn_strings(&ctx, &mut a, size), "MailCraft's AI suggests: Mail from The Weekly Paper always in Promotions"));
    click_named(&ctx, &mut a, "Suggestion: Review all…");
    assert!(matches!(a.ui.dialog, Some(Dialog::Suggestions(_))), "{:?}", a.ui.dialog);
    frame(&ctx, &mut a, vec![]);
    let texts = drawn_strings(&ctx, &mut a, size);
    for line in ["The Weekly Paper · This week in print", "Host Billing · Second reminder: your invoice", "Category Later"] {
        assert!(has(&texts, line), "{line}: {texts:?}");
    }
    click_named(&ctx, &mut a, "The Weekly Paper · This week in print");
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Dismiss");
    assert_eq!(a.session.triage_suggestions().iter().map(|w| w.id.clone()).collect::<Vec<_>>(), [paper.to_id()], "the one left out waits");
    assert!(a.session.triage_suggestion(&again).is_none() && a.ui.dialog.is_some(), "the dialog stays");

    // From Options too, with how many wait.
    a.ui.dialog = None;
    a.ui.backstage = Some(crate::backstage::Backstage::new(crate::backstage::Page::Options));
    for _ in 0..3 {
        frame(&ctx, &mut a, vec![]);
    }
    assert!(has(&drawn_strings(&ctx, &mut a, size), "Waiting: 1"));
    click_named(&ctx, &mut a, "Suggestions: Review…");
    assert!(matches!(a.ui.dialog, Some(Dialog::Suggestions(_))), "{:?}", a.ui.dialog);
}
