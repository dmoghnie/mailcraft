//! File › Options › AI: a model added from its dialog, its key kept out of the settings and the
//! dialog's `Debug`; an account's mail let go to it; the model tested, its answer and the record
//! shown; and each model's buttons named by the model, to a screen reader.

use super::*;
use crate::dialogs::ServerForm;
use mailcraft_engine::model::{AiAnswer, AiApi, AiDone, AiEffort, McpDone, McpOAuth, McpTarget, McpTool, Secret};
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

/// The dialog's temperature (typed with a comma too) and thinking are kept with the model, shown
/// again when it's changed; one that isn't a number keeps the dialog open, saying why.
#[test]
fn a_models_temperature_and_thinking_are_set_in_its_dialog() {
    let _tall = crate::tests::windows::layout::Dressed::new(TALL, "");
    let (mut a, ctx) = options("ai-options-tuning");
    click_named(&ctx, &mut a, "Add…");
    frame(&ctx, &mut a, vec![]);
    let texts = drawn_strings(&ctx, &mut a, TALL);
    assert!(has(&texts, "Temperature") && has(&texts, "Thinking"), "{texts:?}");
    let Some(Dialog::AiModel(f)) = a.ui.dialog.as_mut() else { panic!("the model's dialog: {:?}", a.ui.dialog) };
    (f.name, f.api, f.url, f.model) = ("Local".into(), AiApi::Ollama, "http://localhost:11434".into(), "qwen3".into());
    f.temperature = "warm".into();
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Save");
    frame(&ctx, &mut a, vec![]);
    let Some(Dialog::AiModel(f)) = a.ui.dialog.as_mut() else { panic!("still open: {:?}", a.ui.dialog) };
    assert!(f.error.as_deref().is_some_and(|e| e.contains("temperature")), "{:?}", f.error);
    f.temperature = "0,7".into();
    f.effort = Some(AiEffort::High);
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Save");
    assert!(a.ui.dialog.is_none(), "saved: {:?}", a.ui.dialog);
    let p = &a.session.prefs.ai.profiles[0];
    assert_eq!((p.temperature, p.effort), (Some(70), Some(AiEffort::High)));
    let again = crate::dialogs::AiModelForm::of(p);
    assert_eq!((again.temperature.as_str(), again.effort), ("0.7", Some(AiEffort::High)));
}

