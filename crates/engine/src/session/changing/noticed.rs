//! The buttons on a new-mail notification (Mark as read, Archive, Delete; Mark all as read on
//! one that tells of several messages), done to the messages it told of, by their keys,
//! wherever they are: the folder on view stays as it is (the window may not even be shown).
//! Each can be undone, as the ribbon's can. Reply shows the message, for the ribbon's Reply.

use std::collections::{BTreeMap, HashSet};

use super::*;

/// The most messages one notification's button acts on.
pub(crate) const NOTICED_MOST: usize = 10_000;

/// What a notification's button does to its messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NoticeAct {
    Read,
    Archive,
    Delete,
}

impl NoticeAct {
    pub(crate) fn from_code(code: &str) -> Option<NoticeAct> {
        match code {
            "read" => Some(NoticeAct::Read),
            "archive" => Some(NoticeAct::Archive),
            "delete" => Some(NoticeAct::Delete),
            _ => None,
        }
    }
}

impl Session {
    /// The messages a notification told of marked read, archived or deleted (moved to Deleted
    /// Items), each account's in one change: how many.
    pub(crate) fn act_on_noticed(&mut self, act: NoticeAct, keys: &[MessageKey]) -> CmdResult {
        let found = self.noticed(keys)?;
        let count = match act {
            NoticeAct::Read => {
                self.mark_selection(&found, true, |m| Flag::Seen.get(&m.flags), |keys, on| Op::SetFlag { keys, flag: Flag::Seen, on });
                found.len()
            }
            NoticeAct::Archive => self.move_messages(&found, &MoveTarget::Role(FolderRole::Archive))?,
            NoticeAct::Delete => self.move_messages(&found, &MoveTarget::Role(FolderRole::Trash))?,
        };
        // One moved out of the list on view isn't left selected or shown.
        let gone: HashSet<MessageKey> = found.iter().map(|m| m.key.clone()).filter(|k| self.summary(k).is_none()).collect();
        self.selection.retain(|k| !gone.contains(k));
        for k in [&mut self.focus, &mut self.anchor] {
            if k.as_ref().is_some_and(|k| gone.contains(k)) {
                *k = None;
            }
        }
        if self.reading.as_ref().is_some_and(|r| gone.contains(&r.key)) {
            self.reading = None;
        }
        Ok(serde_json::json!({ "count": count }))
    }

    /// A message a notification told of, opened in its folder's list (in Mail) for Reply: the
    /// list's filter, and Focused or Other (or the Inbox's groups), give way to show it.
    pub(crate) fn open_noticed(&mut self, key: &MessageKey) -> CmdResult {
        let m = self.noticed(std::slice::from_ref(key))?.pop().ok_or_else(|| CmdError::Failed(GONE.into()))?;
        // As `view.module` comes back to Mail (leaving it is what ends a search).
        self.module = crate::Module::Mail;
        if self.summary(key).is_none() {
            self.open_view(View::Folder { account: key.account.clone(), folder: key.folder.clone() });
        }
        if self.summary(key).is_none() && self.filter != ListFilter::All {
            self.filter = ListFilter::All;
            self.list.clear();
            self.refresh_list();
        }
        if self.summary(key).is_none() && self.focus_split() {
            if self.prefs.inbox_groups {
                self.show_group(self.inbox_group(&m));
            } else {
                self.focus_other = !self.is_focused(&m);
                self.refresh_list();
            }
        }
        if self.summary(key).is_none() {
            return Err(CmdError::Failed(GONE.into()));
        }
        self.open_message(key);
        Ok(serde_json::json!({ "key": key.to_id() }))
    }

    /// The messages these keys name, from their folders' lists: refused when a folder isn't
    /// the account's; those gone since are left out, and none left is refused.
    fn noticed(&mut self, keys: &[MessageKey]) -> Result<Vec<SharedSummary>, CmdError> {
        if keys.is_empty() {
            return Err(CmdError::Params("`keys` names no message".into()));
        }
        if keys.len() > NOTICED_MOST {
            return Err(CmdError::Params(format!("at most {NOTICED_MOST} messages")));
        }
        let mut by_folder: BTreeMap<(String, String), HashSet<&MessageKey>> = BTreeMap::new();
        for k in keys {
            if k.folder == OUTBOX || self.folder(&k.account, &k.folder).is_none() {
                return Err(CmdError::Params(format!("no folder `{}` in account `{}`", k.folder, k.account)));
            }
            by_folder.entry((k.account.clone(), k.folder.clone())).or_default().insert(k);
        }
        // Each folder's list read once (Mark all as read can name thousands).
        let mut found = Vec::new();
        for ((account, folder), wanted) in by_folder {
            found.extend(self.folder_state(&account, &folder).messages.iter().filter(|m| wanted.contains(&m.key)).cloned());
        }
        if found.is_empty() {
            return Err(CmdError::Failed(GONE.into()));
        }
        Ok(found)
    }
}

/// Said when the message a notification told of was moved or deleted since.
const GONE: &str = "the message isn't there any more";
