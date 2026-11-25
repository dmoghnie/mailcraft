//! MailCraft's AI assistant: its chats with tools, the changes it proposes and the person
//! approves, the mail it may read, the window it leaves alone, and the chats kept.

use super::*;
use crate::{ContextItem, RunState};
use mailcraft_model::{AiChat, AiRole, AiToolCall, MessageKey};

const STORYBOARD: &str = "Storyboard review on Thursday";

mod again;
mod kept;
mod kept_scripts;
mod ledger;
mod pim_changes;
mod script;
mod script_asks;
mod script_mcp;
mod sent;
mod waiting;

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

/// The name the tools give the message with id `id`: its link, less `mailcraft:`.
fn name_of(id: &str) -> String {
    crate::assistant_message_link(id).replace(crate::ASSISTANT_LINK, "")
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
    let said = &asked.turns.last().unwrap().text;
    assert!(said.starts_with("(Sent ") && said.ends_with(")\n\nWhat does Zoe want for Thursday?"), "{said}");
    calls(&mut s, q, "Let me look.", &[("find_messages", json!({"query": "from:zoe storyboard"}))]);
    let (q, asked) = question(&mut s);
    let found = &asked.turns.last().unwrap().results[0];
    // Named as a link can hold it, and read by that name.
    let name = name_of(&story);
    assert!(found.content.contains(&format!("[{name}]")) && found.content.contains(STORYBOARD), "{}", found.content);
    assert!(!found.error);
    calls(&mut s, q, "", &[("read_message", json!({"id": name}))]);
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
    let (q, asked) = question(&mut s);
    // Proposed together, the person approves them with one click (Approve all).
    assert!(asked.system.contains("in a reply of its own, propose every one at once"), "told to propose a request's changes together");
    // Asked first ("Shall I move it?"), the person would say yes twice: to the model, then on the card.
    assert!(asked.system.contains("propose it: don't ask first whether to"), "told to propose a change asked for, not to ask about it");
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

/// A change proposed beside a find is put off, the chat going on at once, for the model to
/// propose it again with the others: the person approves them together, with one click.
#[test]
fn a_change_proposed_beside_a_find_is_put_off_to_be_approved_with_the_others() {
    let mut s = ready("assistant-together");
    let story = id_of(&s, STORYBOARD);
    let c = send(&mut s, None, "Flag Zoe's storyboard mail and archive it");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("find_messages", json!({"query": "from:zoe"})), ("move_messages", json!({"ids": [story], "folder": "archive"}))]);
    let v = chat(&mut s, &c);
    assert_eq!(v["waitsOnYou"], false, "nothing to approve yet");
    assert_eq!(v["chat"]["runs"][1]["state"], "putOff");
    assert!(s.list.iter().any(|m| m.key.to_id() == story), "nothing moved");
    // Asked again at once: what it found, and the change to propose again.
    let (q, asked) = question(&mut s);
    let results = &asked.turns.last().unwrap().results;
    assert!(results[0].content.contains(STORYBOARD), "{:?}", results[0].content);
    assert!(results[1].content.starts_with("Not proposed yet") && !results[1].error, "{:?}", results[1].content);
    // Proposed together, both are approved at once, and the model is asked once.
    // (Flagged first: moved, its id is its new folder's.)
    calls(
        &mut s,
        q,
        "",
        &[("mark_messages", json!({"ids": [story], "flagged": true})), ("move_messages", json!({"ids": [story], "folder": "archive"}))],
    );
    let v = chat(&mut s, &c);
    let runs = v["chat"]["runs"].as_array().unwrap();
    assert_eq!(runs.iter().filter(|r| r["state"] == "proposed").count(), 2, "{runs:?}");
    assert!(questions(&mut s).is_empty(), "nothing asked while the person decides");
    s.run("assistant.decide", &json!({"chat": c, "approve": true})).unwrap();
    let (_, asked) = question(&mut s);
    let results = &asked.turns.last().unwrap().results;
    assert!(results.iter().all(|r| r.content.starts_with("Approved.")), "{results:?}");
}

