//! Scripts kept to run again: kept once the person approves (or as they write them), each a
//! tool of its own and a function of every script, listed, changed, turned off and deleted.

use super::script::{results, scripts_on};
use super::*;

/// A script that finds the storyboard mail and gives its subject.
const SUBJECT_OF: &str = r#"let found = find_messages(#{query: params.about});
found.messages[0].subject"#;

fn listed(s: &mut Session) -> Vec<Value> {
    s.run("script.list", &json!({})).unwrap()["scripts"].as_array().unwrap().clone()
}

/// The kept script named `name`, as listed.
fn named(s: &mut Session, name: &str) -> Option<Value> {
    listed(s).into_iter().find(|k| k["name"] == name)
}

/// MailCraft's own scripts.
const SHIPPED: [&str; 5] = ["waiting_on_me", "waiting_on_them", "person_history", "today", "tidy"];

fn save(s: &mut Session, name: &str, code: &str) -> Result<Value, crate::CmdError> {
    s.run("script.save", &json!({"name": name, "about": "The subject of the newest message about something", "params": [{"name": "about", "about": "words it's about"}], "code": code}))
}

/// Kept, a script is listed and kept in the data folder; changed, renamed, turned off and
/// deleted as the person says; one that doesn't compile, or takes a tool's name, is refused.
#[test]
fn kept_scripts_are_listed_changed_and_deleted() {
    let mut s = ready("kept-scripts");
    save(&mut s, "subject_of", SUBJECT_OF).unwrap();
    let store = s.store.clone().unwrap();
    let mut again = Session::open(Some(store), mailcraft_model::dates::Clock::new(NOW, 0));
    let kept = named(&mut again, "subject_of").expect("kept");
    assert_eq!(kept["params"][0]["name"], "about");
    assert_eq!(listed(&mut again).len(), SHIPPED.len() + 1, "after MailCraft's own");
    let e = save(&mut again, "broken", "let x = ;").unwrap_err().to_string();
    assert!(e.contains("doesn't compile"), "{e}");
    let e = save(&mut again, "find_messages", "1").unwrap_err().to_string();
    assert!(e.contains("name already"), "{e}");
    let e = save(&mut again, "Not A Name", "1").unwrap_err().to_string();
    assert!(e.contains("lower-case"), "{e}");
    let v = json!({"name": "subject", "was": "subject_of", "about": "Renamed", "code": SUBJECT_OF});
    again.run("script.save", &v).unwrap();
    again.run("script.turn", &json!({"name": "subject", "on": false})).unwrap();
    assert!(named(&mut again, "subject_of").is_none());
    assert_eq!(named(&mut again, "subject").expect("renamed")["off"], true);
    again.run("script.delete", &json!({"name": "subject"})).unwrap();
    again.run("script.delete", &json!({"name": "tidy"})).unwrap();
    let names: Vec<Value> = listed(&mut again).iter().map(|k| k["name"].clone()).collect();
    assert_eq!(names, ["waiting_on_me", "waiting_on_them", "person_history", "today"], "one of MailCraft's own deleted stays deleted");
    assert!(again.run("script.delete", &json!({"name": "subject"})).is_err());
}

/// A kept script is a tool of its own for a model that may run scripts, and a function of a
/// script it writes; turned off, it's neither.
#[test]
fn a_kept_script_is_a_tool_and_a_function() {
    let mut s = ready("kept-scripts-tool");
    save(&mut s, "subject_of", SUBJECT_OF).unwrap();
    send(&mut s, None, "Anything about the storyboard?");
    let (q, asked) = question(&mut s);
    assert!(!asked.tools.iter().any(|t| t.name == "subject_of"), "a model that may not run scripts isn't told of them");
    calls(&mut s, q, "", &[("subject_of", json!({"about": "storyboard"}))]);
    let (q, asked) = question(&mut s);
    assert!(results(&asked)[0].error, "nor runs one");
    answer(&mut s, q, "No.");
    scripts_on(&mut s);
    let c = send(&mut s, None, "Anything about the storyboard?");
    let (q, asked) = question(&mut s);
    let tool = asked.tools.iter().find(|t| t.name == "subject_of").expect("told of it");
    assert_eq!(tool.schema["properties"]["about"]["description"], "words it's about");
    assert!(asked.tools.iter().any(|t| t.name == "run_script" && t.description.contains("subject_of")));
    calls(&mut s, q, "", &[("subject_of", json!({"about": "storyboard"}))]);
    let (q, asked) = question(&mut s);
    let r = &results(&asked)[0];
    assert!(!r.error && r.content.contains(STORYBOARD), "{}", r.content);
    let v = chat(&mut s, &c);
    let runs = v["chat"]["runs"].as_array().unwrap();
    assert!(runs.iter().any(|r| r["tool"] == "subject_of" && r["state"] == "ran"), "{runs:?}");
    assert!(runs.iter().any(|r| r["tool"] == "find_messages" && r["partOf"].is_string()), "its call under its card: {runs:?}");
    let script = r#"subject_of(#{about: "storyboard"})"#;
    calls(&mut s, q, "", &[("run_script", json!({"script": script}))]);
    let (q, asked) = question(&mut s);
    let r = &results(&asked)[0];
    assert!(!r.error && r.content.contains(STORYBOARD), "{}", r.content);
    answer(&mut s, q, "Found it.");
    s.run("script.turn", &json!({"name": "subject_of", "on": false})).unwrap();
    send(&mut s, Some(&c), "Again?");
    let (q, asked) = question(&mut s);
    assert!(!asked.tools.iter().any(|t| t.name == "subject_of"));
    calls(&mut s, q, "", &[("run_script", json!({"script": script}))]);
    let (_, asked) = question(&mut s);
    assert!(results(&asked)[0].error, "off: no function of that name");
}

