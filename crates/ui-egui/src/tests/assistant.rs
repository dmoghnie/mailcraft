//! MailCraft's AI assistant in the window: Home › Ask AI opens it; a suggestion asks about the
//! messages selected, pointed at; the answer is shown formatted, a card for each tool, the
//! change it proposes approved from its card and made; the chats listed, one deleted. The model
//! is the test's: its answers are handed to the session.

use super::*;
use mailcraft_engine::model::{AiAnswer, AiDone, AiToolCall};

/// The sample app with a model on this computer that the sample account's mail may go to.
fn ready(name: &str) -> (MailApp, egui::Context) {
    let (mut a, _) = app(name);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    a.run("ai.allow", json!({"account": "sample", "reach": "local"}));
    a.run("ai.saveProfile", json!({"name": "Local", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small"}));
    for _ in 0..3 {
        frame(&ctx, &mut a, vec![]);
    }
    (a, ctx)
}

fn texts(ctx: &egui::Context, a: &mut MailApp) -> Vec<String> {
    let size = windows::layout::SIZE.with(std::cell::Cell::get);
    drawn_strings(ctx, a, size)
}

/// The model answers the question chat `chat` waits on with `text`, calling `tools`.
fn answers(a: &mut MailApp, chat: &str, text: &str, tools: &[(&str, serde_json::Value)]) {
    let id = a.session.ai.assistant.asking(chat).expect("the chat waits on its model");
    let calls = tools
        .iter()
        .enumerate()
        .map(|(n, (name, input))| AiToolCall { id: format!("call_{n}"), name: (*name).into(), input: input.clone() })
        .collect();
    let answer = AiAnswer { text: text.into(), calls, ..AiAnswer::default() };
    a.session.apply(MailEvent::Ai { done: AiDone::Answered { id, answer: Some(answer), error: None } });
}

#[test]
fn the_assistant_asks_about_whats_selected_and_a_change_is_approved_on_its_card() {
    let (mut a, ctx) = ready("assistant-pane");
    click_named(&ctx, &mut a, "Ask AI");
    assert!(a.session.prefs.assistant_pane.shown, "Home › Ask AI opens the pane");
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "New chat") && has(&t, "What needs my attention today?"), "{t:?}");
    assert!(has(&t, "small at localhost:11434"), "the model, and where it is");
    // A message selected: asked about, pointed at.
    let m = a.session.list.first().cloned().unwrap();
    a.run("list.select", json!({"key": m.key.to_id()}));
    frame(&ctx, &mut a, vec![]);
    click_named(&ctx, &mut a, "Sum up the selected messages");
    let chat = a.ui.assistant.chat.clone().expect("a chat begun");
    let turns = &a.session.ai.assistant.chat(&chat).unwrap().turns;
    assert_eq!(turns[0].context, [mailcraft_engine::ContextItem::Message { id: m.key.to_id() }]);
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "Sum up the selected messages") && has(&t, &m.subject), "what was said, and its message: {t:?}");
    assert!(has(&t, "Stop"), "it can be stopped while it answers");
    // The answer: formatted, a picture shown as a link; a read, and a move proposed.
    answers(
        &mut a,
        &chat,
        "It's about **the review**. ![tracker](https://tracker.example/p.png)",
        &[("read_message", json!({"id": m.key.to_id()})), ("move_messages", json!({"ids": [m.key.to_id()], "folder": "archive"}))],
    );
    let t = texts(&ctx, &mut a);
    for line in ["the review", "tracker", "Read a message", "Move", "Needs your approval", "Approve", "Decline"] {
        assert!(has(&t, line), "{line}: {t:?}");
    }
    assert!(a.session.list.iter().any(|x| x.key == m.key), "nothing moved before it's approved");
    click_named(&ctx, &mut a, "Approve");
    assert!(!a.session.list.iter().any(|x| x.key == m.key), "approved: moved");
    assert!(has(&texts(&ctx, &mut a), "Done"));
    answers(&mut a, &chat, "Archived.", &[]);
    assert!(has(&texts(&ctx, &mut a), "Archived."));
    // The chats kept: listed, and the one shown deleted, the pane showing a new chat.
    click_named(&ctx, &mut a, "Chats");
    assert!(has(&texts(&ctx, &mut a), "Sum up the selected messages"), "listed by how it began");
    click_named(&ctx, &mut a, "Delete this chat");
    frame(&ctx, &mut a, vec![]);
    assert!(a.session.assistant_heads().is_empty());
    assert_eq!(a.ui.assistant.chat, None);
    click_named(&ctx, &mut a, "Close the AI assistant");
    assert!(!a.session.prefs.assistant_pane.shown);
}

/// No model set up: the pane says where to add one, and the message list has no Ask AI.
#[test]
fn without_a_model_the_assistant_says_where_one_is_set_up() {
    let (mut a, _) = app("assistant-unset");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    a.run("assistant.pane", json!({"on": true}));
    for _ in 0..3 {
        frame(&ctx, &mut a, vec![]);
    }
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "Add a model to ask about your mail"), "{t:?}");
    click_named(&ctx, &mut a, "Set up AI");
    assert!(a.ui.backstage.is_some(), "File › Options opens");
}

/// Something said to the AI by a command (a `--cmd` script's, an agent's), not from the pane's
/// box: the pane shows that chat, its answer as it comes.
#[test]
fn a_chat_begun_by_a_command_is_the_one_shown() {
    let (mut a, ctx) = ready("assistant-scripted");
    a.run("assistant.pane", json!({"on": true}));
    a.run("assistant.send", json!({"text": "What came in today?"}));
    let chat = a.ui.assistant.chat.clone().expect("the pane shows the chat begun");
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "What came in today?") && has(&t, "Stop"), "{t:?}");
    answers(&mut a, &chat, "Two messages.", &[]);
    assert!(has(&texts(&ctx, &mut a), "Two messages."));
}