/// A change proposed beside a read that found nothing (it failed) stands: there's nothing to
/// propose it again with.
#[test]
fn a_change_beside_a_read_that_failed_is_not_put_off() {
    let mut s = ready("assistant-not-put-off");
    let story = id_of(&s, STORYBOARD);
    let c = send(&mut s, None, "Flag Zoe's storyboard mail");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("read_message", json!({"id": "no-such-message"})), ("mark_messages", json!({"ids": [story], "flagged": true}))]);
    let v = chat(&mut s, &c);
    let runs = v["chat"]["runs"].as_array().unwrap();
    assert_eq!((&runs[0]["state"], &runs[1]["state"]), (&json!("failed"), &json!("proposed")), "{runs:?}");
    assert_eq!(v["waitsOnYou"], true);
}

/// Two calls an answer gave one id get one each: a card each, and approving one applies only
/// its change.
#[test]
fn calls_given_one_id_are_approved_each_on_its_own() {
    let mut s = ready("assistant-one-id");
    let story = id_of(&s, STORYBOARD);
    let c = send(&mut s, None, "Mark Zoe's storyboard mail read and archive it");
    let (q, _) = question(&mut s);
    let call = |name: &str, input: Value| AiToolCall { id: "call_0".into(), name: name.into(), input };
    let calls =
        vec![call("mark_messages", json!({"ids": [story], "read": true})), call("move_messages", json!({"ids": [story], "folder": "archive"}))];
    s.apply(Event::Ai { done: AiDone::Answered { id: q, answer: Some(AiAnswer { calls, ..AiAnswer::default() }), error: None } });
    let v = chat(&mut s, &c);
    let runs = v["chat"]["runs"].as_array().unwrap();
    let ids: Vec<&str> = runs.iter().map(|r| r["call"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1], "{runs:?}");
    assert!(runs[1]["what"].as_str().unwrap().starts_with("Move"), "each card its own change: {runs:?}");
    s.run("assistant.decide", &json!({"chat": c, "call": ids[0], "approve": true})).unwrap();
    let m = s.list.iter().find(|m| m.key.to_id() == story).expect("not moved");
    assert!(m.flags.seen, "read");
    let v = chat(&mut s, &c);
    assert_eq!(v["chat"]["runs"][1]["state"], "proposed", "the move still waits");
}

