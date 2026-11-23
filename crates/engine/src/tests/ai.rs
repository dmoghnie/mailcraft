//! MailCraft's own AI in the engine: what may be asked where, what's queued for the network
//! layer, the answers, the fallbacks and the record of what went where.

use super::*;
use crate::{AI_LOG_KEPT, AiProfile, AiTest};
use mailcraft_model::{AiAnswer, AiApi, AiDone, AiEndpoint, AiWork};

mod assistant;

const KEY: &str = "sk-secret-0123456789";

fn cloud(s: &mut Session, name: &str) -> String {
    let v = json!({"name": name, "api": "anthropic", "url": "https://api.anthropic.com/v1", "model": "claude-opus-5-5", "key": KEY});
    s.run("ai.saveProfile", &v).unwrap()["id"].as_str().unwrap().to_string()
}

fn local(s: &mut Session, name: &str) -> String {
    let v = json!({"name": name, "api": "openAi", "url": "http://localhost:11434/v1/", "model": "llama3.2"});
    s.run("ai.saveProfile", &v).unwrap()["id"].as_str().unwrap().to_string()
}

/// Each question queued: its id, the profile's key name, the server and model.
fn asked(s: &mut Session) -> Vec<(u64, Option<String>, AiEndpoint, mailcraft_model::AiChat)> {
    s.take_jobs()
        .into_iter()
        .filter_map(|j| match j {
            Job::Ai { work: AiWork::Ask { id, endpoint, key, chat, .. } } => Some((id, key, endpoint, chat)),
            _ => None,
        })
        .collect()
}

fn answer(s: &mut Session, id: u64, text: &str) {
    let a = AiAnswer { text: text.into(), sent: 10, received: 20, ..AiAnswer::default() };
    s.apply(Event::Ai { done: AiDone::Answered { id, answer: Some(a), error: None } });
}

fn fail(s: &mut Session, id: u64, error: &str) {
    s.apply(Event::Ai { done: AiDone::Answered { id, answer: None, error: Some(error.into()) } });
}

fn ask(s: &mut Session, question: &str) -> Result<u64, CmdError> {
    s.run("ai.ask", &json!({"account": "sample", "question": question, "purpose": "summary"})).map(|v| v["id"].as_u64().unwrap())
}

#[test]
fn nothing_goes_to_a_model_until_one_is_set_up_and_the_account_lets_it() {
    let mut s = sample_session("ai-off");
    let e = ask(&mut s, "What's new?").unwrap_err().to_string();
    assert!(e.contains("off for this account"), "{e}");
    s.run("ai.allow", &json!({"account": "sample", "reach": "any"})).unwrap();
    let e = ask(&mut s, "What's new?").unwrap_err().to_string();
    assert!(e.contains("no model is set up"), "{e}");
    let p = cloud(&mut s, "Claude");
    s.take_jobs();
    let id = ask(&mut s, "What's new?").unwrap();
    let q = asked(&mut s);
    assert_eq!(q.len(), 1);
    assert_eq!((q[0].0, q[0].1.as_deref()), (id, Some(p.as_str())), "its key named, never given");
    assert_eq!(q[0].2.url, "https://api.anthropic.com/v1");
    assert_eq!(q[0].3.turns[0].text, "What's new?");
    assert_eq!(q[0].3.max_tokens, 4096, "the profile's longest answer");
    assert_eq!(s.run("ai.answer", &json!({"id": id})).unwrap(), json!({"waiting": true}));
    answer(&mut s, id, "Two meetings moved.");
    assert_eq!(s.run("ai.answer", &json!({"id": id})).unwrap()["text"], "Two meetings moved.");
    assert_eq!(s.run("ai.answer", &json!({"id": id})).unwrap(), json!({"waiting": false}), "taken once");
    // Off again: nothing more goes.
    s.run("ai.allow", &json!({"account": "sample", "reach": "off"})).unwrap();
    assert!(ask(&mut s, "And now?").is_err());
    assert!(asked(&mut s).is_empty());
    assert!(s.run("ai.allow", &json!({"account": "nobody", "reach": "any"})).is_err());
    assert!(s.run("ai.ask", &json!({"question": "no account"})).is_err(), "every question names whose mail it holds");
}

