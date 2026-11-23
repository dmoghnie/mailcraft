//! MailCraft's AI assistant: its chats with tools, the changes it proposes and the person
//! approves, the mail it may read, the window it leaves alone, and the chats kept.

use super::*;
use crate::{ContextItem, RunState};
use mailcraft_model::{AiChat, AiRole, AiToolCall, MessageKey};

const STORYBOARD: &str = "Storyboard review on Thursday";

/// A sample session whose mail may go to a model on this computer, which is the one asked.
fn ready(name: &str) -> Session {
    let mut s = sample_session(name);
    s.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    local(&mut s, "Local");
    s.take_jobs();
    s
}

/// The questions queued, each with its id and what it asks.
fn questions(s: &mut Session) -> Vec<(u64, AiChat)> {
    asked(s).into_iter().map(|(id, _, _, chat)| (id, chat)).collect()
}

/// The one question queued.
fn question(s: &mut Session) -> (u64, AiChat) {
    let mut q = questions(s);
    assert_eq!(q.len(), 1, "one question");
    q.remove(0)
}

fn send(s: &mut Session, chat: Option<&str>, text: &str) -> String {
    let mut v = json!({"text": text});
    if let Some(c) = chat {
        v["chat"] = json!(c);
    }
    s.run("assistant.send", &v).unwrap()["chat"].as_str().unwrap().to_string()
}

/// The model answers question `id` with `text` and calls of `tools` (each a name and its input).
fn calls(s: &mut Session, id: u64, text: &str, tools: &[(&str, Value)]) {
    let calls = tools
        .iter()
        .enumerate()
        .map(|(n, (name, input))| AiToolCall { id: format!("call_{n}"), name: (*name).into(), input: input.clone() })
        .collect();
    let a = AiAnswer { text: text.into(), calls, ..AiAnswer::default() };
    s.apply(Event::Ai { done: AiDone::Answered { id, answer: Some(a), error: None } });
}

fn chat(s: &mut Session, id: &str) -> Value {
    s.run("assistant.chat", &json!({"chat": id})).unwrap()
}

fn id_of(s: &Session, subject: &str) -> String {
    key_of(s, subject)["key"].as_str().unwrap().to_string()
}

/// The person asks; the model finds, then reads, at once, and answers. Nothing the window shows
/// changes: its list, its selection and the message open stay as they were.
#[test]
fn the_assistant_reads_at_once_and_leaves_the_window_alone() {
    let mut s = ready("assistant-reads");
    let story = id_of(&s, STORYBOARD);
    s.run("list.open", &json!({"key": story})).unwrap();
    let (shown, open) = (s.list.len(), s.reading.as_ref().map(|r| r.key.to_id()));
    let c = send(&mut s, None, "What does Zoe want for Thursday?");
    let (q, asked) = question(&mut s);
    let names: Vec<&str> = asked.tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"find_messages") && names.contains(&"move_messages"), "{names:?}");
    assert!(asked.system.contains("- sample:"), "the accounts it may read");
    assert_eq!(asked.turns.last().map(|t| t.text.as_str()), Some("What does Zoe want for Thursday?"));
    calls(&mut s, q, "Let me look.", &[("find_messages", json!({"query": "from:zoe storyboard"}))]);
    let (q, asked) = question(&mut s);
    let found = &asked.turns.last().unwrap().results[0];
    assert!(found.content.contains(&story) && found.content.contains(STORYBOARD), "{}", found.content);
    assert!(!found.error);
    calls(&mut s, q, "", &[("read_message", json!({"id": story}))]);
    let (q, asked) = question(&mut s);
    let read = &asked.turns.last().unwrap().results[0].content;
    assert!(read.contains("revised frames for scenes 4 to 7") && read.contains("colour-key.png"), "{read}");
    answer(&mut s, q, "The revised frames for scenes 4 to 7.");
    assert!(questions(&mut s).is_empty(), "answered without calls: done");
    let v = chat(&mut s, &c);
    let turns = v["chat"]["turns"].as_array().unwrap();
    assert_eq!(turns.len(), 6, "{v}");
    assert_eq!(turns[5]["text"], "The revised frames for scenes 4 to 7.");
    let runs = v["chat"]["runs"].as_array().unwrap();
    assert!(runs.iter().all(|r| r["state"] == "ran"), "{runs:?}");
    assert_eq!(v["chat"]["accounts"], json!(["sample"]), "its mail is in the chat now");
    assert_eq!((s.list.len(), s.reading.as_ref().map(|r| r.key.to_id())), (shown, open), "the window as it was");
    assert!(s.search.is_none());
}

