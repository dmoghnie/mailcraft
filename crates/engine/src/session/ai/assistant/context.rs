//! What a chat is about, and the chat fitted into what its model can take in.
//!
//! The person points at messages, a part of one they selected, or a folder: each is written into
//! the turn as it's sent, once (sent again unchanged later, a server's cache of what it read
//! keeps working). A chat longer than its model's context loses, first, what tools read early
//! on (what they found still named), then its oldest turns, what those held said in a ledger
//! (`ledger.rs`). How many tokens it is is estimated, and corrected by what the model's server
//! counted of the chat's last question when the estimate was low.

use std::fmt;

use mailcraft_model::{AiChat, AiRole, AiTool, AiToolResult, AiTurn, MessageKey};

use super::ledger::{found_names, ledger};
use super::links::message_name;
use super::*;
use crate::{CmdError, Session};

/// The most items pointed at in a turn.
pub const ITEMS_MOST: usize = 20;
/// The most of a message's text written in for it, in characters.
pub(in crate::session::ai) const MESSAGE_CHARS: usize = 12_000;
/// The most of a selected part written in, in characters.
const QUOTE_CHARS: usize = 8000;
/// What a model takes in when its profile doesn't say, in tokens (Ollama's own, for one).
pub const CONTEXT_AT_FIRST: u32 = 32_768;
/// Room kept for the tools' descriptions and what a server adds, in tokens.
const ROOM_KEPT: usize = 1024;
/// What a read's result early in a chat becomes when there's no room for it (and it found
/// nothing to name).
const LEFT_OUT: &str = "[Left out for room: call the tool again if it's needed.]";
/// What a server adds around each message, call and result (its role, a tool's name and id),
/// in tokens.
const PART_TOKENS: usize = 8;
/// The room kept for the ledger of a chat's start left out, in tokens.
const LEDGER_ROOM: usize = 1000;
/// The most the estimate is raised by the server's count, in thousandths: three times.
const SCALE_MOST: usize = 3000;
/// The least a tool's result is cut to for room, in tokens: shorter, it would say too little.
const RESULT_LEAST: usize = 200;

/// Something the person points at.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ContextItem {
    /// A message, whole.
    Message { id: String },
    /// Part of a message, as the person selected it.
    Quote { id: String, text: String },
    /// A folder.
    Folder { account: String, folder: String },
}

impl fmt::Debug for ContextItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextItem::Message { id } => write!(f, "Message({id})"),
            ContextItem::Quote { id, text } => write!(f, "Quote({id}, <{} bytes>)", text.len()),
            ContextItem::Folder { .. } => write!(f, "Folder"),
        }
    }
}

impl ContextItem {
    /// The account whose mail it is.
    pub(super) fn account(&self) -> Option<String> {
        match self {
            ContextItem::Message { id } | ContextItem::Quote { id, .. } => MessageKey::from_id(id).map(|k| k.account),
            ContextItem::Folder { account, .. } => Some(account.clone()),
        }
    }
}

impl Session {
    /// What `items` say, written for the model; the accounts whose mail they are. Each is
    /// checked: there, and its account's mail allowed to go to `profile`.
    pub(super) fn context_text(&mut self, items: &[ContextItem], profile: &AiProfile) -> Result<(String, Vec<String>), CmdError> {
        if items.len() > ITEMS_MOST {
            return Err(CmdError::Params(format!("at most {ITEMS_MOST} things can be pointed at")));
        }
        let mut parts = Vec::new();
        let mut accounts: Vec<String> = Vec::new();
        for item in items {
            let account = item.account().ok_or_else(|| CmdError::Params("not a message".into()))?;
            if !self.prefs.ai.allows(profile, Some(&account)) {
                return Err(CmdError::Disabled(format!("this account's mail doesn't go to {}: File › Options › AI says where it may", profile.name)));
            }
            parts.push(self.context_part(item)?);
            if !accounts.contains(&account) {
                accounts.push(account);
            }
        }
        if parts.is_empty() {
            return Ok((String::new(), accounts));
        }
        Ok((format!("The person points at:\n\n{}", parts.join("\n\n")), accounts))
    }

