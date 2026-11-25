//! Ollama's own chat API (`/api/chat`): as OpenAI's format, but told how much the model takes in
//! (`num_ctx`), which Ollama's OpenAI-compatible address can't be. There, the server keeps to its
//! own context (4096 tokens in some versions, unless it's set), and a prompt longer than that
//! can lose its start without a word, the instructions and the tools with it; here it's told the
//! profile's, and asked to refuse a longer prompt rather than cut it. Its stream is a JSON
//! object a line.

use std::collections::HashMap;

use super::streamed::Building;
use super::*;
use mailcraft_model::{AiRole, AiToolCall, AiTurn};
use serde_json::json;

pub(super) fn request(base: &str, endpoint: &AiEndpoint, key: Option<&str>, chat: &AiChat, stream: bool) -> Request {
    let system = (!chat.system.is_empty()).then(|| json!({"role": "system", "content": chat.system}));
    // A tool's result is told by the tool's name: the call it answers names it.
    let names: HashMap<&str, &str> = chat.turns.iter().flat_map(|t| &t.calls).map(|c| (c.id.as_str(), c.name.as_str())).collect();
    let messages: Vec<Value> = system.into_iter().chain(chat.turns.iter().flat_map(|t| messages(t, &names))).collect();
    let mut options = json!({"num_predict": chat.max_tokens});
    if chat.context > 0 {
        options["num_ctx"] = json!(chat.context);
    }
    if let Some(t) = temperature(chat) {
        options["temperature"] = t;
    }
    // Longer than that, the question is to be refused rather than its start dropped (`truncate`)
    // or its middle (`shift`), and the answer to stop where the context ends.
    let mut body = json!({"model": endpoint.model, "messages": messages, "stream": stream, "options": options, "truncate": false, "shift": false});
    if !chat.tools.is_empty() {
        let tools: Vec<Value> = chat
            .tools
            .iter()
            .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.schema}}))
            .collect();
        body["tools"] = Value::Array(tools);
    }
    // Off is `false`; a level, for a model that has levels (one that hasn't thinks as it does).
    match chat.effort {
        Some(AiEffort::Off) => body["think"] = Value::Bool(false),
        Some(e) => body["think"] = json!(e.code()),
        None => {}
    }
    let bearer = key.map(|k| format!("Bearer {k}"));
    let headers: Vec<(&str, &str)> = bearer.iter().map(|b| ("Authorization", b.as_str())).collect();
    post(format!("{}/api/chat", root(base)), &headers, &body)
}

/// The server's own address: an OpenAI-compatible one (`…/v1`) without its version.
fn root(base: &str) -> &str {
    base.strip_suffix("/v1").unwrap_or(base)
}

/// A turn's messages: a user's tool results (a message each, `tool`, with the tool's name) before
/// what they said; an assistant's words with the tools it called.
fn messages(t: &AiTurn, names: &HashMap<&str, &str>) -> Vec<Value> {
    match t.role {
        AiRole::User => {
            let results = t.results.iter().map(|r| {
                let name = names.get(r.id.as_str()).copied().unwrap_or_default();
                json!({"role": "tool", "content": r.content, "tool_name": name, "tool_call_id": r.id})
            });
            let said = (!t.text.is_empty() || t.results.is_empty() || !t.images.is_empty()).then(|| {
                let mut m = json!({"role": "user", "content": t.text});
                if !t.images.is_empty() {
                    m["images"] = t.images.iter().map(|i| Value::String(i.base64.clone())).collect();
                }
                m
            });
            results.chain(said).collect()
        }
        AiRole::Assistant if t.calls.is_empty() => vec![json!({"role": "assistant", "content": t.text})],
        AiRole::Assistant => {
            let calls: Vec<Value> = t.calls.iter().map(|c| json!({"id": c.id, "function": {"name": c.name, "arguments": arguments(c)}})).collect();
            vec![json!({"role": "assistant", "content": t.text, "tool_calls": calls})]
        }
    }
}

/// A call's input as this format sends it: a JSON object. One that wasn't (its text cut short,
/// broken) goes back empty, as the other formats send it.
fn arguments(c: &AiToolCall) -> Value {
    if c.input.is_object() { c.input.clone() } else { json!({}) }
}

/// A whole answer: its text, thinking and tool calls, or why there's none.
pub(super) fn answer(body: &Value, key: Option<&str>) -> Result<AiAnswer> {
    let mut b = Building::default();
    event(&mut b, body, key);
    if !b.done && b.refused.is_none() {
        return Err(NetError::Protocol("the model's server sent no answer".into()));
    }
    b.finish()
}

/// One line of the stream (or the whole answer) built into the answer: what more was said and
/// thought, the tools it calls (each sent whole), and at its end the tokens counted; or the
/// error it stopped with, [`quoted`].
pub(super) fn event(b: &mut Building, v: &Value, key: Option<&str>) {
    if let Some(e) = v.get("error") {
        let said = e.get("message").and_then(Value::as_str).or_else(|| e.as_str()).unwrap_or_default();
        b.refused = Some(stopped(said, key));
        return;
    }
    let message = v.get("message").unwrap_or(&Value::Null);
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        b.text.push_str(text);
    }
    if let Some(thought) = message.get("thinking").and_then(Value::as_str) {
        b.thinking.push_str(thought);
    }
    for c in message.get("tool_calls").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
        let Some(p) = b.call(u64::try_from(b.calls.len()).unwrap_or(u64::MAX)) else { break };
        let function = c.get("function").unwrap_or(&Value::Null);
        p.id = c.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        p.name = function.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        p.input = match function.get("arguments") {
            Some(Value::String(s)) => s.clone(),
            Some(a @ Value::Object(_)) => a.to_string(),
            _ => String::new(),
        };
    }
    if v.get("done").and_then(Value::as_bool) == Some(true) {
        b.done = true;
        b.cut |= v.get("done_reason").and_then(Value::as_str) == Some("length");
        b.input_tokens = count(v, "prompt_eval_count").or(b.input_tokens);
        b.output_tokens = count(v, "eval_count").or(b.output_tokens);
    }
}
