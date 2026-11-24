//! A model server of the tests' own, so MailCraft's AI is tested without a network: it speaks
//! OpenAI's Chat Completions (`/v1/chat/completions`), Anthropic's Messages (`/v1/messages`) and
//! Ollama's own (`/api/chat`), keeps every request it was sent, and gives the answers a test
//! scripted, in order: whole, or streamed as events or JSON lines. With none scripted, it
//! answers in the request's own format with "You said: " and the last turn, streamed when the
//! request asks.

use std::collections::VecDeque;
use std::io::BufReader;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

/// A request the server was sent.
#[derive(Clone, Debug)]
pub(crate) struct Seen {
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// An answer to give: a status line (`200 OK`) and a body, whole or as events in pieces (sent
/// a while apart).
enum Answer {
    Whole(String, String),
    Events(Vec<String>, Duration),
    Lines(Vec<String>),
}

#[derive(Default)]
struct Shared {
    seen: Vec<Seen>,
    /// The answers to give next.
    script: VecDeque<Answer>,
}

pub(crate) struct ModelServer {
    /// The API address, up to its version: `http://127.0.0.1:<port>/v1`.
    pub base: String,
    shared: Arc<Mutex<Shared>>,
}

impl ModelServer {
    pub fn start() -> ModelServer {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}/v1", l.local_addr().unwrap().port());
        let shared = Arc::new(Mutex::new(Shared::default()));
        let state = shared.clone();
        std::thread::spawn(move || {
            for conn in l.incoming() {
                let Ok(mut s) = conn else { continue };
                let Ok(reading) = s.try_clone() else { continue };
                let Some(r) = super::http::read_request(&mut BufReader::new(reading)) else { continue };
                let body: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
                let seen = Seen { target: r.target.clone(), headers: r.headers.clone(), body };
                let answer = {
                    let mut st = state.lock().unwrap();
                    st.seen.push(seen.clone());
                    st.script.pop_front().unwrap_or_else(|| echo(&seen))
                };
                match answer {
                    Answer::Whole(status, body) => {
                        let headers = [("Content-Type", "application/json".to_string()), ("Connection", "close".into())];
                        super::http::respond(&mut s, &status, &headers, body.as_bytes());
                    }
                    Answer::Events(pieces, gap) => {
                        let headers = [("Content-Type", "text/event-stream".to_string()), ("Connection", "close".into())];
                        super::http::respond_chunked(&mut s, "200 OK", &headers, &pieces, gap);
                    }
                    Answer::Lines(pieces) => {
                        let headers = [("Content-Type", "application/x-ndjson".to_string()), ("Connection", "close".into())];
                        super::http::respond_chunked(&mut s, "200 OK", &headers, &pieces, Duration::ZERO);
                    }
                }
            }
        });
        ModelServer { base, shared }
    }

    /// Give `body` with `status` (`429 Too Many Requests`) to the next request.
    pub fn then(&self, status: &str, body: &str) {
        self.shared.lock().unwrap().script.push_back(Answer::Whole(status.into(), body.into()));
    }

    /// Stream `pieces` to the next request, each a chunk of its own (events, split anywhere).
    pub fn then_events(&self, pieces: &[&str]) {
        self.then_events_every(pieces, Duration::ZERO);
    }

    /// Stream `pieces` as [`ModelServer::then_events`] does, `gap` apart: a model writing.
    pub fn then_events_every(&self, pieces: &[&str], gap: Duration) {
        self.shared.lock().unwrap().script.push_back(Answer::Events(pieces.iter().map(|p| p.to_string()).collect(), gap));
    }

    /// Stream `pieces` to the next request as JSON lines (Ollama's), each a chunk of its own.
    pub fn then_lines(&self, pieces: &[&str]) {
        self.shared.lock().unwrap().script.push_back(Answer::Lines(pieces.iter().map(|p| p.to_string()).collect()));
    }

    /// Every request sent so far, in order.
    pub fn seen(&self) -> Vec<Seen> {
        self.shared.lock().unwrap().seen.clone()
    }
}

/// "You said: " and the request's last turn, in its own format: streamed, a word an event.
fn echo(r: &Seen) -> Answer {
    let last = r.body.get("messages").and_then(Value::as_array).and_then(|m| m.last());
    let said = last.and_then(|m| m.get("content")).and_then(Value::as_str).unwrap_or_default();
    let text = format!("You said: {said}");
    if r.target.ends_with("/api/chat") {
        return ollama(r.body.get("stream") != Some(&Value::Bool(false)), &text);
    }
    let anthropic = r.target.ends_with("/messages");
    if r.body.get("stream") == Some(&Value::Bool(true)) {
        return Answer::Events(stream(anthropic, &text), Duration::ZERO);
    }
    Answer::Whole("200 OK".into(), whole(anthropic, &text))
}

/// `text` streamed as Anthropic's events, or OpenAI's chunks, a word each.
fn stream(anthropic: bool, text: &str) -> Vec<String> {
    let words = text.split_inclusive(' ');
    let event = |name: &str, v: Value| format!("event: {name}\ndata: {v}\n\n");
    if anthropic {
        let start = event("message_start", json!({"type": "message_start", "message": {"usage": {"input_tokens": 12}}}));
        let block = event("content_block_start", json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}));
        let deltas = words
            .map(|w| event("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": w}})));
        let end = [
            event("content_block_stop", json!({"type": "content_block_stop", "index": 0})),
            event("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 5}})),
            event("message_stop", json!({"type": "message_stop"})),
        ];
        return [start, block].into_iter().chain(deltas).chain(end).collect();
    }
    let chunk = |delta: Value, finish: Value| format!("data: {}\n\n", json!({"choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}));
    let deltas = words.map(|w| chunk(json!({"content": w}), Value::Null));
    let end = [
        chunk(json!({}), json!("stop")),
        format!("data: {}\n\n", json!({"choices": [], "usage": {"prompt_tokens": 12, "completion_tokens": 5}})),
        "data: [DONE]\n\n".to_string(),
    ];
    deltas.chain(end).collect()
}

/// `text` as a whole answer in Anthropic's format, or OpenAI's.
fn whole(anthropic: bool, text: &str) -> String {
    if anthropic {
        json!({"type": "message", "role": "assistant", "content": [{"type": "text", "text": text}],
               "stop_reason": "end_turn", "usage": {"input_tokens": 12, "output_tokens": 5}})
    } else {
        json!({"choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}],
               "usage": {"prompt_tokens": 12, "completion_tokens": 5}})
    }
    .to_string()
}

/// `text` as Ollama's own answer: JSON lines, a word each, then its end with the tokens counted;
/// or whole.
fn ollama(stream: bool, text: &str) -> Answer {
    let end = json!({"message": {"role": "assistant", "content": ""}, "done": true, "done_reason": "stop", "prompt_eval_count": 12, "eval_count": 5});
    if !stream {
        let whole =
            json!({"message": {"role": "assistant", "content": text}, "done": true, "done_reason": "stop", "prompt_eval_count": 12, "eval_count": 5});
        return Answer::Whole("200 OK".into(), whole.to_string());
    }
    let words = text.split_inclusive(' ').map(|w| format!("{}\n", json!({"message": {"role": "assistant", "content": w}, "done": false})));
    Answer::Lines(words.chain([format!("{end}\n")]).collect())
}