/// Approved together, a message's move comes after its other changes, whatever the model's
/// order: moved first, its id would be its new folder's, and the mark would find it gone.
#[test]
fn approved_together_a_move_comes_after_the_other_changes() {
    let mut s = ready("assistant-move-last");
    let story = id_of(&s, STORYBOARD);
    let c = send(&mut s, None, "Archive Zoe's storyboard mail and mark it read");
    let (q, _) = question(&mut s);
    calls(
        &mut s,
        q,
        "",
        &[("move_messages", json!({"ids": [story], "folder": "archive"})), ("mark_messages", json!({"ids": [story], "read": true}))],
    );
    s.run("assistant.decide", &json!({"chat": c, "approve": true})).unwrap();
    let archived = s.folder_state("sample", "Archive").messages.iter().find(|m| m.subject == STORYBOARD).map(|m| m.flags.seen);
    assert_eq!(archived, Some(true), "archived and read");
    let (_, asked) = question(&mut s);
    let results = &asked.turns.last().unwrap().results;
    assert!(results.iter().all(|r| r.content.starts_with("Approved.")), "{results:?}");
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

/// What a search found, what's selected and the folders listed are in the chat as a message read
/// is (who wrote what, about what; the account's name and folders): a chat holding them never
/// goes to a model their account doesn't allow.
#[test]
fn mail_found_listed_or_selected_is_kept_from_a_model_its_account_does_not_allow() {
    let mut s = sample_session("assistant-found-allowed");
    s.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    let cloud = cloud(&mut s, "Claude");
    let local = local(&mut s, "Local");
    s.run("ai.use", &json!({"task": "chat", "profile": local})).unwrap();
    let story = id_of(&s, STORYBOARD);
    s.run("list.select", &json!({"key": story})).unwrap();
    s.take_jobs();
    for tool in [("find_messages", json!({"query": "storyboard"})), ("list_folders", json!({})), ("selected_messages", json!({}))] {
        let c = send(&mut s, None, "Look");
        let (q, _) = question(&mut s);
        calls(&mut s, q, "", std::slice::from_ref(&tool));
        let (q, asked) = question(&mut s);
        assert!(!asked.turns.last().unwrap().results[0].error, "{}: {:?}", tool.0, asked.turns.last().unwrap().results);
        answer(&mut s, q, "Done");
        s.run("assistant.model", &json!({"chat": c, "profile": cloud})).unwrap();
        let e = s.run("assistant.send", &json!({"chat": c, "text": "And now?"})).unwrap_err().to_string();
        assert!(e.contains("holds mail that may not go to Claude"), "{}: {e}", tool.0);
        assert!(questions(&mut s).is_empty(), "{}", tool.0);
    }
}

/// A change to mail is the model's to propose only when that mail may go to it: named by its id
/// or not, no move, mark or answer of an account the model may not read is proposed, and it
/// isn't told whether such a message is there.
#[test]
fn a_change_is_never_proposed_on_mail_its_model_may_not_read() {
    let mut s = sample_session("assistant-change-allowed");
    s.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    let cloud = cloud(&mut s, "Claude");
    s.run("ai.use", &json!({"task": "chat", "profile": cloud})).unwrap();
    s.take_jobs();
    let story = id_of(&s, STORYBOARD);
    let c = send(&mut s, None, "Tidy up");
    let (q, _) = question(&mut s);
    calls(
        &mut s,
        q,
        "",
        &[
            ("move_messages", json!({"ids": [story], "folder": "archive"})),
            ("mark_messages", json!({"ids": ["sample:1:999999:INBOX"], "read": true})),
            ("write_message", json!({"reply_to": story, "body": "Hi"})),
            ("write_message", json!({"account": "sample", "to": "zoe@studio.example", "body": "Hi"})),
        ],
    );
    let v = chat(&mut s, &c);
    let runs = v["chat"]["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 4);
    for r in runs {
        assert_eq!(r["state"], "failed", "{r}");
        assert!(r["result"].as_str().unwrap().contains("may not go to Claude"), "{r}");
    }
    assert_eq!(v["waitsOnYou"], false);
}

/// A chat stopped (or deleted) is let go: when its model then fails, its mail goes to no
/// fallback model.
#[test]
fn a_stopped_chat_goes_to_no_fallback() {
    let mut s = sample_session("assistant-stop-fallback");
    s.run("ai.allow", &json!({"account": "sample", "reach": "any"})).unwrap();
    let cloud = cloud(&mut s, "Claude");
    let local = local(&mut s, "Local");
    s.run(
        "ai.saveProfile",
        &json!({"id": local, "name": "Local", "api": "openAi", "url": "http://localhost:11434/v1/", "model": "llama3.2", "fallback": cloud}),
    )
    .unwrap();
    s.run("ai.use", &json!({"task": "chat", "profile": local})).unwrap();
    s.take_jobs();
    let story = id_of(&s, STORYBOARD);
    let c = send(&mut s, None, "Summarise");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("read_message", json!({"id": story}))]);
    let (q, _) = question(&mut s);
    s.run("assistant.stop", &json!({"chat": c})).unwrap();
    fail(&mut s, q, "network: connection refused");
    assert!(questions(&mut s).is_empty(), "nothing asked of the fallback");
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
    assert_eq!((last.role, last.text.ends_with(")\n\nGo on")), (AiRole::User, true), "{}", last.text);
    assert_eq!(last.results.len(), 1, "said with what the last call gave");
}

