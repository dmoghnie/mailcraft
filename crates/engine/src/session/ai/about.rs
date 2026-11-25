//! What MailCraft's AI writes about a message for the reading pane: its conversation summed up
//! (`summary.rs`), the message translated (`translation.rs`), or what's to act on in it found
//! (`facts.rs`). Each is one question to the
//! model chat work goes to ([`Session::ai_ask`]): only as far as the message's account lets its
//! mail go, recorded by what it was for, and streamed, so what came so far shows while it's
//! written. The newest are kept, by message and kind; an answer to one asked for again or closed
//! since is dropped. The mail is others': the model is told never to follow what it says.

use std::collections::HashMap;

use mailcraft_model::{AiAnswer, AiChat, MessageKey};

pub use facts::{FactAmount, FactEvent, FactTask, FactTracking, Facts, facts_from};
use serde_json::{Value, json};

use super::settings::AiTask;
use super::{ai_server, assistant};
use crate::{CmdError, CommandSpec, Session, p};

mod facts;
mod summary;
#[cfg(test)]
mod tests;
mod translation;

/// Where the commands are.
const WHERE: &str = "Reading pane";
/// The most kept, the newest.
const KEPT: usize = 16;
/// The longest language code taken.
const LANGUAGE_MOST: usize = 16;

/// What's written about a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AboutKind {
    /// Its conversation summed up.
    Summary,
    /// It translated.
    Translation,
    /// Its dates, tasks, amounts and tracking numbers found.
    Facts,
}

impl AboutKind {
    /// What the record of what went where says its question was for.
    pub fn purpose(self) -> &'static str {
        match self {
            AboutKind::Summary => "summary",
            AboutKind::Translation => "translation",
            AboutKind::Facts => "facts",
        }
    }
}

/// What MailCraft's AI wrote about a message, or is writing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AboutMessage {
    /// Its question, while the answer is on its way (what came so far: [`crate::AiState::part`]).
    pub asking: Option<u64>,
    pub text: String,
    /// Why there's none.
    pub error: Option<String>,
    /// The profile asked: the one that answered, once one did.
    pub profile: String,
    /// The answer was cut short at the longest answer set for the model.
    pub cut: bool,
    /// The messages sent, and those left out for room (the oldest of a conversation).
    pub messages: usize,
    pub left_out: usize,
    /// The question it was asked with, which numbers them in the order asked: the newest are
    /// kept (a time could be the same for two).
    order: u64,
}

/// What's written about messages, by message and kind, and the questions on their way.
#[derive(Clone, Debug, Default)]
pub struct About {
    by_key: HashMap<(MessageKey, AboutKind), AboutMessage>,
    asks: HashMap<u64, (MessageKey, AboutKind)>,
}

impl About {
    /// `written`, asked for with question `id`, kept as message `key`'s `kind` in place of any
    /// before it; past [`KEPT`], the oldest asked for let go.
    fn keep(&mut self, key: MessageKey, kind: AboutKind, written: AboutMessage, id: u64) {
        self.by_key.insert((key.clone(), kind), AboutMessage { asking: Some(id), order: id, ..written });
        self.asks.insert(id, (key, kind));
        while self.by_key.len() > KEPT {
            let Some(oldest) = self.by_key.iter().min_by_key(|(_, w)| w.order).map(|(k, _)| k.clone()) else { break };
            self.by_key.remove(&oldest);
        }
    }
}

/// A question about a message: what the model is told, what it's asked, the longest answer
/// wanted, and how many messages went with it (and were left out for room).
struct Question {
    system: String,
    text: String,
    tokens: u32,
    messages: usize,
    left_out: usize,
}

impl Session {
    /// What MailCraft's AI wrote about message `key`, of `kind`, or is writing.
    pub fn about_message(&self, key: &MessageKey, kind: AboutKind) -> Option<&AboutMessage> {
        self.ai.about.by_key.get(&(key.clone(), kind))
    }

