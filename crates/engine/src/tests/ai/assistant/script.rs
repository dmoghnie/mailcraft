//! The assistant's scripts (code mode): a script finds and reads in one step, through the tools'
//! own checks, waits for a message to download, and runs only for a model the person let.

use super::waiting::online;
use super::*;

/// The model asked may run scripts.
pub(super) fn scripts_on(s: &mut Session) {
    let p = s.prefs.ai.profiles.first().unwrap().clone();
    let v = json!({"id": p.id, "name": p.name, "api": "openAi", "url": p.endpoint.url, "model": p.endpoint.model, "scripts": true});
    s.run("ai.saveProfile", &v).unwrap();
}

/// What the model was given back for its last calls.
pub(super) fn results(chat: &AiChat) -> Vec<mailcraft_model::AiToolResult> {
    chat.turns.last().unwrap().results.clone()
}

/// A script finds a message, reads it and says what it found, in one step: what it printed
/// and came to go back to the model, and each of its calls shows under its card.
#[test]
fn a_script_finds_and_reads_in_one_step() {
    let mut s = ready("script-reads");
    scripts_on(&mut s);
    let id = send(&mut s, None, "Who wrote about the storyboard review?");
    let (q, asked) = question(&mut s);
    assert!(asked.tools.iter().any(|t| t.name == "run_script"), "told of scripts");
    let script = r#"
        let found = find_messages(#{query: "storyboard"});
        let m = read_message(found.messages[0].id);
        print(`From: ${m.from.email}`);
        m.subject
    "#;
    calls(&mut s, q, "", &[("run_script", json!({"script": script, "what": "Read the storyboard mail"}))]);
    let (_, asked) = question(&mut s);
    let r = &results(&asked)[0];
    assert!(!r.error, "{}", r.content);
    assert!(r.content.contains("From: ") && r.content.contains('@'), "{}", r.content);
    assert!(r.content.contains("It came to: ") && r.content.contains(STORYBOARD), "{}", r.content);
    let c = chat(&mut s, &id);
    let runs = c["chat"]["runs"].as_array().unwrap();
    let card = runs.iter().find(|r| r["tool"] == "run_script").unwrap();
    assert_eq!((card["state"].as_str(), card["what"].as_str()), (Some("ran"), Some("Read the storyboard mail")));
    let parts: Vec<&str> = runs.iter().filter(|r| r["partOf"] == card["call"]).map(|r| r["tool"].as_str().unwrap()).collect();
    assert_eq!(parts, ["find_messages", "read_message"]);
}

/// The model is told the fields a message has, each of them and no others.
#[test]
fn the_model_is_told_the_fields_a_message_has() {
    let mut s = ready("script-fields");
    scripts_on(&mut s);
    send(&mut s, None, "What does a message hold?");
    let (q, asked) = question(&mut s);
    let told = &asked.tools.iter().find(|t| t.name == "run_script").unwrap().description;
    let listed = told.split("A message has ").nth(1).and_then(|t| t.split(':').next()).unwrap().replace(" and ", ", ");
    let mut listed: Vec<&str> = listed.split(", ").collect();
    listed.sort_unstable();
    let script = r#"find_messages(#{query: "storyboard"}).messages[0].keys()"#;
    calls(&mut s, q, "", &[("run_script", json!({"script": script}))]);
    let (_, asked) = question(&mut s);
    let r = &results(&asked)[0];
    let came: Vec<String> = serde_json::from_str(r.content.split("It came to: ").nth(1).unwrap()).unwrap();
    let mut has: Vec<&str> = came.iter().map(String::as_str).collect();
    has.sort_unstable();
    assert_eq!(listed, has);
}

/// A model the person didn't let run scripts isn't told of them, and one it calls anyway
/// doesn't run.
#[test]
fn a_model_not_let_run_scripts_runs_none() {
    let mut s = ready("script-off");
    send(&mut s, None, "Count my unread mail");
    let (q, asked) = question(&mut s);
    assert!(!asked.tools.iter().any(|t| t.name == "run_script"));
    calls(&mut s, q, "", &[("run_script", json!({"script": "find_messages().total"}))]);
    let (_, asked) = question(&mut s);
    let r = &results(&asked)[0];
    assert!(r.error && r.content.contains("may not run scripts"), "{}", r.content);
}

/// A script reads only the mail its model may: an account whose mail may not go to it isn't
/// there.
#[test]
fn a_script_reads_only_mail_its_model_may_read() {
    let mut s = ready("script-allowed");
    let keys = online(&mut s, &["Quarterly figures"]);
    s.run("ai.allow", &json!({"account": keys[0].account, "reach": "off"})).unwrap();
    scripts_on(&mut s);
    send(&mut s, None, "Which accounts?");
    let (q, _) = question(&mut s);
    let script = r#"let n = []; for a in list_folders() { n.push(a.account); } n"#;
    calls(&mut s, q, "", &[("run_script", json!({"script": script}))]);
    let (_, asked) = question(&mut s);
    let r = &results(&asked)[0];
    assert!(r.content.contains("sample") && !r.content.contains(&keys[0].account), "{}", r.content);
}

/// A script that reads a message still to download waits for it, and goes on once it came:
/// the model is asked again only then.
#[test]
fn a_script_waits_for_a_message_to_download() {
    let mut s = ready("script-waits");
    let keys = online(&mut s, &["Quarterly figures"]);
    scripts_on(&mut s);
    let id = send(&mut s, None, "What do the quarterly figures say?");
    let (q, _) = question(&mut s);
    let script = r#"let f = find_messages(#{query: "Quarterly"}); read_message(f.messages[0].id).text"#;
    calls(&mut s, q, "", &[("run_script", json!({"script": script}))]);
    let jobs = s.take_jobs();
    assert!(jobs.iter().any(|j| matches!(j, Job::FetchBody { key } if *key == keys[0])), "asked for");
    assert!(!jobs.iter().any(|j| matches!(j, Job::Ai { .. })), "not asked while it waits");
    let c = chat(&mut s, &id);
    assert_eq!(c["chat"]["runs"].as_array().unwrap().iter().find(|r| r["tool"] == "run_script").unwrap()["state"], "waiting");
    let raw = b"From: Ana <ana@online.example>\r\nSubject: Quarterly figures\r\n\r\nRevenue is up 4%.\r\n";
    s.store.as_ref().unwrap().write_body(&keys[0], raw).unwrap();
    s.apply(Event::Body { key: keys[0].clone(), raw: raw.to_vec() });
    let (_, asked) = question(&mut s);
    let r = &results(&asked)[0];
    assert!(!r.error && r.content.contains("It came to: Revenue is up 4%."), "{}", r.content);
}

/// A chat stopped while its script waits lets it go: the message coming later runs nothing.
#[test]
fn a_script_stopped_while_it_waits_is_let_go() {
    let mut s = ready("script-stopped");
    let keys = online(&mut s, &["Quarterly figures"]);
    scripts_on(&mut s);
    let id = send(&mut s, None, "What do the quarterly figures say?");
    let (q, _) = question(&mut s);
    let script = r#"let f = find_messages(#{query: "Quarterly"}); read_message(f.messages[0].id).text"#;
    calls(&mut s, q, "", &[("run_script", json!({"script": script}))]);
    s.take_jobs();
    s.run("assistant.stop", &json!({"chat": id})).unwrap();
    let c = chat(&mut s, &id);
    let runs = c["chat"]["runs"].as_array().unwrap();
    let card = runs.iter().find(|r| r["tool"] == "run_script").unwrap();
    assert_eq!((card["state"].as_str(), card["result"].as_str()), (Some("failed"), Some("stopped before the script ended")));
    assert!(runs.iter().all(|r| r["state"] != "waiting"), "its read stopped too");
    let raw = b"From: Ana <ana@online.example>\r\nSubject: Quarterly figures\r\n\r\nRevenue is up 4%.\r\n";
    s.store.as_ref().unwrap().write_body(&keys[0], raw).unwrap();
    s.apply(Event::Body { key: keys[0].clone(), raw: raw.to_vec() });
    assert!(questions(&mut s).is_empty(), "nothing runs");
}

/// A call the model names as a script's own would be named (`s#1`) is given another id: the
/// script's call doesn't take its place, and each goes back to the model.
#[test]
fn a_models_call_named_as_a_scripts_own_keeps_its_place() {
    let mut s = ready("script-ids");
    scripts_on(&mut s);
    let id = send(&mut s, None, "How many folders, and how much mail about the storyboard?");
    let (q, _) = question(&mut s);
    let script = r#"find_messages(#{query: "storyboard"}).total"#;
    let made = [("s", "run_script", json!({"script": script})), ("s#1", "list_folders", json!({}))];
    let calls = made.iter().map(|(id, name, input)| AiToolCall { id: (*id).into(), name: (*name).into(), input: input.clone() }).collect();
    let a = AiAnswer { calls, ..AiAnswer::default() };
    s.apply(Event::Ai { done: AiDone::Answered { id: q, answer: Some(a), error: None } });
    let c = chat(&mut s, &id);
    let tools: Vec<(&str, &str)> =
        c["chat"]["runs"].as_array().unwrap().iter().map(|r| (r["call"].as_str().unwrap(), r["tool"].as_str().unwrap())).collect();
    assert_eq!(tools.iter().filter(|(_, t)| *t == "list_folders").count(), 1, "{tools:?}");
    assert_eq!(tools.iter().filter(|(_, t)| *t == "find_messages").count(), 1, "{tools:?}");
    let (_, asked) = question(&mut s);
    let r = results(&asked);
    assert!(r[0].content.starts_with("It came to: "), "{}", r[0].content);
    assert!(r[1].content.contains("Inbox") && !r[1].content.contains(STORYBOARD), "the folders: {}", r[1].content);
}

/// The script that archives the storyboard mail: what it proposes.
const ARCHIVES: &str = r#"
    let found = find_messages(#{query: "storyboard"});
    let ids = [];
    for m in found.messages { ids.push(m.id); }
    let p = move_messages(#{ids: ids, folder: "archive"});
    print(p.proposed);
    ids.len()
"#;

/// The proposal script `call` made: its run.
fn proposal(c: &Value, call: &str) -> Value {
    c["chat"]["runs"].as_array().unwrap().iter().find(|r| r["partOf"] == call && r["tool"] != "find_messages").cloned().unwrap()
}

/// A change a script calls for is proposed, on a card under it: nothing changes, and the
/// model isn't asked again, until the person decides; approved, it's made, and the model is told.
#[test]
fn a_script_proposes_a_change_made_once_approved() {
    let mut s = ready("script-proposes");
    scripts_on(&mut s);
    let story = id_of(&s, STORYBOARD);
    let id = send(&mut s, None, "Archive the storyboard mail");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("run_script", json!({"script": ARCHIVES, "what": "Archive the storyboard mail"}))]);
    assert!(questions(&mut s).is_empty(), "not asked again before the person decides");
    let c = chat(&mut s, &id);
    assert_eq!(c["waitsOnYou"], true);
    let p = proposal(&c, "call_0");
    assert_eq!((p["tool"].as_str(), p["state"].as_str()), (Some("move_messages"), Some("proposed")), "{p}");
    assert!(s.list.iter().any(|r| r.key.to_id() == story), "nothing moved before it's approved");
    let decided = s.run("assistant.decide", &json!({"chat": id, "call": p["call"], "approve": true})).unwrap();
    assert_eq!(decided["decided"], 1);
    assert!(!s.list.iter().any(|r| r.key.to_id() == story), "approved: moved");
    assert_eq!(proposal(&chat(&mut s, &id), "call_0")["state"], "done");
    let (_, asked) = question(&mut s);
    let r = &results(&asked)[0];
    assert!(r.content.contains("It came to: 2") && r.content.contains("What it proposed:"), "{}", r.content);
    assert!(r.content.contains(": approved."), "{}", r.content);
}

/// Declined, a script's change isn't made, and the model is told so.
#[test]
fn a_script_change_declined_is_not_made() {
    let mut s = ready("script-declined");
    scripts_on(&mut s);
    let story = id_of(&s, STORYBOARD);
    let id = send(&mut s, None, "Archive the storyboard mail");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("run_script", json!({"script": ARCHIVES}))]);
    s.run("assistant.decide", &json!({"chat": id, "approve": false})).unwrap();
    assert!(s.list.iter().any(|r| r.key.to_id() == story));
    let (_, asked) = question(&mut s);
    let r = &results(&asked)[0];
    assert!(r.content.contains("the person declined: nothing changed"), "{}", r.content);
}

