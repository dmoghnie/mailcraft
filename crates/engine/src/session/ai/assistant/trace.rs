//! A record of each reply of the assistant: from what the person said (or Try again) until the
//! model answers without calling a tool, fails, is stopped or reaches a limit. It keeps how it
//! went, never what was said: when, which model, how many questions and steps, the tools called
//! by name and how each went, and the tokens read and written. Kept in `ai-trace.json`, the
//! newest [`TRACES_MOST`] of the last [`TRACE_DAYS`] days; shown on request (`ai.trace`).
//!
//! A reply is also held to limits of its own, beside [`STEPS_MOST`]: [`REPLY_TOKENS_MOST`]
//! tokens, and [`REPLY_SECONDS_MOST`] seconds since the person last said or decided something.
//! Past one, it stops as when its steps run out, and says so.

use mailcraft_model::AiAnswer;

use crate::Session;

use super::*;

pub(crate) const TRACE_DOC: &str = "ai-trace.json";
/// The most replies kept, the newest.
pub const TRACES_MOST: usize = 500;
/// How long a reply's record is kept, in days.
pub const TRACE_DAYS: i64 = 30;
/// The most tokens one reply reads and writes, over all its questions (each step sends the
/// chat again): a few dozen long messages read, many times over.
pub const REPLY_TOKENS_MOST: u64 = 2_000_000;
/// The longest one reply goes on by itself, in seconds: since the person last said or decided
/// something in it.
pub const REPLY_SECONDS_MOST: i64 = 15 * 60;
/// The most tools a record names; past it, the count alone.
const TOOLS_KEPT: usize = 200;
/// Bytes a token is taken for, where a server doesn't count them (as the budget counts).
const BYTES_A_TOKEN: u64 = 3;
const DAY: i64 = 86_400;

/// How a reply ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TraceEnd {
    /// Still going, or waiting for the person to decide what it proposed.
    #[default]
    Going,
    /// The model answered without calling a tool.
    Answered,
    /// A question to the model failed, or couldn't be asked.
    Failed,
    /// The person stopped it.
    Stopped,
    /// It reached a limit: [`ReplyTrace::limit`] says which.
    Limit,
    /// Something else was said in the chat before it ended, or MailCraft closed.
    Left,
}

/// A limit a reply reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Limit {
    Steps,
    Tokens,
    Time,
}

impl Limit {
    /// Why it stopped, said to the model and the person: "after …".
    pub(super) fn said(self) -> String {
        match self {
            Limit::Steps => format!("{STEPS_MOST} steps"),
            Limit::Tokens => format!("using {REPLY_TOKENS_MOST} tokens"),
            Limit::Time => format!("{} minutes", REPLY_SECONDS_MOST / 60),
        }
    }
}

/// One tool a reply called, by name, and how it went.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceTool {
    pub tool: String,
    pub state: RunState,
}

/// One reply, as it went.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ReplyTrace {
    /// Its chat, by id.
    pub chat: String,
    /// When it began, and ended (Unix seconds); not ended while it's going.
    pub began: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended: Option<i64>,
    /// The model profile last asked, by its name then, and its model.
    pub profile: String,
    pub model: String,
    /// Questions to the model, and steps (answers with tool calls).
    pub asks: u32,
    pub steps: u32,
    /// Tokens read and written, as the server counted them (estimated where it didn't).
    pub read: u64,
    pub written: u64,
    /// The tools called, in order, and how many past those named.
    pub tools: Vec<TraceTool>,
    #[serde(skip_serializing_if = "is_zero")]
    pub tools_more: usize,
    pub end: TraceEnd,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<Limit>,
    /// Where the chat's tool runs stood as it began: those after it are its.
    #[serde(skip)]
    from: usize,
    /// When the person last said or decided something in it.
    #[serde(skip)]
    since: i64,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// The records kept, and whether they changed since saved.
#[derive(Debug, Default)]
pub(crate) struct Traces {
    replies: Vec<ReplyTrace>,
    changed: bool,
    /// The file couldn't be read: it isn't written over.
    unread: bool,
}

impl Traces {
    /// Chat `chat`'s reply going now, if one is.
    fn going(&mut self, chat: &str) -> Option<&mut ReplyTrace> {
        self.replies.iter_mut().rev().find(|r| r.chat == chat && r.end == TraceEnd::Going)
    }

