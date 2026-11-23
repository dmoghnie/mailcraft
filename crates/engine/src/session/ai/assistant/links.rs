//! How a chat names what its tools show the model: a short name, a letter (`m` a message, `e`
//! an event, `p` a person, `t` a task, `n` a note) and ten hex digits of a hash of what it is.
//! A message's id holds its folder's name, which can have spaces and brackets, so it can't be a
//! Markdown link; a name can (`[subject](mailcraft:m…)`), and the window opens it. A name is
//! looked up only among what the chat has seen (found, read, changed or pointed at), so a link
//! opens nothing else, whoever wrote it: an answer quotes mail others wrote. Names are kept in
//! saved chats: how one is made must never change.

use mailcraft_model::MessageKey;
use mailcraft_model::hash::fnv1a;
use serde::{Deserialize, Serialize};

use super::{Chat, ContextItem};

/// What a link to something in MailCraft starts with.
pub const LINK: &str = "mailcraft:";

/// The hash's bits a name keeps: forty, so names in a chat of thousands of messages don't meet.
const NAME_BITS: u64 = (1 << 40) - 1;

/// The name a chat's tools give the message with id `id`.
pub(super) fn message_name(id: &str) -> String {
    name('m', id)
}

/// A name: `letter` and ten hex digits of the hash of `what`.
fn name(letter: char, what: &str) -> String {
    format!("{letter}{:010x}", fnv1a(what.as_bytes()) & NAME_BITS)
}

/// What a chat's tools found besides messages: an event, at one of its occurrences, a person in
/// the contacts, a task or a note.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Found {
    #[serde(rename_all = "camelCase")]
    Event {
        account: String,
        calendar: String,
        uid: String,
        occurrence: i64,
        start: i64,
    },
    Person {
        account: String,
        uid: String,
    },
    Task {
        account: String,
        uid: String,
    },
    Note {
        account: String,
        uid: String,
    },
}

impl Found {
    /// What the tools call it.
    pub(super) fn name(&self) -> String {
        match self {
            Found::Event { account, calendar, uid, occurrence, .. } => name('e', &format!("event:{account}:{calendar}:{occurrence}:{uid}")),
            Found::Person { account, uid } => name('p', &format!("person:{account}:{uid}")),
            Found::Task { account, uid } => name('t', &format!("task:{account}:{uid}")),
            Found::Note { account, uid } => name('n', &format!("note:{account}:{uid}")),
        }
    }

    /// What it is, in a word.
    pub(super) fn kind(&self) -> &'static str {
        match self {
            Found::Event { .. } => "event",
            Found::Person { .. } => "person",
            Found::Task { .. } => "task",
            Found::Note { .. } => "note",
        }
    }

    /// The account it's of.
    pub(super) fn account(&self) -> &str {
        match self {
            Found::Event { account, .. } | Found::Person { account, .. } | Found::Task { account, .. } | Found::Note { account, .. } => account,
        }
    }
}

/// What a link opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Linked {
    Message(MessageKey),
    Found(Found),
}

/// The link to the message with id `id`, as an answer has it.
pub fn message_link(id: &str) -> String {
    format!("{LINK}{}", message_name(id))
}

impl Chat {
    /// The ids of the messages it has seen: what its tools found, read or changed, and what the
    /// person pointed at.
    fn seen(&self) -> impl Iterator<Item = &str> {
        let found = self.runs.iter().flat_map(|r| r.messages.iter().map(String::as_str));
        let pointed = self.turns.iter().flat_map(|t| t.context.iter()).filter_map(|c| match c {
            ContextItem::Message { id } | ContextItem::Quote { id, .. } => Some(id.as_str()),
            ContextItem::Folder { .. } => None,
        });
        found.chain(pointed)
    }

    /// The message `name` names in it: by its name (or its link), one it has seen; by its id
    /// (chats begun before names, and agents), any.
    pub(super) fn message_named(&self, name: &str) -> Option<MessageKey> {
        let name = name.trim();
        let bare = name.strip_prefix(LINK).unwrap_or(name);
        MessageKey::from_id(name)
            .or_else(|| MessageKey::from_id(bare))
            .or_else(|| self.seen().find(|id| message_name(id) == bare).and_then(MessageKey::from_id))
    }

    /// The message `link` opens: one it has seen.
    pub fn linked_message(&self, link: &str) -> Option<MessageKey> {
        let key = self.message_named(link)?;
        let id = key.to_id();
        self.seen().any(|s| s == id).then_some(key)
    }

    /// What `link` opens: a message, an event, a person, a task or a note it has seen.
    pub fn linked(&self, link: &str) -> Option<Linked> {
        if let Some(key) = self.linked_message(link) {
            return Some(Linked::Message(key));
        }
        self.found_named(link).cloned().map(Linked::Found)
    }

    /// What its tools found that `name` (or its link) names.
    pub(super) fn found_named(&self, name: &str) -> Option<&Found> {
        let name = name.trim();
        let name = name.strip_prefix(LINK).unwrap_or(name);
        self.runs.iter().flat_map(|r| r.found.iter()).find(|f| f.name() == name)
    }
}
