//! How a chat names the messages its tools show the model: a short name, `m` and ten hex digits
//! of a hash of the message's id. An id holds its folder's name, which can have spaces and
//! brackets, so it can't be a Markdown link; a name can (`[subject](mailcraft:m…)`), and the
//! window opens it. A name is looked up only among the messages the chat has seen (found, read,
//! changed or pointed at), so a link opens nothing else, whoever wrote it: an answer quotes
//! mail others wrote. Names are kept in saved chats: how one is made must never change.

use mailcraft_model::MessageKey;
use mailcraft_model::hash::fnv1a;

use super::{Chat, ContextItem};

/// What a link to something in MailCraft starts with.
pub const LINK: &str = "mailcraft:";

/// The hash's bits a name keeps: forty, so names in a chat of thousands of messages don't meet.
const NAME_BITS: u64 = (1 << 40) - 1;

/// The name a chat's tools give the message with id `id`.
pub(super) fn message_name(id: &str) -> String {
    format!("m{:010x}", fnv1a(id.as_bytes()) & NAME_BITS)
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
}
