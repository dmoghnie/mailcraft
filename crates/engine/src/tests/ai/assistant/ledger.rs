//! A long chat fitted into its model's context: what's left out of it is still named, what the
//! person asked and what was decided kept in a ledger; the estimate of its size corrected by
//! what the model's server counted.

use super::*;

/// The model's answer to question `q`, its server having counted `read` tokens of it.
fn answer_counted(s: &mut Session, q: u64, text: &str, read: u64) {
    let a = AiAnswer { text: text.into(), input_tokens: Some(read), ..AiAnswer::default() };
    s.apply(Event::Ai { done: AiDone::Answered { id: q, answer: Some(a), error: None } });
}

/// The profile the chats ask, taking in `context` tokens.
fn context(s: &mut Session, context: u64) {
    let v =
        json!({"id": "p1", "name": "Local", "api": "openAi", "url": "http://localhost:11434/v1", "model": "m", "context": context, "maxTokens": 512});
    s.run("ai.saveProfile", &v).unwrap();
}

/// The start of a chat left out for room, the model is told what it held: what the person
/// asked, what was found, by name, and how a change it proposed was decided.
#[test]
fn what_a_chats_left_out_start_held_is_kept_in_a_ledger() {
    let mut s = ready("ledger-start");
    context(&mut s, 8192);
    let story = id_of(&s, STORYBOARD);
    let c = send(&mut s, None, "Find Zoe's storyboard mail");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("find_messages", json!({"query": "Storyboard"}))]);
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("delete_messages", json!({"ids": [story]}))]);
    s.run("assistant.decide", &json!({"chat": c, "approve": false})).unwrap();
    let (q, _) = question(&mut s);
    answer(&mut s, q, "Kept it.");
    for n in 0..3 {
        send(&mut s, Some(&c), &format!("{}{n}", "word ".repeat(1500)));
        let (q, _) = question(&mut s);
        answer(&mut s, q, "Noted.");
    }
    send(&mut s, Some(&c), "And now?");
    let (_, asked) = question(&mut s);
    let system = &asked.system;
    assert!(system.contains("The start of this chat was left out for room"), "{system}");
    assert!(system.contains("- The person asked: \"Find Zoe's storyboard mail\""), "{system}");
    let found = system.lines().find(|l| l.starts_with("- Find “Storyboard”: found ")).unwrap_or_default();
    assert!(found.contains(&name_of(&story)), "{system}");
    assert!(system.contains(&format!("Delete “{STORYBOARD}”: declined")), "{system}");
    assert!(!asked.turns.iter().any(|t| t.text.contains("storyboard mail")), "the start itself is left out");
}

/// A read left out for room early in a chat still names what it found, so the model can link it
/// without reading it again.
#[test]
fn a_read_left_out_for_room_still_names_what_it_found() {
    let mut s = ready("ledger-left-out");
    context(&mut s, 8192);
    send(&mut s, None, "Read everything");
    for _ in 0..6 {
        let (q, _) = question(&mut s);
        calls(&mut s, q, "", &[("find_messages", json!({"limit": 50}))]);
    }
    let (_, asked) = question(&mut s);
    let named = name_of(&id_of(&s, STORYBOARD));
    let left: Vec<&str> =
        asked.turns.iter().flat_map(|t| &t.results).map(|r| r.content.as_str()).filter(|c| c.starts_with("[Left out for room")).collect();
    assert!(!left.is_empty() || asked.system.contains("left out for room"), "something gave way");
    assert!(left.iter().all(|c| c.contains(&named)), "{left:?}");
}

/// What the model's server counted of a question is kept with the chat, and a count above the
/// estimate makes the next fitting tighter: what fitted before no longer does.
#[test]
fn the_servers_count_corrects_a_low_estimate() {
    let mut s = ready("ledger-counted");
    context(&mut s, 1_000_000);
    let c = send(&mut s, None, "Hello");
    let (q, _) = question(&mut s);
    answer_counted(&mut s, q, "Hi.", 1);
    let v = chat(&mut s, &c);
    let estimated = v["chat"]["counted"]["estimated"].as_u64().unwrap();
    assert_eq!(v["chat"]["counted"]["read"], 1);
    // Room for the chat twice over, as estimated: a count lower than the estimate changes nothing.
    context(&mut s, 2 * estimated + 512 + 1024);
    send(&mut s, Some(&c), "Next");
    let (q, asked) = question(&mut s);
    assert_eq!(asked.turns.len(), 3);
    answer_counted(&mut s, q, "Sure.", u64::MAX);
    assert_eq!(chat(&mut s, &c)["chat"]["counted"]["read"], u64::MAX);
    // Counted as three times the estimate, it doesn't fit.
    let e = s.run("assistant.send", &json!({"chat": c, "text": "Again"})).unwrap_err();
    assert!(e.to_string().contains("more than Local takes in"), "{e}");
}
