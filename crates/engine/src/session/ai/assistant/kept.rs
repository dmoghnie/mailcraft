//! The assistant's chats kept on this computer: a list of them (`assistant.json`), and each
//! chat in a file of its own (`assistant-<id>.json`), read when it's first wanted. A deleted
//! chat's file goes. Without a cache (the browser's), they last as long as the session.

use super::*;
use crate::{CmdError, Session};
use mailcraft_store::Store;

/// The list of chats kept.
const HEADS_DOC: &str = "assistant.json";

/// How each chat's file is named: this, its id, `.json`.
const CHAT_DOC: &str = "assistant-";

/// What a chat says, read again, when MailCraft closed before the answer it waited for came.
const CUT_SHORT: &str = "MailCraft closed before the answer came: send again to go on";

/// What a read says, read again, when MailCraft closed before its message was downloaded.
const NOT_READ: &str = "MailCraft closed before the message came";

/// What a script says, read again, when MailCraft closed while it waited.
const NOT_ENDED: &str = "MailCraft closed before the script ended";

/// What a script's question says, read again, when MailCraft closed before its answer came.
const NOT_ANSWERED: &str = "MailCraft closed before the answer came";

/// `chat` as the last run left it, read again: nothing is on its way now, so the answer or the
/// message it waited for says it never came, and the person sends again to go on.
fn left_off(chat: Chat) -> Chat {
    let waited = chat.runs.iter().any(|r| r.state == RunState::Waiting);
    let deciding = chat.runs.iter().any(|r| r.state == RunState::Proposed);
    let runs = chat
        .runs
        .into_iter()
        .map(|r| match r.state {
            RunState::Waiting => {
                let result = match r.tool.as_str() {
                    script::RUN_SCRIPT => NOT_ENDED,
                    script::ASK => NOT_ANSWERED,
                    _ => NOT_READ,
                };
                ToolRun { state: RunState::Failed, result: result.into(), ..r }
            }
            _ => r,
        })
        .collect();
    let cut = chat.answer_due || (waited && !deciding);
    let error = chat.error.or_else(|| cut.then(|| CUT_SHORT.to_string()));
    Chat { runs, error, answer_due: false, ..chat }
}

/// A chat's file.
fn chat_doc(id: &str) -> String {
    format!("{CHAT_DOC}{id}.json")
}

/// The ids of the chats whose files are kept, listed or not.
fn kept_ids(store: &Store) -> Vec<String> {
    store.doc_names(CHAT_DOC).iter().filter_map(|n| n.strip_prefix(CHAT_DOC)?.strip_suffix(".json").map(str::to_string)).collect()
}

impl Session {
    /// The list of chats, read once. A chat whose file is kept but isn't listed (the list
    /// damaged and set aside, or not read) is listed again from its file: left out, Delete all
    /// would miss it.
    pub(super) fn assistant_load(&mut self) {
        if std::mem::replace(&mut self.ai.assistant.loaded, true) {
            return;
        }
        let Some(store) = self.store.clone() else { return };
        let listed = match store.load_doc::<Vec<ChatHead>>(HEADS_DOC) {
            Ok(heads) => heads,
            Err(e) => {
                log::warn!("the list of MailCraft's AI chats couldn't be read: {e}");
                Vec::new()
            }
        };
        let unlisted: Vec<ChatHead> = kept_ids(&store)
            .into_iter()
            .filter(|id| !listed.iter().any(|h| h.id == *id))
            .filter_map(|id| match store.load_doc::<Chat>(&chat_doc(&id)) {
                Ok(c) if !c.turns.is_empty() => Some(ChatHead { id, title: c.title, created: c.created, updated: c.updated }),
                _ => None,
            })
            .collect();
        if unlisted.is_empty() {
            self.ai.assistant.heads = listed;
            return;
        }
        let mut heads: Vec<ChatHead> = listed.into_iter().chain(unlisted).collect();
        heads.sort_by_key(|h| h.created);
        self.ai.assistant.heads = heads;
        self.ai.assistant.heads_changed = true;
    }

    /// Chat `id`, read if it isn't yet; an error when there's none.
    pub(super) fn assistant_chat(&mut self, id: &str) -> Result<&mut Chat, CmdError> {
        self.assistant_load();
        let a = &self.ai.assistant;
        if !a.chats.contains_key(id) {
            if !a.heads.iter().any(|h| h.id == id) {
                return Err(CmdError::Params(format!("no chat {id}")));
            }
            let chat = match &self.store {
                Some(store) => store.load_doc::<Chat>(&chat_doc(id)).map_err(|e| CmdError::Failed(format!("the chat couldn't be read: {e}")))?,
                None => Chat::default(),
            };
            let head = self.ai.assistant.heads.iter().find(|h| h.id == id).cloned().unwrap_or_default();
            // One whose file was lost is begun again, empty.
            let chat = left_off(Chat { id: head.id, title: head.title, created: head.created, ..chat });
            self.ai.assistant.chats.insert(id.to_string(), chat);
        }
        self.ai.assistant.chats.get_mut(id).ok_or_else(|| CmdError::Params(format!("no chat {id}")))
    }

