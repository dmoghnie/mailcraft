//! A model's monthly budget, in tokens: each answer counted against its profile, as the server
//! said or estimated, and kept across runs; past it, questions go to its fallback or aren't
//! asked and say why, in the assistant too; a new month starts the count again.

use super::*;

/// Question `id` answered, the server counting `tokens` in all.
fn answered(s: &mut Session, id: u64, tokens: u64) {
    let answer = AiAnswer { text: "Done.".into(), input_tokens: Some(tokens), output_tokens: Some(0), ..AiAnswer::default() };
    s.apply(Event::Ai { done: AiDone::Answered { id, answer: Some(answer), error: None } });
}

/// A profile's monthly budget: each answer's tokens counted against it (as the server said, or
/// estimated), and kept across runs; past it, questions go to its fallback, or aren't asked and
/// say why; a new month starts the count again.
#[test]
fn a_profile_past_its_monthly_budget_is_asked_no_more() {
    let mut s = sample_session("ai-budget");
    s.run("ai.allow", &json!({"account": "sample", "reach": "any"})).unwrap();
    let cloud = json!({"name": "Cloud", "api": "openAi", "url": "https://models.example/v1", "model": "large", "budget": 1000});
    let cloud = s.run("ai.saveProfile", &cloud).unwrap()["id"].as_str().unwrap().to_string();
    s.take_jobs();
    ask(&mut s, "What's new?").unwrap();
    let (id, ..) = asked(&mut s).pop().unwrap();
    let answer = AiAnswer { text: "Nothing.".into(), input_tokens: Some(700), output_tokens: Some(200), ..AiAnswer::default() };
    s.apply(Event::Ai { done: AiDone::Answered { id, answer: Some(answer), error: None } });
    assert_eq!(s.ai_used(&cloud), 900);
    let settings = s.run("ai.settings", &json!({})).unwrap();
    assert_eq!((&settings["profiles"][0]["used"], &settings["profiles"][0]["overBudget"]), (&json!(900), &json!(false)));
    // Its server didn't count: a token for every three bytes, the question's and the answer's.
    ask(&mut s, "And?").unwrap();
    let (id, ..) = asked(&mut s).pop().unwrap();
    s.apply(Event::Ai { done: AiDone::Answered { id, answer: Some(AiAnswer { text: "x".repeat(300), ..AiAnswer::default() }), error: None } });
    assert!(s.ai_used(&cloud) >= 1000, "{}", s.ai_used(&cloud));
    let err = ask(&mut s, "Again?").unwrap_err();
    assert!(err.to_string().contains("monthly budget"), "{err}");
    assert!(asked(&mut s).is_empty(), "nothing sent past it");
    // With a fallback, it's asked instead.
    let local = local(&mut s, "Local");
    let mut changed = json!({"id": cloud, "name": "Cloud", "api": "openAi", "url": "https://models.example/v1", "model": "large", "fallback": local});
    s.run("ai.saveProfile", &changed).unwrap();
    s.take_jobs();
    ask(&mut s, "Again?").unwrap();
    assert_eq!(asked(&mut s).pop().unwrap().2.model, "llama3.2", "its fallback asked");
    // Kept across runs; counted again from a new month.
    s.take_jobs();
    let mut again = Session::open(s.store.clone(), mailcraft_model::dates::Clock::new(s.clock.now, 0));
    assert!(again.ai_used(&cloud) >= 1000, "kept");
    again.clock.now += 32 * 86_400;
    assert_eq!(again.ai_used(&cloud), 0, "a new month");
    // Raised or taken off, it's asked again.
    changed["budget"] = Value::Null;
    s.run("ai.saveProfile", &changed).unwrap();
    s.take_jobs();
    ask(&mut s, "Now?").unwrap();
    assert_eq!(asked(&mut s).pop().unwrap().2.model, "large");
}

/// The assistant: a chat goes to the chat model's fallback once it has used its budget, and one
/// whose model was chosen for it is refused, saying why.
#[test]
fn the_assistant_is_kept_within_a_models_budget() {
    let mut s = sample_session("ai-budget-assistant");
    s.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    let small = json!({"name": "Small", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small", "budget": 100});
    let small = s.run("ai.saveProfile", &small).unwrap()["id"].as_str().unwrap().to_string();
    s.take_jobs();
    let c = s.run("assistant.send", &json!({"text": "Hello"})).unwrap()["chat"].as_str().unwrap().to_string();
    let (q, ..) = asked(&mut s).pop().expect("asked");
    answered(&mut s, q, 150);
    assert!(s.ai_over_budget(s.prefs.ai.profile(&small).unwrap()));
    let e = s.run("assistant.send", &json!({"chat": c, "text": "Again"})).unwrap_err().to_string();
    assert!(e.contains("Small has used its monthly budget"), "{e}");
    s.run("assistant.model", &json!({"chat": c, "profile": small})).unwrap();
    let e = s.run("assistant.send", &json!({"chat": c, "text": "Again"})).unwrap_err().to_string();
    assert!(e.contains("choose another model for this chat"), "{e}");
    assert!(asked(&mut s).is_empty(), "nothing sent past it");
    let big = local(&mut s, "Big");
    let fallback = json!({"id": small, "name": "Small", "api": "openAi", "url": "http://localhost:11434/v1", "model": "small", "fallback": big});
    s.run("ai.saveProfile", &fallback).unwrap();
    let other = s.run("assistant.send", &json!({"text": "Hello again"})).unwrap()["chat"].as_str().unwrap().to_string();
    assert!(!other.is_empty());
    assert_eq!(asked(&mut s).pop().expect("asked").2.model, "llama3.2", "its fallback");
}
