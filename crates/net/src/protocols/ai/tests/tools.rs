//! Tools: offered in each format's own shape, a model's calls of them read from its answer, and
//! what they gave back sent with the next question.

use super::*;
use mailcraft_model::{AiTool, AiToolCall, AiToolResult};

/// A chat that offered a tool, the model called it twice, and one call failed.
fn chat_with_tools() -> AiChat {
    let mut said = AiTurn::assistant("Looking.");
    said.calls = vec![
        AiToolCall { id: "c1".into(), name: "search_mail".into(), input: json!({"query": "invoice"}) },
        AiToolCall { id: "c2".into(), name: "read_message".into(), input: json!({"key": "a:1:2:INBOX"}) },
    ];
    let mut back = AiTurn::user("And the dates?");
    back.results = vec![
        AiToolResult { id: "c1".into(), content: "2 found".into(), error: false },
        AiToolResult { id: "c2".into(), content: "no such message".into(), error: true },
    ];
    AiChat {
        system: "Help.".into(),
        turns: vec![AiTurn::user("Find the invoice"), said, back],
        max_tokens: 512,
        tools: vec![AiTool {
            name: "search_mail".into(),
            description: "Search the mail".into(),
            schema: json!({"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}),
        }],
    }
}

#[test]
fn openais_format_offers_tools_and_sends_their_calls_and_results() {
    let server = ModelServer::start();
    ask(&endpoint(&server, AiApi::OpenAi), None, &chat_with_tools()).unwrap();
    let body = &server.seen()[0].body;
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["function"]["name"], "search_mail");
    assert_eq!(body["tools"][0]["function"]["parameters"]["required"][0], "query");
    let m = body["messages"].as_array().unwrap();
    let roles: Vec<&str> = m.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["system", "user", "assistant", "tool", "tool", "user"], "each result a message of its own, before what was said");
    assert_eq!(m[2]["content"], "Looking.");
    assert_eq!(m[2]["tool_calls"][0]["id"], "c1");
    assert_eq!(m[2]["tool_calls"][0]["function"]["arguments"], r#"{"query":"invoice"}"#, "JSON in a string");
    assert_eq!((m[3]["tool_call_id"].as_str(), m[3]["content"].as_str()), (Some("c1"), Some("2 found")));
    assert_eq!(m[4]["tool_call_id"], "c2");
    assert_eq!(m[5]["content"], "And the dates?");
}

#[test]
fn anthropics_format_offers_tools_and_sends_their_calls_and_results() {
    let server = ModelServer::start();
    ask(&endpoint(&server, AiApi::Anthropic), Some(KEY), &chat_with_tools()).unwrap();
    let body = &server.seen()[0].body;
    assert_eq!(body["tools"][0]["name"], "search_mail");
    assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
    let m = body["messages"].as_array().unwrap();
    assert_eq!(m.len(), 3);
    assert_eq!(m[1]["content"][0], json!({"type": "text", "text": "Looking."}));
    assert_eq!(m[1]["content"][1], json!({"type": "tool_use", "id": "c1", "name": "search_mail", "input": {"query": "invoice"}}));
    assert_eq!(m[2]["role"], "user");
    assert_eq!(m[2]["content"][0], json!({"type": "tool_result", "tool_use_id": "c1", "content": "2 found"}));
    assert_eq!(m[2]["content"][1]["is_error"], true);
    assert_eq!(m[2]["content"][2], json!({"type": "text", "text": "And the dates?"}), "what was said after the results");
}

/// A call whose input wasn't an object (its text cut short or broken, an array) goes back as an
/// empty one in both formats: a server that reads the history's calls would refuse the chat for
/// it. An answer that said nothing, or only blanks beside its calls, is never sent as empty or
/// blank content, which Anthropic's refuses.
#[test]
fn what_a_model_gave_broken_or_empty_goes_back_as_each_format_takes_it() {
    let mut said = AiTurn::assistant("\n\n");
    said.calls = vec![
        AiToolCall { id: "c1".into(), name: "search_mail".into(), input: json!("{\"query\": \"inv") },
        AiToolCall { id: "c2".into(), name: "search_mail".into(), input: json!(["invoice"]) },
    ];
    let mut back = AiTurn::user("");
    back.results = vec![
        AiToolResult { id: "c1".into(), content: "not JSON".into(), error: true },
        AiToolResult { id: "c2".into(), content: "not JSON".into(), error: true },
    ];
    let chat = AiChat {
        system: "Help.".into(),
        turns: vec![AiTurn::user("Find it"), said, back, AiTurn::assistant(""), AiTurn::user("Again?")],
        max_tokens: 512,
        tools: Vec::new(),
    };
    let server = ModelServer::start();
    ask(&endpoint(&server, AiApi::OpenAi), None, &chat).unwrap();
    let m = server.seen()[0].body["messages"].clone();
    assert_eq!(m[2]["tool_calls"][0]["function"]["arguments"], "{}", "{m}");
    assert_eq!(m[2]["tool_calls"][1]["function"]["arguments"], "{}", "{m}");
    ask(&endpoint(&server, AiApi::Anthropic), Some(KEY), &chat).unwrap();
    let m = server.seen()[1].body["messages"].clone();
    for msg in m.as_array().unwrap() {
        match &msg["content"] {
            Value::String(t) => assert!(!t.trim().is_empty(), "{msg}"),
            Value::Array(blocks) => {
                assert!(!blocks.is_empty(), "{msg}");
                for b in blocks.iter().filter(|b| b["type"] == "text") {
                    assert!(!b["text"].as_str().unwrap().trim().is_empty(), "{msg}");
                }
            }
            other => panic!("content of an unknown kind: {other}"),
        }
    }
}

#[test]
fn a_models_calls_are_read_from_a_whole_answer() {
    let server = ModelServer::start();
    server.then(
        "200 OK",
        &json!({"choices": [{"message": {"content": null, "reasoning": "They want the invoice.", "tool_calls": [
            {"id": "call_a", "type": "function", "function": {"name": "search_mail", "arguments": "{\"query\":\"invoice\"}"}},
            {"type": "function", "function": {"name": "read_message", "arguments": "not json"}},
            {"id": "call_c", "type": "function", "function": {"name": "list_folders", "arguments": ""}},
        ]}, "finish_reason": "tool_calls"}]})
        .to_string(),
    );
    let a = ask(&endpoint(&server, AiApi::OpenAi), None, &chat()).unwrap();
    assert_eq!(a.text, "");
    assert_eq!(a.thinking, "They want the invoice.");
    let calls: Vec<(&str, &str, &Value)> = a.calls.iter().map(|c| (c.id.as_str(), c.name.as_str(), &c.input)).collect();
    assert_eq!(calls[0], ("call_a", "search_mail", &json!({"query": "invoice"})));
    assert_eq!(calls[1], ("call_1", "read_message", &json!("not json")), "no id: one made; not JSON: its text, for the engine to refuse");
    assert_eq!(calls[2], ("call_c", "list_folders", &json!({})), "nothing given: no input");
    server.then(
        "200 OK",
        &json!({"content": [{"type": "thinking", "thinking": "Search first."}, {"type": "text", "text": "Searching."},
                {"type": "tool_use", "id": "toolu_1", "name": "search_mail", "input": {"query": "invoice"}},
                {"type": "tool_use", "id": "toolu_2", "name": "read_message", "input": "a string"}], "stop_reason": "tool_use"})
        .to_string(),
    );
    let a = ask(&endpoint(&server, AiApi::Anthropic), Some(KEY), &chat()).unwrap();
    assert_eq!((a.text.as_str(), a.thinking.as_str()), ("Searching.", "Search first."));
    assert_eq!(a.calls.len(), 2);
    assert_eq!((a.calls[0].id.as_str(), &a.calls[0].input), ("toolu_1", &json!({"query": "invoice"})));
    assert_eq!(a.calls[1].input, json!({}), "an input that isn't an object goes as none");
}

#[test]
fn a_flood_of_calls_is_cut_at_the_most_one_answer_makes() {
    let server = ModelServer::start();
    let calls: Vec<Value> = (0..500).map(|n| json!({"id": format!("c{n}"), "function": {"name": "x", "arguments": "{}"}})).collect();
    server.then("200 OK", &json!({"choices": [{"message": {"tool_calls": calls}, "finish_reason": "tool_calls"}]}).to_string());
    let a = ask(&endpoint(&server, AiApi::OpenAi), None, &chat()).unwrap();
    assert_eq!(a.calls.len(), streamed::MAX_CALLS);
}