#[test]
fn a_key_goes_to_the_keychain_and_never_into_the_settings() {
    let mut s = sample_session("ai-key");
    let p = cloud(&mut s, "Claude");
    let jobs = s.take_jobs();
    assert!(jobs.iter().any(|j| matches!(j, Job::Ai { work: AiWork::SaveKey { name, key: Some(k) } } if *name == p && k.0 == KEY)), "{jobs:?}");
    let saved = serde_json::to_string(&s.prefs).unwrap();
    assert!(!saved.contains(KEY), "{saved}");
    let shown = s.run("ai.settings", &json!({})).unwrap().to_string();
    assert!(!shown.contains(KEY) && shown.contains("\"hasKey\":true"), "{shown}");
    assert!(!format!("{jobs:?}").contains(KEY), "a job's Debug hides it");
    // Changed without a key: the one kept stays. A new address forgets it, unless one is given.
    s.run("ai.saveProfile", &json!({"id": p, "name": "Claude Opus", "api": "anthropic", "url": "https://api.anthropic.com/v1", "model": "m"}))
        .unwrap();
    assert!(s.prefs.ai.profile(&p).unwrap().has_key);
    assert!(s.take_jobs().is_empty());
    s.run("ai.saveProfile", &json!({"id": p, "name": "Elsewhere", "api": "openAi", "url": "https://models.example/v1", "model": "m"})).unwrap();
    assert!(!s.prefs.ai.profile(&p).unwrap().has_key);
    let jobs = s.take_jobs();
    assert!(jobs.iter().any(|j| matches!(j, Job::Ai { work: AiWork::SaveKey { key: None, .. } })), "{jobs:?}");
    // A keychain that can't keep it: no key, and why.
    s.run("ai.saveProfile", &json!({"id": p, "name": "Elsewhere", "api": "openAi", "url": "https://models.example/v1", "model": "m", "key": "k2"}))
        .unwrap();
    s.apply(Event::Ai { done: AiDone::KeySaved { name: p.clone(), error: Some("there's no keychain to keep it in".into()) } });
    assert!(!s.prefs.ai.profile(&p).unwrap().has_key);
    assert_eq!(s.run("ai.settings", &json!({})).unwrap()["profiles"][0]["keyError"], "there's no keychain to keep it in");
    // Removed: its key forgotten, and nothing names it.
    s.run("ai.use", &json!({"task": "chat", "profile": p})).unwrap();
    let l = local(&mut s, "Ollama");
    s.run(
        "ai.saveProfile",
        &json!({"id": l, "name": "Ollama", "api": "openAi", "url": "http://localhost:11434/v1", "model": "llama3.2", "fallback": p}),
    )
    .unwrap();
    s.take_jobs();
    s.run("ai.removeProfile", &json!({"id": p})).unwrap();
    assert!(s.take_jobs().iter().any(|j| matches!(j, Job::Ai { work: AiWork::SaveKey { name, key: None } } if *name == p)));
    assert!(s.prefs.ai.uses.is_empty());
    assert_eq!(s.prefs.ai.profile(&l).unwrap().fallback, None);
}

#[test]
fn a_profile_is_checked_before_it_is_kept() {
    let mut s = sample_session("ai-profile");
    let base = json!({"name": "M", "api": "openAi", "url": "https://models.example/v1", "model": "m"});
    let with = |k: &str, v: Value| {
        let mut b = base.clone();
        b[k] = v;
        b
    };
    for bad in [
        with("url", json!("http://models.example/v1")),
        with("url", json!("http://192.168.1.5:11434/v1")),
        with("url", json!("ftp://models.example")),
        with("url", json!("https://user@models.example/v1")),
        with("api", json!("gemini")),
        with("name", json!("  ")),
        with("model", json!("x".repeat(201))),
        with("maxTokens", json!(0)),
        with("maxTokens", json!(1_000_000)),
        with("fallback", json!("p9")),
        with("key", json!("a\nb")),
        with("id", json!("p9")),
    ] {
        assert!(s.run("ai.saveProfile", &bad).is_err(), "{bad}");
    }
    assert!(s.prefs.ai.profiles.is_empty());
    let id = s.run("ai.saveProfile", &base).unwrap()["id"].as_str().unwrap().to_string();
    assert!(
        s.run(
            "ai.saveProfile",
            &with("id", json!(id))
                .as_object()
                .map(|o| {
                    let mut o = o.clone();
                    o.insert("fallback".into(), json!(id));
                    Value::Object(o)
                })
                .unwrap()
        )
        .is_err(),
        "not its own fallback"
    );
    for n in 1..32 {
        s.run("ai.saveProfile", &with("name", json!(format!("M{n}")))).unwrap();
    }
    assert!(s.run("ai.saveProfile", &base).is_err(), "32 at most");
}

