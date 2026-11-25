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

/// The colour `text` is painted in, where the frame has it, and its first line's baseline.
fn painted_at(ctx: &egui::Context, a: &mut MailApp, text: &str) -> Option<(egui::Color32, f32)> {
    let size = windows::layout::SIZE.with(std::cell::Cell::get);
    let out = sized_frame(ctx, a, size, vec![]);
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.job.text.trim() == text => {
            // A section's own colour, else (a placeholder) the colour the shape is drawn in.
            let own = t.galley.job.sections.first().map(|x| x.format.color).filter(|c| *c != egui::Color32::PLACEHOLDER);
            let row = t.galley.rows.first()?;
            let baseline = t.pos.y + row.pos.y + row.glyphs.first()?.pos.y;
            Some((t.override_text_color.or(own).unwrap_or(t.fallback_color), baseline))
        }
        _ => None,
    })
}

fn painted(ctx: &egui::Context, a: &mut MailApp, text: &str) -> Option<egui::Color32> {
    painted_at(ctx, a, text).map(|(c, _)| c)
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
    // The answer: formatted, a picture shown as a link; a read, and a move proposed beside it,
    // put off (no card) for the model to propose again with any other changes.
    let move_it = ("move_messages", json!({"ids": [m.key.to_id()], "folder": "archive"}));
    answers(
        &mut a,
        &chat,
        "It's about **the review**. ![tracker](https://tracker.example/p.png)",
        &[("read_message", json!({"id": m.key.to_id()})), move_it.clone()],
    );
    let t = texts(&ctx, &mut a);
    for line in ["the review", "tracker", "Read a message"] {
        assert!(has(&t, line), "{line}: {t:?}");
    }
    assert!(!has(&t, "Needs your approval") && !has(&t, "to Archive"), "put off: {t:?}");
    // Proposed again: a card to approve it on.
    answers(&mut a, &chat, "", &[move_it]);
    let t = texts(&ctx, &mut a);
    for line in ["Move", "Needs your approval", "Approve", "Decline"] {
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

/// A message an answer links to opens with a click: its folder, and the message in it.
#[test]
fn a_message_an_answer_links_to_opens_with_a_click() {
    let (mut a, ctx) = ready("assistant-link");
    a.run("assistant.pane", json!({"on": true}));
    let m = a.session.list.first().cloned().unwrap();
    let id = m.key.to_id();
    a.run("assistant.send", json!({"text": "What's this about?", "context": [{"kind": "message", "id": id}]}));
    let chat = a.ui.assistant.chat.clone().unwrap();
    let link = mailcraft_engine::assistant_message_link(&id);
    answers(&mut a, &chat, &format!("It's this one:\n\n- [the message you pointed at]({link}) from Zoe"), &[]);
    // Drawn as a link, in the links' colour; the words around it as text.
    let t = a.ui.tokens;
    assert_eq!(painted(&ctx, &mut a, "the message you pointed at"), Some(t.accent));
    assert_eq!(painted(&ctx, &mut a, "from Zoe"), Some(t.text));
    // On the line of the words around it, not above them.
    let (link, text) = (painted_at(&ctx, &mut a, "the message you pointed at").unwrap().1, painted_at(&ctx, &mut a, "from Zoe").unwrap().1);
    assert!((link - text).abs() < 0.5, "the link's baseline {link}, the text's {text}");
    a.run("folder.open", json!({"account": "sample", "folder": "Archive"}));
    frame(&ctx, &mut a, vec![]);
    assert!(a.session.reading.as_ref().is_none_or(|r| r.key != m.key));
    click_named(&ctx, &mut a, "the message you pointed at");
    assert!(matches!(&a.session.view, mailcraft_engine::View::Folder { folder, .. } if *folder == m.key.folder), "{:?}", a.session.view);
    assert_eq!(a.session.reading.as_ref().map(|r| r.key.clone()), Some(m.key.clone()));
}

/// An event or a person an answer links to opens with a click: the event in its window, on the
/// calendar's day; the person's card in People.
#[test]
fn an_event_or_a_person_an_answer_links_to_opens_with_a_click() {
    let (mut a, ctx) = ready("assistant-link-pim");
    a.run("assistant.pane", json!({"on": true}));
    a.run("assistant.send", json!({"text": "When's lunch with Jun, and who's Zoe?"}));
    let chat = a.ui.assistant.chat.clone().unwrap();
    answers(&mut a, &chat, "", &[("find_events", json!({"query": "Jun"})), ("find_people", json!({"query": "zoe"}))]);
    // The names the tools gave: what's between the brackets their lines start with.
    let runs = a.session.ai.assistant.chat(&chat).unwrap().runs.clone();
    let name = |n: usize| runs[n].result.split(['[', ']']).nth(1).unwrap().to_string();
    let (event, person) = (name(0), name(1));
    answers(&mut a, &chat, &format!("- [the lunch](mailcraft:{event}) today\n- [who Zoe is](mailcraft:{person})"), &[]);
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "Searched the calendar") && has(&t, "Searched the contacts"), "the tools' cards: {t:?}");
    click_named(&ctx, &mut a, "the lunch");
    assert_eq!(a.session.module, mailcraft_engine::Module::Calendar);
    assert!(matches!(&a.ui.dialog, Some(Dialog::Event(f)) if f.summary == "Lunch with Jun"), "the event opens");
    a.ui.dialog = None;
    a.ui.people_view = crate::panes::people::PeopleView::Cards;
    click_named(&ctx, &mut a, "who Zoe is");
    assert_eq!(a.session.module, mailcraft_engine::Module::People);
    assert_eq!(a.ui.person.as_deref(), Some("zoe@sample.mailcraft"));
    assert_eq!(a.ui.people_view, crate::panes::people::PeopleView::People, "the card shown");
    assert!(has(&texts(&ctx, &mut a), "Director"), "Zoe's card");
}

