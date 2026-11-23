//! OpenAI's Chat Completions, as most model servers speak it.

use super::streamed::{Building, input_of};
use super::*;
use mailcraft_model::{AiRole, AiToolCall, AiTurn};
use serde_json::json;

/// The host whose models take `max_completion_tokens` and refuse `max_tokens` (its reasoning
/// models); the other servers speaking this format know `max_tokens`.
const OPENAI: &str = "api.openai.com";

pub(super) fn request(base: &str, endpoint: &AiEndpoint, key: Option<&str>, chat: &AiChat, stream: bool) -> Request {
    let system = (!chat.system.is_empty()).then(|| json!({"role": "system", "content": chat.system}));
    let messages: Vec<Value> = system.into_iter().chain(chat.turns.iter().flat_map(messages)).collect();
    let limit = if Url::parse(base).is_ok_and(|u| u.host == OPENAI) { "max_completion_tokens" } else { "max_tokens" };
    let mut body = json!({"model": endpoint.model, "messages": messages, limit: chat.max_tokens, "stream": stream});
    if stream {
        // The tokens counted, in a last event of their own.
        body["stream_options"] = json!({"include_usage": true});
    }
    if !chat.tools.is_empty() {
        let tools: Vec<Value> = chat
            .tools
            .iter()
            .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.schema}}))
            .collect();
        body["tools"] = Value::Array(tools);
    }
    let bearer = key.map(|k| format!("Bearer {k}"));
    let headers: Vec<(&str, &str)> = bearer.iter().map(|b| ("Authorization", b.as_str())).collect();
    post(format!("{base}/chat/completions"), &headers, &body)
}

/// A turn's messages: a user's tool results (a message each, `tool`) before what they said; an
/// assistant's words with the tools it called.
fn messages(t: &AiTurn) -> Vec<Value> {
    match t.role {
        AiRole::User => {
            let results = t.results.iter().map(|r| json!({"role": "tool", "tool_call_id": r.id, "content": r.content}));
            let said = (!t.text.is_empty() || t.results.is_empty()).then(|| json!({"role": "user", "content": t.text}));
            results.chain(said).collect()
        }
        AiRole::Assistant if t.calls.is_empty() => vec![json!({"role": "assistant", "content": t.text})],
        AiRole::Assistant => {
            let calls: Vec<Value> =
                t.calls.iter().map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": arguments(c)}})).collect();
            let content = if t.text.is_empty() { Value::Null } else { Value::String(t.text.clone()) };
            vec![json!({"role": "assistant", "content": content, "tool_calls": calls})]
        }
    }
}

/// A call's input as this format sends it: JSON in a string (the model's own text, when it
/// wasn't JSON).
fn arguments(c: &AiToolCall) -> String {
    match &c.input {
        Value::String(s) => s.clone(),
        v => v.to_string(),
    }
}

/// The first choice's text, thinking and tool calls: its text a string, or its text parts
/// joined. A refusal is an error, its words [`quoted`].
pub(super) fn answer(body: &Value, key: Option<&str>) -> Result<AiAnswer> {
    let choice = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .ok_or_else(|| NetError::Protocol("the model's server sent no answer".into()))?;
    let message = choice.get("message").unwrap_or(&Value::Null);
    if let Some(no) = message.get("refusal").and_then(Value::as_str).filter(|r| !r.trim().is_empty()) {
        return Err(NetError::No(format!("the model declined: {}", quoted(no, key))));
    }
    let text = match message.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(""),
        _ => String::new(),
    };
    let calls = message.get("tool_calls").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let calls = calls.iter().take(super::streamed::MAX_CALLS).enumerate().map(|(n, c)| {
        let function = c.get("function").unwrap_or(&Value::Null);
        let id = c.get("id").and_then(Value::as_str).filter(|id| !id.is_empty()).map_or_else(|| format!("call_{n}"), str::to_string);
        let input = match function.get("arguments") {
            Some(Value::String(s)) => input_of(s),
            Some(v @ Value::Object(_)) => v.clone(),
            _ => json!({}),
        };
        AiToolCall { id, name: function.get("name").and_then(Value::as_str).unwrap_or_default().to_string(), input }
    });
    let usage = body.get("usage").unwrap_or(&Value::Null);
    Ok(AiAnswer {
        text,
        calls: calls.collect(),
        thinking: thinking(message).unwrap_or_default().to_string(),
        cut: choice.get("finish_reason").and_then(Value::as_str) == Some("length"),
        input_tokens: count(usage, "prompt_tokens"),
        output_tokens: count(usage, "completion_tokens"),
        ..AiAnswer::default()
    })
}

/// What a model thought, as the servers that send it apart name it: Ollama's `reasoning`, and
/// `reasoning_content` (DeepSeek, vLLM, llama.cpp).
fn thinking(v: &Value) -> Option<&str> {
    v.get("reasoning").or_else(|| v.get("reasoning_content")).and_then(Value::as_str)
}

/// One streamed event (a chunk, an error, or the tokens counted) built into the answer.
pub(super) fn event(b: &mut Building, v: &Value, key: Option<&str>) {
    if let Some(e) = v.get("error") {
        let said = e.get("message").and_then(Value::as_str).or_else(|| e.as_str()).unwrap_or_default();
        b.refused = Some(stopped(said, key));
        return;
    }
    if let Some(usage) = v.get("usage").filter(|u| u.is_object()) {
        b.input_tokens = count(usage, "prompt_tokens").or(b.input_tokens);
        b.output_tokens = count(usage, "completion_tokens").or(b.output_tokens);
    }
    let Some(choice) = v.get("choices").and_then(Value::as_array).and_then(|c| c.first()) else { return };
    let delta = choice.get("delta").unwrap_or(&Value::Null);
    if let Some(no) = delta.get("refusal").and_then(Value::as_str).filter(|r| !r.trim().is_empty()) {
        b.refused = Some(format!("the model declined: {}", quoted(no, key)));
        return;
    }
    if let Some(text) = delta.get("content").and_then(Value::as_str) {
        b.text.push_str(text);
    }
    if let Some(thought) = thinking(delta) {
        b.thinking.push_str(thought);
    }
    for (n, c) in delta.get("tool_calls").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default().iter().enumerate() {
        tool_call(b, n, c);
    }
    match choice.get("finish_reason").and_then(Value::as_str) {
        Some("length") => (b.cut, b.done) = (true, true),
        Some(_) => b.done = true,
        None => {}
    }
}

/// A piece of a tool call: by its `index`; a server that sends each call whole, without one
/// (Ollama), by its id, or after the calls before it.
fn tool_call(b: &mut Building, n: usize, c: &Value) {
    let id = c.get("id").and_then(Value::as_str).filter(|id| !id.is_empty());
    let index = match c.get("index").and_then(Value::as_u64) {
        Some(i) => i,
        None => match id.and_then(|id| b.calls.iter().find(|p| p.id == id)) {
            Some(p) => p.index,
            None => b.calls.iter().map(|p| p.index.saturating_add(1)).max().unwrap_or(0).saturating_add(u64::try_from(n).unwrap_or(0)),
        },
    };
    let Some(p) = b.call(index) else { return };
    if let Some(id) = id {
        p.id = id.to_string();
    }
    let function = c.get("function").unwrap_or(&Value::Null);
    if let Some(name) = function.get("name").and_then(Value::as_str) {
        p.name.push_str(name);
    }
    match function.get("arguments") {
        Some(Value::String(s)) => p.input.push_str(s),
        Some(v @ Value::Object(_)) => p.input.push_str(&v.to_string()),
        _ => {}
    }
}