#[test]
fn what_a_model_is_and_how_far_mail_goes_decide_where_it_is_asked() {
    let mut s = sample_session("ai-route");
    let c = cloud(&mut s, "Claude");
    let l = local(&mut s, "Ollama");
    s.take_jobs();
    let shown = s.run("ai.settings", &json!({})).unwrap();
    assert_eq!((&shown["profiles"][0]["local"], &shown["profiles"][1]["local"]), (&json!(false), &json!(true)));
    // Local mail only: the cloud profile, first, isn't asked; with no fallback to one that is
    // local, nothing is.
    s.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    let e = ask(&mut s, "q").unwrap_err().to_string();
    assert!(e.contains("Claude isn't on this computer"), "{e}");
    s.run(
        "ai.saveProfile",
        &json!({"id": c, "name": "Claude", "api": "anthropic", "url": "https://api.anthropic.com/v1", "model": "m", "fallback": l}),
    )
    .unwrap();
    ask(&mut s, "q").unwrap();
    assert_eq!(asked(&mut s)[0].2.url, "http://localhost:11434/v1", "its local fallback");
    // Any model: the one named for the work, else the first.
    s.run("ai.allow", &json!({"account": "sample", "reach": "any"})).unwrap();
    ask(&mut s, "q").unwrap();
    assert_eq!(asked(&mut s)[0].2.url, "https://api.anthropic.com/v1");
    s.run("ai.use", &json!({"task": "decision", "profile": l})).unwrap();
    s.run("ai.ask", &json!({"account": "sample", "question": "urgent?", "task": "decision"})).unwrap();
    assert_eq!(asked(&mut s)[0].2.model, "llama3.2");
    // Local only, whatever the account allows: the cloud isn't asked, not even a test.
    s.run("ai.localOnly", &json!({"on": true})).unwrap();
    ask(&mut s, "q").unwrap();
    assert_eq!(asked(&mut s)[0].2.url, "http://localhost:11434/v1");
    assert!(s.run("ai.test", &json!({"profile": c})).is_err());
    assert!(asked(&mut s).is_empty());
    s.run("ai.test", &json!({"profile": l})).unwrap();
    assert_eq!(asked(&mut s).len(), 1);
}

#[test]
fn a_failed_question_goes_to_the_fallback_once() {
    let mut s = sample_session("ai-fallback");
    let c = cloud(&mut s, "Claude");
    let l = local(&mut s, "Ollama");
    s.run(
        "ai.saveProfile",
        &json!({"id": c, "name": "Claude", "api": "anthropic", "url": "https://api.anthropic.com/v1", "model": "m", "fallback": l}),
    )
    .unwrap();
    s.run(
        "ai.saveProfile",
        &json!({"id": l, "name": "Ollama", "api": "openAi", "url": "http://localhost:11434/v1", "model": "llama3.2", "fallback": c}),
    )
    .unwrap();
    s.run("ai.allow", &json!({"account": "sample", "reach": "any"})).unwrap();
    s.take_jobs();
    let id = ask(&mut s, "q").unwrap();
    assert_eq!(asked(&mut s)[0].2.url, "https://api.anthropic.com/v1");
    fail(&mut s, id, "network: connection refused");
    let again = asked(&mut s);
    assert_eq!((again.len(), again[0].0, again[0].2.url.as_str()), (1, id, "http://localhost:11434/v1"), "the same question, to its fallback");
    assert_eq!(again[0].1, None, "a profile without a key names none");
    assert_eq!(s.run("ai.answer", &json!({"id": id})).unwrap(), json!({"waiting": true}));
    // The fallback falls back to the first: asked already, so the question fails.
    fail(&mut s, id, "model not found");
    assert!(asked(&mut s).is_empty(), "each profile asked once, however they loop");
    let e = s.run("ai.answer", &json!({"id": id})).unwrap_err().to_string();
    assert!(e.contains("model not found"), "{e}");
    let log = s.run("ai.log", &json!({})).unwrap();
    let outcomes: Vec<&str> = log["entries"].as_array().unwrap().iter().map(|e| e["outcome"].as_str().unwrap()).collect();
    assert_eq!(outcomes, ["failed", "failed"]);
}

