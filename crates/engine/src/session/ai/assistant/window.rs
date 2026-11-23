//! What the window needs of the assistant: its pane (shown or not, and how wide), the chats to
//! pick from, a chat as it stands, and the model a chat asks. Nothing here changes a chat.

use std::ops::RangeInclusive;

use super::*;
use crate::{CmdError, Session};

/// How wide the assistant's pane may be dragged.
pub const ASSISTANT_WIDTH: RangeInclusive<u16> = 300..=720;

/// The assistant's pane beside the mail: shown or not, and its width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AssistantPane {
    pub shown: bool,
    pub width: u16,
}

impl Default for AssistantPane {
    fn default() -> Self {
        AssistantPane { shown: false, width: 380 }
    }
}

impl Session {
    /// The chats kept, newest first (their list read from this computer the first time).
    pub fn assistant_heads(&mut self) -> Vec<ChatHead> {
        self.assistant_load();
        self.ai.assistant.heads().cloned().collect()
    }

    /// Chat `id` as it stands, read from this computer if it isn't yet: none when there's no
    /// such chat or its file couldn't be read.
    pub fn assistant_shown(&mut self, id: &str) -> Option<&Chat> {
        self.assistant_chat(id).ok().map(|c| &*c)
    }

    /// The model `chat` asks now (a new chat: one with no turns yet), or why there's none it may
    /// ask.
    pub fn assistant_model(&self, chat: &Chat) -> Result<AiProfile, CmdError> {
        self.assistant_profile(chat)
    }

    /// What `item` is called in the window: a message's subject (a quote's, its message's), a
    /// folder's name; none for a message no longer here.
    pub fn assistant_item_name(&mut self, item: &ContextItem) -> Option<String> {
        match item {
            ContextItem::Message { id } | ContextItem::Quote { id, .. } => {
                let key = mailcraft_model::MessageKey::from_id(id)?;
                self.shared_of(&key).map(|m| m.subject.clone())
            }
            ContextItem::Folder { account, folder } => Some(self.folder_name(account, folder)),
        }
    }
}
