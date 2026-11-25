//! Ollama's own format: told what the model takes in, refusing a longer question rather than
//! cutting its start; its tool calls and their results; its answer whole or as JSON lines.

use super::*;
use mailcraft_model::{AiTool, AiToolCall, AiToolResult};

fn ollama(server: &ModelServer) -> AiEndpoint {
    endpoint(server, AiApi::Ollama)
}

/// A line of Ollama's stream.
fn line(v: Value) -> String {
    format!("{v}\n")
}

#[test]
fn ollamas_own_format_is_told_what_the_model_takes_in() {
    let server = ModelServer::start();
    let a = ask(&ollama(&server), None, &AiChat { context: 16_384, ..chat() }).unwrap();
    assert_eq!(a.text, "You said: hello");
    assert_eq!((a.input_tokens, a.output_tokens, a.cut), (Some(12), Some(5), false));
    let r = &server.seen()[0];
    // Its own address, at the server's root: not the OpenAI-compatible one under /v1.
    assert_eq!(r.target, "/api/chat");
    assert_eq!(r.header("Authorization"), None);
    assert_eq!(r.body["options"], json!({"num_ctx": 16_384, "num_predict": 256}));
    assert_eq!(
        (&r.body["truncate"], &r.body["shift"], &r.body["stream"]),
        (&json!(false), &json!(false), &json!(false)),
        "too long is refused, never cut"
    );
    let roles: Vec<&str> = r.body["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["system", "user", "assistant", "user"]);
    // None said: the server's own.
    ask(&ollama(&server), Some(KEY), &chat()).unwrap();
    let r = &server.seen()[1];
    assert!(r.body["options"].get("num_ctx").is_none(), "{}", r.body);
    assert_eq!(r.header("Authorization"), Some(format!("Bearer {KEY}").as_str()), "Ollama's cloud takes a key");
}

#[test]
fn ollamas_tool_calls_and_results_go_by_the_tools_name() {
    let server = ModelServer::start();
    let mut said = AiTurn::assistant("Looking.");
    said.calls = vec![
        AiToolCall { id: "c1".into(), name: "search_mail".into(), input: json!({"query": "invoice"}) },
        AiToolCall { id: "c2".into(), name: "read_message".into(), input: Value::String("{broken".into()) },
    ];
    let mut back = AiTurn::user("");
    back.results = vec![
        AiToolResult { id: "c1".into(), content: "2 found".into(), error: false },
        AiToolResult { id: "c2".into(), content: "no such message".into(), error: true },
    ];
    let tools = vec![AiTool { name: "search_mail".into(), description: "Search the mail".into(), schema: json!({"type": "object"}) }];
    let chat = AiChat {
        system: "Help.".into(),
        turns: vec![AiTurn::user("Find the invoice"), said, back],
        max_tokens: 512,
        tools,
        context: 8192,
        ..AiChat::default()
    };
    let calls = json!([
        {"function": {"name": "search_mail", "arguments": {"query": "receipt"}}},
        {"id": "x7", "function": {"name": "read_message", "arguments": {"id": "m1"}}},
    ]);
    let answer = json!({"message": {"role": "assistant", "content": "", "thinking": "Search first.", "tool_calls": calls},
                        "done": true, "done_reason": "length", "prompt_eval_count": 80, "eval_count": 20});
    server.then("200 OK", &answer.to_string());
    let a = ask(&ollama(&server), None, &chat).unwrap();
    let body = &server.seen()[0].body;
    let m = &body["messages"];
    assert_eq!(m[2]["tool_calls"][0]["function"]["arguments"], json!({"query": "invoice"}), "an object, not a string: {m}");
    assert_eq!(m[2]["tool_calls"][1]["function"]["arguments"], json!({}), "a broken input goes back empty");
    assert_eq!((m[3]["role"].as_str(), m[3]["tool_name"].as_str(), m[3]["content"].as_str()), (Some("tool"), Some("search_mail"), Some("2 found")));
    assert_eq!(m[4]["tool_name"], "read_message");
    assert_eq!(body["tools"][0]["function"]["name"], "search_mail");
    let got: Vec<(&str, &str, &Value)> = a.calls.iter().map(|c| (c.id.as_str(), c.name.as_str(), &c.input)).collect();
    assert_eq!(got, [("call_0", "search_mail", &json!({"query": "receipt"})), ("x7", "read_message", &json!({"id": "m1"}))]);
    assert_eq!((a.thinking.as_str(), a.cut), ("Search first.", true), "cut short at its longest");
    assert_eq!((a.input_tokens, a.output_tokens), (Some(80), Some(20)));
}

#[test]
fn ollamas_answer_streams_as_json_lines() {
    let server = ModelServer::start();
    let mut pieces = Vec::new();
    let a = ask_streamed(&ollama(&server), None, &chat(), &mut |t: &str, th: &str| pieces.push((t.to_string(), th.to_string()))).unwrap();
    assert_eq!(a.text, "You said: hello");
    assert!(pieces.len() >= 3, "a word at a time: {pieces:?}");
    assert_eq!(pieces.iter().map(|(t, _)| t.as_str()).collect::<String>(), a.text);
    assert_eq!((a.input_tokens, a.output_tokens), (Some(12), Some(5)));
    assert_eq!(server.seen()[0].body["stream"], true);
    // A line split anywhere, thinking, a tool call sent whole.
    let first = line(json!({"message": {"content": "", "thinking": "Hmm."}, "done": false}));
    let (a1, a2) = first.split_at(10);
    let call =
        line(json!({"message": {"content": "", "tool_calls": [{"function": {"name": "search_mail", "arguments": {"query": "x"}}}]}, "done": false}));
    let end = line(json!({"message": {"content": ""}, "done": true, "done_reason": "stop", "prompt_eval_count": 9, "eval_count": 3}));
    server.then_lines(&[a1, a2, &call, &end]);
    let a = ask_streamed(&ollama(&server), None, &chat(), &mut |_: &str, _: &str| {}).unwrap();
    assert_eq!((a.thinking.as_str(), a.calls.len()), ("Hmm.", 1));
    assert_eq!((a.calls[0].name.as_str(), &a.calls[0].input), ("search_mail", &json!({"query": "x"})));
    // An error as it writes, and a stream ending before its end: errors, never part of an answer.
    let part = line(json!({"message": {"content": "Par"}, "done": false}));
    server.then_lines(&[&part, &line(json!({"error": "model runner has unexpectedly stopped"}))]);
    let e = ask_streamed(&ollama(&server), None, &chat(), &mut |_: &str, _: &str| {}).unwrap_err().to_string();
    assert!(e.contains("unexpectedly stopped"), "{e}");
    server.then_lines(&[&part]);
    let e = ask_streamed(&ollama(&server), None, &chat(), &mut |_: &str, _: &str| {}).unwrap_err().to_string();
    assert!(e.contains("stopped before its end"), "{e}");
}

#[test]
fn a_question_longer_than_ollama_takes_in_is_refused_in_its_words() {
    let server = ModelServer::start();
    let refused = r#"{"error":"prompt is longer than the context length"}"#;
    server.then("400 Bad Request", refused);
    let e = ask(&ollama(&server), None, &chat()).unwrap_err().to_string();
    assert!(e.contains("HTTP 400") && e.contains("prompt is longer than the context length"), "{e}");
    server.then("400 Bad Request", refused);
    let e = ask_streamed(&ollama(&server), None, &chat(), &mut |_: &str, _: &str| {}).unwrap_err().to_string();
    assert!(e.contains("prompt is longer than the context length"), "streamed too: {e}");
    // An answer that never says it ended is none.
    server.then("200 OK", r#"{"message": {"content": "hi"}}"#);
    let e = ask(&ollama(&server), None, &chat()).unwrap_err().to_string();
    assert!(e.contains("sent no answer"), "{e}");
}
