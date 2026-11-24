//! A model's answer as it's written: the server-sent events OpenAI's and Anthropic's formats
//! stream (`data:` lines, an `event:` name, a blank line ending each), or the JSON object a line
//! Ollama's own streams, read as they come and built into the answer,
//! its text and thinking handed on as they grow. Hostile input as every answer is: an event and
//! the tool calls are capped, and the whole answer is capped by the body's own limit.

use std::io::{self, Write};

use mailcraft_model::{AiAnswer, AiApi, AiToolCall};
use serde_json::{Value, json};

use super::*;

/// The longest event (its `data:` lines together): far more than a delta of an answer, or a
/// tool's whole input sent at once.
const MAX_EVENT: usize = 2 << 20;
/// The most tool calls one answer may make.
pub(super) const MAX_CALLS: usize = 64;

/// A tool call as it comes: by its place in the answer, its id, name and input's text.
#[derive(Default)]
pub(super) struct Partial {
    pub index: u64,
    pub id: String,
    pub name: String,
    pub input: String,
}

/// An answer as it's built from its events.
#[derive(Default)]
pub(super) struct Building {
    pub text: String,
    pub thinking: String,
    pub calls: Vec<Partial>,
    pub cut: bool,
    /// The model declined, or the server stopped with an error: its words, [`quoted`].
    pub refused: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// The server said the answer is whole (`[DONE]`, a finish reason, `message_stop`).
    pub done: bool,
}

impl Building {
    /// The call at `index`, started when it's new: `None` past [`MAX_CALLS`].
    pub fn call(&mut self, index: u64) -> Option<&mut Partial> {
        let at = match self.calls.iter().position(|c| c.index == index) {
            Some(at) => at,
            None if self.calls.len() < MAX_CALLS => {
                self.calls.push(Partial { index, ..Partial::default() });
                self.calls.len() - 1
            }
            None => return None,
        };
        self.calls.get_mut(at)
    }

    /// The answer it built, or why there's none.
    pub(super) fn finish(self) -> Result<AiAnswer> {
        if let Some(no) = self.refused {
            return Err(NetError::No(no));
        }
        if !self.done {
            return Err(NetError::Closed("the model's answer stopped before its end".into()));
        }
        let calls = self.calls.into_iter().enumerate().map(|(n, c)| AiToolCall {
            id: if c.id.is_empty() { format!("call_{n}") } else { c.id },
            name: c.name,
            input: input_of(&c.input),
        });
        Ok(AiAnswer {
            text: self.text,
            calls: calls.collect(),
            thinking: self.thinking,
            cut: self.cut,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            ..AiAnswer::default()
        })
    }
}

/// A tool's input from the text a model sent: a JSON object, none for nothing, or the text
/// itself when it isn't JSON (the engine tells the model so).
pub(super) fn input_of(text: &str) -> Value {
    if text.trim().is_empty() {
        return json!({});
    }
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
}

/// How the body is read: as events, or whole (an error, or a server that didn't stream).
enum Reading {
    Events,
    Whole(Vec<u8>),
}

/// The body's reader: events parsed as their lines come, the answer built, and its text and
/// thinking handed to `on` as they grow.
pub(super) struct Events<'a> {
    api: AiApi,
    key: Option<&'a str>,
    on: &'a mut dyn FnMut(&str, &str),
    reading: Reading,
    /// Each line is an event of its own (Ollama's JSON lines).
    lines: bool,
    line: Vec<u8>,
    data: Vec<u8>,
    name: String,
    built: Building,
    /// Why reading stopped, when the events did it.
    failed: Option<NetError>,
    received: u64,
}

