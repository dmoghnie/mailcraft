//! Clean-up suggestions: what could leave an account's Inbox, worked out on this computer from
//! the mail kept there, with nothing sent anywhere: lists the person never reads (Unsubscribe,
//! where the newest message names a way out), senders whose mail piles up (Sweep, keeping their
//! newest), and mail read and left there for a month (Archive). Only mail sent in bulk is
//! suggested by sender, never a VIP's; nothing is done until the person chooses.

use std::collections::HashMap;

use mailcraft_model::PINNED;

use super::*;

/// A list's messages in the Inbox, none of them read nor answered, before leaving it is
/// suggested.
const UNREAD_LEAST: usize = 3;
/// A sender's messages in the Inbox before sweeping all but the newest is suggested.
const PILE_LEAST: usize = 5;
/// How old mail that's been read is before archiving it is suggested, in days.
pub const OLD_DAYS: i64 = 30;
/// The most senders suggested: those with the most mail first.
const SENDERS_MOST: usize = 30;
const DAY: i64 = 86_400;

/// What could be done about one sender's mail in an Inbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SenderCleanup {
    pub account: String,
    /// Their address in lower case, and their name as their newest message gives it.
    pub sender: String,
    pub name: String,
    /// Their messages there, and how many of them are unread.
    pub count: usize,
    pub unread: usize,
    /// Their newest message there ([`MessageKey::to_id`]): Unsubscribe and Sweep go from it.
    pub newest: String,
    /// Leaving their list suggested: none of it was read or answered, and the newest message
    /// names a way out.
    pub leave: bool,
    /// Sweeping all but their newest suggested: it piles up.
    pub sweep: bool,
}

/// Mail read and left in an account's Inbox for more than [`OLD_DAYS`] days.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OldMail {
    pub account: String,
    pub count: usize,
}

/// What could leave the Inboxes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cleanup {
    pub senders: Vec<SenderCleanup>,
    pub old: Vec<OldMail>,
}

impl Session {
    /// Account `id`'s Inbox, its path.
    fn inbox_path(&self, id: &str) -> Option<String> {
        self.account(id).and_then(|a| a.folder(FolderRole::Inbox)).map(|f| f.path.clone())
    }

    /// What could leave each account's Inbox, as its list is kept here (an Inbox not yet read
    /// from the cache suggests nothing: [`Session::cleanup_read`] reads them first).
    pub fn cleanup(&self) -> Cleanup {
        let mut out = Cleanup::default();
        for a in &self.accounts {
            let id = &a.config.id;
            let Some(state) = self.inbox_path(id).and_then(|inbox| self.cache.get(&(id.clone(), inbox))) else { continue };
            out.senders.extend(self.senders_to_clean(id, &state.messages));
            let count = self.old_mail(&state.messages).len();
            if count > 0 {
                out.old.push(OldMail { account: id.clone(), count });
            }
        }
        out.senders.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.sender.cmp(&b.sender)));
        out.senders.truncate(SENDERS_MOST);
        out
    }

    /// Each account's Inbox read from the cache, then what could leave them.
    pub fn cleanup_read(&mut self) -> Cleanup {
        let inboxes: Vec<(String, String)> =
            self.accounts.iter().filter_map(|a| self.inbox_path(&a.config.id).map(|p| (a.config.id.clone(), p))).collect();
        for (id, inbox) in inboxes {
            self.folder_state(&id, &inbox);
        }
        self.cleanup()
    }

    /// The bulk senders of `messages` (not VIPs) whose mail could go: their list left, or
    /// all but their newest swept.
    fn senders_to_clean(&self, account: &str, messages: &[SharedSummary]) -> Vec<SenderCleanup> {
        let mut by: HashMap<String, Vec<&SharedSummary>> = HashMap::new();
        for m in messages.iter().filter(|m| m.bulk) {
            let sender = m.from.email.trim().to_ascii_lowercase();
            if !sender.is_empty() && !self.is_vip(&sender) {
                by.entry(sender).or_default().push(m);
            }
        }
        by.into_iter()
            .filter_map(|(sender, mail)| {
                let newest = mail.iter().copied().max_by_key(|m| (m.date, m.key.uid))?;
                let unread = mail.iter().filter(|m| !m.flags.seen).count();
                let untouched = unread == mail.len() && !mail.iter().any(|m| m.flags.answered);
                // The newest message's source is read only for those it could be suggested for.
                let leave = untouched && mail.len() >= UNREAD_LEAST && self.leaving(&newest.key).is_some();
                let sweep = mail.len() >= PILE_LEAST;
                let name = if newest.from.name.trim().is_empty() { sender.clone() } else { newest.from.name.trim().to_string() };
                (leave || sweep).then(|| SenderCleanup {
                    account: account.to_string(),
                    sender,
                    name,
                    count: mail.len(),
                    unread,
                    newest: newest.key.to_id(),
                    leave,
                    sweep,
                })
            })
            .collect()
    }

    /// The mail of `messages` older than [`OLD_DAYS`] days that's been read and isn't flagged,
    /// pinned or waiting on the person.
    fn old_mail<'m>(&self, messages: &'m [SharedSummary]) -> Vec<&'m SharedSummary> {
        let before = self.clock.now.saturating_sub(OLD_DAYS * DAY);
        messages.iter().filter(|m| m.date < before && m.flags.seen && !m.flags.flagged && !m.flags.has(PINNED) && !self.needs_me(m)).collect()
    }

    /// Account `id`'s old mail in its Inbox ([`Session::old_mail`]) to its Archive, as one Undo.
    pub fn cleanup_archive(&mut self, id: &str) -> CmdResult {
        let inbox = self.inbox_path(id).ok_or_else(|| CmdError::Params(format!("no account `{id}` with an Inbox")))?;
        self.folder_state(id, &inbox);
        let old: Vec<SharedSummary> =
            self.cache.get(&(id.to_string(), inbox)).map(|state| self.old_mail(&state.messages).into_iter().cloned().collect()).unwrap_or_default();
        if old.is_empty() {
            return Ok(serde_json::json!({"moved": 0}));
        }
        // The message open, or selected, among it: the next one takes its place, as Sweep does.
        let keys: Vec<MessageKey> = old.iter().map(|m| m.key.clone()).collect();
        let set: std::collections::HashSet<&MessageKey> = keys.iter().collect();
        let shown = self.reading.as_ref().is_some_and(|r| set.contains(&r.key)) || self.selection.iter().any(|k| set.contains(k));
        let next = if shown { self.successor(&keys) } else { None };
        let moved = self.move_messages(&old, &MoveTarget::Role(FolderRole::Archive))?;
        if shown && moved > 0 {
            self.after_removal(next);
        }
        Ok(serde_json::json!({"moved": moved}))
    }
}