/// A task or a note an answer links to opens with a click: the task's form, or the note in
/// Notes, to write on.
#[test]
fn a_task_or_a_note_an_answer_links_to_opens_with_a_click() {
    let (mut a, ctx) = ready("assistant-link-todo");
    a.run("assistant.pane", json!({"on": true}));
    a.run("assistant.send", json!({"text": "When's the sound stage booked, and what did I note about the poster?"}));
    let chat = a.ui.assistant.chat.clone().unwrap();
    answers(&mut a, &chat, "", &[("find_tasks", json!({"query": "sound stage"})), ("find_notes", json!({"query": "poster"}))]);
    let runs = a.session.ai.assistant.chat(&chat).unwrap().runs.clone();
    let name = |n: usize| runs[n].result.split(['[', ']']).nth(1).unwrap().to_string();
    let (task, note) = (name(0), name(1));
    answers(&mut a, &chat, &format!("- [the stage](mailcraft:{task})\n- [the poster note](mailcraft:{note})"), &[]);
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "Searched the tasks") && has(&t, "Searched the notes"), "the tools' cards: {t:?}");
    click_named(&ctx, &mut a, "the stage");
    assert_eq!(a.session.module, mailcraft_engine::Module::Tasks);
    assert!(matches!(&a.ui.dialog, Some(Dialog::Task(f)) if f.title == "Book the sound stage"), "the task opens");
    a.ui.dialog = None;
    a.ui.notes_query = "fox".into();
    click_named(&ctx, &mut a, "the poster note");
    assert_eq!(a.session.module, mailcraft_engine::Module::Notes);
    assert_eq!(a.ui.note_edit.as_ref().map(|(uid, _)| uid.as_str()), Some("note-poster@sample.mailcraft"));
    assert!(a.ui.notes_query.is_empty(), "shown, whatever the list was searching for");
}

