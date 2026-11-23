//! Answers streamed as they're written: their text and thinking handed on as they come, tool
//! calls built from their pieces, an event split anywhere, and every way a stream goes wrong.

use super::*;

/// Ask `api` with the answer streamed: the answer, or why there's none, and the pieces seen.
fn streamed(server: &ModelServer, api: AiApi, key: Option<&str>) -> (Result<AiAnswer>, Vec<(String, String)>) {
    let mut pieces = Vec::new();
    let mut on = |text: &str, thinking: &str| pieces.push((text.to_string(), thinking.to_string()));
    let a = ask_streamed(&endpoint(server, api), key, &chat(), &mut on);
    (a, pieces)
}

fn said(pieces: &[(String, String)]) -> String {
    pieces.iter().map(|(t, _)| t.as_str()).collect()
}

#[test]
fn an_answer_streams_as_its_written_in_either_format() {
    for api in [AiApi::OpenAi, AiApi::Anthropic] {
        let server = ModelServer::start();
        let (a, pieces) = streamed(&server, api, Some(KEY));
        let a = a.unwrap();
        assert_eq!(a.text, "You said: hello", "{api:?}");
        assert!(pieces.len() >= 3, "a word at a time: {pieces:?}");
        assert_eq!(said(&pieces), a.text);
        assert_eq!((a.input_tokens, a.output_tokens), (Some(12), Some(5)), "{api:?}");
        assert!(a.sent > 0 && a.received > 0);
        let body = &server.seen()[0].body;
        assert_eq!(body["stream"], true);
        if api == AiApi::OpenAi {
            assert_eq!(body["stream_options"]["include_usage"], true, "the tokens counted, at the end");
        }
    }
}

#[test]
fn openais_streamed_calls_and_thinking_are_built_from_their_pieces() {
    let server = ModelServer::start();
    let chunk = |delta: Value| format!("data: {}\n\n", json!({"choices": [{"index": 0, "delta": delta, "finish_reason": null}]}));
    let first = chunk(json!({"reasoning": "Search "}));
    let (a, b) = first.split_at(9);
    server.then_events(&[
        ": a comment, ignored\n\n",
        a,
        b,
        &chunk(json!({"reasoning": "first."})),
        &chunk(json!({"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "search_mail", "arguments": "{\"qu"}}]})),
        &chunk(json!({"tool_calls": [{"index": 0, "function": {"arguments": "ery\":\"caf\u{e9}\"}"}}]})),
        &chunk(json!({"tool_calls": [{"index": 1, "id": "call_2", "function": {"name": "list_folders", "arguments": ""}}]})),
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\r\n\r\n",
        "data: [DONE]\n\n",
    ]);
    let (a, pieces) = streamed(&server, AiApi::OpenAi, None);
    let a = a.unwrap();
    assert_eq!(a.thinking, "Search first.");
    assert_eq!(pieces.iter().map(|(_, th)| th.as_str()).collect::<String>(), "Search first.", "thinking handed on as it came");
    assert_eq!(a.calls.len(), 2);
    assert_eq!((a.calls[0].id.as_str(), a.calls[0].name.as_str()), ("call_1", "search_mail"));
    assert_eq!(a.calls[0].input, json!({"query": "caf\u{e9}"}), "its input joined from its pieces");
    assert_eq!((a.calls[1].name.as_str(), &a.calls[1].input), ("list_folders", &json!({})));
}

/// Ollama sends each call whole, without its place: each is a call of its own.
#[test]
fn calls_sent_whole_without_their_place_are_each_their_own() {
    let server = ModelServer::start();
    let call = |id: &str, name: &str| {
        format!("data: {}\n\n", json!({"choices": [{"delta": {"tool_calls": [{"id": id, "function": {"name": name, "arguments": {"limit": 5}}}]}}]}))
    };
    server.then_events(&[
        &call("call_a", "list_messages"),
        &call("call_b", "list_folders"),
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    ]);
    let (a, _) = streamed(&server, AiApi::OpenAi, None);
    let a = a.unwrap();
    let names: Vec<&str> = a.calls.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["list_messages", "list_folders"]);
    assert_eq!(a.calls[0].input, json!({"limit": 5}), "arguments given as an object");
}