    /// Ask `q` about message `key`, for `kind`: what was written of it before is replaced.
    fn about_ask(&mut self, key: &MessageKey, kind: AboutKind, q: Question) -> Result<(), CmdError> {
        let profile = self.ai_route(AiTask::Chat, Some(&key.account)).map_err(CmdError::Disabled)?.id.clone();
        let id = self.ai_ask(AiTask::Chat, Some(&key.account), kind.purpose(), AiChat::ask(&q.system, &q.text, q.tokens), true)?;
        let written = AboutMessage { profile, messages: q.messages, left_out: q.left_out, ..AboutMessage::default() };
        self.ai.about.keep(key.clone(), kind, written, id);
        Ok(())
    }

    /// Question `id`'s answer, from `profile`, if it's about a message: kept with it, unless
    /// that was asked for again or closed since. Whether it was about one.
    pub(crate) fn about_came(&mut self, id: u64, profile: &str, result: &Result<AiAnswer, String>) -> bool {
        let Some(of) = self.ai.about.asks.remove(&id) else { return false };
        if let Some(w) = self.ai.about.by_key.get_mut(&of).filter(|w| w.asking == Some(id)) {
            w.asking = None;
            w.profile = profile.to_string();
            match result {
                Ok(a) => (w.text, w.cut, w.error) = (a.text.trim().to_string(), a.cut, None),
                Err(e) => w.error = Some(e.clone()),
            }
        }
        true
    }
}

/// `language`, a language code ("de", "pt-BR"), said for the model; none (or not a code): `or`.
pub(super) fn language_named(language: Option<&str>, or: &str) -> String {
    language
        .map(str::trim)
        .filter(|l| !l.is_empty() && l.len() <= LANGUAGE_MOST && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .map_or_else(|| or.to_string(), |l| format!("the language whose code is {l}"))
}

/// The message a command is about: `key`, else the one in the reading pane.
fn key_of(s: &Session, v: &Value) -> Result<MessageKey, CmdError> {
    match p::str(v, "key") {
        Some(id) => MessageKey::from_id(id).ok_or_else(|| CmdError::Params("`key` isn't a message's".into())),
        None => s.reading.as_ref().map(|r| r.key.clone()).ok_or_else(|| CmdError::Failed("no message is open".into())),
    }
}

/// What's written about message `key`, of `kind`, as the commands answer: while it's written,
/// what came so far.
fn shown(s: &Session, key: &MessageKey, kind: AboutKind) -> Value {
    let mut out = json!({"key": key.to_id()});
    let Some(w) = s.about_message(key, kind) else {
        out[kind.purpose()] = Value::Null;
        return out;
    };
    let text = match w.asking.and_then(|id| s.ai.part(id)) {
        Some((part, _)) => part.to_string(),
        None => w.text.clone(),
    };
    let model = s.prefs.ai.profile(&w.profile).map(|p| json!({"name": p.name, "model": p.endpoint.model, "server": ai_server(&p.endpoint.url)}));
    out[kind.purpose()] = json!({"text": text, "writing": w.asking.is_some(), "error": w.error, "cut": w.cut, "messages": w.messages, "leftOut": w.left_out, "model": model});
    out
}

/// A command asking for `kind` about the message `v` names (`close`: let go), by `ask`.
fn asked(s: &mut Session, v: &Value, kind: AboutKind, ask: fn(&mut Session, &MessageKey, Option<&str>) -> Result<(), CmdError>) -> crate::CmdResult {
    let key = key_of(s, v)?;
    if p::bool(v, "close") == Some(true) {
        s.ai.about.by_key.remove(&(key.clone(), kind));
    } else {
        ask(s, &key, p::str(v, "language"))?;
    }
    Ok(shown(s, &key, kind))
}

/// A command showing `kind` about the message `v` names.
fn showing(s: &mut Session, v: &Value, kind: AboutKind) -> crate::CmdResult {
    let key = key_of(s, v)?;
    Ok(shown(s, &key, kind))
}

pub(crate) fn commands() -> Vec<CommandSpec> {
    let mut all = summary::commands();
    all.extend(translation::commands());
    all.extend(facts::commands());
    all
}
