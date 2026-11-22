//! Anthropic's Messages.

use super::*;
use mailcraft_model::AiRole;
use serde_json::json;

/// The API version asked for. Since this one, no model takes `temperature`, which MailCraft
/// doesn't send.
const VERSION: &str = "2026-10-21";

pub(super) fn request(base: &str, endpoint: &AiEndpoint, key: Option<&str>, chat: &AiChat) -> Request {
    let messages: Vec<Value> = chat.turns.iter().map(|t| json!({"role": role(t.role), "content": t.text})).collect();
    let mut body = json!({"model": endpoint.model, "max_tokens": chat.max_tokens, "messages": messages});
    if !chat.system.is_empty() {
        body["system"] = Value::String(chat.system.clone());
    }
    let headers: Vec<(&str, &str)> = key.map(|k| ("x-api-key", k)).into_iter().chain([("anthropic-version", VERSION)]).collect();
    post(format!("{base}/messages"), &headers, &body)
}

fn role(r: AiRole) -> &'static str {
    match r {
        AiRole::User => "user",
        AiRole::Assistant => "assistant",
    }
}

/// Its text blocks joined (its thinking blocks, when it thought, left out).
pub(super) fn answer(body: &Value) -> Result<AiAnswer> {
    let blocks = body.get("content").and_then(Value::as_array).ok_or_else(|| NetError::Protocol("the model's server sent no answer".into()))?;
    let stop = body.get("stop_reason").and_then(Value::as_str);
    if stop == Some("refusal") {
        return Err(NetError::No("the model declined to answer".into()));
    }
    let text = blocks
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("");
    let usage = body.get("usage").unwrap_or(&Value::Null);
    Ok(AiAnswer {
        text,
        cut: stop == Some("max_tokens"),
        input_tokens: count(usage, "input_tokens"),
        output_tokens: count(usage, "output_tokens"),
        ..AiAnswer::default()
    })
}
