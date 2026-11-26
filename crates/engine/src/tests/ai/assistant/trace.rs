//! How each reply went, kept apart from the chat: its questions, steps, tools and tokens, how
//! it ended, never what was said. A reply stops at its tokens or its time as at its steps; time
//! waiting for the person doesn't count.

use super::*;
use crate::{REPLY_SECONDS_MOST, REPLY_TOKENS_MOST};

fn replies(s: &mut Session, chat: Option<&str>) -> Vec<Value> {
    let v = match chat {
        Some(c) => json!({"chat": c}),
        None => json!({}),
    };
    s.run("assistant.trace", &v).unwrap()["replies"].as_array().cloned().unwrap_or_default()
}

/// The model answers question `id` with a call of `tool`, said to have read `read` tokens.
fn calls_reading(s: &mut Session, id: u64, tool: (&str, Value), read: u64) {
    let call = AiToolCall { id: format!("call_r{id}"), name: tool.0.into(), input: tool.1 };
    let a = AiAnswer { calls: vec![call], input_tokens: Some(read), output_tokens: Some(10), ..AiAnswer::default() };
    s.apply(Event::Ai { done: AiDone::Answered { id, answer: Some(a), error: None } });
}

#[test]
fn each_reply_is_kept_as_it_went_without_what_was_said() {
    let mut s = ready("assistant-trace");
    let c = send(&mut s, None, "What does Zoe want for Thursday?");
    let (q, _) = question(&mut s);
    assert_eq!(replies(&mut s, Some(&c))[0]["end"], "going");
    calls(&mut s, q, "Let me look.", &[("find_messages", json!({"query": "from:zoe storyboard"})), ("list_folders", json!({}))]);
    let (q, _) = question(&mut s);
    answer(&mut s, q, "She wants the revised frames for scenes 4 to 7.");
    let r = &replies(&mut s, Some(&c))[0];
    assert_eq!((r["end"].as_str(), r["asks"].as_u64(), r["steps"].as_u64()), (Some("answered"), Some(2), Some(1)), "{r}");
    assert_eq!(r["tools"], json!([{"tool": "find_messages", "state": "ran"}, {"tool": "list_folders", "state": "ran"}]));
    assert_eq!((r["profile"].as_str(), r["chat"].as_str()), (Some("Local"), Some(c.as_str())));
    assert!(r["read"].as_u64() > Some(0) && r["written"].as_u64() > Some(0), "estimated where the server didn't count: {r}");
    assert!(r["ended"].as_i64() >= r["began"].as_i64());

    // A second reply in the chat, failed; another chat's apart.
    send(&mut s, Some(&c), "And Sam?");
    let (q, _) = question(&mut s);
    fail(&mut s, q, "the server went away");
    let other = send(&mut s, None, "Anything from Ana?");
    question(&mut s);
    assert_eq!(replies(&mut s, Some(&c)).iter().map(|r| r["end"].clone()).collect::<Vec<_>>(), [json!("failed"), json!("answered")], "newest first");
    assert_eq!(replies(&mut s, None).len(), 3);
    assert_eq!(replies(&mut s, Some(&other)).len(), 1);

    // Kept across runs, with nothing said in it; one going when MailCraft closed went no further.
    s.take_jobs();
    let kept = std::fs::read_to_string(s.store.as_ref().unwrap().root().join("ai-trace.json")).unwrap();
    for said in ["Zoe", "Thursday", "frames", "Sam", "Ana", "storyboard", "went away"] {
        assert!(!kept.contains(said), "{said} kept: {kept}");
    }
    let mut again = Session::open(s.store.clone(), mailcraft_model::dates::Clock::new(s.clock.now, 0));
    let r = replies(&mut again, None);
    assert_eq!(r.iter().map(|r| r["end"].clone()).collect::<Vec<_>>(), [json!("left"), json!("failed"), json!("answered")]);
    assert_eq!(r[2]["tools"].as_array().map(Vec::len), Some(2));
}

#[test]
fn a_reply_stops_at_its_tokens_or_its_time_and_the_person_waiting_doesnt_count() {
    let mut s = ready("assistant-limits");
    let c = send(&mut s, None, "Look around");
    let (q, _) = question(&mut s);
    calls_reading(&mut s, q, ("list_folders", json!({})), REPLY_TOKENS_MOST - 20);
    let (q, _) = question(&mut s);
    calls_reading(&mut s, q, ("list_folders", json!({})), 100);
    assert!(questions(&mut s).is_empty(), "stopped");
    let v = chat(&mut s, &c);
    assert!(v["chat"]["error"].as_str().unwrap().contains(&format!("stopped after using {REPLY_TOKENS_MOST} tokens")), "{v}");
    let r = &replies(&mut s, Some(&c))[0];
    assert_eq!((r["end"].as_str(), r["limit"].as_str(), r["read"].as_u64()), (Some("limit"), Some("tokens"), Some(REPLY_TOKENS_MOST + 80)), "{r}");
    assert_eq!(r["tools"][1]["state"], "failed", "the call past it isn't made");

    // Told to go on, a new reply begins, with tokens of its own.
    send(&mut s, Some(&c), "Go on");
    let (q, _) = question(&mut s);
    calls_reading(&mut s, q, ("list_folders", json!({})), 100);
    question(&mut s);
    assert_eq!(replies(&mut s, Some(&c))[0]["end"], "going");

    // A change waits for the person as long as they like; past the time by itself, it stops.
    let c = send(&mut s, None, "Archive Zoe's storyboard mail");
    let story = id_of(&s, STORYBOARD);
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("move_messages", json!({"ids": [story], "folder": "archive"}))]);
    let now = s.clock.now + 2 * REPLY_SECONDS_MOST;
    s.set_clock(now, 0);
    s.run("assistant.decide", &json!({"chat": c, "approve": true})).unwrap();
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("list_folders", json!({}))]);
    let (q, _) = question(&mut s);
    s.set_clock(now + REPLY_SECONDS_MOST + 1, 0);
    calls(&mut s, q, "", &[("list_folders", json!({}))]);
    assert!(questions(&mut s).is_empty(), "stopped");
    let r = &replies(&mut s, Some(&c))[0];
    assert_eq!((r["end"].as_str(), r["limit"].as_str(), r["steps"].as_u64()), (Some("limit"), Some("time"), Some(3)), "{r}");
    assert_eq!(r["tools"][0], json!({"tool": "move_messages", "state": "done"}));
    let v = chat(&mut s, &c);
    assert!(v["chat"]["error"].as_str().unwrap().contains(&format!("stopped after {} minutes", REPLY_SECONDS_MOST / 60)), "{v}");

    // Stopped by the person, it says so.
    let c = send(&mut s, None, "Look around");
    question(&mut s);
    s.run("assistant.stop", &json!({"chat": c})).unwrap();
    assert_eq!(replies(&mut s, Some(&c))[0]["end"], "stopped");
}