/// An event the model proposes is a card named as the window's New event, added once Approve is
/// clicked; the answer's link to it opens it.
#[test]
fn an_event_proposed_is_added_from_its_card_and_opens_from_the_answer() {
    let (mut a, ctx) = ready("assistant-new-event");
    a.run("assistant.pane", json!({"on": true}));
    a.run("assistant.send", json!({"text": "Remind me tomorrow to zoom the team"}));
    let chat = a.ui.assistant.chat.clone().unwrap();
    let day = mailcraft_engine::model::dates::day_string(mailcraft_engine::model::dates::today(&a.session.clock) + 1);
    answers(&mut a, &chat, "", &[("add_event", json!({"summary": "Zoom the team", "start": format!("{day}T10:00")}))]);
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "New event") && has(&t, "Needs your approval"), "{t:?}");
    click_named(&ctx, &mut a, "Approve");
    let run = a.session.ai.assistant.chat(&chat).unwrap().runs[0].clone();
    assert_eq!(run.state, mailcraft_engine::RunState::Done, "{}", run.result);
    let name = run.result.trim_end_matches('.').rsplit(' ').next().unwrap().to_string();
    answers(&mut a, &chat, &format!("[Zoom the team](mailcraft:{name}) is in your calendar."), &[]);
    click_named(&ctx, &mut a, "Zoom the team");
    assert_eq!(a.session.module, mailcraft_engine::Module::Calendar);
    assert!(matches!(&a.ui.dialog, Some(Dialog::Event(f)) if f.summary == "Zoom the team"), "the event opens");
}

/// A model with a long name (Ollama's `owner/Model-Name:tag`) is named in a line that fits the
/// pane, cut short: it stretched the pane past the width it was set to, and where the window
/// had no room for that, the pane's left side (its title, the model, the cards' buttons) was
/// cut off.
#[test]
fn a_long_model_name_keeps_the_pane_within_its_width() {
    let (mut a, ctx) = ready("assistant-long-model");
    let long = "orcarouter/Qwen3.8-27B-Uncensored:mlx-4bit";
    a.run("ai.saveProfile", json!({"id": "p1", "name": "Local", "api": "openAi", "url": "http://localhost:11434/v1", "model": long}));
    a.run("assistant.pane", json!({"on": true, "width": 320}));
    for _ in 0..3 {
        frame(&ctx, &mut a, vec![]);
    }
    let size = windows::layout::SIZE.with(std::cell::Cell::get);
    let all = named(&ctx, &mut a, size);
    let pane = a.ui.panes.iter().find(|(p, _)| *p == crate::Pane::Assistant).map(|(_, r)| *r).expect("the pane shown");
    assert!(pane.width() <= 321.0, "the pane is {} wide, not the 320 it was set to", pane.width());
    for label in ["Model", "Close the AI assistant", "Send"] {
        let r = all.iter().find(|n| n.label.as_deref() == Some(label)).and_then(|n| n.rect).unwrap_or_else(|| panic!("no {label}"));
        assert!(pane.expand(1.0).contains_rect(r), "{label} at {r:?}, outside the pane {pane:?}");
    }
}

/// A script the model ran shows on a card of its own, saying what it does; its code and a card
/// for each call it made unfold under it.
#[test]
fn a_script_shows_on_its_card_and_its_code_and_calls_unfold_under_it() {
    let (mut a, ctx) = ready("assistant-script");
    let id = a.session.prefs.ai.profiles[0].id.clone();
    a.run(
        "ai.saveProfile",
        json!({"id": id, "name": "Local", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small", "scripts": true}),
    );
    click_named(&ctx, &mut a, "Ask AI");
    click_named(&ctx, &mut a, "What needs my attention today?");
    let chat = a.ui.assistant.chat.clone().expect("a chat begun");
    answers(&mut a, &chat, "", &[("run_script", json!({"script": "find_messages(#{limit: 3}).total", "what": "Count the newest"}))]);
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "Ran a script") && has(&t, "Count the newest") && has(&t, "The script"), "{t:?}");
    assert!(!has(&t, "Searched the mail"), "folded: {t:?}");
    click_named(&ctx, &mut a, "The script");
    // Unfolded once it's done opening.
    for _ in 0..20 {
        frame(&ctx, &mut a, vec![]);
    }
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "find_messages(#{limit: 3}).total") && has(&t, "Searched the mail"), "{t:?}");
}