impl<'a> Events<'a> {
    pub fn new(api: AiApi, key: Option<&'a str>, on: &'a mut dyn FnMut(&str, &str)) -> Events<'a> {
        Events {
            api,
            key,
            on,
            reading: Reading::Events,
            lines: false,
            line: Vec::new(),
            data: Vec::new(),
            name: String::new(),
            built: Building::default(),
            failed: None,
            received: 0,
        }
    }

    /// The answer's head came: its body is events (or JSON lines) when it says so, and whole
    /// otherwise.
    pub fn opened(&mut self, head: &Response) {
        let kind = head.header("Content-Type").map(str::to_ascii_lowercase).unwrap_or_default();
        self.lines = kind.contains("ndjson");
        if !head.ok() || !(self.lines || kind.contains("text/event-stream")) {
            self.reading = Reading::Whole(Vec::new());
        }
    }

    /// The answer, once its body was read (`got`: how reading it went), or why there's none.
    pub fn finish(mut self, got: Result<Response>) -> Result<AiAnswer> {
        if let Some(e) = self.failed.take() {
            return Err(e);
        }
        let head = got?;
        let received = self.received;
        let answer = match self.reading {
            Reading::Whole(body) => read_reply(self.api, &Response { body, ..head }, self.key),
            Reading::Events => {
                // The last event, when the stream ended without the blank line after it.
                self.line_ended();
                self.dispatch();
                match self.failed.take() {
                    Some(e) => Err(e),
                    None => self.built.finish(),
                }
            }
        }?;
        Ok(AiAnswer { received, ..answer })
    }

    /// A line of the stream, without its end (`\n`, `\r\n`).
    fn line_ended(&mut self) {
        let mut line = std::mem::take(&mut self.line);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if self.lines {
            self.data = line;
            return self.dispatch();
        }
        if line.is_empty() {
            return self.dispatch();
        }
        let (field, value) = match line.iter().position(|b| *b == b':') {
            Some(0) => return, // a comment (`: ping`)
            Some(at) => (line.get(..at).unwrap_or_default(), line.get(at + 1..).unwrap_or_default()),
            None => (line.as_slice(), &[][..]),
        };
        let value = value.strip_prefix(b" ").unwrap_or(value);
        match field {
            b"data" => {
                if !self.data.is_empty() {
                    self.data.push(b'\n');
                }
                self.data.extend_from_slice(value);
            }
            b"event" => self.name = String::from_utf8_lossy(value).into_owned(),
            _ => {}
        }
    }

    /// The event its lines made, built into the answer, its growth handed on.
    fn dispatch(&mut self) {
        let data = std::mem::take(&mut self.data);
        let name = std::mem::take(&mut self.name);
        if data.is_empty() || self.failed.is_some() {
            return;
        }
        if data.as_slice() == b"[DONE]" {
            self.built.done = true;
            return;
        }
        let v: Value = match serde_json::from_slice(&data) {
            Ok(v) => v,
            Err(e) => {
                self.failed = Some(NetError::Protocol(format!("the model's server streamed something other than JSON: {e}")));
                return;
            }
        };
        let (text, thinking) = (self.built.text.len(), self.built.thinking.len());
        let key = self.key;
        let built = &mut self.built;
        match self.api {
            AiApi::OpenAi => openai::event(built, &v, key),
            AiApi::Anthropic => anthropic::event(built, &name, &v, key),
            AiApi::Ollama => ollama::event(built, &v, key),
        }
        let said = self.built.text.get(text..).unwrap_or_default();
        let thought = self.built.thinking.get(thinking..).unwrap_or_default();
        if !said.is_empty() || !thought.is_empty() {
            (self.on)(said, thought);
        }
    }
}

impl Write for Events<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.received = self.received.saturating_add(u64::try_from(buf.len()).unwrap_or(u64::MAX));
        if let Reading::Whole(body) = &mut self.reading {
            body.extend_from_slice(buf);
            return Ok(buf.len());
        }
        for &b in buf {
            if b == b'\n' {
                self.line_ended();
            } else {
                self.line.push(b);
            }
            if self.line.len().saturating_add(self.data.len()) > MAX_EVENT {
                self.failed = Some(NetError::Protocol(format!("the model's server streamed an event longer than {MAX_EVENT} bytes")));
            }
            if self.failed.is_some() {
                return Err(io::Error::other("the model's answer couldn't be read"));
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