/// Stopped, the answer it waited for is let go as it comes, and its question is stopped: the
/// model answers no more. What it proposed is declined. A chat deleted as it waits is stopped
/// too.
#[test]
fn a_stopped_chat_lets_its_answer_go() {
    let mut s = ready("assistant-stop");
    let stopped = |s: &mut Session, q: u64| s.take_jobs().iter().any(|j| matches!(j, Job::Ai { work: AiWork::Cancel { id } } if *id == q));
    let c = send(&mut s, None, "Anything new?");
    let (q, _) = question(&mut s);
    assert_eq!(chat(&mut s, &c)["asking"], true);
    s.run("assistant.stop", &json!({"chat": c})).unwrap();
    assert!(stopped(&mut s, q), "its question stopped");
    answer(&mut s, q, "Too late.");
    let v = chat(&mut s, &c);
    assert_eq!(v["chat"]["turns"].as_array().map(Vec::len), Some(1), "{v}");
    assert_eq!(v["asking"], false);
    assert!(questions(&mut s).is_empty());
    let d = send(&mut s, None, "And now?");
    let (q, _) = question(&mut s);
    s.run("assistant.delete", &json!({"chat": d})).unwrap();
    assert!(stopped(&mut s, q), "deleted, its question stopped");
}

/// A question that fails goes to its profile's fallback fitted again for what that model takes
/// in, not as it was fitted for the first; one too long for any fallback goes to none.
#[test]
fn a_fallback_gets_the_chat_fitted_for_its_own_context() {
    let mut s = ready("assistant-fallback-fitted");
    let profile = |s: &mut Session, name: &str, context: u32, fallback: &str| {
        let v = json!({"name": name, "api": "openAi", "url": "http://localhost:11434/v1", "model": name, "context": context, "maxTokens": 512, "fallback": fallback});
        s.run("ai.saveProfile", &v).unwrap()["id"].as_str().unwrap().to_string()
    };
    let small = profile(&mut s, "small", 8192, "");
    let big = profile(&mut s, "big", 131_072, &small);
    s.run("ai.use", &json!({"task": "chat", "profile": big})).unwrap();
    send(&mut s, None, "Read everything");
    for _ in 0..10 {
        let (q, _) = question(&mut s);
        calls(&mut s, q, "", &[("find_messages", json!({"limit": 50}))]);
    }
    let size = |c: &AiChat| c.turns.iter().map(|t| t.size() / 3 + 1).sum::<usize>();
    let (q, asked) = question(&mut s);
    assert!(size(&asked) > 8192, "the big model took it all: {}", size(&asked));
    fail(&mut s, q, "network: connection refused");
    let (q, again) = question(&mut s);
    assert!(size(&again) < 8192 - 512, "fitted for the small one: {}", size(&again));
    // Too long for the small one at all: not asked.
    fail(&mut s, q, "network: connection refused");
    let c = send(&mut s, None, &"word ".repeat(9000));
    let (q, _) = question(&mut s);
    fail(&mut s, q, "network: connection refused");
    assert!(questions(&mut s).is_empty(), "no fallback that can't hold it");
    assert!(chat(&mut s, &c)["chat"]["error"].as_str().is_some_and(|e| e.contains("connection refused")));
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
    let c = send(&mut s, None, &format!("{}first", "word ".repeat(1200)));
    let (q, _) = question(&mut s);
    answer(&mut s, q, "Long.");
    send(&mut s, Some(&c), &format!("{}second", "word ".repeat(1200)));
    let (_, asked) = question(&mut s);
    assert!(asked.system.contains("left out for room"));
    assert_eq!(asked.turns.len(), 1);
    assert!(asked.turns[0].text.ends_with("second"));
    // What the person said, and what its calls gave, too long together: what the calls gave is
    // cut to fit, never left out (their call and the words would go with it).
    send(&mut s, None, &"word ".repeat(1200));
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("find_messages", json!({"limit": 50}))]);
    let (_, asked) = question(&mut s);
    let size: usize = asked.turns.iter().map(|t| t.size() / 3 + 1).sum();
    assert!(size < 8192 - 512, "{size}");
    let given = &asked.turns.last().unwrap().results[0].content;
    assert!(given.contains("[… cut here: ") && given.contains(STORYBOARD), "{given}");
    // The person's own words too long: said so.
    let e = s.run("assistant.send", &json!({"text": "word ".repeat(9000)})).unwrap_err();
    assert!(e.to_string().contains("more than Local takes in"), "{e}");
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

/// Names are kept in saved chats (in what the tools gave, and in the answers' links): one is
/// made as it always was.
#[test]
fn a_messages_name_is_made_as_it_always_was() {
    assert_eq!(crate::assistant_message_link("sample:1:1:INBOX"), "mailcraft:m36c1254ac9");
}