#[test]
fn the_record_says_where_and_how_much_and_never_what() {
    let mut s = sample_session("ai-log");
    let store = s.store.clone().unwrap();
    cloud(&mut s, "Claude");
    s.run("ai.allow", &json!({"account": "sample", "reach": "any"})).unwrap();
    s.take_jobs();
    let id = ask(&mut s, "The secret plan is in the attachment").unwrap();
    let log = s.run("ai.log", &json!({})).unwrap();
    let e = &log["entries"][0];
    assert_eq!((e["purpose"].as_str(), e["profile"].as_str(), e["server"].as_str()), (Some("summary"), Some("Claude"), Some("api.anthropic.com")));
    assert_eq!((e["account"].as_str(), e["outcome"].as_str(), e["at"].as_i64()), (Some("sample"), Some("waiting"), Some(NOW)));
    assert_eq!(e["sent"], "The secret plan is in the attachment".len());
    assert!(!store.root().join("ai-log.json").exists(), "saved as the jobs are taken");
    assert_eq!(asked(&mut s).len(), 1);
    assert!(std::fs::read_to_string(store.root().join("ai-log.json")).unwrap().contains("\"waiting\""), "before the question goes");
    answer(&mut s, id, "It's about the launch.");
    s.take_jobs();
    let kept = std::fs::read_to_string(store.root().join("ai-log.json")).unwrap();
    assert!(kept.contains("\"answered\"") && kept.contains("\"received\":20"), "{kept}");
    assert!(!kept.contains("secret plan") && !kept.contains("launch") && !kept.contains(KEY), "{kept}");
    // Kept across a restart, and capped.
    let mut again = Session::open(Some(store.clone()), mailcraft_model::dates::Clock::new(NOW, 0));
    assert_eq!(again.ai.log.len(), 1);
    again.prefs.ai = s.prefs.ai.clone();
    for n in 0..AI_LOG_KEPT {
        again.run("ai.ask", &json!({"account": "sample", "question": format!("q{n}")})).unwrap();
    }
    assert_eq!(again.ai.log.len(), AI_LOG_KEPT);
    assert_eq!(again.ai.log.front().unwrap().1.purpose, "question", "the oldest went first");
    again.take_jobs();
    let kept: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(store.root().join("ai-log.json")).unwrap()).unwrap();
    assert_eq!(kept.len(), AI_LOG_KEPT);
}

#[test]
fn a_damaged_record_is_set_aside_and_a_new_one_begun() {
    let store = temp("ai-log-damaged");
    std::fs::write(store.root().join("ai-log.json"), b"{ not a list").unwrap();
    let mut s = Session::open(Some(store.clone()), mailcraft_model::dates::Clock::new(NOW, 0));
    assert!(s.ai.log.is_empty());
    assert_eq!(std::fs::read(store.root().join("ai-log.json.bad")).unwrap(), b"{ not a list", "kept as it was");
    s.run("ai.saveProfile", &json!({"name": "Ollama", "api": "openAi", "url": "http://localhost:11434/v1", "model": "llama3.2"})).unwrap();
    s.run("ai.test", &json!({"profile": "p1"})).unwrap();
    s.take_jobs();
    let kept = std::fs::read_to_string(store.root().join("ai-log.json")).unwrap();
    assert!(kept.contains("\"purpose\":\"test\""), "{kept}");
}