/// A model's monthly budget is set in its dialog, and Options › AI says what it used of it, and
/// when it has used it all.
#[test]
fn a_models_budget_is_set_and_its_use_shown() {
    let _tall = crate::tests::windows::layout::Dressed::new(TALL, "");
    let (mut a, ctx) = options("ai-options-budget");
    click_named(&ctx, &mut a, "Add…");
    frame(&ctx, &mut a, vec![]);
    let Some(Dialog::AiModel(f)) = a.ui.dialog.as_mut() else { panic!("the model's dialog: {:?}", a.ui.dialog) };
    (f.name, f.api, f.url, f.model) = ("Local".into(), AiApi::Ollama, "http://localhost:11434".into(), "qwen3".into());
    f.budget = "1000".into();
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Save");
    assert!(a.ui.dialog.is_none(), "saved: {:?}", a.ui.dialog);
    assert_eq!(a.session.prefs.ai.profiles[0].budget, Some(1000));
    assert!(has(&drawn_strings(&ctx, &mut a, TALL), "0 of 1000 tokens used this month"));
    a.run("ai.allow", json!({"account": "sample", "reach": "local"}));
    a.run("ai.test", json!({"profile": a.session.prefs.ai.profiles[0].id}));
    let id = waiting(&a);
    let answer = AiAnswer { text: "Hello".into(), input_tokens: Some(1200), output_tokens: Some(0), ..AiAnswer::default() };
    a.session.apply(MailEvent::Ai { done: AiDone::Answered { id, answer: Some(answer), error: None } });
    let texts = drawn_strings(&ctx, &mut a, TALL);
    assert!(has(&texts, "It has used its 1000 tokens this month: questions go to the model it falls back to"), "{texts:?}");
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

/// File › Options › AI says how long chats are kept: always at first, as the setting says after.
#[test]
fn how_long_chats_are_kept_is_shown_on_options() {
    let _tall = crate::tests::windows::layout::Dressed::new(TALL, "");
    let (mut a, ctx) = options("ai-options-keep-chats");
    let texts = drawn_strings(&ctx, &mut a, TALL);
    assert!(has(&texts, "Keep chats") && !has(&texts, "For 30 days"), "{texts:?}");
    a.run("ai.keepChats", json!({"days": 30}));
    frame(&ctx, &mut a, vec![]);
    assert!(has(&drawn_strings(&ctx, &mut a, TALL), "For 30 days"));
    assert_eq!(a.session.prefs.ai.chats_days, 30);
}

/// File › Options › AI lists the scripts MailCraft's AI keeps once there's a model: one turned
/// off by its switch, one changed in its dialog (code that doesn't compile refused there), and
/// deleted, each button named by its script.
#[test]
fn kept_scripts_are_turned_off_changed_and_deleted_on_options() {
    let _tall = crate::tests::windows::layout::Dressed::new(TALL, "");
    let (mut a, ctx) = options("ai-options-scripts");
    assert!(!has(&drawn_strings(&ctx, &mut a, TALL), "waiting_on_me"), "no model yet: no scripts");
    a.run("ai.saveProfile", json!({"name": "Ollama", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small"}));
    frame(&ctx, &mut a, vec![]);
    let texts = drawn_strings(&ctx, &mut a, TALL);
    assert!(["Scripts", "waiting_on_me", "today", "tidy"].iter().all(|w| has(&texts, w)), "{texts:?}");
    let switch = crate::tests::named(&ctx, &mut a, TALL)
        .into_iter()
        .find(|n| n.role == egui::accesskit::Role::CheckBox && n.label.as_deref() == Some("tidy"))
        .and_then(|n| n.centre())
        .expect("tidy's switch");
    crate::tests::click(&ctx, &mut a, switch);
    let kept = |a: &mut MailApp| a.session.run("script.list", &json!({})).unwrap()["scripts"].as_array().unwrap().clone();
    assert!(kept(&mut a).iter().any(|k| k["name"] == "tidy" && k["off"] == true), "turned off");
    click_named(&ctx, &mut a, "today: Change…");
    let Some(Dialog::Script(f)) = a.ui.dialog.as_mut() else { panic!("the script's dialog: {:?}", a.ui.dialog) };
    assert_eq!(f.was.as_deref(), Some("today"));
    f.name = "the_day".into();
    f.code = "let x = ;".into();
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Save");
    let Some(Dialog::Script(f)) = a.ui.dialog.as_mut() else { panic!("kept open: {:?}", a.ui.dialog) };
    assert!(f.error.as_deref().is_some_and(|e| e.contains("compile")), "{:?}", f.error);
    f.code = "find_tasks(#{}).total".into();
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Save");
    assert!(a.ui.dialog.is_none(), "saved: {:?}", a.ui.dialog);
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "the_day: Delete");
    let names: Vec<serde_json::Value> = kept(&mut a).iter().map(|k| k["name"].clone()).collect();
    assert_eq!(names, ["waiting_on_me", "waiting_on_them", "person_history", "tidy"]);
}

/// MCP servers, once there's a model: one added from its dialog (a program, its key kept out of
/// the dialog's `Debug` and the settings), its tools counted once listed, turned off by its
/// switch and removed, each button named by its server.
#[test]
fn an_mcp_server_is_added_turned_off_and_removed_on_options() {
    let _tall = crate::tests::windows::layout::Dressed::new(TALL, "");
    let (mut a, ctx) = options("ai-options-mcp");
    assert!(!has(&drawn_strings(&ctx, &mut a, TALL), "MCP servers"), "no model yet: no servers");
    a.run("ai.saveProfile", json!({"name": "Ollama", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small"}));
    frame(&ctx, &mut a, vec![]);
    assert!(has(&drawn_strings(&ctx, &mut a, TALL), "Add an MCP server"));
    a.ui.dialog = Some(Dialog::Server(Box::new(ServerForm::new())));
    frame(&ctx, &mut a, vec![]);
    let Some(Dialog::Server(f)) = a.ui.dialog.as_mut() else { panic!("the server's dialog: {:?}", a.ui.dialog) };
    (f.name, f.command, f.key_env, f.key) = ("notes".into(), "notes-mcp".into(), "NOTES_KEY".into(), Secret(KEY.into()));
    assert!(!format!("{f:?}").contains(KEY), "the key out of the dialog's Debug");
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Save");
    assert!(a.ui.dialog.is_none(), "saved: {:?}", a.ui.dialog);
    let saved = a.session.prefs.ai.mcp.first().expect("added").clone();
    assert!(saved.on && saved.has_key, "{saved:?}");
    assert!(matches!(&saved.target, McpTarget::Program { key_env: Some(e), .. } if e == "NOTES_KEY"), "{saved:?}");
    assert!(!serde_json::to_string(&a.session.prefs).unwrap().contains(KEY), "the key out of the settings");
    let tool = McpTool { name: "find_note".into(), read_only: true, ..McpTool::default() };
    a.session.apply(MailEvent::Mcp { done: McpDone::Listed { server: saved.id.clone(), tools: vec![tool], error: None } });
    frame(&ctx, &mut a, vec![]);
    let texts = drawn_strings(&ctx, &mut a, TALL);
    assert!(["MCP servers", "notes", "A program on this computer: notes-mcp", "Tools: 1"].iter().all(|w| has(&texts, w)), "{texts:?}");
    let switch = crate::tests::named(&ctx, &mut a, TALL)
        .into_iter()
        .find(|n| n.role == egui::accesskit::Role::CheckBox && n.label.as_deref() == Some("notes"))
        .and_then(|n| n.centre())
        .expect("the server's switch");
    crate::tests::click(&ctx, &mut a, switch);
    assert!(!a.session.prefs.ai.mcp[0].on, "turned off");
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "notes: Remove");
    assert!(a.session.prefs.ai.mcp.is_empty(), "removed");
}

/// A server on the web has Sign in…: it begins the sign-in, and the row says to finish it in
/// the browser; signed in, it says so and offers to sign in again. A program has none.
#[test]
fn an_mcp_server_on_the_web_is_signed_in_to_on_options() {
    let _tall = crate::tests::windows::layout::Dressed::new(TALL, "");
    let (mut a, ctx) = options("ai-options-mcp-sign-in");
    a.run("ai.saveProfile", json!({"name": "Ollama", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small"}));
    a.run("mcpServer.save", json!({"name": "files", "command": "files-mcp"}));
    a.run("mcpServer.save", json!({"name": "notes", "url": "https://mcp.example/mcp"}));
    frame(&ctx, &mut a, vec![]);
    let texts = drawn_strings(&ctx, &mut a, TALL);
    assert_eq!(texts.iter().filter(|t| t.as_str() == "Sign in…").count(), 1, "the server on the web alone: {texts:?}");
    assert!(!has(&texts, "Signed in"), "{texts:?}");
    a.session.take_jobs();
    click_named(&ctx, &mut a, "notes: Sign in…");
    let id = a.session.prefs.ai.mcp[1].id.clone();
    assert!(a.session.mcp_tools().get(&id).is_some_and(|k| k.signing), "the sign-in began");
    frame(&ctx, &mut a, vec![]);
    assert!(has(&drawn_strings(&ctx, &mut a, TALL), "Finish signing in in your browser"));
    let how = McpOAuth { issuer: "https://auth.example".into(), resource: "https://mcp.example/mcp".into(), ..McpOAuth::default() };
    a.session.apply(MailEvent::Mcp { done: McpDone::SignedIn { server: id, oauth: Some(how), error: None } });
    frame(&ctx, &mut a, vec![]);
    let texts = drawn_strings(&ctx, &mut a, TALL);
    assert!(has(&texts, "Signed in") && has(&texts, "Sign in again…"), "{texts:?}");
}
