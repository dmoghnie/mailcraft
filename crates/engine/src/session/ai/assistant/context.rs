//! What a chat is about, and the chat fitted into what its model can take in.
//!
//! The person points at messages, a part of one they selected, or a folder: each is written into
//! the turn as it's sent, once (sent again unchanged later, a server's cache of what it read
//! keeps working). A chat longer than its model's context loses, first, what tools gave back
//! early on, then its oldest turns.

use std::fmt;

use mailcraft_model::{AiChat, AiRole, AiTurn, MessageKey};

use super::*;
use crate::{CmdError, Session};

/// The most items pointed at in a turn.
pub const ITEMS_MOST: usize = 20;
/// The most of a message's text written in for it, in characters.
pub(super) const MESSAGE_CHARS: usize = 12_000;
/// The most of a selected part written in, in characters.
const QUOTE_CHARS: usize = 8000;
/// What a model takes in when its profile doesn't say, in tokens (Ollama's own, for one).
pub const CONTEXT_AT_FIRST: u32 = 32_768;
/// Room kept for the tools' descriptions and what a server adds, in tokens.
const ROOM_KEPT: usize = 1024;
/// What a tool's result early in a chat becomes when there's no room for it.
const LEFT_OUT: &str = "[Left out for room: call the tool again if it's needed.]";

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
                Ok(format!("<message id=\"{id}\">\n{}\n\n{text}\n</message>", self.message_head(&m)))
            }
            ContextItem::Quote { id, text } => {
                let key = MessageKey::from_id(id).ok_or_else(|| CmdError::Params("not a message".into()))?;
                let m = self.shared_of(&key).ok_or_else(|| CmdError::Params("that message isn't there any more".into()))?;
                Ok(format!("<selected part of=\"{id}\">\n{}\n\n{}\n</selected>", self.message_head(&m), cut(text, QUOTE_CHARS)))
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
        let context = usize::try_from(if profile.context > 0 { profile.context } else { CONTEXT_AT_FIRST }).unwrap_or(usize::MAX);
        let answer = usize::try_from(profile.max_tokens.max(1)).unwrap_or(usize::MAX);
        let tool_room: usize = tools.iter().map(|t| tokens(t.description.len().saturating_add(t.schema.to_string().len()))).sum();
        let room = context.saturating_sub(answer).saturating_sub(ROOM_KEPT).saturating_sub(tool_room).saturating_sub(tokens(system.len()));
        let mut turns: Vec<AiTurn> = chat.turns.iter().map(ChatTurn::sent).collect();
        let size = |turns: &[AiTurn]| turns.iter().map(|t| tokens(t.size())).sum::<usize>();
        // What tools gave back, oldest first, but the last turn's.
        let last = turns.len().saturating_sub(1);
        let mut i = 0;
        while size(&turns) > room && i < last {
            if let Some(t) = turns.get_mut(i) {
                for r in t.results.iter_mut().filter(|r| r.content.len() > LEFT_OUT.len()) {
                    r.content = LEFT_OUT.into();
                }
            }
            i += 1;
        }
        // The oldest turns go, never the person's latest words nor what came after them (the
        // calls and their results belong together).
        let latest = turns.iter().rposition(|t| t.role == AiRole::User && !t.text.is_empty()).unwrap_or(0);
        let mut dropped = 0;
        while size(&turns) > room && dropped < latest {
            turns.remove(0);
            dropped += 1;
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
        if size(&turns) > room {
            return Err(CmdError::Failed(format!(
                "this is more than {} takes in at once ({context} tokens): point at less, or set its context higher in File › Options › AI",
                profile.name
            )));
        }
        let system = if dropped > 0 { format!("{system}\n\nThe start of this chat was left out for room.") } else { system };
        Ok(AiChat { system, turns, max_tokens: profile.max_tokens.max(1), tools })
    }
}

/// About how many tokens `bytes` of text are: a token for every three bytes (a little more than
/// English takes, and about what other languages do).
pub(super) fn tokens(bytes: usize) -> usize {
    bytes / 3 + 1
}

/// `text` cut at `chars` characters, saying so.
pub(super) fn cut(text: &str, chars: usize) -> String {
    match text.char_indices().nth(chars) {
        Some((at, _)) => format!("{}\n[… cut here: {} more characters]", text.get(..at).unwrap_or(text), text.chars().count().saturating_sub(chars)),
        None => text.to_string(),
    }
}