/// A script's change still waiting when MailCraft closed is decided after it opens again: what
/// the script called it with is kept with the chat.
#[test]
fn a_script_change_waiting_is_decided_after_a_restart() {
    let mut s = ready("script-kept");
    scripts_on(&mut s);
    let id = send(&mut s, None, "Archive the storyboard mail");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("run_script", json!({"script": ARCHIVES}))]);
    s.take_jobs();
    let store = s.store.clone().unwrap();
    let mut again = Session::open(Some(store), mailcraft_model::dates::Clock::new(NOW, 0));
    let c = chat(&mut again, &id);
    assert_eq!(c["waitsOnYou"], true, "{c}");
    let p = proposal(&c, "call_0");
    assert_eq!(p["input"]["folder"], "archive");
    let decided = again.run("assistant.decide", &json!({"chat": id, "call": p["call"], "approve": false})).unwrap();
    assert_eq!(decided["decided"], 1);
    assert_eq!(proposal(&chat(&mut again, &id), "call_0")["state"], "declined");
}

/// A script proposes so many changes at most: past them, a call fails (as an error it can
/// catch), and nothing is proposed.
#[test]
fn a_script_proposes_so_many_changes_at_most() {
    let mut s = ready("script-most");
    scripts_on(&mut s);
    let id = send(&mut s, None, "Mark the storyboard mail read, many times");
    let (q, _) = question(&mut s);
    let script = r#"
        let m = find_messages(#{query: "storyboard"}).messages[0].id;
        let n = 0;
        for i in 0..25 { try { mark_messages(#{ids: [m], read: true}); n += 1; } catch (e) { print(e); } }
        n
    "#;
    calls(&mut s, q, "", &[("run_script", json!({"script": script}))]);
    let c = chat(&mut s, &id);
    let proposed = c["chat"]["runs"].as_array().unwrap().iter().filter(|r| r["state"] == "proposed").count();
    assert_eq!(proposed, 20);
    s.run("assistant.decide", &json!({"chat": id, "approve": false})).unwrap();
    let (_, asked) = question(&mut s);
    let r = &results(&asked)[0];
    assert!(r.content.contains("It came to: 20") && r.content.contains("at most 20 changes"), "{}", r.content);
}

/// A script stopped after its MailCraft closed while it waited for a message says so.
#[test]
fn a_script_waiting_when_mailcraft_closed_says_it_never_ended() {
    let mut s = ready("script-closed");
    online(&mut s, &["Quarterly figures"]);
    scripts_on(&mut s);
    let id = send(&mut s, None, "What do the quarterly figures say?");
    let (q, _) = question(&mut s);
    let script = r#"let f = find_messages(#{query: "Quarterly"}); read_message(f.messages[0].id).text"#;
    calls(&mut s, q, "", &[("run_script", json!({"script": script}))]);
    s.take_jobs();
    let store = s.store.clone().unwrap();
    let mut again = Session::open(Some(store), mailcraft_model::dates::Clock::new(NOW, 0));
    let c = chat(&mut again, &id);
    let card = c["chat"]["runs"].as_array().unwrap().iter().find(|r| r["tool"] == "run_script").cloned().unwrap();
    assert_eq!((card["state"].as_str(), card["result"].as_str()), (Some("failed"), Some("MailCraft closed before the script ended")));
}

/// A script that fails after proposing a change withdraws it: only a script that ends is heard,
/// and the model, asked again at once, is told.
#[test]
fn a_script_that_fails_withdraws_what_it_proposed() {
    let mut s = ready("script-withdraws");
    scripts_on(&mut s);
    let id = send(&mut s, None, "Archive the storyboard mail");
    let (q, _) = question(&mut s);
    let script = format!("{ARCHIVES}; throw \"something went wrong\"");
    calls(&mut s, q, "", &[("run_script", json!({"script": script}))]);
    let c = chat(&mut s, &id);
    assert_eq!(c["waitsOnYou"], false);
    let p = proposal(&c, "call_0");
    assert_eq!((p["state"].as_str(), p["result"].as_str()), (Some("failed"), Some("not made: the script failed before it ended")));
    let (_, asked) = question(&mut s);
    let r = &results(&asked)[0];
    assert!(r.error && r.content.contains("something went wrong") && r.content.contains("couldn't be done: not made"), "{}", r.content);
}
