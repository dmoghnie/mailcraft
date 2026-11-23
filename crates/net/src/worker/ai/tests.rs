use super::*;
use crate::fakes::fake_ai::ModelServer;
use crate::waiting::{events_until, until};
use mailcraft_model::{AiApi, AiChat, AiEndpoint};
use std::net::TcpListener;

fn ask(id: u64, url: &str, key: Option<&str>) -> Job {
    let endpoint = AiEndpoint { api: AiApi::OpenAi, url: url.into(), model: "m".into() };
    Job::Ai { work: AiWork::Ask { id, endpoint, key: key.map(str::to_string), chat: AiChat::ask("", "hi", 16), stream: false } }
}

/// The answer to question `id`: its text, or its error.
fn answer_to(seen: &[Event], id: u64) -> Option<std::result::Result<String, String>> {
    seen.iter().find_map(|e| match e {
        Event::Ai { done: AiDone::Answered { id: i, answer, error } } if *i == id => {
            Some(answer.as_ref().map(|a| a.text.clone()).ok_or_else(|| error.clone().unwrap_or_default()))
        }
        _ => None,
    })
}

#[test]
fn a_kept_key_goes_to_the_model_and_a_missing_one_is_said() {
    let server = ModelServer::start();
    let creds = Arc::new(MemoryCredentials::default());
    creds.set_key("p1", "sk-kept").unwrap();
    let mut p = Pool::new(crate::fake::store("ai-keys"), creds.clone());
    p.submit(ask(1, &server.base, Some("p1")));
    p.submit(ask(2, &server.base, Some("p2")));
    p.submit(ask(3, &server.base, None));
    let seen = events_until(&p, "three answers", |s| (1..=3).all(|id| answer_to(s, id).is_some()));
    assert_eq!(answer_to(&seen, 1), Some(Ok("You said: hi".into())));
    assert!(answer_to(&seen, 2).is_some_and(|a| a.is_err_and(|e| e.contains("no key is kept"))), "{seen:?}");
    assert_eq!(answer_to(&seen, 3), Some(Ok("You said: hi".into())));
    let sent = server.seen();
    assert_eq!(sent.len(), 2, "nothing asked without its key");
    assert!(sent.iter().any(|r| r.header("Authorization") == Some("Bearer sk-kept")));
    assert!(sent.iter().any(|r| r.header("Authorization").is_none()));
    // Kept, trimmed, then forgotten.
    p.submit(Job::Ai { work: AiWork::SaveKey { name: "p2".into(), key: Some(Secret(" sk-new \n".into())) } });
    until(&p, "the key kept", |e| matches!(e, Event::Ai { done: AiDone::KeySaved { name, error: None } } if name == "p2"));
    assert_eq!(creds.get_key("p2").as_deref(), Some("sk-new"));
    p.submit(Job::Ai { work: AiWork::SaveKey { name: "p2".into(), key: None } });
    until(&p, "the key forgotten", |e| matches!(e, Event::Ai { done: AiDone::KeySaved { name, error: None } } if name == "p2"));
    assert_eq!(creds.get_key("p2"), None);
    // A keychain that can't keep one says so.
    struct NoKeychain;
    impl Credentials for NoKeychain {
        fn get(&self, _: &AccountConfig) -> Option<String> {
            None
        }
        fn set(&self, _: &AccountConfig, _: &str) -> std::result::Result<(), String> {
            Ok(())
        }
        fn delete(&self, _: &AccountConfig) {}
        fn get_smtp(&self, _: &AccountConfig) -> Option<String> {
            None
        }
        fn set_smtp(&self, _: &AccountConfig, _: &str) -> std::result::Result<(), String> {
            Ok(())
        }
        fn delete_smtp(&self, _: &AccountConfig) {}
    }
    let mut p = Pool::new(crate::fake::store("ai-no-keychain"), Arc::new(NoKeychain));
    p.submit(Job::Ai { work: AiWork::SaveKey { name: "p1".into(), key: Some(Secret("sk".into())) } });
    let seen = until(&p, "the key refused", |e| matches!(e, Event::Ai { done: AiDone::KeySaved { .. } }));
    assert!(seen.iter().any(|e| matches!(e, Event::Ai { done: AiDone::KeySaved { error: Some(e), .. } } if e.contains("keychain"))));
}