proptest::proptest! {
    /// Whatever its id holds (a folder's name with spaces and brackets), a message's link is
    /// one a Markdown link can hold.
    #[test]
    fn a_link_is_always_one_markdown_holds(id in "\\PC{0,80}") {
        let link = crate::assistant_message_link(&id);
        let name = link.strip_prefix("mailcraft:m").unwrap_or_default();
        proptest::prop_assert!(name.len() == 10 && name.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{}", link);
    }
}

/// The model names each message by a name a link holds and links what it names; a link opens
/// the message it names (Mail, its folder, the message), from wherever the window is. Only a
/// message the chat has seen: a link is written from mail others wrote.
#[test]
fn an_answers_links_open_the_messages_the_chat_has_seen() {
    let mut s = ready("assistant-links");
    let story = id_of(&s, STORYBOARD);
    let other = s.list.iter().find(|m| m.subject != STORYBOARD).map(|m| m.key.to_id()).unwrap();
    let c = send(&mut s, None, "Find Zoe's mail");
    let (q, asked) = question(&mut s);
    assert!(asked.system.contains("[Storyboard review](mailcraft:m0123456789)"), "told to link what it names");
    calls(&mut s, q, "", &[("find_messages", json!({"query": "from:zoe"}))]);
    let (q, _) = question(&mut s);
    let link = crate::assistant_message_link(&story);
    // A change takes the name too.
    calls(&mut s, q, &format!("Zoe wrote [{STORYBOARD}]({link})."), &[("mark_messages", json!({"ids": [name_of(&story)], "flagged": false}))]);
    let v = chat(&mut s, &c);
    assert_eq!(v["chat"]["runs"][1]["messages"], json!([story]), "{v}");
    // From the calendar, another folder open.
    s.run("folder.open", &json!({"account": "sample", "folder": "Archive"})).unwrap();
    s.run("view.calendar", &json!({})).unwrap();
    let opened = s.run("assistant.open", &json!({"chat": c, "link": link})).unwrap();
    assert_eq!(opened["key"], json!(story));
    assert_eq!(s.module, crate::Module::Mail);
    assert!(matches!(&s.view, crate::View::Folder { folder, .. } if folder == "INBOX"), "{:?}", s.view);
    assert_eq!(s.reading.as_ref().map(|r| r.key.to_id()), Some(story.clone()));
    // Not one it hasn't seen, by its name or its id; nor junk.
    for link in [crate::assistant_message_link(&other), format!("mailcraft:{other}"), other.clone(), "mailcraft:m0123456789".into(), String::new()] {
        assert!(s.run("assistant.open", &json!({"chat": c, "link": link})).is_err(), "{link}");
    }
    assert_eq!(s.reading.as_ref().map(|r| r.key.to_id()), Some(story), "left open");
}

/// The name in a tool's line: what's between its first brackets.
fn bracketed(line: &str) -> String {
    let from = line.find('[').unwrap() + 1;
    let to = line[from..].find(']').unwrap() + from;
    line[from..to].to_string()
}

/// Events and people are found and named as messages are, and an answer's link to one goes to
/// it: the calendar on the event's day (the window then opens the event), or People (the window
/// shows the person's card). Only what the chat found; nothing gone.
#[test]
fn the_calendar_and_contacts_are_found_and_their_links_go_to_them() {
    let mut s = ready("assistant-pim");
    let c = send(&mut s, None, "When do I see Jun, and how do I reach Zoe?");
    let (q, asked) = question(&mut s);
    let names: Vec<&str> = asked.tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"find_events") && names.contains(&"find_people"), "{names:?}");
    assert!(asked.system.contains("[Zoe Park](mailcraft:p0123456789)"), "told to link people too");
    calls(&mut s, q, "", &[("find_events", json!({"query": "Jun"})), ("find_people", json!({"query": "zoe"}))]);
    let (q, asked) = question(&mut s);
    let results = &asked.turns.last().unwrap().results;
    let (events, people) = (&results[0].content, &results[1].content);
    assert!(events.contains("| Lunch with Jun | Where: Café on the corner | Calendar: Calendar"), "{events}");
    assert!(people.contains("Zoe Park | zoe@studio.example | Phone: +1 555 0142 | Director, Studio Lantern"), "{people}");
    let (event, person) = (bracketed(events), bracketed(people));
    assert!(event.starts_with('e') && person.starts_with('p') && event.len() == 11, "{event} {person}");
    answer(&mut s, q, &format!("[Lunch with Jun](mailcraft:{event}) with [Zoe Park](mailcraft:{person})."));
    // The event: the calendar on its day, for the window to open it.
    s.run("calendar.goto", &json!({"date": "2001-01-01"})).unwrap();
    let opened = s.run("assistant.open", &json!({"chat": c, "link": format!("mailcraft:{event}")})).unwrap();
    assert_eq!(opened["event"]["uid"], json!("lunch@sample.mailcraft"), "{opened}");
    assert_eq!(s.module, crate::Module::Calendar);
    assert_eq!(s.cal_day(), mailcraft_model::dates::today(&s.clock));
    // The person: People, for the window to show their card.
    let opened = s.run("assistant.open", &json!({"chat": c, "link": format!("mailcraft:{person}")})).unwrap();
    assert_eq!(opened["person"], json!("zoe@sample.mailcraft"), "{opened}");
    assert_eq!(s.module, crate::Module::People);
    // Nothing the chat didn't find; an event deleted since, said to be gone.
    for link in ["mailcraft:p0123456789", "mailcraft:e0123456789", "e0123456789"] {
        assert!(s.run("assistant.open", &json!({"chat": c, "link": link})).is_err(), "{link}");
    }
    s.run("event.delete", &json!({"uid": "lunch@sample.mailcraft"})).unwrap();
    let e = s.run("assistant.open", &json!({"chat": c, "link": format!("mailcraft:{event}")})).unwrap_err().to_string();
    assert!(e.contains("isn't in the calendar any more"), "{e}");
}