#[test]
fn a_test_asks_without_mail_and_says_how_it_went() {
    let mut s = sample_session("ai-test");
    let l = local(&mut s, "Ollama");
    s.take_jobs();
    s.run("ai.test", &json!({"profile": l})).unwrap();
    let q = asked(&mut s);
    assert_eq!(q[0].3.turns.len(), 1);
    assert!(q[0].3.turns[0].text.contains("OK") && q[0].3.max_tokens <= 1024);
    assert_eq!(s.ai.tests.get(&l), Some(&AiTest::Asking));
    answer(&mut s, q[0].0, "  OK\n");
    assert_eq!(s.ai.tests.get(&l), Some(&AiTest::Answered { text: "OK".into() }));
    assert_eq!(s.run("ai.answer", &json!({"id": q[0].0})).unwrap(), json!({"waiting": false}), "a test's answer is its profile's");
    s.run("ai.test", &json!({"profile": l})).unwrap();
    let q = asked(&mut s);
    fail(&mut s, q[0].0, "network: connection refused");
    assert!(matches!(s.ai.tests.get(&l), Some(AiTest::Failed { error }) if error.contains("refused")));
    assert!(s.run("ai.test", &json!({"profile": "p9"})).is_err());
}

#[test]
fn a_model_is_local_only_on_this_computer_or_its_network() {
    let at =
        |url: &str| AiProfile { endpoint: AiEndpoint { api: AiApi::OpenAi, url: url.into(), model: "m".into() }, ..AiProfile::default() }.local();
    for url in [
        "http://localhost:11434/v1",
        "http://127.0.0.1:1234/v1",
        "http://[::1]:8080/v1",
        "https://192.168.1.5/v1",
        "https://10.0.0.2:8443/v1",
        "https://172.20.1.1/v1",
        "https://ollama.local/v1",
        "https://[fd00::5]/v1",
        "HTTPS://LOCALHOST/v1",
    ] {
        assert!(at(url), "{url}");
    }
    for url in [
        "https://api.openai.com/v1",
        "https://localhost.example/v1",
        "https://8.8.8.8/v1",
        "https://172.32.0.1/v1",
        "https://user@localhost/v1",
        "localhost:11434",
        "",
        "https://[2001:db8::1]/v1",
    ] {
        assert!(!at(url), "{url}");
    }
}

/// Streamed, an answer's parts are kept as they come, given by `ai.answer` while it waits, and
/// begun again when a failed question goes to its fallback.
#[test]
fn a_streamed_answer_is_kept_as_it_comes() {
    let mut s = sample_session("ai-stream");
    s.run("ai.allow", &json!({"account": "sample", "reach": "any"})).unwrap();
    let first = local(&mut s, "Ollama");
    let second = local(&mut s, "Another");
    s.run(
        "ai.saveProfile",
        &json!({"id": first, "name": "Ollama", "api": "openAi", "url": "http://localhost:11434/v1", "model": "llama3.2", "fallback": second}),
    )
    .unwrap();
    s.take_jobs();
    let id = s.run("ai.ask", &json!({"account": "sample", "question": "Summarise", "stream": true})).unwrap()["id"].as_u64().unwrap();
    let streamed = |s: &mut Session| s.take_jobs().into_iter().any(|j| matches!(j, Job::Ai { work: AiWork::Ask { stream: true, .. } }));
    assert!(streamed(&mut s), "asked streamed");
    let part = |s: &mut Session, text: &str, thinking: &str| {
        s.apply(Event::Ai { done: AiDone::Part { id, text: text.into(), thinking: thinking.into() } });
    };
    part(&mut s, "Two ", "Read it. ");
    part(&mut s, "meetings", "");
    assert_eq!(s.run("ai.answer", &json!({"id": id})).unwrap(), json!({"waiting": true, "text": "Two meetings", "thinking": "Read it. "}));
    // Failed: its fallback asked, from nothing again.
    fail(&mut s, id, "the model's answer stopped before its end");
    assert!(streamed(&mut s), "the fallback asked streamed too");
    assert_eq!(s.run("ai.answer", &json!({"id": id})).unwrap(), json!({"waiting": true, "text": "", "thinking": ""}));
    part(&mut s, "Two meetings moved.", "");
    answer(&mut s, id, "Two meetings moved.");
    assert_eq!(s.run("ai.answer", &json!({"id": id})).unwrap()["text"], "Two meetings moved.");
    assert_eq!(s.run("ai.answer", &json!({"id": id})).unwrap(), json!({"waiting": false}), "its parts gone with it");
    // A part for a question not waiting, or not streamed, is dropped.
    part(&mut s, "late", "");
    assert_eq!(s.run("ai.answer", &json!({"id": id})).unwrap(), json!({"waiting": false}));
}