/// The pool waits for a few answers at once; one more is answered at once, and a place comes
/// back once a wait ends, however it ends.
#[test]
fn past_the_most_waiting_a_question_is_answered_at_once() {
    // A server that takes the questions and never answers, until its connections are dropped.
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/v1", silent.local_addr().unwrap().port());
    let held: Arc<Mutex<Vec<std::net::TcpStream>>> = Arc::default();
    let holding = held.clone();
    std::thread::spawn(move || {
        for c in silent.incoming().flatten() {
            holding.lock().unwrap().push(c);
        }
    });
    let mut p = Pool::new(crate::fake::store("ai-most"), Arc::new(MemoryCredentials::default()));
    let most = u64::try_from(ASKING_MOST).unwrap();
    for id in 1..=most + 1 {
        p.submit(ask(id, &url, None));
    }
    let seen = until(&p, "the one too many", |e| matches!(e, Event::Ai { done: AiDone::Answered { id, .. } } if *id == most + 1));
    assert!(answer_to(&seen, most + 1).is_some_and(|a| a.is_err_and(|e| e.contains("already waiting"))), "{seen:?}");
    assert!((1..=most).all(|id| answer_to(&seen, id).is_none()), "the others still wait: {seen:?}");
    // Their connections closed: each wait ends with an error, and gives its place back.
    crate::waiting::eventually("every question to reach the server", || held.lock().unwrap().len() == ASKING_MOST);
    held.lock().unwrap().clear();
    let seen = events_until(&p, "the waits to end", |s| (1..=most).all(|id| answer_to(s, id).is_some()));
    assert!((1..=most).all(|id| answer_to(&seen, id).is_some_and(|a| a.is_err())), "{seen:?}");
    crate::waiting::eventually("the places given back", || p.asking.load(Ordering::Acquire) == 0);
    let server = ModelServer::start();
    p.submit(ask(99, &server.base, None));
    let seen = until(&p, "a question asked again", |e| matches!(e, Event::Ai { done: AiDone::Answered { id: 99, .. } }));
    assert_eq!(answer_to(&seen, 99), Some(Ok("You said: hi".into())));
}

/// Streamed, what more of the answer came is sent on as it's written, a while apart, and the
/// answer whole after it.
#[test]
fn a_streamed_answer_comes_in_parts_then_whole() {
    let server = ModelServer::start();
    let chunk = |text: &str| format!("data: {}\n\n", serde_json::json!({"choices": [{"delta": {"content": text, "reasoning": "."}}]}));
    let pieces =
        [chunk("One "), chunk("two "), chunk("three "), chunk("four."), "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n".into()];
    let pieces: Vec<&str> = pieces.iter().map(String::as_str).collect();
    server.then_events_every(&pieces, PART_EVERY + std::time::Duration::from_millis(40));
    let mut p = Pool::new(crate::fake::store("ai-stream"), Arc::new(MemoryCredentials::default()));
    let endpoint = AiEndpoint { api: AiApi::OpenAi, url: server.base.clone(), model: "m".into() };
    p.submit(Job::Ai { work: AiWork::Ask { id: 7, endpoint, key: None, chat: AiChat::ask("", "hi", 16), stream: true } });
    let seen = events_until(&p, "the answer", |s| answer_to(s, 7).is_some());
    let parts: Vec<(&str, &str)> = seen
        .iter()
        .filter_map(|e| match e {
            Event::Ai { done: AiDone::Part { id: 7, text, thinking } } => Some((text.as_str(), thinking.as_str())),
            _ => None,
        })
        .collect();
    assert!(parts.len() >= 2, "sent on as it came: {parts:?}");
    let so_far: String = parts.iter().map(|(t, _)| *t).collect();
    assert!("One two three four.".starts_with(&so_far) && !so_far.is_empty(), "{so_far:?}");
    assert!(parts.iter().any(|(_, th)| !th.is_empty()), "thinking too");
    assert_eq!(answer_to(&seen, 7), Some(Ok("One two three four.".into())), "then the answer whole");
    assert_eq!(server.seen()[0].body["stream"], true);
}
