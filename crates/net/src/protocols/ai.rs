//! Asking a language model over HTTP: OpenAI's Chat Completions (most servers, the ones on the
//! person's own computer included) and Anthropic's Messages. A key goes over https only, or to
//! this computer.

use std::time::Duration;

use mailcraft_model::{AiAnswer, AiApi, AiChat, AiEndpoint};
use serde_json::Value;

use crate::http::{self, Request, Response, Url};
use crate::{NetError, Result};

mod anthropic;
mod openai;
#[cfg(test)]
mod tests;

/// The longest answer read: far more than any reply a person reads.
pub(crate) const MAX_ANSWER: usize = 8 << 20;
/// The longest question sent (a long thread, with room to spare).
pub(crate) const MAX_QUESTION: usize = 4 << 20;
/// How long a model may think before its answer starts.
pub(crate) const THINKING: Duration = Duration::from_secs(300);
/// The most of a server's error message shown, in characters.
const MAX_ERROR: usize = 300;

/// Ask `endpoint`'s model `chat`, with `key` when the server takes one.
pub fn ask(endpoint: &AiEndpoint, key: Option<&str>, chat: &AiChat) -> Result<AiAnswer> {
    let base = address(&endpoint.url)?;
    let key = key.filter(|k| !k.trim().is_empty()).map(str::trim);
    let req = match endpoint.api {
        AiApi::OpenAi => openai::request(&base, endpoint, key, chat),
        AiApi::Anthropic => anthropic::request(&base, endpoint, key, chat),
    };
    if req.body.len() > MAX_QUESTION {
        return Err(NetError::Protocol(format!("the question is too long to send ({} bytes, at most {MAX_QUESTION})", req.body.len())));
    }
    let resp = http::send_waiting(&req, THINKING)?;
    let answer = read_reply(endpoint.api, &resp, key)?;
    let bytes = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    Ok(AiAnswer { sent: bytes(req.body.len()), received: bytes(resp.body.len()), ..answer })
}

/// A model's server's reply read, in `api`'s format: its answer, or why there's none. What the
/// server says in an error is [`quoted`].
pub fn read_reply(api: AiApi, resp: &Response, key: Option<&str>) -> Result<AiAnswer> {
    if !resp.ok() {
        return Err(refused(resp, key));
    }
    let body: Value =
        serde_json::from_slice(&resp.body).map_err(|e| NetError::Protocol(format!("the model's server sent something other than JSON: {e}")))?;
    match api {
        AiApi::OpenAi => openai::answer(&body, key),
        AiApi::Anthropic => anthropic::answer(&body),
    }
}

/// A server's own words, as an error shows them: without control characters, with `key` blanked
/// out (what an error says is shown, and may be logged), and cut short, in that order: no
/// character taken out can join a key back together, and no cut leaves part of one.
pub(crate) fn quoted(said: &str, key: Option<&str>) -> String {
    let clean: String = said.chars().filter(|c| !c.is_control()).collect();
    let clean = match key.filter(|k| !k.is_empty()) {
        Some(k) => clean.replace(k, "…"),
        None => clean,
    };
    clean.chars().take(MAX_ERROR).collect::<String>().trim().to_string()
}

/// The server's API address without its trailing `/`: https, or plain http to this computer
/// alone, as Graph, EWS and DAV are. A key or mail sent in clear to another computer could be
/// read on the way.
fn address(url: &str) -> Result<String> {
    let parsed = Url::parse(url)?;
    if !parsed.https && !http::is_loopback(&parsed.host) {
        return Err(NetError::Protocol("a model's server is reached over https, unless it's on this computer".into()));
    }
    Ok(url.trim().trim_end_matches('/').to_string())
}

/// A request posting `body` to `url` with `headers`, its answer at most [`MAX_ANSWER`].
fn post(url: String, headers: &[(&str, &str)], body: &Value) -> Request {
    let req = Request::post(&url, "application/json", body.to_string().into_bytes(), MAX_ANSWER);
    headers.iter().fold(req, |r, (name, value)| r.header(name, value))
}

/// Why the server said no: its status, and its own words when it gave them (OpenAI's and
/// Anthropic's `error.message`, Ollama's `error`), [`quoted`].
fn refused(resp: &Response, key: Option<&str>) -> NetError {
    let said = serde_json::from_slice::<Value>(&resp.body).ok().and_then(|v| {
        let e = v.get("error")?;
        e.get("message").and_then(Value::as_str).or_else(|| e.as_str()).map(str::to_string)
    });
    let said = quoted(&said.unwrap_or_default(), key);
    let what = match resp.status {
        401 | 403 => "the key was refused".to_string(),
        404 => "no such model or address".to_string(),
        429 => "too many requests, or the account's credit or limit is used up".to_string(),
        s => format!("HTTP {s}"),
    };
    if said.is_empty() { NetError::No(format!("the model's server: {what}")) } else { NetError::No(format!("the model's server: {what}: {said}")) }
}

/// A number the server counted, when it gave one.
fn count(v: &Value, name: &str) -> Option<u64> {
    v.get(name).and_then(Value::as_u64)
}
