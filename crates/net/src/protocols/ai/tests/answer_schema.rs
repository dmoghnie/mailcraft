//! An answer kept to a JSON Schema (a rule written from the person's words, read straight into
//! the rule MailCraft runs), asked of each server as its structured output takes it; none asked,
//! nothing is said and the answer is free text.

use super::*;
use mailcraft_model::AiEffort;

fn schema() -> Value {
    json!({"type": "object", "additionalProperties": false, "required": ["name"], "properties": {"name": {"type": "string"}}})
}

fn kept() -> AiChat {
    AiChat { answer_schema: Some(schema()), ..chat() }
}

#[test]
fn openais_format_keeps_the_answer_to_the_schema_strictly() {
    let b = sent(AiApi::OpenAi, &kept());
    assert_eq!(b["response_format"], json!({"type": "json_schema", "json_schema": {"name": "answer", "strict": true, "schema": schema()}}), "{b}");
    assert!(sent(AiApi::OpenAi, &chat()).get("response_format").is_none());
}

#[test]
fn ollamas_own_format_is_given_the_schema() {
    let b = sent(AiApi::Ollama, &kept());
    assert_eq!(b["format"], schema(), "{b}");
    assert!(sent(AiApi::Ollama, &chat()).get("format").is_none());
}

/// Anthropic's takes it in `output_config`, beside how much the model thinks.
#[test]
fn anthropics_format_takes_the_schema_beside_the_effort() {
    let format = json!({"type": "json_schema", "schema": schema()});
    let b = sent(AiApi::Anthropic, &kept());
    assert_eq!(b["output_config"], json!({ "format": format }), "{b}");
    let b = sent(AiApi::Anthropic, &AiChat { effort: Some(AiEffort::High), ..kept() });
    assert_eq!(b["output_config"], json!({"effort": "high", "format": format}), "{b}");
    assert!(sent(AiApi::Anthropic, &chat()).get("output_config").is_none());
}
