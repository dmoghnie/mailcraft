//! Anthropic's Messages.

use super::streamed::Building;
use super::*;
use mailcraft_model::{AiRole, AiToolCall, AiTurn};
use serde_json::json;

/// The API version asked for. Since this one, no model takes `temperature`, which MailCraft
/// doesn't send.
const VERSION: &str = "2026-10-21";
/// What a turn that said nothing is sent as: the API takes no empty content but the last.
const NO_TEXT: &str = "(no answer)";

pub(super) fn request(base: &str, endpoint: &AiEndpoint, key: Option<&str>, chat: &AiChat, stream: bool) -> Request {
    let messages: Vec<Value> = chat.turns.iter().map(message).collect();
    let mut body = json!({"model": endpoint.model, "max_tokens": chat.max_tokens, "messages": messages});
    if stream {
        body["stream"] = Value::Bool(true);
    }
    if !chat.system.is_empty() {
        body["system"] = Value::String(chat.system.clone());
    }
    if !chat.tools.is_empty() {
        let tools: Vec<Value> = chat.tools.iter().map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.schema})).collect();
        body["tools"] = Value::Array(tools);
    }
    // Its current models take no temperature: one set is for an older model that does.
    if let Some(t) = temperature(chat) {
        body["temperature"] = t;
    }
    // Off turns thinking off; a level is the effort the model puts into its answer.
    match chat.effort {
        Some(AiEffort::Off) => body["thinking"] = json!({"type": "disabled"}),
        Some(e) => body["output_config"] = json!({"effort": e.code()}),
        None => {}
    }
    // An answer kept to a schema, beside the effort.
    if let Some(schema) = &chat.answer_schema {
        body["output_config"]["format"] = json!({"type": "json_schema", "schema": schema});
    }
    let headers: Vec<(&str, &str)> = key.map(|k| ("x-api-key", k)).into_iter().chain([("anthropic-version", VERSION)]).collect();
    post(format!("{base}/messages"), &headers, &body)
}

/// A turn as a message: its text alone, or blocks: a user's tool results and pictures before what
/// they said, an assistant's words before the tools it called.
fn message(t: &AiTurn) -> Value {
    let role = match t.role {
        AiRole::User => "user",
        AiRole::Assistant => "assistant",
    };
    // No content may be empty or blank, but the last: an answer that came with none (an empty
    // end of turn, thinking cut at the longest answer) is said to have been so.
    if t.calls.is_empty() && t.results.is_empty() && t.images.is_empty() {
        let text = if t.text.trim().is_empty() { NO_TEXT } else { t.text.as_str() };
        return json!({"role": role, "content": text});
    }
    let text = (!t.text.trim().is_empty()).then(|| json!({"type": "text", "text": t.text}));
    let blocks: Vec<Value> = match t.role {
        AiRole::User => t
            .results
            .iter()
            .map(|r| {
                let mut b = json!({"type": "tool_result", "tool_use_id": r.id, "content": r.content});
                if r.error {
                    b["is_error"] = Value::Bool(true);
                }
                b
            })
            .chain(t.images.iter().map(|i| json!({"type": "image", "source": {"type": "base64", "media_type": i.media_type, "data": i.base64}})))
            .chain(text)
            .collect(),
        AiRole::Assistant => text.into_iter().chain(t.calls.iter().map(tool_use)).collect(),
    };
    json!({"role": role, "content": blocks})
}

/// A call as the model made it: its input an object (one that wasn't JSON goes back empty).
fn tool_use(c: &AiToolCall) -> Value {
    let input = if c.input.is_object() { c.input.clone() } else { json!({}) };
    json!({"type": "tool_use", "id": c.id, "name": c.name, "input": input})
}

/// Its text blocks joined, its thinking apart, and the tools it called.
pub(super) fn answer(body: &Value) -> Result<AiAnswer> {
    let blocks = body.get("content").and_then(Value::as_array).ok_or_else(|| NetError::Protocol("the model's server sent no answer".into()))?;
    let stop = body.get("stop_reason").and_then(Value::as_str);
    if stop == Some("refusal") {
        return Err(NetError::No("the model declined to answer".into()));
    }
    let of = |kind: &str, field: &str| {
        blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some(kind))
            .filter_map(|b| b.get(field).and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("")
    };
    let calls = blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use")).take(super::streamed::MAX_CALLS);
    let calls = calls.enumerate().map(|(n, b)| AiToolCall {
        id: b.get("id").and_then(Value::as_str).filter(|id| !id.is_empty()).map_or_else(|| format!("call_{n}"), str::to_string),
        name: b.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
        input: b.get("input").filter(|i| i.is_object()).cloned().unwrap_or_else(|| json!({})),
    });
    let usage = body.get("usage").unwrap_or(&Value::Null);
    Ok(AiAnswer {
        text: of("text", "text"),
        calls: calls.collect(),
        thinking: of("thinking", "thinking"),
        cut: stop == Some("max_tokens"),
        input_tokens: count(usage, "input_tokens"),
        output_tokens: count(usage, "output_tokens"),
        ..AiAnswer::default()
    })
}

/// One streamed event, by its type (`message_start`, `content_block_start`, `…_delta`,
/// `message_delta`, `message_stop`, `error`), built into the answer.
pub(super) fn event(b: &mut Building, name: &str, v: &Value, key: Option<&str>) {
    let kind = v.get("type").and_then(Value::as_str).unwrap_or(name);
    let index = v.get("index").and_then(Value::as_u64).unwrap_or(0);
    match kind {
        "message_start" => {
            let usage = v.get("message").and_then(|m| m.get("usage")).unwrap_or(&Value::Null);
            b.input_tokens = count(usage, "input_tokens").or(b.input_tokens);
        }
        "content_block_start" => {
            let block = v.get("content_block").unwrap_or(&Value::Null);
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    if let Some(p) = b.call(index) {
                        p.id = block.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                        p.name = block.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
                    }
                }
                Some("text") => b.text.push_str(block.get("text").and_then(Value::as_str).unwrap_or_default()),
                _ => {}
            }
        }
        "content_block_delta" => {
            let delta = v.get("delta").unwrap_or(&Value::Null);
            let piece = |field: &str| delta.get(field).and_then(Value::as_str).unwrap_or_default();
            match delta.get("type").and_then(Value::as_str) {
                Some("text_delta") => b.text.push_str(piece("text")),
                Some("thinking_delta") => b.thinking.push_str(piece("thinking")),
                Some("input_json_delta") => {
                    // A call's input, for a block started as one alone.
                    if b.calls.iter().any(|c| c.index == index)
                        && let Some(p) = b.call(index)
                    {
                        p.input.push_str(piece("partial_json"));
                    }
                }
                _ => {}
            }
        }
        "message_delta" => {
            match v.get("delta").and_then(|d| d.get("stop_reason")).and_then(Value::as_str) {
                Some("max_tokens") => b.cut = true,
                Some("refusal") => b.refused = Some("the model declined to answer".into()),
                _ => {}
            }
            b.output_tokens = v.get("usage").and_then(|u| count(u, "output_tokens")).or(b.output_tokens);
        }
        "message_stop" => b.done = true,
        "error" => {
            let said = v.get("error").and_then(|e| e.get("message")).and_then(Value::as_str).unwrap_or_default();
            b.refused = Some(stopped(said, key));
        }
        _ => {}
    }
}