/// A change waits for the person, and happens only once they approve it; one declined changes
/// nothing. Either way the model hears how it went, and goes on.
#[test]
fn changes_wait_for_the_person_and_happen_only_when_approved() {
    let mut s = ready("assistant-changes");
    let story = id_of(&s, STORYBOARD);
    let c = send(&mut s, None, "Archive Zoe's storyboard mail and delete it");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("move_messages", json!({"ids": [story], "folder": "archive"})), ("delete_messages", json!({"ids": [story]}))]);
    assert!(questions(&mut s).is_empty(), "nothing asked while the person decides");
    let v = chat(&mut s, &c);
    assert_eq!(v["waitsOnYou"], true);
    let runs = v["chat"]["runs"].as_array().unwrap();
    assert_eq!(runs[0]["state"], "proposed");
    assert_eq!(runs[0]["what"], format!("Move “{STORYBOARD}” to Archive"));
    assert!(s.list.iter().any(|m| m.key.to_id() == story), "nothing moved yet");
    s.run("assistant.decide", &json!({"chat": c, "call": "call_0", "approve": true})).unwrap();
    assert!(questions(&mut s).is_empty(), "the delete still waits");
    assert!(!s.list.iter().any(|m| m.key.to_id() == story), "moved");
    let archived = s.folder_state("sample", "Archive").messages.iter().any(|m| m.subject == STORYBOARD);
    assert!(archived);
    s.run("assistant.decide", &json!({"chat": c, "approve": false})).unwrap();
    let archived = s.folder_state("sample", "Archive").messages.iter().any(|m| m.subject == STORYBOARD);
    assert!(archived, "declined: still in Archive");
    let (_, asked) = question(&mut s);
    let results = &asked.turns.last().unwrap().results;
    assert!(results[0].content.starts_with("Approved. Done: 1 moved"), "{:?}", results[0].content);
    assert!(results[1].content.contains("declined"), "{:?}", results[1].content);
    // Undone as the window's own moves are.
    s.run("edit.undo", &json!({})).unwrap();
    assert!(s.folder_state("sample", "INBOX").messages.iter().any(|m| m.subject == STORYBOARD), "back in the Inbox");
}

/// A message it writes is opened for the person to send, an answer threaded to the message it
/// answers; it's never sent.
#[test]
fn a_message_written_is_opened_never_sent() {
    let mut s = ready("assistant-writes");
    let story = id_of(&s, STORYBOARD);
    let c = send(&mut s, None, "Tell Zoe I'll bring them");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("write_message", json!({"reply_to": story, "body": "Hi Zoe,\n\nI'll bring the frames.\n\nRiley"}))]);
    assert!(s.compose.is_none() && s.composes.is_empty(), "nothing written before it's approved");
    s.run("assistant.decide", &json!({"chat": c, "approve": true})).unwrap();
    let w = s.compose.as_ref().or(s.composes.last()).expect("opened");
    assert_eq!(w.subject, format!("Re: {STORYBOARD}"));
    assert!(w.to.contains("zoe@studio.example"), "{}", w.to);
    assert!(w.body.starts_with("Hi Zoe,\n\nI'll bring the frames."), "{:?}", w.body);
    assert!(!w.in_reply_to.is_empty() && w.source.as_ref().map(MessageKey::to_id) == Some(story));
    assert!(!w.sending);
    assert!(!s.take_jobs().iter().any(|j| matches!(j, Job::Send { .. })), "not sent");
}

