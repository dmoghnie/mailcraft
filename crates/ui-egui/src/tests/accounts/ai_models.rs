//! File › Options › AI: a model added from its dialog, its key kept out of the settings and the
//! dialog's `Debug`; an account's mail let go to it; the model tested, its answer and the record
//! shown; and each model's buttons named by the model, to a screen reader.

use super::*;
use mailcraft_engine::model::{AiAnswer, AiApi, AiDone, Secret};
use mailcraft_engine::{AiReach, AiTest};

const KEY: &str = "sk-ui-test-0123";
const TALL: egui::Vec2 = egui::vec2(1280.0, 8000.0);

fn options(name: &str) -> (MailApp, egui::Context) {
    let (mut a, _) = app(name);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    a.ui.backstage = Some(crate::backstage::Backstage::new(crate::backstage::Page::Options));
    // The page is laid out over its first frames: clicked once it's still.
    for _ in 0..3 {
        frame(&ctx, &mut a, vec![]);
    }
    (a, ctx)
}

/// The question waiting now (the test's backend takes each job, and sends nothing).
fn waiting(a: &MailApp) -> u64 {
    (0..64).find(|id| a.session.ai.is_waiting(*id)).expect("a question waits")
}

#[test]
fn a_model_is_added_on_options_its_key_kept_out_of_the_settings() {
    let _tall = crate::tests::windows::layout::Dressed::new(TALL, "");
    let (mut a, ctx) = options("ai-options-add");
    let texts = drawn_strings(&ctx, &mut a, TALL);
    assert!(has(&texts, "Add a model"));
    assert!(!has(&texts, "Whose mail may go to a model"), "no model yet: no account to let");
    assert!(!has(&texts, "What was sent"), "nor a record");
    click_named(&ctx, &mut a, "Add…");
    let Some(Dialog::AiModel(f)) = a.ui.dialog.as_mut() else { panic!("the model's dialog: {:?}", a.ui.dialog) };
    f.name = "Cloud".into();
    f.api = AiApi::Anthropic;
    f.url = "https://models.example/v1".into();
    f.model = "large".into();
    f.key = Secret(KEY.into());
    assert!(!format!("{:?}", a.ui.dialog).contains(KEY), "the dialog's Debug hides the key");
    frame(&ctx, &mut a, vec![]);
    let texts = drawn_strings(&ctx, &mut a, TALL);
    assert!(has(&texts, "At a cloud provider: mail you let go to it leaves this computer"), "where it is, said as it's typed");
    click_named(&ctx, &mut a, "Save");
    assert!(a.ui.dialog.is_none(), "saved: {:?}", a.ui.dialog);
    let p = &a.session.prefs.ai.profiles;
    assert_eq!(p.len(), 1);
    assert!(p[0].has_key, "a key was given");
    assert!(!serde_json::to_string(&a.session.prefs).unwrap().contains(KEY), "the key isn't in the settings");
    let texts = drawn_strings(&ctx, &mut a, TALL);
    for line in ["Cloud", "large at models.example", "Its key is kept in this computer's keychain", "Whose mail may go to a model"] {
        assert!(has(&texts, line), "{line}: {texts:?}");
    }
    // The keychain refused it: said, and no key is counted on.
    let id = a.session.prefs.ai.profiles[0].id.clone();
    a.session.apply(MailEvent::Ai { done: AiDone::KeySaved { name: id, error: Some("there's no keychain to keep it in".into()) } });
    let texts = drawn_strings(&ctx, &mut a, TALL);
    assert!(has(&texts, "Its key couldn't be kept: there's no keychain to keep it in"), "{texts:?}");
    assert!(!a.session.prefs.ai.profiles[0].has_key);
}

#[test]
fn an_account_is_let_a_model_tested_and_the_record_shown() {
    let _tall = crate::tests::windows::layout::Dressed::new(TALL, "");
    let (mut a, ctx) = options("ai-options-test");
    a.run("ai.saveProfile", json!({"name": "Ollama", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small"}));
    a.run("ai.saveProfile", json!({"name": "Cloud", "api": "anthropic", "url": "https://models.example/v1", "model": "large"}));
    for _ in 0..3 {
        frame(&ctx, &mut a, vec![]);
    }
    // Each account's choice, and each model's buttons, named by their row.
    click_named(&ctx, &mut a, "riley@studio.example: Local models");
    assert_eq!(a.session.prefs.ai.accounts.get("sample"), Some(&AiReach::Local));
    click_named(&ctx, &mut a, "riley@studio.example: Off");
    assert!(a.session.prefs.ai.accounts.is_empty(), "off again");
    click_named(&ctx, &mut a, "Ollama: Test");
    assert_eq!(a.session.ai.tests.get("p1"), Some(&AiTest::Asking));
    let texts = drawn_strings(&ctx, &mut a, TALL);
    for line in ["Testing…", "What was sent", "Waiting", "Ollama · small at localhost:11434"] {
        assert!(has(&texts, line), "{line}: {texts:?}");
    }
    let disabled = named(&ctx, &mut a, TALL).into_iter().any(|n| n.label.as_deref() == Some("Ollama: Test") && n.disabled);
    assert!(disabled, "a model being tested isn't tested again");
    let answer = AiAnswer { text: "OK".into(), ..Default::default() };
    a.session.apply(MailEvent::Ai { done: AiDone::Answered { id: waiting(&a), answer: Some(answer), error: None } });
    let texts = drawn_strings(&ctx, &mut a, TALL);
    assert!(has(&texts, "It answered: OK") && has(&texts, "Answered"), "{texts:?}");
    // Which model does each kind of work, with two to choose from.
    assert!(has(&texts, "Model for questions") && has(&texts, "Model for quick decisions"));
    // Changed: the dialog holds the profile, never a key.
    click_named(&ctx, &mut a, "Cloud: Change…");
    let Some(Dialog::AiModel(f)) = &a.ui.dialog else { panic!("the model's dialog: {:?}", a.ui.dialog) };
    assert_eq!((f.id.as_deref(), f.model.as_str(), f.key.0.as_str()), (Some("p2"), "large", ""));
    a.ui.dialog = None;
    click_named(&ctx, &mut a, "Cloud: Remove");
    let left: Vec<&str> = a.session.prefs.ai.profiles.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(left, ["Ollama"]);
}
