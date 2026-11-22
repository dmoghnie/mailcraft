//! OpenAI's Chat Completions, as most model servers speak it.

use super::*;
use mailcraft_model::AiRole;
use serde_json::json;

/// The host whose models take `max_completion_tokens` and refuse `max_tokens` (its reasoning
/// models); the other servers speaking this format know `max_tokens`.
const OPENAI: &str = "api.openai.com";

pub(super) fn request(base: &str, endpoint: &AiEndpoint, key: Option<&str>, chat: &AiChat) -> Request {
    let system = (!chat.system.is_empty()).then(|| json!({"role": "system", "content": chat.system}));
    let turns = chat.turns.iter().map(|t| json!({"role": role(t.role), "content": t.text}));
    let messages: Vec<Value> = system.into_iter().chain(turns).collect();
    let limit = if Url::parse(base).is_ok_and(|u| u.host == OPENAI) { "max_completion_tokens" } else { "max_tokens" };
    let body = json!({"model": endpoint.model, "messages": messages, limit: chat.max_tokens, "stream": false});
    let bearer = key.map(|k| format!("Bearer {k}"));
    let headers: Vec<(&str, &str)> = bearer.iter().map(|b| ("Authorization", b.as_str())).collect();
    post(format!("{base}/chat/completions"), &headers, &body)
}

fn role(r: AiRole) -> &'static str {
    match r {
        AiRole::User => "user",
        AiRole::Assistant => "assistant",
    }
}

/// The first choice's text: a string, or its text parts joined. A refusal is an error, its
/// words [`quoted`].
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
    let usage = body.get("usage").unwrap_or(&Value::Null);
    Ok(AiAnswer {
        text,
        cut: choice.get("finish_reason").and_then(Value::as_str) == Some("length"),
        input_tokens: count(usage, "prompt_tokens"),
        output_tokens: count(usage, "completion_tokens"),
        ..AiAnswer::default()
    })
}