/// Mail of an account the model isn't allowed never goes to it: not read by a tool, not pointed
/// at, and a chat holding an account's mail never goes to a model that account doesn't allow.
#[test]
fn mail_never_goes_to_a_model_its_account_does_not_allow() {
    let mut s = sample_session("assistant-allowed");
    s.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    let cloud = cloud(&mut s, "Claude");
    let local = local(&mut s, "Local");
    s.run("ai.use", &json!({"task": "chat", "profile": cloud})).unwrap();
    s.take_jobs();
    let story = id_of(&s, STORYBOARD);
    let e = s.run("assistant.send", &json!({"text": "Summarise", "context": [{"kind": "message", "id": story}]})).unwrap_err().to_string();
    assert!(e.contains("may not go to Claude"), "{e}");
    assert!(questions(&mut s).is_empty());
    // A chat with no mail in it goes to the cloud model, which may read none of it.
    let c = send(&mut s, None, "Hello");
    let (q, asked) = question(&mut s);
    assert!(asked.system.contains("may read:\nnone"), "{}", asked.system);
    calls(&mut s, q, "", &[("read_message", json!({"id": story})), ("find_messages", json!({"account": "sample"}))]);
    let (_, asked) = question(&mut s);
    let results = &asked.turns.last().unwrap().results;
    assert!(results.iter().all(|r| r.error && r.content.contains("may not go to Claude")), "{results:?}");
    assert!(results.iter().all(|r| !r.content.contains("Zoe")));
    // Its own model chosen, it reads; then the cloud one can't be asked about the chat.
    s.run("assistant.stop", &json!({"chat": c})).unwrap();
    s.run("assistant.model", &json!({"chat": c, "profile": local})).unwrap();
    send(&mut s, Some(&c), "Read it");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("read_message", json!({"id": story}))]);
    let (q, _) = question(&mut s);
    answer(&mut s, q, "Done");
    s.run("assistant.model", &json!({"chat": c, "profile": cloud})).unwrap();
    let e = s.run("assistant.send", &json!({"chat": c, "text": "And now?"})).unwrap_err().to_string();
    assert!(e.contains("holds mail that may not go to Claude"), "{e}");
    assert!(questions(&mut s).is_empty());
}

/// What the person points at goes with what they say: a message whole, a part they selected,
/// a folder.
#[test]
fn what_the_person_points_at_goes_with_what_they_say() {
    let mut s = ready("assistant-context");
    let story = id_of(&s, STORYBOARD);
    let context = json!([{"kind": "message", "id": story}, {"kind": "quote", "id": story, "text": "scenes 4 to 7"}, {"kind": "folder", "account": "sample", "folder": "INBOX"}]);
    let c = s.run("assistant.send", &json!({"text": "What's asked here?", "context": context})).unwrap()["chat"].as_str().unwrap().to_string();
    let (_, asked) = question(&mut s);
    let said = &asked.turns[0].text;
    assert!(said.contains("revised frames") && said.contains("<selected part") && said.contains("<folder account=\"sample\""), "{said}");
    assert!(said.ends_with("What's asked here?"));
    let v = chat(&mut s, &c);
    assert_eq!(v["chat"]["turns"][0]["text"], "What's asked here?", "what was said, apart from what it points at");
    assert_eq!(v["chat"]["turns"][0]["context"].as_array().map(Vec::len), Some(3));
    let e = s.run("assistant.send", &json!({"text": "x", "context": [{"kind": "message", "id": "nope"}]})).unwrap_err().to_string();
    assert!(e.contains("not a message"), "{e}");
}

