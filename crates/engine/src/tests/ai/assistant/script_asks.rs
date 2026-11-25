//! A script's questions to the model: asked of the chat's model as a question of their own,
//! answered, failed, bounded, and let go when the chat stops, is deleted or MailCraft closes.

use super::script::{results, scripts_on};
use super::*;

/// A script that asks about the storyboard mail.
const ASKS: &str = r#"
    let m = find_messages(#{query: "storyboard"}).messages[0];
    let r = read_message(m.id);
    ask("Does this message ask for a quote? Answer yes or no.", r.text)
"#;

/// A chat whose script waits for the answer to its question: the chat's id and the question's.
fn asking(name: &str, script: &str) -> (Session, String, u64) {
    let mut s = ready(name);
    scripts_on(&mut s);
    let id = send(&mut s, None, "Does the storyboard mail ask for a quote?");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("run_script", json!({"script": script}))]);
    let (asked, _) = question(&mut s);
    (s, id, asked)
}

/// Chat `id`'s run of `tool`.
fn run_of(s: &mut Session, id: &str, tool: &str) -> Value {
    chat(s, id)["chat"]["runs"].as_array().unwrap().iter().find(|r| r["tool"] == tool).cloned().unwrap()
}

/// A script asks the chat's model a question about a message, as one of its own with no
/// tools, and goes on with the answer; the record says what it was for.
#[test]
fn a_script_asks_the_model_and_goes_on_with_its_answer() {
    let mut s = ready("script-asks");
    scripts_on(&mut s);
    let id = send(&mut s, None, "Does the storyboard mail ask for a quote?");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("run_script", json!({"script": ASKS}))]);
    let (asked, put) = question(&mut s);
    assert!(put.tools.is_empty() && put.turns.len() == 1, "a question of its own");
    let said = &put.turns[0].text;
    assert!(said.starts_with("Does this message ask for a quote?") && said.contains("The text:\n"), "{said}");
    let ask = run_of(&mut s, &id, "ask");
    assert_eq!(ask["state"], "waiting");
    assert!(ask["what"].as_str().unwrap().starts_with("Ask Local: Does this message"), "{ask}");
    assert_eq!(run_of(&mut s, &id, "run_script")["state"], "waiting");
    answer(&mut s, asked, " no ");
    let (_, back) = question(&mut s);
    let r = &results(&back)[0];
    assert!(!r.error && r.content.contains("It came to: no"), "{}", r.content);
    let ask = run_of(&mut s, &id, "ask");
    assert_eq!((ask["state"].as_str(), ask["result"].as_str()), (Some("ran"), Some("no")));
    let log = s.run("ai.log", &json!({})).unwrap().to_string();
    assert!(log.contains("assistant script"), "{log}");
}

/// A question that gets no answer is an error the script can catch.
#[test]
fn a_question_not_answered_is_an_error_the_script_catches() {
    let (mut s, _, asked) = asking("script-ask-fails", r#"let r = ""; try { r = ask("Is it urgent?"); } catch (e) { r = `caught: ${e}`; } r"#);
    fail(&mut s, asked, "the model is busy");
    let (_, back) = question(&mut s);
    let r = &results(&back)[0];
    assert!(r.content.contains("It came to: caught: the model couldn't answer: the model is busy"), "{}", r.content);
}

/// A script asks so many questions at most.
#[test]
fn a_script_asks_so_many_questions_at_most() {
    let script = r#"let n = 0; for i in 0..25 { try { ask(`question ${i}`); n += 1; } catch (e) { print(e); } } n"#;
    let (mut s, _, mut asked) = asking("script-asks-most", script);
    for _ in 1..20 {
        answer(&mut s, asked, "ok");
        let (next, put) = question(&mut s);
        assert!(put.tools.is_empty(), "another of its questions");
        asked = next;
    }
    answer(&mut s, asked, "ok");
    let (_, back) = question(&mut s);
    let r = &results(&back)[0];
    assert!(r.content.contains("It came to: 20") && r.content.contains("at most 20 questions"), "{}", r.content);
}

/// A chat stopped while its script waits for an answer lets the question go: it's stopped,
/// and its answer coming later runs nothing.
#[test]
fn a_script_stopped_while_it_asks_lets_its_question_go() {
    let (mut s, id, asked) = asking("script-ask-stopped", ASKS);
    s.run("assistant.stop", &json!({"chat": id})).unwrap();
    let jobs = s.take_jobs();
    assert!(jobs.iter().any(|j| matches!(j, Job::Ai { work: AiWork::Cancel { id } } if *id == asked)), "the question stopped");
    let ask = run_of(&mut s, &id, "ask");
    assert_eq!((ask["state"].as_str(), ask["result"].as_str()), (Some("failed"), Some("stopped before the answer came")));
    assert_eq!(run_of(&mut s, &id, "run_script")["result"], "stopped before the script ended");
    answer(&mut s, asked, "yes");
    assert!(questions(&mut s).is_empty(), "nothing runs");
}

/// A chat deleted while its script waits for an answer lets the question go.
#[test]
fn a_chat_deleted_while_its_script_asks_lets_the_question_go() {
    let (mut s, id, asked) = asking("script-ask-deleted", ASKS);
    s.run("assistant.delete", &json!({"chat": id})).unwrap();
    let jobs = s.take_jobs();
    assert!(jobs.iter().any(|j| matches!(j, Job::Ai { work: AiWork::Cancel { id } } if *id == asked)), "the question stopped");
    answer(&mut s, asked, "yes");
    assert!(questions(&mut s).is_empty());
}

/// A script's question waiting when MailCraft closed says its answer never came.
#[test]
fn a_question_waiting_when_mailcraft_closed_says_no_answer_came() {
    let (mut s, id, _) = asking("script-ask-closed", ASKS);
    s.take_jobs();
    let store = s.store.clone().unwrap();
    let mut again = Session::open(Some(store), mailcraft_model::dates::Clock::new(NOW, 0));
    let ask = run_of(&mut again, &id, "ask");
    assert_eq!((ask["state"].as_str(), ask["result"].as_str()), (Some("failed"), Some("MailCraft closed before the answer came")));
}