#[test]
fn anthropics_streamed_calls_and_thinking_are_built_from_their_pieces() {
    let server = ModelServer::start();
    let event = |name: &str, v: Value| format!("event: {name}\ndata: {v}\n\n");
    server.then_events(&[
        &event("message_start", json!({"type": "message_start", "message": {"usage": {"input_tokens": 40}}})),
        &event("content_block_start", json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": ""}})),
        &event("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "Look it up."}})),
        &event("content_block_start", json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}})),
        &event("content_block_delta", json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "One moment."}})),
        &event("content_block_start", json!({"type": "content_block_start", "index": 2, "content_block": {"type": "tool_use", "id": "toolu_9", "name": "search_mail", "input": {}}})),
        &event("content_block_delta", json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "{\"query\": "}})),
        &event("content_block_delta", json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "\"invoice\"}"}})),
        // Input for a block that isn't a call: no call made of it.
        &event("content_block_delta", json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{}"}})),
        "event: ping\ndata: {\"type\": \"ping\"}\n\n",
        &event("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 30}})),
        &event("message_stop", json!({"type": "message_stop"})),
    ]);
    let (a, pieces) = streamed(&server, AiApi::Anthropic, Some(KEY));
    let a = a.unwrap();
    assert_eq!((a.text.as_str(), a.thinking.as_str()), ("One moment.", "Look it up."));
    assert_eq!(said(&pieces), "One moment.");
    assert_eq!(a.calls.len(), 1);
    assert_eq!((a.calls[0].id.as_str(), a.calls[0].name.as_str(), &a.calls[0].input), ("toolu_9", "search_mail", &json!({"query": "invoice"})));
    assert_eq!((a.input_tokens, a.output_tokens), (Some(40), Some(30)));
    assert!(!a.cut);
}

#[test]
fn a_stream_that_goes_wrong_says_why_without_the_key() {
    let server = ModelServer::start();
    // An error in the stream, quoting the key.
    let error = json!({"type": "error", "error": {"type": "overloaded_error", "message": format!("overloaded for {KEY}")}});
    server.then_events(&["event: message_start\ndata: {\"type\":\"message_start\"}\n\n", &format!("event: error\ndata: {error}\n\n")]);
    let e = streamed(&server, AiApi::Anthropic, Some(KEY)).0.unwrap_err().to_string();
    assert!(e.contains("stopped") && e.contains("overloaded") && !e.contains(KEY), "{e}");
    server.then_events(&["data: {\"error\": {\"message\": \"out of memory\"}}\n\n"]);
    let e = streamed(&server, AiApi::OpenAi, None).0.unwrap_err().to_string();
    assert!(e.contains("out of memory"), "{e}");
    // Cut short: no end said.
    server.then_events(&["data: {\"choices\":[{\"delta\":{\"content\":\"Half\"}}]}\n\n"]);
    let (r, pieces) = streamed(&server, AiApi::OpenAi, None);
    assert!(r.unwrap_err().to_string().contains("stopped before its end"));
    assert_eq!(said(&pieces), "Half", "what came was still handed on");
    // Not JSON.
    server.then_events(&["data: {not json\n\n"]);
    assert!(streamed(&server, AiApi::OpenAi, None).0.unwrap_err().to_string().contains("other than JSON"));
    // An event without end, longer than any is read.
    server.then_events(&[&format!("data: {}", "a".repeat(3 << 20))]);
    assert!(streamed(&server, AiApi::OpenAi, None).0.unwrap_err().to_string().contains("event longer than"));
    // A refusal.
    server.then_events(&["data: {\"choices\":[{\"delta\":{\"refusal\":\"No.\"}}]}\n\n", "data: [DONE]\n\n"]);
    assert!(streamed(&server, AiApi::OpenAi, None).0.unwrap_err().to_string().contains("declined"));
}

#[test]
fn a_server_that_answers_whole_or_refuses_is_read_as_a_whole_answer() {
    let server = ModelServer::start();
    server.then("200 OK", &json!({"choices": [{"message": {"content": "All at once"}, "finish_reason": "stop"}]}).to_string());
    let (a, pieces) = streamed(&server, AiApi::OpenAi, None);
    assert_eq!(a.unwrap().text, "All at once");
    assert!(pieces.is_empty());
    server.then("401 Unauthorized", &json!({"error": {"message": format!("bad key {KEY}")}}).to_string());
    let e = streamed(&server, AiApi::OpenAi, Some(KEY)).0.unwrap_err().to_string();
    assert!(e.contains("the key was refused") && !e.contains(KEY), "{e}");
}

#[test]
fn a_cut_stream_says_so() {
    let server = ModelServer::start();
    server.then_events(&["data: {\"choices\":[{\"delta\":{\"content\":\"Long\"},\"finish_reason\":\"length\"}]}\n\n"]);
    let a = streamed(&server, AiApi::OpenAi, None).0.unwrap();
    assert!(a.cut && a.text == "Long");
    let event = |name: &str, v: Value| format!("event: {name}\ndata: {v}\n\n");
    server.then_events(&[
        &event("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Long"}})),
        &event("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "max_tokens"}})),
        // The last event without the blank line after it.
        "event: message_stop\ndata: {\"type\":\"message_stop\"}",
    ]);
    let a = streamed(&server, AiApi::Anthropic, None).0.unwrap();
    assert!(a.cut && a.text == "Long");
}
