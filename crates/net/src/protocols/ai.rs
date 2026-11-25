//! Asking a language model over HTTP: OpenAI's Chat Completions (most servers, the ones on the
//! person's own computer included), Anthropic's Messages and Ollama's own, the tools it may call with them,
//! its answer whole or streamed as it's written. A key goes over https only, or to this
//! computer.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mailcraft_model::{AiAnswer, AiApi, AiChat, AiEndpoint};
use serde_json::Value;

use crate::http::{self, Request, Response, Url};
use crate::{NetError, Result};

mod again;
mod anthropic;
mod ollama;
mod openai;
mod streamed;
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
/// The shortest start of a key taken for part of it, where words quoting it were cut.
const KEY_START: usize = 4;

/// What a question stopped ends with.
const STOPPED: &str = "the question was stopped";

/// A question that can be stopped from another thread: its connection, once open, is shut, so
/// a read waiting on it (a model still reading the question, an answer half written) ends at
/// once.
#[derive(Clone, Default)]
pub struct Stop(Arc<Mutex<Stopping>>);

#[derive(Default)]
struct Stopping {
    stopped: bool,
    socket: Option<TcpStream>,
}

impl Stop {
    fn state(&self) -> std::sync::MutexGuard<'_, Stopping> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stopped: its connection shut now, or as it opens.
    pub fn stop(&self) {
        let mut s = self.state();
        s.stopped = true;
        if let Some(socket) = s.socket.take()
            && let Err(e) = socket.shutdown(Shutdown::Both)
        {
            log::debug!("a model's connection was closed already: {e}");
        }
    }

    pub fn stopped(&self) -> bool {
        self.state().stopped
    }

    /// The question's connection, `s`, open: kept to be shut. Stopped already, it isn't used.
    fn opened(&self, s: &crate::stream::Stream) -> Result<()> {
        let mut state = self.state();
        if state.stopped {
            return Err(NetError::Refused(STOPPED.into()));
        }
        state.socket = s.socket().ok();
        Ok(())
    }

    /// What `r` ended with, said as stopped when it was.
    fn said<T>(&self, r: Result<T>) -> Result<T> {
        r.map_err(|e| if self.stopped() { NetError::Refused(STOPPED.into()) } else { e })
    }
}

/// Ask `endpoint`'s model `chat`, with `key` when the server takes one.
pub fn ask(endpoint: &AiEndpoint, key: Option<&str>, chat: &AiChat) -> Result<AiAnswer> {
    ask_stoppable(endpoint, key, chat, &Stop::default())
}

/// Ask as [`ask`] does; `stop` stops it.
pub fn ask_stoppable(endpoint: &AiEndpoint, key: Option<&str>, chat: &AiChat, stop: &Stop) -> Result<AiAnswer> {
    let key = key.filter(|k| !k.trim().is_empty()).map(str::trim);
    let req = request(endpoint, key, chat, false)?;
    let url = Url::parse(&req.url)?;
    again::twice(stop, &|| false, || {
        let mut s = http::open_waiting(&url, THINKING)?;
        stop.opened(&s)?;
        let resp = stop.said(http::exchange(&mut s, &req, &url)).map_err(|e| keyless(e, key))?;
        let answer = read_reply(endpoint.api, &resp, key)?;
        Ok(AiAnswer { sent: bytes(req.body.len()), received: bytes(resp.body.len()), ..answer })
    })
}

/// Ask as [`ask`] does, the answer streamed: its text and its thinking go to `on` as they're
/// written (what more came of each), then the whole answer comes back.
pub fn ask_streamed(endpoint: &AiEndpoint, key: Option<&str>, chat: &AiChat, on: &mut dyn FnMut(&str, &str)) -> Result<AiAnswer> {
    ask_streamed_stoppable(endpoint, key, chat, &Stop::default(), on)
}

/// Ask as [`ask_streamed`] does; `stop` stops it.
pub fn ask_streamed_stoppable(
    endpoint: &AiEndpoint,
    key: Option<&str>,
    chat: &AiChat,
    stop: &Stop,
    on: &mut dyn FnMut(&str, &str),
) -> Result<AiAnswer> {
    let key = key.filter(|k| !k.trim().is_empty()).map(str::trim);
    let req = request(endpoint, key, chat, true)?;
    let url = Url::parse(&req.url)?;
    // Once some of the answer came (and was shown), it isn't asked again: it would come twice.
    let came = std::cell::Cell::new(false);
    let mut heard = |text: &str, thinking: &str| {
        if !text.is_empty() || !thinking.is_empty() {
            came.set(true);
        }
        on(text, thinking);
    };
    again::twice(stop, &|| came.get(), || {
        let mut s = http::open_waiting(&url, THINKING)?;
        stop.opened(&s)?;
        let answer = stop.said(read_stream(endpoint.api, &mut s, &req, &url, key, &mut heard))?;
        Ok(AiAnswer { sent: bytes(req.body.len()), ..answer })
    })
}

