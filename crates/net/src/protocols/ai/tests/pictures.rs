//! Pictures sent with a question, each server told of them in its own words: OpenAI's format as
//! parts of the content, Anthropic's as image blocks before the text, Ollama's as a list beside
//! it; a question without pictures is sent as text, as before.

use super::*;
use mailcraft_model::AiImage;

const PNG: &str = "iVBORw0KGgo=";

/// The request `api`'s server saw for `chat`.
fn sent(api: AiApi, chat: &AiChat) -> Value {
    let server = ModelServer::start();
    let _ = ask(&endpoint(&server, api), None, chat);
    server.seen().first().map(|r| r.body.clone()).unwrap_or_default()
}

fn with_picture() -> AiChat {
    let mut turn = AiTurn::user("What does it show?");
    turn.images = vec![AiImage { media_type: "image/png".into(), base64: PNG.into() }];
    AiChat { turns: vec![turn], ..chat() }
}

/// The last message the server was sent.
fn last(b: &Value) -> Value {
    b["messages"].as_array().and_then(|m| m.last()).cloned().unwrap_or_default()
}

#[test]
fn openais_format_sends_a_picture_as_a_part_of_the_content() {
    let m = last(&sent(AiApi::OpenAi, &with_picture()));
    assert_eq!(
        m["content"],
        json!([{"type": "text", "text": "What does it show?"}, {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{PNG}")}}]),
        "{m}"
    );
    assert_eq!(last(&sent(AiApi::OpenAi, &chat()))["content"], "hello", "text alone, as before");
}

#[test]
fn anthropics_format_sends_a_picture_as_a_block_before_the_text() {
    let m = last(&sent(AiApi::Anthropic, &with_picture()));
    assert_eq!(
        m["content"],
        json!([{"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": PNG}}, {"type": "text", "text": "What does it show?"}]),
        "{m}"
    );
    assert_eq!(last(&sent(AiApi::Anthropic, &chat()))["content"], "hello", "text alone, as before");
}

#[test]
fn ollamas_own_format_sends_pictures_beside_the_text() {
    let m = last(&sent(AiApi::Ollama, &with_picture()));
    assert_eq!((&m["content"], &m["images"]), (&json!("What does it show?"), &json!([PNG])), "{m}");
    assert!(last(&sent(AiApi::Ollama, &chat())).get("images").is_none(), "none, as before");
}