/// A change a script proposes shows on a card of its own under the script, unfolded, and is
/// approved there.
#[test]
fn a_change_a_script_proposes_is_approved_on_its_card() {
    let (mut a, ctx) = ready("assistant-script-change");
    let id = a.session.prefs.ai.profiles[0].id.clone();
    a.run(
        "ai.saveProfile",
        json!({"id": id, "name": "Local", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small", "scripts": true}),
    );
    let m = a.session.list.first().cloned().unwrap();
    click_named(&ctx, &mut a, "Ask AI");
    click_named(&ctx, &mut a, "What needs my attention today?");
    let chat = a.ui.assistant.chat.clone().expect("a chat begun");
    let script = r#"let m = find_messages(#{limit: 1}).messages[0]; move_messages(#{ids: [m.id], folder: "archive"}).proposed"#;
    answers(&mut a, &chat, "", &[("run_script", json!({"script": script, "what": "Archive the newest"}))]);
    let t = texts(&ctx, &mut a);
    for line in ["Ran a script", "Move", "Needs your approval", "Approve", "Decline"] {
        assert!(has(&t, line), "{line}: {t:?}");
    }
    assert!(!has(&t, "Searched the mail"), "its reads stay folded: {t:?}");
    click_named(&ctx, &mut a, "Approve");
    assert!(!a.session.list.iter().any(|x| x.key == m.key), "approved: moved");
    assert!(has(&texts(&ctx, &mut a), "Done"));
}

/// A script waiting for the answer to its question says so on its card; its question unfolds
/// under it.
#[test]
fn a_script_waiting_for_its_question_says_so() {
    let (mut a, ctx) = ready("assistant-script-asks");
    let id = a.session.prefs.ai.profiles[0].id.clone();
    a.run(
        "ai.saveProfile",
        json!({"id": id, "name": "Local", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small", "scripts": true}),
    );
    click_named(&ctx, &mut a, "Ask AI");
    click_named(&ctx, &mut a, "What needs my attention today?");
    let chat = a.ui.assistant.chat.clone().expect("a chat begun");
    answers(&mut a, &chat, "", &[("run_script", json!({"script": r#"ask("Is anything urgent?")"#}))]);
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "Ran a script") && has(&t, "Waiting for the answer…") && !has(&t, "Downloading the message…"), "{t:?}");
    click_named(&ctx, &mut a, "The script");
    for _ in 0..20 {
        frame(&ctx, &mut a, vec![]);
    }
    assert!(has(&texts(&ctx, &mut a), "Asked the model"));
}

/// A question that failed says why, with Try again, which asks again; a stopped answer says
/// it was stopped.
#[test]
fn a_failed_question_is_tried_again_from_the_pane() {
    let (mut a, ctx) = ready("assistant-again");
    a.run("assistant.pane", json!({"on": true}));
    a.run("assistant.send", json!({"text": "What came in today?"}));
    let chat = a.ui.assistant.chat.clone().expect("the pane shows the chat begun");
    let q = a.session.ai.assistant.asking(&chat).expect("asked");
    a.session.apply(MailEvent::Ai { done: AiDone::Part { id: q, text: "So far".into(), thinking: String::new() } });
    a.run("assistant.stop", json!({"chat": chat}));
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "So far") && has(&t, "Stopped") && has(&t, "Try again"), "{t:?}");
    click_named(&ctx, &mut a, "Try again");
    frame(&ctx, &mut a, vec![]);
    assert!(a.session.ai.assistant.asking(&chat).is_some(), "asked again");
    let q = a.session.ai.assistant.asking(&chat).expect("asked");
    a.session.apply(MailEvent::Ai { done: AiDone::Answered { id: q, answer: None, error: Some("the model's server: HTTP 503".into()) } });
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "Try again") && !has(&t, "So far"), "{t:?}");
}

/// A script the model proposes to keep has its card, to approve, with its code under it; kept,
/// it's listed.
#[test]
fn a_script_proposed_to_be_kept_is_approved_from_its_card() {
    let (mut a, ctx) = ready("assistant-keep-script");
    let id = a.session.prefs.ai.profiles[0].id.clone();
    a.run(
        "ai.saveProfile",
        json!({"id": id, "name": "Local", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small", "scripts": true}),
    );
    a.run("assistant.pane", json!({"on": true}));
    a.run("assistant.send", json!({"text": "Keep that"}));
    let chat = a.ui.assistant.chat.clone().expect("the pane shows the chat begun");
    let keep = json!({"name": "unread_count", "about": "How many messages are unread", "script": "find_messages(#{query: \"is:unread\"}).total"});
    answers(&mut a, &chat, "", &[("keep_script", keep)]);
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "Keep a script") && has(&t, "The script") && has(&t, "Approve"), "{t:?}");
    click_named(&ctx, &mut a, "Approve");
    frame(&ctx, &mut a, vec![]);
    let kept = a.session.run("script.list", &json!({})).unwrap();
    assert!(kept["scripts"].as_array().unwrap().iter().any(|k| k["name"] == "unread_count"), "{kept}");
}

/// The last control named `label` in the window, clicked (a menu's item is drawn over the rest).
fn click_last(ctx: &egui::Context, a: &mut MailApp, label: &str) {
    let size = windows::layout::SIZE.with(std::cell::Cell::get);
    let all = named(ctx, a, size);
    let at = all.iter().filter(|n| n.name() == Some(label)).filter_map(Named::centre).next_back();
    let at = at.unwrap_or_else(|| panic!("no {label} to click: {:?}", all.iter().filter_map(Named::name).collect::<Vec<_>>()));
    click(ctx, a, at);
}

/// A conversation summed up from its message's More actions: a card at the top of the reading
/// pane shows what's written as it comes and the model writing it, then the summary; Close lets
/// it go.
#[test]
fn a_conversation_is_summed_up_from_more_actions_and_closed() {
    let (mut a, ctx) = ready("assistant-summary");
    let key = a.session.list.iter().find(|m| m.subject == "Re: Storyboard review on Thursday").map(|m| m.key.clone()).unwrap();
    a.run("list.open", json!({"key": key.to_id()}));
    for _ in 0..3 {
        frame(&ctx, &mut a, vec![]);
    }
    click_last(&ctx, &mut a, "More actions");
    click_last(&ctx, &mut a, "Summarize with AI");
    let id = a.session.thread_summary(&key).and_then(|s| s.asking).expect("asked for, on its way");
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "Summary") && has(&t, "Summarizing…") && has(&t, "small at localhost:11434"), "{t:?}");
    a.session.apply(MailEvent::Ai { done: AiDone::Part { id, text: "- Zoe wants scenes 4 to 7".into(), thinking: String::new() } });
    assert!(has(&texts(&ctx, &mut a), "Zoe wants scenes 4 to 7"), "what came so far");
    let answer = AiAnswer { text: "- Zoe wants scenes 4 to 7 on Thursday.".into(), ..AiAnswer::default() };
    a.session.apply(MailEvent::Ai { done: AiDone::Answered { id, answer: Some(answer), error: None } });
    let t = texts(&ctx, &mut a);
    assert!(has(&t, "Zoe wants scenes 4 to 7 on Thursday.") && !has(&t, "Summarizing…"), "{t:?}");
    click_last(&ctx, &mut a, "Close the summary");
    frame(&ctx, &mut a, vec![]);
    assert!(a.session.thread_summary(&key).is_none(), "let go");
    assert!(!has(&texts(&ctx, &mut a), "Zoe wants scenes 4 to 7 on Thursday."));
}