    /// A new chat, named after what it begins with.
    pub(super) fn assistant_new(&mut self, title: &str, profile: Option<String>) -> String {
        self.assistant_load();
        // Not the id of a chat whose file is kept, listed or not (one that couldn't be read):
        // its file would be written over.
        let kept = self.store.as_ref().map(kept_ids).unwrap_or_default();
        let ids = self.ai.assistant.heads.iter().map(|h| h.id.as_str()).chain(kept.iter().map(String::as_str));
        let n = ids.filter_map(|id| id.strip_prefix('c').and_then(|n| n.parse::<u64>().ok())).max().unwrap_or(0);
        let id = format!("c{}", n.saturating_add(1));
        let now = self.clock.now;
        let title = title_of(title);
        self.ai.assistant.heads.push(ChatHead { id: id.clone(), title: title.clone(), created: now, updated: now });
        let chat = Chat { id: id.clone(), title, created: now, updated: now, profile, ..Chat::default() };
        self.ai.assistant.chats.insert(id.clone(), chat);
        self.assistant_changed(&id);
        id
    }

    /// Chat `id` changed: kept as the jobs are next taken.
    pub(super) fn assistant_changed(&mut self, id: &str) {
        let now = self.clock.now;
        let a = &mut self.ai.assistant;
        if let Some(c) = a.chats.get_mut(id) {
            c.updated = now;
            if let Some(h) = a.heads.iter_mut().find(|h| h.id == id) {
                h.updated = now;
                h.title.clone_from(&c.title);
            }
        }
        a.changed.insert(id.to_string());
        a.heads_changed = true;
    }

    /// The chats changed since they were last kept, kept.
    pub(crate) fn save_chats(&mut self) {
        let a = &mut self.ai.assistant;
        if a.changed.is_empty() && !a.heads_changed {
            return;
        }
        let changed = std::mem::take(&mut a.changed);
        let heads = std::mem::take(&mut a.heads_changed);
        let Some(store) = self.store.clone() else { return };
        for id in changed {
            if let Some(chat) = self.ai.assistant.chats.get(&id)
                && let Err(e) = store.save_doc(&chat_doc(&id), chat)
            {
                log::warn!("a chat with MailCraft's AI couldn't be kept: {e}");
            }
        }
        if heads && let Err(e) = store.save_doc(HEADS_DOC, &self.ai.assistant.heads) {
            log::warn!("the list of MailCraft's AI chats couldn't be kept: {e}");
        }
    }

    /// Chat `id` gone, from this computer too; its question's answer is let go as it comes.
    pub(super) fn assistant_delete(&mut self, id: &str) -> Result<(), CmdError> {
        self.assistant_load();
        let a = &mut self.ai.assistant;
        if !a.heads.iter().any(|h| h.id == id) {
            return Err(CmdError::Params(format!("no chat {id}")));
        }
        a.heads.retain(|h| h.id != id);
        a.chats.remove(id);
        a.changed.remove(id);
        a.steps.remove(id);
        let asked: Vec<u64> = a.asking.iter().filter(|(_, c)| *c == id).map(|(q, _)| *q).collect();
        a.heads_changed = true;
        for q in asked {
            self.assistant_let_go(q);
        }
        self.scripts_let_go(id);
        if let Some(store) = &self.store {
            // Its copy set aside as damaged too: it holds mail as well.
            for doc in [chat_doc(id), format!("{}.bad", chat_doc(id))] {
                store.remove_doc(&doc).map_err(|e| CmdError::Failed(format!("the chat couldn't be deleted: {e}")))?;
            }
        }
        Ok(())
    }

    /// Every chat deleted, and every chat file left with them: one never listed, a copy set
    /// aside as damaged, the list's own (each holds mail, or what was asked). How many listed
    /// chats went.
    pub(super) fn assistant_delete_all(&mut self) -> Result<usize, CmdError> {
        self.assistant_load();
        let ids: Vec<String> = self.ai.assistant.heads().map(|h| h.id.clone()).collect();
        for id in &ids {
            self.assistant_delete(id)?;
        }
        if let Some(store) = &self.store {
            let left = store.doc_names(CHAT_DOC).into_iter().chain(store.doc_names(&format!("{HEADS_DOC}.bad")));
            for doc in left {
                store.remove_doc(&doc).map_err(|e| CmdError::Failed(format!("a chat couldn't be deleted: {e}")))?;
            }
        }
        Ok(ids.len())
    }
}

/// A chat's name, from what it begins with: its first line, at most 60 characters.
fn title_of(said: &str) -> String {
    let line = said.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    let mut title: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        title.push('…');
    }
    if title.is_empty() { "New chat".into() } else { title }
}
