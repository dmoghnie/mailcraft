//! A model server of the tests' own, so MailCraft's AI is tested without a network: it speaks
//! OpenAI's Chat Completions (`/v1/chat/completions`) and Anthropic's Messages (`/v1/messages`),
//! keeps every request it was sent, and gives the answers a test scripted, in order. With none
//! scripted, it answers in the request's own format with "You said: " and the last turn.

use std::collections::VecDeque;
use std::io::BufReader;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

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

#[derive(Default)]
struct Shared {
    seen: Vec<Seen>,
    /// The answers to give next: a status line (`200 OK`) and a body.
    script: VecDeque<(String, String)>,
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
                let (status, answer) = {
                    let mut st = state.lock().unwrap();
                    st.seen.push(seen.clone());
                    st.script.pop_front().unwrap_or_else(|| ("200 OK".into(), echo(&seen)))
                };
                let headers = [("Content-Type", "application/json".to_string()), ("Connection", "close".into())];
                super::http::respond(&mut s, &status, &headers, answer.as_bytes());
            }
        });
        ModelServer { base, shared }
    }

    /// Give `body` with `status` (`429 Too Many Requests`) to the next request.
    pub fn then(&self, status: &str, body: &str) {
        self.shared.lock().unwrap().script.push_back((status.into(), body.into()));
    }

    /// Every request sent so far, in order.
    pub fn seen(&self) -> Vec<Seen> {
        self.shared.lock().unwrap().seen.clone()
    }
}

/// "You said: " and the request's last turn, in its own format.
fn echo(r: &Seen) -> String {
    let last = r.body.get("messages").and_then(Value::as_array).and_then(|m| m.last());
    let said = last.and_then(|m| m.get("content")).and_then(Value::as_str).unwrap_or_default();
    let text = format!("You said: {said}");
    if r.target.ends_with("/messages") {
        json!({"type": "message", "role": "assistant", "content": [{"type": "text", "text": text}],
               "stop_reason": "end_turn", "usage": {"input_tokens": 12, "output_tokens": 5}})
    } else {
        json!({"choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}],
               "usage": {"prompt_tokens": 12, "completion_tokens": 5}})
    }
    .to_string()
}
