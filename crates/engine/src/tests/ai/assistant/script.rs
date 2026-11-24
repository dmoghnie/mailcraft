//! The assistant's scripts (code mode): a script finds and reads in one step, through the tools'
//! own checks, waits for a message to download, and runs only for a model the person let.

use super::waiting::online;
use super::*;

/// The model asked may run scripts.
fn scripts_on(s: &mut Session) {
    let p = s.prefs.ai.profiles.first().unwrap().clone();
    let v = json!({"id": p.id, "name": p.name, "api": "openAi", "url": p.endpoint.url, "model": p.endpoint.model, "scripts": true});
    s.run("ai.saveProfile", &v).unwrap();
}

/// What the model was given back for its last calls.
fn results(chat: &AiChat) -> Vec<mailcraft_model::AiToolResult> {
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
