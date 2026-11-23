use super::*;
use crate::fakes::fake_ai::ModelServer;

mod streaming;
mod tools;
use mailcraft_model::AiTurn;
use serde_json::json;

const KEY: &str = "sk-test-0123456789";

fn endpoint(server: &ModelServer, api: AiApi) -> AiEndpoint {
    AiEndpoint { api, url: server.base.clone(), model: "small-model".into() }
}

fn chat() -> AiChat {
    AiChat {
        system: "Answer briefly.".into(),
        turns: vec![AiTurn::user("Who wrote?"), AiTurn::assistant("Sam did."), AiTurn::user("hello")],
        max_tokens: 256,
        tools: Vec::new(),
    }
}

#[test]
fn openais_format_is_asked_with_the_key_and_answered() {
    let server = ModelServer::start();
    let a = ask(&endpoint(&server, AiApi::OpenAi), Some(KEY), &chat()).unwrap();
    assert_eq!(a.text, "You said: hello");
    assert!(!a.cut);
    assert_eq!((a.input_tokens, a.output_tokens), (Some(12), Some(5)));
    assert!(a.sent > 0 && a.received > 0);
    let seen = server.seen();
    assert_eq!(seen.len(), 1);
    let r = &seen[0];
    assert_eq!(r.target, "/v1/chat/completions");
    assert_eq!(r.header("Authorization"), Some(format!("Bearer {KEY}").as_str()));
    assert_eq!(r.body["model"], "small-model");
    assert_eq!(r.body["stream"], false);
    // Another server than OpenAI's own knows max_tokens.
    assert_eq!(r.body["max_tokens"], 256);
    assert!(r.body.get("max_completion_tokens").is_none());
    let roles: Vec<&str> = r.body["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["system", "user", "assistant", "user"]);
    assert_eq!(r.body["messages"][0]["content"], "Answer briefly.");
}

#[test]
fn openais_own_server_is_given_max_completion_tokens() {
    let e = AiEndpoint { api: AiApi::OpenAi, url: "https://api.openai.com/v1".into(), model: "m".into() };
    let r = openai::request("https://api.openai.com/v1", &e, None, &chat(), false);
    let body: Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(body["max_completion_tokens"], 256);
    assert!(body.get("max_tokens").is_none(), "its reasoning models refuse max_tokens");
}

#[test]
fn anthropics_format_is_asked_with_its_headers_and_answered() {
    let server = ModelServer::start();
    let a = ask(&endpoint(&server, AiApi::Anthropic), Some(KEY), &chat()).unwrap();
    assert_eq!(a.text, "You said: hello");
    assert_eq!((a.input_tokens, a.output_tokens), (Some(12), Some(5)));
    let r = &server.seen()[0];
    assert_eq!(r.target, "/v1/messages");
    assert_eq!(r.header("x-api-key"), Some(KEY));
    assert_eq!(r.header("anthropic-version"), Some("2026-10-21"));
    assert!(r.header("Authorization").is_none());
    assert_eq!(r.body["system"], "Answer briefly.");
    assert_eq!(r.body["max_tokens"], 256);
    assert!(r.body.get("temperature").is_none(), "no model takes it since 2026-10-21");
    let roles: Vec<&str> = r.body["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["user", "assistant", "user"], "its instructions go apart");
}

#[test]
fn only_the_text_is_kept_and_a_cut_answer_says_so() {
    let server = ModelServer::start();
    server.then(
        "200 OK",
        &json!({"content": [{"type": "thinking", "thinking": "hmm"}, {"type": "text", "text": "Part one, "}, {"type": "text", "text": "part two"}],
                "stop_reason": "max_tokens"})
        .to_string(),
    );
    let a = ask(&endpoint(&server, AiApi::Anthropic), Some(KEY), &chat()).unwrap();
    assert_eq!(a.text, "Part one, part two");
    assert!(a.cut);
    server.then(
        "200 OK",
        &json!({"choices": [{"message": {"content": [{"type": "text", "text": "A"}, {"type": "text", "text": "B"}]}, "finish_reason": "length"}]})
            .to_string(),
    );
    let a = ask(&endpoint(&server, AiApi::OpenAi), None, &chat()).unwrap();
    assert_eq!(a.text, "AB");
    assert!(a.cut);
    assert_eq!(a.input_tokens, None, "no usage given");
}

#[test]
fn a_server_without_a_key_is_sent_none() {
    let server = ModelServer::start();
    ask(&endpoint(&server, AiApi::OpenAi), None, &chat()).unwrap();
    ask(&endpoint(&server, AiApi::Anthropic), Some("  "), &chat()).unwrap();
    let seen = server.seen();
    assert!(seen[0].header("Authorization").is_none());
    assert!(seen[1].header("x-api-key").is_none());
}

#[test]
fn a_refusal_says_why_without_the_key() {
    let server = ModelServer::start();
    server.then(
        "401 Unauthorized",
        &json!({"error": {"message": format!("Incorrect API key provided: {KEY}"), "type": "invalid_request_error"}}).to_string(),
    );
    let e = ask(&endpoint(&server, AiApi::OpenAi), Some(KEY), &chat()).unwrap_err().to_string();
    assert!(e.contains("the key was refused") && e.contains("Incorrect API key provided"), "{e}");
    assert!(!e.contains(KEY), "the key quoted back is blanked out: {e}");
    // Ollama's: the error a string.
    server.then("404 Not Found", r#"{"error":"model \"small-model\" not found, try pulling it first"}"#);
    let e = ask(&endpoint(&server, AiApi::OpenAi), None, &chat()).unwrap_err().to_string();
    assert!(e.contains("no such model or address") && e.contains("try pulling it first"), "{e}");
    // Anthropic's, and one with nothing to say.
    server.then("429 Too Many Requests", &json!({"type": "error", "error": {"type": "rate_limit_error", "message": "slow down"}}).to_string());
    let e = ask(&endpoint(&server, AiApi::Anthropic), Some(KEY), &chat()).unwrap_err().to_string();
    assert!(e.contains("too many requests") && e.contains("slow down"), "{e}");
    server.then("500 Internal Server Error", "<html>oops</html>");
    let e = ask(&endpoint(&server, AiApi::Anthropic), Some(KEY), &chat()).unwrap_err().to_string();
    assert!(e.contains("HTTP 500") && !e.contains("html"), "{e}");
}

/// However a server words it, the key it quotes back never shows, whole or in part.
#[test]
fn a_key_quoted_back_is_blanked_however_it_is_hidden() {
    let split = format!("{}\u{7}{}", &KEY[..6], &KEY[6..]);
    let at_the_cut = format!("{}{KEY}", "x".repeat(MAX_ERROR - 6));
    for said in [format!("bad key {KEY}"), split, at_the_cut] {
        let q = quoted(&said, Some(KEY));
        assert!(!q.contains(KEY) && !q.contains(&KEY[..6]), "{said:?} -> {q:?}");
    }
    let server = ModelServer::start();
    server.then("200 OK", &json!({"choices": [{"message": {"refusal": format!("not with {KEY}")}}]}).to_string());
    let e = ask(&endpoint(&server, AiApi::OpenAi), Some(KEY), &chat()).unwrap_err().to_string();
    assert!(e.contains("declined") && !e.contains(KEY), "{e}");
}

#[test]
fn a_model_that_declines_is_an_error() {
    let server = ModelServer::start();
    server.then("200 OK", &json!({"choices": [{"message": {"content": null, "refusal": "I can't help with that."}}]}).to_string());
    let e = ask(&endpoint(&server, AiApi::OpenAi), None, &chat()).unwrap_err().to_string();
    assert!(e.contains("declined") && e.contains("can't help"), "{e}");
    server.then("200 OK", &json!({"content": [], "stop_reason": "refusal"}).to_string());
    let e = ask(&endpoint(&server, AiApi::Anthropic), Some(KEY), &chat()).unwrap_err().to_string();
    assert!(e.contains("declined"), "{e}");
}

#[test]
fn hostile_answers_are_errors() {
    let server = ModelServer::start();
    server.then("200 OK", "not json at all");
    assert!(ask(&endpoint(&server, AiApi::OpenAi), None, &chat()).unwrap_err().to_string().contains("other than JSON"));
    server.then("200 OK", r#"{"choices": []}"#);
    assert!(ask(&endpoint(&server, AiApi::OpenAi), None, &chat()).unwrap_err().to_string().contains("no answer"));
    server.then("200 OK", r#"{"content": "a string, not blocks"}"#);
    assert!(ask(&endpoint(&server, AiApi::Anthropic), None, &chat()).unwrap_err().to_string().contains("no answer"));
    // Nested deeper than serde_json reads.
    server.then("200 OK", &format!("{}{}", "[".repeat(10_000), "]".repeat(10_000)));
    assert!(ask(&endpoint(&server, AiApi::OpenAi), None, &chat()).is_err());
    // Longer than an answer is read.
    server.then("200 OK", &format!(r#"{{"choices":[{{"message":{{"content":"{}"}}}}]}}"#, "a".repeat(MAX_ANSWER)));
    assert!(ask(&endpoint(&server, AiApi::OpenAi), None, &chat()).is_err());
}

#[test]
fn a_key_is_never_sent_in_clear_to_another_computer() {
    for url in ["http://192.0.2.10:11434/v1", "http://models.example/v1", "ftp://models.example/v1", "not an address"] {
        let e = AiEndpoint { api: AiApi::OpenAi, url: url.into(), model: "m".into() };
        assert!(ask(&e, Some(KEY), &chat()).is_err(), "{url}");
    }
    assert!(address("http://localhost:11434/v1/").is_ok_and(|a| a == "http://localhost:11434/v1"));
    assert!(address("https://api.example/v1").is_ok());
}

#[test]
fn a_question_too_long_isnt_sent() {
    let server = ModelServer::start();
    let long = AiChat::ask("", &"x".repeat(MAX_QUESTION), 16);
    let e = ask(&endpoint(&server, AiApi::OpenAi), None, &long).unwrap_err().to_string();
    assert!(e.contains("too long"), "{e}");
    assert!(server.seen().is_empty());
}

/// A server whose status line quotes the key back, whole, split by a control character, or cut
/// where the HTTP layer cuts what it quotes: the error says so without it, whole or streamed.
#[test]
fn a_key_in_a_broken_status_line_never_shows() {
    let cut = format!("{}{KEY}", "x".repeat(60 - "HTTP/1.1 ".len() - 8));
    let split = format!("2x0 {}\u{7}{}", &KEY[..6], &KEY[6..]);
    for status in [format!("2x0 {KEY} oops"), split, cut] {
        let server = ModelServer::start();
        server.then(&status, "{}");
        server.then(&status, "{}");
        let whole = ask(&endpoint(&server, AiApi::OpenAi), Some(KEY), &chat()).unwrap_err().to_string();
        let streamed = ask_streamed(&endpoint(&server, AiApi::Anthropic), Some(KEY), &chat(), &mut |_, _| {}).unwrap_err().to_string();
        for e in [whole, streamed] {
            assert!(!e.contains(&KEY[..6]), "{status:?} -> {e}");
            assert!(e.contains("couldn't be read"), "{e}");
        }
    }
}
