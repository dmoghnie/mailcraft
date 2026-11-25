//! Rules in plain words, in the Rules dialog: offered once a model is set up; Write rule asks the
//! model with the rule's schema, the rule written is said in words with the model that wrote it,
//! and Add rule adds it.

use mailcraft_engine::model::{AiAnswer, AiDone};

use super::*;

const WORDS: &str = "list mail goes to Archive";

fn names(ctx: &egui::Context, a: &mut MailApp) -> Vec<String> {
    named(ctx, a, egui::vec2(1280.0, 800.0)).iter().filter_map(Named::name).map(str::to_string).collect()
}

fn click_named(ctx: &egui::Context, a: &mut MailApp, label: &str) {
    let all = named(ctx, a, egui::vec2(1280.0, 800.0));
    let at = all.iter().filter(|n| n.name() == Some(label)).find_map(Named::centre);
    click(ctx, a, at.unwrap_or_else(|| panic!("no {label}: {:?}", all.iter().filter_map(Named::name).collect::<Vec<_>>())));
}

#[test]
fn a_rule_is_written_from_words_in_the_rules_dialog() {
    let (mut a, _) = app("rule-words-dialog");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    a.ui.dialog = Some(Dialog::Rules);
    frame(&ctx, &mut a, vec![]);
    frame(&ctx, &mut a, vec![]);
    assert!(!names(&ctx, &mut a).iter().any(|n| n == "Write rule"), "no model, nothing offered");

    a.run("ai.allow", json!({"account": "sample", "reach": "local"}));
    a.run("ai.saveProfile", json!({"name": "Local", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small"}));
    a.session.take_jobs();
    a.ui.dialog = Some(Dialog::Rules);
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("rule-words"), WORDS.to_string()));
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Write rule");
    let id = a.session.rule_proposal().and_then(|p| p.asking).expect("asked");
    assert_eq!(a.session.rule_proposal().map(|p| p.words.as_str()), Some(WORDS));
    frame(&ctx, &mut a, vec![]);
    assert!(drawn_strings(&ctx, &mut a, egui::vec2(1280.0, 800.0)).iter().any(|t| t == "MailCraft's AI is writing the rule…"));

    let rule = json!({"rule": {"name": "Lists", "where": {"if": "bulk"}, "actions": [{"do": "moveToRole", "value": "archive"}], "stop": false, "sent": false}, "whyNot": null});
    let answer = AiAnswer { text: rule.to_string(), ..AiAnswer::default() };
    a.session.apply(MailEvent::Ai { done: AiDone::Answered { id, answer: Some(answer), error: None } });
    frame(&ctx, &mut a, vec![]);
    let texts = drawn_strings(&ctx, &mut a, egui::vec2(1280.0, 800.0));
    assert!(texts.iter().any(|t| t == "If it is list mail: move to Archive"), "said in words: {texts:?}");
    assert!(texts.iter().any(|t| t == "Written by small at localhost:11434"), "with the model that wrote it: {texts:?}");
    assert!(a.session.rules.is_empty(), "nothing added yet");
    click_named(&ctx, &mut a, "Add rule");
    assert_eq!(a.session.rules.len(), 1);
    assert_eq!(a.session.rules[0].name, "Lists");
    assert!(a.session.rule_proposal().is_none());
}
