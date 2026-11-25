//! Asking again: a chat whose question failed, or was stopped, asked again as it is (Try
//! again), and what came of a stopped answer kept.

use super::*;

/// The question chat `id` waits on fails with `error`.
fn fails(s: &mut Session, q: u64, error: &str) {
    s.apply(Event::Ai { done: AiDone::Answered { id: q, answer: None, error: Some(error.into()) } });
}

/// A question that failed is asked again as the chat is, without the person writing, and its
/// error goes.
#[test]
fn a_failed_question_is_asked_again_as_the_chat_is() {
    let mut s = ready("again-failed");
    let c = send(&mut s, None, "What came today?");
    let (q, first) = question(&mut s);
    fails(&mut s, q, "the model's server: HTTP 503");
    assert!(questions(&mut s).is_empty());
    assert!(chat(&mut s, &c)["chat"]["error"].as_str().unwrap().contains("HTTP 503"));
    s.run("assistant.retry", &json!({"chat": c})).unwrap();
    let (_, again) = question(&mut s);
    assert_eq!(again.turns, first.turns, "asked as it was");
    assert!(chat(&mut s, &c)["chat"]["error"].is_null());
}

/// Nothing to ask again: while it answers, after its answer, or while a change waits on the
/// person.
#[test]
fn nothing_is_asked_again_while_it_answers_or_after_it_answered() {
    let mut s = ready("again-refused");
    let c = send(&mut s, None, "What came today?");
    let refused = |s: &mut Session| s.run("assistant.retry", &json!({"chat": c})).unwrap_err().to_string();
    assert!(refused(&mut s).contains("still answering"));
    let (q, _) = question(&mut s);
    calls(&mut s, q, "Two messages.", &[]);
    assert!(refused(&mut s).contains("nothing to ask again"));
    send(&mut s, Some(&c), "Archive the storyboard mail");
    let (q, _) = question(&mut s);
    let id = id_of(&s, STORYBOARD);
    calls(&mut s, q, "", &[("move_messages", json!({"ids": [name_of(&id)], "folder": "archive"}))]);
    let (q, _) = question(&mut s);
    calls(&mut s, q, "It waits for you to approve it.", &[]);
    let why = refused(&mut s);
    assert!(why.contains("nothing to ask again"), "the change waits on the person: {why}");
    assert!(questions(&mut s).is_empty());
}

/// Stopped as it's written, what came of the answer stays, to read; asked again, the new answer
/// takes its place.
#[test]
fn what_came_of_an_answer_stopped_stays_until_it_is_asked_again() {
    let mut s = ready("again-stopped");
    let c = send(&mut s, None, "Sum up my week");
    let (q, first) = question(&mut s);
    s.apply(Event::Ai { done: AiDone::Part { id: q, text: "Your week: three meetings".into(), thinking: "Count them.".into() } });
    s.run("assistant.stop", &json!({"chat": c})).unwrap();
    let v = chat(&mut s, &c);
    let last = v["chat"]["turns"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(
        (last["role"].as_str(), last["text"].as_str(), last["stopped"].as_bool()),
        (Some("assistant"), Some("Your week: three meetings"), Some(true))
    );
    assert_eq!(last["thinking"], "Count them.");
    s.take_jobs();
    s.run("assistant.retry", &json!({"chat": c})).unwrap();
    let (_, again) = question(&mut s);
    assert_eq!(again.turns, first.turns, "the stopped answer gave way");
    // Stopped before any of it came, nothing is added.
    s.run("assistant.stop", &json!({"chat": c})).unwrap();
    assert_eq!(chat(&mut s, &c)["chat"]["turns"].as_array().map(Vec::len), Some(1));
}