    fn context_part(&mut self, item: &ContextItem) -> Result<String, CmdError> {
        match item {
            ContextItem::Message { id } => {
                let key = MessageKey::from_id(id).ok_or_else(|| CmdError::Params("not a message".into()))?;
                let m = self.shared_of(&key).ok_or_else(|| CmdError::Params("that message isn't there any more".into()))?;
                let text = match self.message_text(&key) {
                    Some(t) => cut(&t, MESSAGE_CHARS),
                    None => "(Its text isn't downloaded yet: read_message gets it.)".into(),
                };
                Ok(format!("<message id=\"{}\">\n{}\n\n{text}\n</message>", message_name(id), self.message_head(&m)))
            }
            ContextItem::Quote { id, text } => {
                let key = MessageKey::from_id(id).ok_or_else(|| CmdError::Params("not a message".into()))?;
                let m = self.shared_of(&key).ok_or_else(|| CmdError::Params("that message isn't there any more".into()))?;
                Ok(format!("<selected part of=\"{}\">\n{}\n\n{}\n</selected>", message_name(id), self.message_head(&m), cut(text, QUOTE_CHARS)))
            }
            ContextItem::Folder { account, folder } => {
                let f = self.folder(account, folder).ok_or_else(|| CmdError::Params(format!("no folder `{folder}`")))?;
                Ok(format!(
                    "<folder account=\"{account}\" path=\"{}\">{}: {} messages, {} unread</folder>",
                    f.path,
                    f.display_name(),
                    f.total,
                    f.unread
                ))
            }
        }
    }

    /// Chat `chat` as `profile` is asked it: its turns as sent, fitted into the profile's
    /// context (what tools gave early on left out first, then the oldest turns).
    pub(super) fn fitted(&self, chat: &Chat, profile: &AiProfile, system: String, tools: Vec<mailcraft_model::AiTool>) -> Result<AiChat, CmdError> {
        let context = usize::try_from(profile.takes_in()).unwrap_or(usize::MAX);
        let answer = usize::try_from(profile.max_tokens.max(1)).unwrap_or(usize::MAX);
        let scale = scale(chat);
        let est = |n: usize| n.saturating_mul(scale) / 1000;
        let fixed = est(tools_tokens(&tools).saturating_add(tokens(system.len())));
        let room = context.saturating_sub(answer).saturating_sub(ROOM_KEPT).saturating_sub(fixed);
        let when = |at| mailcraft_model::dates::full_time(at, &self.clock);
        let mut turns: Vec<AiTurn> = chat.turns.iter().map(|t| t.sent(&when)).collect();
        let size = |turns: &[AiTurn]| est(turns.iter().map(turn_tokens).fold(0, usize::saturating_add));
        // What tools read, oldest first, but the last turn's: what they found still named, and
        // what a change came to kept.
        let last = turns.len().saturating_sub(1);
        let mut i = 0;
        while size(&turns) > room && i < last {
            if let Some(t) = turns.get_mut(i) {
                for r in &mut t.results {
                    if let Some(note) = left_out(chat, r).filter(|n| r.content.len() > n.len()) {
                        r.content = note;
                    }
                }
            }
            i += 1;
        }
        // The oldest turns go, never the person's latest words nor what came after them (the
        // calls and their results belong together), with room kept for what they held.
        let latest = turns.iter().rposition(|t| t.role == AiRole::User && !t.text.is_empty()).unwrap_or(0);
        let mut dropped = 0;
        if size(&turns) > room {
            let fits = room.saturating_sub(est(LEDGER_ROOM));
            while size(&turns) > fits && dropped < latest {
                turns.remove(0);
                dropped += 1;
            }
        }
        if dropped > 0 {
            // A chat begins with the person: what came before them may answer a call now gone.
            while turns.first().is_some_and(|t| t.role == AiRole::Assistant || t.text.is_empty()) && dropped < latest {
                turns.remove(0);
                dropped += 1;
            }
            if let Some(t) = turns.first_mut() {
                t.results.clear();
            }
        }
        // What the tools gave since (several messages read at once) is cut, each to a share of
        // the room the rest leaves, rather than the chat stopping.
        if size(&turns) > room {
            turns = shortened(turns, room.saturating_mul(1000) / scale);
        }
        if size(&turns) > room {
            return Err(CmdError::Failed(format!(
                "this is more than {} takes in at once ({context} tokens): point at less, or set its context higher in File › Options › AI",
                profile.name
            )));
        }
        let system = if dropped > 0 {
            // What's left of the room, in bytes as the estimate counts them.
            let spare = room.saturating_sub(size(&turns)).saturating_mul(3000) / scale;
            format!("{system}\n\n{}", ledger(chat, dropped, spare.min(LEDGER_ROOM.saturating_mul(3))))
        } else {
            system
        };
        Ok(AiChat { system, turns, max_tokens: profile.max_tokens.max(1), tools, ..AiChat::default() })
    }
}

