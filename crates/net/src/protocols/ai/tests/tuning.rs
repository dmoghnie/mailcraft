//! A profile's temperature and how much its model thinks, told each server in its own words;
//! neither set, nothing is said and the model does as it does.

use super::*;
use mailcraft_model::AiEffort;

fn tuned(effort: AiEffort) -> AiChat {
    AiChat { temperature: Some(70), effort: Some(effort), ..chat() }
}

#[test]
fn openais_format_takes_the_temperature_and_reasoning_effort() {
    let b = sent(AiApi::OpenAi, &tuned(AiEffort::High));
    assert_eq!((&b["temperature"], &b["reasoning_effort"]), (&json!(0.7), &json!("high")), "{b}");
    assert_eq!(sent(AiApi::OpenAi, &tuned(AiEffort::Off))["reasoning_effort"], "none");
    let plain = sent(AiApi::OpenAi, &chat());
    assert!(plain.get("temperature").is_none() && plain.get("reasoning_effort").is_none(), "{plain}");
}

#[test]
fn anthropics_format_takes_the_effort_or_thinking_off() {
    let b = sent(AiApi::Anthropic, &tuned(AiEffort::Medium));
    assert_eq!((&b["temperature"], &b["output_config"]), (&json!(0.7), &json!({"effort": "medium"})), "{b}");
    assert!(b.get("thinking").is_none(), "{b}");
    let off = sent(AiApi::Anthropic, &tuned(AiEffort::Off));
    assert_eq!(off["thinking"], json!({"type": "disabled"}), "{off}");
    assert!(off.get("output_config").is_none(), "{off}");
    let plain = sent(AiApi::Anthropic, &chat());
    assert!(["temperature", "thinking", "output_config"].iter().all(|k| plain.get(k).is_none()), "{plain}");
}

#[test]
fn ollamas_own_format_takes_the_temperature_and_think() {
    let b = sent(AiApi::Ollama, &tuned(AiEffort::Low));
    assert_eq!((&b["options"]["temperature"], &b["think"]), (&json!(0.7), &json!("low")), "{b}");
    assert_eq!(sent(AiApi::Ollama, &tuned(AiEffort::Off))["think"], false);
    let plain = sent(AiApi::Ollama, &chat());
    assert!(plain.get("think").is_none() && plain["options"].get("temperature").is_none(), "{plain}");
}