/// Chats are kept on this computer, each in its own file, read again after a restart; one
/// deleted is gone from it.
#[test]
fn chats_are_kept_on_this_computer_and_deleted_whole() {
    let mut s = ready("assistant-kept");
    let c = send(&mut s, None, "Remember this chat\nplease");
    let (q, _) = question(&mut s);
    answer(&mut s, q, "Remembered.");
    s.take_jobs();
    let store = s.store.clone().unwrap();
    let file = store.root().join(format!("assistant-{c}.json"));
    assert!(file.exists());
    let mut again = Session::open(Some(store.clone()), mailcraft_model::dates::Clock::new(NOW, 0));
    let list = again.run("assistant.chats", &json!({})).unwrap();
    assert_eq!(list["chats"][0]["title"], "Remember this chat");
    let v = chat(&mut again, &c);
    assert_eq!(v["chat"]["turns"][1]["text"], "Remembered.");
    let log = again.run("ai.log", &json!({})).unwrap().to_string();
    assert!(!log.contains("Remember"), "the record holds no chat: {log}");
    again.run("assistant.delete", &json!({"chat": c})).unwrap();
    again.take_jobs();
    assert!(!file.exists());
    let again = Session::open(Some(store), mailcraft_model::dates::Clock::new(NOW, 0)).run("assistant.chats", &json!({})).unwrap();
    assert_eq!(again["chats"], json!([]));
}

/// Past [`STEPS_MOST`] steps for one thing said, it stops and says so; said again, it goes on.
#[test]
fn it_stops_after_its_steps_and_goes_on_when_told() {
    let mut s = ready("assistant-steps");
    let c = send(&mut s, None, "Look around");
    for _ in 0..crate::STEPS_MOST {
        let (q, _) = question(&mut s);
        calls(&mut s, q, "", &[("list_folders", json!({}))]);
    }
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("list_folders", json!({}))]);
    assert!(questions(&mut s).is_empty(), "stopped");
    let v = chat(&mut s, &c);
    assert!(v["chat"]["error"].as_str().unwrap().contains("stopped after"), "{v}");
    send(&mut s, Some(&c), "Go on");
    let (_, asked) = question(&mut s);
    let last = asked.turns.last().unwrap();
    assert_eq!((last.role, last.text.as_str()), (AiRole::User, "Go on"));
    assert_eq!(last.results.len(), 1, "said with what the last call gave");
}

/// Stopped, the answer it waited for is let go as it comes; what it proposed is declined.
#[test]
fn a_stopped_chat_lets_its_answer_go() {
    let mut s = ready("assistant-stop");
    let c = send(&mut s, None, "Anything new?");
    let (q, _) = question(&mut s);
    assert_eq!(chat(&mut s, &c)["asking"], true);
    s.run("assistant.stop", &json!({"chat": c})).unwrap();
    answer(&mut s, q, "Too late.");
    let v = chat(&mut s, &c);
    assert_eq!(v["chat"]["turns"].as_array().map(Vec::len), Some(1), "{v}");
    assert_eq!(v["asking"], false);
    assert!(questions(&mut s).is_empty());
}

/// A chat longer than its model takes in loses what tools gave early on, then its oldest
/// turns; it still begins with the person.
#[test]
fn a_long_chat_is_fitted_into_its_models_context() {
    let mut s = ready("assistant-fitted");
    s.run(
        "ai.saveProfile",
        &json!({"id": "p1", "name": "Local", "api": "openAi", "url": "http://localhost:11434/v1", "model": "m", "context": 8192, "maxTokens": 512}),
    )
    .unwrap();
    send(&mut s, None, "Read everything");
    // Every message listed, again and again: more than 8192 tokens in all.
    for _ in 0..6 {
        let (q, _) = question(&mut s);
        calls(&mut s, q, "", &[("find_messages", json!({"limit": 50}))]);
    }
    let (listed, asked) = question(&mut s);
    let size: usize = asked.turns.iter().map(|t| t.size() / 3 + 1).sum();
    assert!(size < 8192 - 512, "{size}");
    let cut = asked.turns.iter().any(|t| t.results.iter().any(|r| r.content.starts_with("[Left out for room")));
    assert!(cut || asked.system.contains("left out for room"), "something gave way: {} turns", asked.turns.len());
    let all: usize = 13;
    assert!(asked.turns.len() < all || cut, "six listings and what they gave, and what was said, didn't all fit");
    let first = asked.turns.first().unwrap();
    assert_eq!(first.role, AiRole::User);
    assert!(first.results.is_empty(), "no result without its call");
    assert!(asked.turns.last().unwrap().results[0].content.contains(STORYBOARD), "the latest kept whole");
    // A long thing said, answered: the next goes without it, the chat beginning with the person.
    answer(&mut s, listed, "Listed.");
    let c = send(&mut s, None, &format!("{}first", "word ".repeat(1800)));
    let (q, _) = question(&mut s);
    answer(&mut s, q, "Long.");
    send(&mut s, Some(&c), &format!("{}second", "word ".repeat(1800)));
    let (_, asked) = question(&mut s);
    assert!(asked.system.contains("left out for room"));
    assert_eq!(asked.turns.len(), 1);
    assert!(asked.turns[0].text.ends_with("second"));
    // What the person said, and what its calls gave, too long together: said so, never sent
    // without them (what the calls gave alone, their call and the words gone).
    let c = send(&mut s, None, &"word ".repeat(2700));
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("find_messages", json!({"limit": 50}))]);
    assert!(questions(&mut s).is_empty());
    let v = chat(&mut s, &c);
    assert!(v["chat"]["error"].as_str().is_some_and(|e| e.contains("more than Local takes in")), "{}", v["chat"]["error"]);
    let e = s
        .run(
            "ai.saveProfile",
            &json!({"id": "p1", "name": "Local", "api": "openAi", "url": "http://localhost:11434/v1", "model": "m", "context": 100}),
        )
        .unwrap_err();
    assert!(e.to_string().contains("`context`"));
}