/// An account's events and contacts go only to a model its mail may go to, as its mail does.
#[test]
fn events_and_people_go_only_to_a_model_their_account_allows() {
    let mut s = sample_session("assistant-pim-allowed");
    s.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    let cloud = cloud(&mut s, "Claude");
    s.run("ai.use", &json!({"task": "chat", "profile": cloud})).unwrap();
    s.take_jobs();
    send(&mut s, None, "Who's Zoe, and what's next?");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("find_events", json!({})), ("find_people", json!({"query": "zoe"})), ("find_events", json!({"account": "sample"}))]);
    let (_, asked) = question(&mut s);
    let results = &asked.turns.last().unwrap().results;
    assert_eq!(results[0].content, "No events found.");
    assert_eq!(results[1].content, "No one found.");
    assert!(results[2].error && results[2].content.contains("may not go to Claude"), "{:?}", results[2]);
}

/// An event is proposed as the other changes are, said on its card, and added to the calendar
/// only once the person approves it, with the window's reminder; the answer's link to it goes
/// to it. It's the person's alone: no one is invited, nothing is sent.
#[test]
fn an_event_is_added_to_the_calendar_only_once_approved() {
    let mut s = ready("assistant-event");
    let zooms = |s: &Session| -> Vec<mailcraft_model::ical::CalEvent> {
        s.pim.values().flat_map(|p| p.events.iter()).filter(|e| e.event.summary == "Zoom the team").map(|e| e.event.clone()).collect()
    };
    let tomorrow = mailcraft_model::dates::today(&s.clock) + 1;
    let day = mailcraft_model::dates::day_string(tomorrow);
    let c = send(&mut s, None, "Add a calendar reminder for tomorrow to zoom the team");
    let (q, asked) = question(&mut s);
    assert!(asked.tools.iter().any(|t| t.name == "add_event"));
    let input = json!({"summary": "Zoom the team", "start": format!("{day}T10:00"), "end": format!("{day}T10:30"), "location": "Zoom"});
    calls(&mut s, q, "", &[("add_event", input)]);
    let v = chat(&mut s, &c);
    let run = &v["chat"]["runs"][0];
    assert_eq!(run["state"], "proposed");
    let what = run["what"].as_str().unwrap();
    assert!(
        what.starts_with("Add “Zoom the team” to Calendar: ") && what.ends_with("10:00 to 10:30, at Zoom; a reminder 15 minutes before"),
        "{what}"
    );
    assert!(zooms(&s).is_empty(), "nothing added before it's approved");
    s.run("assistant.decide", &json!({"chat": c, "approve": true})).unwrap();
    let added = zooms(&s);
    assert_eq!(added.len(), 1);
    assert_eq!(mailcraft_model::ical::reminder_minutes(&added[0]), Some(15));
    assert_eq!(added[0].end - added[0].start, 1800);
    assert!(added[0].attendees.is_empty() && added[0].organizer.is_none());
    let jobs = s.take_jobs();
    assert!(!jobs.iter().any(|j| matches!(j, Job::Send { .. })), "no one invited");
    // The model is told it's done and what it's named; the answer's link goes to it.
    let (q, asked) = jobs
        .into_iter()
        .find_map(|j| match j {
            Job::Ai { work: AiWork::Ask { id, chat, .. } } => Some((id, chat)),
            _ => None,
        })
        .expect("asked again");
    let done = &asked.turns.last().unwrap().results[0].content;
    assert!(done.starts_with("Approved. Done: added to Calendar, as e"), "{done}");
    let name = done.trim_end_matches('.').rsplit(' ').next().unwrap().to_string();
    answer(&mut s, q, &format!("[Zoom the team](mailcraft:{name}) is in your calendar."));
    let opened = s.run("assistant.open", &json!({"chat": c, "link": format!("mailcraft:{name}")})).unwrap();
    assert_eq!(opened["event"]["uid"], json!(added[0].uid), "{opened}");
    assert_eq!(s.module, crate::Module::Calendar);
    assert_eq!(s.cal_day(), tomorrow);
}