/// Keeping a script is a proposal: its card waits for the person, and only approved is it kept.
#[test]
fn keeping_a_script_is_approved_by_the_person() {
    let mut s = ready("kept-scripts-keep");
    scripts_on(&mut s);
    let c = send(&mut s, None, "Keep that as a script");
    let (q, asked) = question(&mut s);
    assert!(asked.tools.iter().any(|t| t.name == "keep_script"));
    let keep = json!({"name": "subject_of", "about": "The subject of the newest message about something", "params": [{"name": "about", "about": "words"}], "script": SUBJECT_OF});
    calls(&mut s, q, "", &[("keep_script", keep.clone())]);
    assert!(questions(&mut s).is_empty(), "it waits for the person");
    assert!(named(&mut s, "subject_of").is_none(), "not kept yet");
    let v = chat(&mut s, &c);
    assert_eq!(v["chat"]["runs"][0]["state"], "proposed");
    assert!(v["chat"]["runs"][0]["what"].as_str().unwrap().contains("subject_of"), "{}", v["chat"]["runs"][0]["what"]);
    s.run("assistant.decide", &json!({"chat": c, "approve": true})).unwrap();
    assert!(named(&mut s, "subject_of").is_some());
    let (q, asked) = question(&mut s);
    assert!(results(&asked)[0].content.contains("Kept"), "{}", results(&asked)[0].content);
    // Again, under a name kept already: refused before the person sees it.
    calls(&mut s, q, "", &[("keep_script", keep)]);
    let (_, asked) = question(&mut s);
    let r = &results(&asked)[0];
    assert!(r.error && r.content.contains("kept already"), "{}", r.content);
}

/// MailCraft's own scripts are kept at first, and each runs on the sample mailbox as a tool:
/// what it gives back is no error.
#[test]
fn mailcrafts_own_scripts_run() {
    let mut s = ready("kept-scripts-shipped");
    scripts_on(&mut s);
    let names: Vec<Value> = listed(&mut s).iter().map(|k| k["name"].clone()).collect();
    assert_eq!(names, SHIPPED);
    let c = send(&mut s, None, "Run them all");
    let (q, _) = question(&mut s);
    let run: [(&str, Value); 5] = [
        ("waiting_on_me", json!({"days": 1})),
        ("waiting_on_them", json!({})),
        ("person_history", json!({"who": "zoe"})),
        ("today", json!({})),
        ("tidy", json!({"days": 1})),
    ];
    calls(&mut s, q, "", &run);
    // Tidy proposes a move when there's mail from lists to move: declined.
    let asked = match questions(&mut s).pop() {
        Some((_, asked)) => asked,
        None => {
            s.run("assistant.decide", &json!({"chat": c, "approve": false})).unwrap();
            question(&mut s).1
        }
    };
    for (r, (name, _)) in results(&asked).iter().zip(&run) {
        assert!(!r.error && r.content.contains("It came to: "), "{name}: {}", r.content);
    }
}

/// The CLI runs a kept script or code of its own at once (`script.run`), to read: it reads every
/// account, whatever MailCraft's AI may read; a change it calls for fails, as a question to a
/// model does; one turned off isn't run.
#[test]
fn script_run_runs_a_script_that_only_reads() {
    let mut s = ready("kept-scripts-agent");
    s.run("ai.allow", &json!({"account": "sample", "reach": "off"})).unwrap();
    save(&mut s, "subject_of", SUBJECT_OF).unwrap();
    let v = s.run("script.run", &json!({"name": "subject_of", "params": {"about": "storyboard"}})).unwrap();
    assert!(v["value"].as_str().is_some_and(|t| t.contains(STORYBOARD)), "{v}");
    let v = s.run("script.run", &json!({"code": r#"print("found"); subject_of(#{about: "storyboard"})"#})).unwrap();
    assert!(v["value"].as_str().is_some_and(|t| t.contains(STORYBOARD)) && v["printed"].as_str().is_some_and(|p| p.contains("found")), "{v}");
    for (code, why) in [(r#"move_messages(#{ids: ["m1"], folder: "archive"})"#, "only reads"), (r#"ask("Which?", "text")"#, "no model")] {
        let e = s.run("script.run", &json!({"code": code})).unwrap_err().to_string();
        assert!(e.contains(why), "{e}");
    }
    s.run("script.turn", &json!({"name": "subject_of", "on": false})).unwrap();
    assert!(s.run("script.run", &json!({"name": "subject_of"})).is_err(), "off");
    assert!(s.run("script.run", &json!({"name": "today", "code": "1"})).is_err(), "a name or code, not both");
}