/// `turns` with what the tools gave cut, each to an even share of the room the rest leaves
/// (unless that's less than [`RESULT_LEAST`]: they stay as they are).
fn shortened(turns: Vec<AiTurn>, room: usize) -> Vec<AiTurn> {
    let results: usize = turns.iter().map(|t| t.results.len()).sum();
    let given = |t: &AiTurn| t.results.iter().map(|r| r.content.len()).sum::<usize>();
    let rest: usize = turns.iter().map(|t| turn_tokens(t).saturating_sub(given(t) / 3)).sum();
    let share = room.saturating_sub(rest).checked_div(results).unwrap_or(0);
    if share < RESULT_LEAST {
        return turns;
    }
    // A character is a token at most (three bytes), a few take four; the note a cut adds is room
    // too.
    let chars = share.saturating_sub(24).saturating_mul(3) / 4;
    turns
        .into_iter()
        .map(|t| AiTurn { results: t.results.into_iter().map(|r| AiToolResult { content: cut(&r.content, chars), ..r }).collect(), ..t })
        .collect()
}

/// What a result of `chat`'s says when it's left out for room: what its read found, by name (the
/// model can still link it, and needn't look again); none for a change's, whose outcome stays.
fn left_out(chat: &Chat, r: &AiToolResult) -> Option<String> {
    let run = chat.run(&r.id);
    if run.is_some_and(|run| run.state != RunState::Ran) {
        return None;
    }
    let names = run.map(found_names).unwrap_or_default();
    Some(if names.is_empty() { LEFT_OUT.into() } else { format!("[Left out for room: it found {names}. Call the tool again for the rest.]") })
}

/// About how many tokens a turn is: its text, calls and results, and what a server adds around
/// each.
fn turn_tokens(t: &AiTurn) -> usize {
    let parts = t.calls.len().saturating_add(t.results.len()).saturating_add(1);
    tokens(t.size()).saturating_add(parts.saturating_mul(PART_TOKENS))
}

/// About how many tokens the tools' descriptions are.
fn tools_tokens(tools: &[AiTool]) -> usize {
    tools
        .iter()
        .map(|t| tokens(t.name.len().saturating_add(t.description.len()).saturating_add(t.schema.to_string().len())))
        .fold(0, usize::saturating_add)
}

/// About how many tokens `chat` is, as it's sent: what its server's count is set beside.
pub(super) fn estimate(chat: &AiChat) -> u64 {
    let turns = chat.turns.iter().map(turn_tokens).fold(0, usize::saturating_add);
    let all = tokens(chat.system.len()).saturating_add(tools_tokens(&chat.tools)).saturating_add(turns);
    u64::try_from(all).unwrap_or(u64::MAX)
}

/// How much more of `chat` its model reads than estimated, in thousandths: what its server
/// counted of the last question, against the estimate of it. Never less than the estimate (a
/// server that reuses what it read before, as Ollama does, counts only what's new), at most
/// [`SCALE_MOST`].
fn scale(chat: &Chat) -> usize {
    let Some(c) = chat.counted.filter(|c| c.estimated > 0) else { return 1000 };
    usize::try_from(c.read.saturating_mul(1000) / c.estimated).unwrap_or(usize::MAX).clamp(1000, SCALE_MOST)
}

/// About how many tokens `bytes` of text are: a token for every three bytes (a little more than
/// English takes, and about what other languages do).
pub(super) fn tokens(bytes: usize) -> usize {
    bytes / 3 + 1
}

/// `text` cut at `chars` characters, saying so.
pub(in crate::session::ai) fn cut(text: &str, chars: usize) -> String {
    match text.char_indices().nth(chars) {
        Some((at, _)) => format!("{}\n[… cut here: {} more characters]", text.get(..at).unwrap_or(text), text.chars().count().saturating_sub(chars)),
        None => text.to_string(),
    }
}