/// What the assistant is given that isn't what it asks is refused, never a crash.
#[test]
fn the_assistants_tools_refuse_junk() {
    let mut s = ready("assistant-junk");
    send(&mut s, None, "Try things");
    let (q, _) = question(&mut s);
    let junk = [
        ("read_message", json!({"id": 7})),
        ("read_message", json!({"id": "sample:9:9:Nowhere"})),
        ("move_messages", json!({"ids": "all", "folder": "archive"})),
        ("move_messages", json!({"ids": [], "folder": "archive"})),
        ("move_messages", json!({"ids": ["sample:1:1:INBOX"], "folder": "No such folder"})),
        ("mark_messages", json!({"ids": ["sample:1:1:INBOX"]})),
        ("categorize_messages", json!({"ids": ["sample:1:1:INBOX"], "category": "Nope"})),
        ("write_message", json!({"to": "not an address", "body": "x"})),
        ("format_disk", json!({})),
        ("find_messages", Value::String("not an object".into())),
    ];
    calls(&mut s, q, "", &junk);
    let (_, asked) = question(&mut s);
    let results = &asked.turns.last().unwrap().results;
    assert_eq!(results.len(), junk.len());
    assert!(results.iter().all(|r| r.error), "{results:?}");
    assert!(s.compose.is_none());
    let _ = ContextItem::Message { id: String::new() };
    let _ = RunState::Ran;
}

/// Home › Ask AI shows the assistant's pane or hides it; dragged, its width is kept, within
/// bounds. The window reads the chats as they stand, and the model a new chat would ask.
#[test]
fn the_assistants_pane_is_shown_and_its_chats_read() {
    let mut s = ready("assistant-pane");
    assert!(!s.prefs.assistant_pane.shown);
    assert_eq!(s.run("assistant.pane", &json!({})).unwrap()["shown"], true);
    assert_eq!(s.run("assistant.pane", &json!({})).unwrap()["shown"], false);
    let v = s.run("assistant.pane", &json!({"width": 5000})).unwrap();
    assert_eq!((v["shown"].clone(), v["width"].clone()), (json!(true), json!(*crate::ASSISTANT_WIDTH.end())));
    assert_eq!(s.run("assistant.pane", &json!({"width": 10, "on": false})).unwrap()["width"], *crate::ASSISTANT_WIDTH.start());
    assert!(!s.prefs.assistant_pane.shown);
    assert_eq!(s.assistant_model(&crate::Chat::default()).unwrap().id, "p1", "a new chat asks the model chat work goes to");
    let c = send(&mut s, None, "Hello there");
    assert_eq!(s.assistant_heads().iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), ["Hello there"]);
    assert_eq!(s.assistant_shown(&c).map(|c| c.turns.len()), Some(1));
    assert!(s.assistant_shown("c999").is_none());
}