/// An event is checked as it's proposed: its title, its times, its calendar, and an account
/// whose mail the model may read. Each refused goes back to the model at once.
#[test]
fn an_event_is_checked_as_it_is_proposed() {
    let mut s = sample_session("assistant-event-checked");
    s.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    let cloud = cloud(&mut s, "Claude");
    let local = local(&mut s, "Local");
    s.run("ai.use", &json!({"task": "chat", "profile": local})).unwrap();
    s.take_jobs();
    send(&mut s, None, "Add it");
    let (q, _) = question(&mut s);
    let at = |more: Value| {
        let mut v = json!({"summary": "Zoom the team", "start": "2026-11-24T10:00"});
        v.as_object_mut().unwrap().extend(more.as_object().unwrap().clone());
        v
    };
    let tries = [
        json!({"start": "2026-11-24T10:00"}),
        at(json!({"start": "tomorrow"})),
        at(json!({"end": "2026-11-24T09:00"})),
        at(json!({"calendar": "Holidays"})),
        at(json!({"reminder": -5})),
    ];
    calls(&mut s, q, "", &tries.iter().map(|t| ("add_event", t.clone())).collect::<Vec<_>>());
    let (_, asked) = question(&mut s);
    let results = &asked.turns.last().unwrap().results;
    let said: Vec<&str> = results.iter().map(|r| r.content.as_str()).collect();
    assert!(results.iter().all(|r| r.error), "{said:?}");
    for (n, why) in
        ["`summary`", "not a date", "ends before it starts", "no calendar `Holidays`: those an event can go in are Calendar", "`reminder`"]
            .iter()
            .enumerate()
    {
        assert!(said[n].contains(why), "{n}: {}", said[n]);
    }
    // A model the account's mail may not go to adds nothing to its calendar.
    s.run("ai.use", &json!({"task": "chat", "profile": cloud})).unwrap();
    s.take_jobs();
    send(&mut s, None, "Add it");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("add_event", at(json!({}))), ("add_event", at(json!({"account": "sample"})))]);
    let (_, asked) = question(&mut s);
    let results = &asked.turns.last().unwrap().results;
    assert!(results.iter().all(|r| r.error), "{results:?}");
    assert!(results[0].content.contains("no account whose mail may go to Claude has one"), "{}", results[0].content);
    assert!(results[1].content.contains("account sample's mail may not go to Claude"), "{}", results[1].content);
}