/// Send `req` on `s` and read the streamed answer, in `api`'s format: its text and thinking to
/// `on` as they come, then the answer, or why there's none (a server's words [`quoted`]). A
/// server that answers whole, or with an error, is read as [`read_reply`] reads it.
pub fn read_stream<S: Read + Write>(
    api: AiApi,
    s: &mut S,
    req: &Request,
    url: &Url,
    key: Option<&str>,
    on: &mut dyn FnMut(&str, &str),
) -> Result<AiAnswer> {
    let mut events = streamed::Events::new(api, key, on);
    let reading = &mut events;
    let got = http::exchange_into_as(s, req, url, "model's answer", move |head| {
        reading.opened(head);
        Ok(Some(reading))
    });
    events.finish(got).map_err(|e| keyless(e, key))
}

/// The request asking `chat`, streamed or not; one past [`MAX_QUESTION`] isn't sent.
fn request(endpoint: &AiEndpoint, key: Option<&str>, chat: &AiChat, stream: bool) -> Result<Request> {
    let base = address(&endpoint.url)?;
    let req = match endpoint.api {
        AiApi::OpenAi => openai::request(&base, endpoint, key, chat, stream),
        AiApi::Anthropic => anthropic::request(&base, endpoint, key, chat, stream),
        AiApi::Ollama => ollama::request(&base, endpoint, key, chat, stream),
    };
    if req.body.len() > MAX_QUESTION {
        return Err(NetError::Protocol(format!("the question is too long to send ({} bytes, at most {MAX_QUESTION})", req.body.len())));
    }
    Ok(req)
}

fn bytes(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
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
        AiApi::Ollama => ollama::answer(&body, key),
    }
}

/// A server's own words, as an error shows them: without control characters, with `key` blanked
/// out (what an error says is shown, and may be logged), and cut short, in that order: no
/// character taken out can join a key back together, and no cut leaves part of one.
pub(crate) fn quoted(said: &str, key: Option<&str>) -> String {
    blanked(said, key).chars().take(MAX_ERROR).collect::<String>().trim().to_string()
}

/// `said` without control characters, then `key` blanked out of it, and the start of one where
/// it ends (words quoting it that were cut short before they came here).
fn blanked(said: &str, key: Option<&str>) -> String {
    let clean: String = said.chars().filter(|c| !c.is_control()).collect();
    let Some(k) = key.filter(|k| !k.is_empty()) else { return clean };
    let mut clean = clean.replace(k, "…");
    let start = (KEY_START..k.len()).rev().filter(|n| k.is_char_boundary(*n)).find(|n| clean.ends_with(k.get(..*n).unwrap_or(k)));
    if let Some(n) = start {
        clean.truncate(clean.len().saturating_sub(n));
        clean.push('…');
    }
    clean
}

/// An error from asking a model, `key` kept out of it: the network layer's own errors quote what
/// a server sent (a status line it couldn't read, cut short), which could hold the key, whole
/// or in part. One that does says less.
fn keyless(e: NetError, key: Option<&str>) -> NetError {
    let said = e.to_string();
    let clean: String = said.chars().filter(|c| !c.is_control()).collect();
    if blanked(&said, key) == clean {
        return e;
    }
    NetError::Protocol("the model's server sent an answer that couldn't be read".into())
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
    let why = if said.is_empty() { format!("the model's server: {what}") } else { format!("the model's server: {what}: {said}") };
    // Busy, or failing on its side: that may pass, and the question is asked again.
    if resp.status == 429 || (500..600).contains(&resp.status) { NetError::Later(why) } else { NetError::No(why) }
}

/// Why a stream stopped, in the server's words when it gave them ([`quoted`]).
fn stopped(said: &str, key: Option<&str>) -> String {
    let said = quoted(said, key);
    if said.is_empty() { "the model's server stopped".into() } else { format!("the model's server stopped: {said}") }
}

/// A number the server counted, when it gave one.
fn count(v: &Value, name: &str) -> Option<u64> {
    v.get(name).and_then(Value::as_u64)
}