    /// A reply of `chat` begun at `now`: one still going there is left. The oldest go.
    pub(super) fn begin(&mut self, chat: &Chat, now: i64) {
        self.end(chat, TraceEnd::Left, None, now);
        self.replies.push(ReplyTrace { chat: chat.id.clone(), began: now, from: chat.runs.len(), since: now, ..ReplyTrace::default() });
        let before = now.saturating_sub(TRACE_DAYS.saturating_mul(DAY));
        self.replies.retain(|r| r.began >= before);
        let skip = self.replies.len().saturating_sub(TRACES_MOST);
        self.replies.drain(..skip);
        self.changed = true;
    }

    /// Chat `chat`'s reply asked `profile` again.
    pub(super) fn ask(&mut self, chat: &str, profile: &AiProfile) {
        let Some(r) = self.going(chat) else { return };
        r.asks = r.asks.saturating_add(1);
        r.profile.clone_from(&profile.name);
        r.model.clone_from(&profile.endpoint.model);
        self.changed = true;
    }

    /// The person decided something in chat `chat`'s reply at `now`: its time goes from then.
    pub(super) fn decided(&mut self, chat: &str, now: i64) {
        if let Some(r) = self.going(chat) {
            r.since = now;
        }
    }

    /// Chat `chat`'s reply answered at `now` with `a`: `asked` is the question's tokens as
    /// estimated, for a server that doesn't count them. The limit it's past now, if one.
    pub(super) fn answered(&mut self, chat: &str, asked: Option<u64>, a: &AiAnswer, now: i64) -> Option<Limit> {
        let answered = u64::try_from(a.text.len().saturating_add(a.thinking.len())).unwrap_or(u64::MAX);
        let read = a.input_tokens.or(asked).unwrap_or(0);
        let written = a.output_tokens.unwrap_or_else(|| answered.div_ceil(BYTES_A_TOKEN));
        let r = self.going(chat)?;
        r.read = r.read.saturating_add(read);
        r.written = r.written.saturating_add(written);
        if !a.calls.is_empty() {
            r.steps = r.steps.saturating_add(1);
        }
        let over = if r.read.saturating_add(r.written) > REPLY_TOKENS_MOST {
            Some(Limit::Tokens)
        } else if now.saturating_sub(r.since) > REPLY_SECONDS_MOST {
            Some(Limit::Time)
        } else {
            None
        };
        self.changed = true;
        over
    }

    /// Chat `chat`'s reply ended at `now`, as `end` says: the tools it called taken from the
    /// chat, by name and how each went.
    pub(super) fn end(&mut self, chat: &Chat, end: TraceEnd, limit: Option<Limit>, now: i64) {
        let Some(r) = self.going(&chat.id) else { return };
        let mine = chat.runs.get(r.from..).unwrap_or_default();
        r.tools = mine.iter().take(TOOLS_KEPT).map(|run| TraceTool { tool: run.tool.clone(), state: run.state }).collect();
        r.tools_more = mine.len().saturating_sub(TOOLS_KEPT);
        r.end = end;
        r.ended = Some(now);
        r.limit = limit;
        self.changed = true;
    }
}

impl Session {
    /// The replies kept, newest first; of chat `chat` alone when named.
    pub fn ai_traces(&self, chat: Option<&str>) -> Vec<ReplyTrace> {
        self.ai.assistant.traces.replies.iter().rev().filter(|r| chat.is_none_or(|c| r.chat == c)).cloned().collect()
    }

    /// The records, as the last run left them: one going when MailCraft closed went no
    /// further. A damaged file is set aside, and never written over.
    pub(crate) fn load_traces(&mut self) {
        let Some(store) = self.store.clone() else { return };
        let t = &mut self.ai.assistant.traces;
        match store.load_doc::<Vec<ReplyTrace>>(TRACE_DOC) {
            Ok(replies) => {
                let skip = replies.len().saturating_sub(TRACES_MOST);
                t.replies = replies
                    .into_iter()
                    .skip(skip)
                    .map(|r| if r.end == TraceEnd::Going { ReplyTrace { end: TraceEnd::Left, ..r } } else { r })
                    .collect();
            }
            Err(e) => {
                log::warn!("the record of MailCraft's AI's replies couldn't be read: {e}");
                t.unread = true;
            }
        }
    }

    /// The records saved, once they changed.
    pub(crate) fn save_traces(&mut self) {
        let t = &mut self.ai.assistant.traces;
        if !std::mem::take(&mut t.changed) || t.unread {
            return;
        }
        if let Some(store) = &self.store
            && let Err(e) = store.save_doc(TRACE_DOC, &t.replies)
        {
            log::warn!("the record of MailCraft's AI's replies couldn't be saved: {e}");
        }
    }
}
