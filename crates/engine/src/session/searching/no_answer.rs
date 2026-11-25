//! Sent mail nobody answered, brought back: a message the user sent a few days ago
//! ([`Prefs::no_answer_days`]) to people rather than a list, that no message in any folder
//! answers, is listed at the top of the Inbox under No answer yet, to follow up or dismiss. What
//! counts is what `is:awaiting` finds ([`AwaitingScan`]); here it's worked out a few units a
//! tick, the folders read a page at a time as a search reads them, so a big mailbox never holds
//! a frame.

use mailcraft_store::FolderLoad;

use super::run::{Loaded, UNIT_MESSAGES};
use super::*;

/// How many days after it was sent unanswered mail comes back, unless the user chose otherwise.
pub const NO_ANSWER_DAYS: u32 = 3;
/// The most days the user can choose (it stays well inside [`AWAITING_WITHIN_DAYS`]).
pub const NO_ANSWER_DAYS_MAX: u32 = 14;
/// The search folder that lists the same messages ([`Session::open_no_answer_folder`]).
pub const NO_ANSWER_FOLDER: &str = "No answer yet";
/// The most dismissed messages remembered: the oldest are forgotten first (by then they're
/// older than [`AWAITING_WITHIN_DAYS`] and wouldn't come back anyway).
const DISMISSED_MAX: usize = 1_000;
/// Units of work a tick does: a page of a folder read, or [`UNIT_MESSAGES`] messages looked at.
const UNITS_A_TICK: u32 = 8;
/// Worked out again this long after the last time (seconds): days going by bring more back.
const LOOK_AGAIN: i64 = 3_600;
/// With mail changed (an answer may have come), worked out again once this long has gone by
/// since the last time (seconds): marking mail read changes it too, and isn't worth a scan each.
const CHANGED_AFTER: i64 = 300;

/// What the last scan went by: a change to any of it makes it out of date.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Basis {
    changes: u64,
    days: u32,
    accounts: usize,
}

/// The Inbox's No answer yet, and the scan working it out.
#[derive(Default)]
pub(crate) struct NoAnswer {
    /// The messages, newest first.
    found: Vec<SharedSummary>,
    /// The scan under way: it, the folder being read and where it is in that folder's list.
    scan: Option<(AwaitingScan, Option<FolderLoad>, usize)>,
    /// When the last scan began (`None`: never).
    at: Option<i64>,
    basis: Basis,
}

impl Session {
    fn no_answer_basis(&self) -> Basis {
        Basis { changes: self.search_state.changes, days: self.prefs.no_answer_days, accounts: self.accounts.len() }
    }

    /// The sent mail nobody answered that the Inbox on view lists, newest first: none when it's
    /// turned off, searched, or its Other pane is shown, and only its account's in one account's
    /// Inbox.
    pub fn no_answer_shown(&self) -> Vec<SharedSummary> {
        let other = self.focus_split() && self.focus_other;
        if !self.prefs.no_answer || self.search.is_some() || self.view_role() != Some(FolderRole::Inbox) || other {
            return Vec::new();
        }
        let account = match &self.view {
            View::Folder { account, .. } => Some(account.as_str()),
            _ => None,
        };
        self.search_state.no_answer.found.iter().filter(|m| account.is_none_or(|a| m.key.account == a)).cloned().collect()
    }

    /// All the sent mail nobody answered, newest first, whatever is on view: as last worked out,
    /// or worked out now when it never was or is being (what's found so far is only part of it);
    /// none when it's turned off.
    pub(crate) fn no_answer_known(&mut self) -> Vec<SharedSummary> {
        let na = &self.search_state.no_answer;
        if !self.prefs.no_answer {
            return Vec::new();
        }
        if na.at.is_none() || na.scan.is_some() {
            return self.no_answer_now();
        }
        na.found.clone()
    }

    /// Worked out again now, to the end (an agent's or the CLI's `mail.noAnswer`).
    pub fn no_answer_now(&mut self) -> Vec<SharedSummary> {
        self.start_no_answer();
        while self.search_state.no_answer.scan.is_some() {
            self.no_answer_unit(true);
        }
        self.search_state.no_answer.found.clone()
    }

    /// From [`Session::tick`]: with an Inbox on view, worked out again when it never was, or is
    /// out of date, a few units a tick.
    pub(in crate::session) fn no_answer_due(&mut self) {
        if !self.prefs.no_answer {
            let na = &mut self.search_state.no_answer;
            if na.scan.is_some() || !na.found.is_empty() || na.at.is_some() {
                *na = NoAnswer::default();
            }
            return;
        }
        if self.search_state.no_answer.scan.is_none() {
            if self.view_role() != Some(FolderRole::Inbox) || !self.no_answer_stale() {
                return;
            }
            self.start_no_answer();
        }
        for _ in 0..UNITS_A_TICK {
            if self.search_state.no_answer.scan.is_none() {
                break;
            }
            self.no_answer_unit(false);
        }
    }

    /// Whether it's to be worked out again ([`LOOK_AGAIN`], [`CHANGED_AFTER`]); at once when the
    /// days or the accounts changed, or the clock went back.
    fn no_answer_stale(&self) -> bool {
        let (na, now, basis) = (&self.search_state.no_answer, self.clock.now, self.no_answer_basis());
        let Some(at) = na.at else { return true };
        let moved = na.basis.days != basis.days || na.basis.accounts != basis.accounts || now < at;
        moved || now >= at.saturating_add(LOOK_AGAIN) || (na.basis.changes != basis.changes && now >= at.saturating_add(CHANGED_AFTER))
    }

    fn start_no_answer(&mut self) {
        let scan = AwaitingScan::new(self);
        let (now, basis) = (self.clock.now, self.no_answer_basis());
        let na = &mut self.search_state.no_answer;
        na.scan = Some((scan, None, 0));
        na.at = Some(now);
        na.basis = basis;
    }

    /// One unit of the scan: a page of a folder read, or a slice of its messages looked at; at
    /// the end, what it found is what's listed. `wait`: read marks still being written are
    /// waited for (the CLI's, an agent's), not gone round.
    fn no_answer_unit(&mut self, wait: bool) {
        let Some((mut scan, mut load, mut at)) = self.search_state.no_answer.scan.take() else { return };
        let Some((a, f, sent)) = scan.folders.first().cloned() else {
            let mut found = scan.finish_messages();
            found.sort_by(|x, y| y.date.cmp(&x.date).then_with(|| y.key.uid.cmp(&x.key.uid)));
            self.search_state.no_answer.found = found;
            return;
        };
        let k = (a, f);
        match self.load_unit(&k, &mut load, wait) {
            Loaded::Read | Loaded::Waits => {
                self.search_state.no_answer.scan = Some((scan, load, at));
                return;
            }
            Loaded::Failed => {
                scan.folders.remove(0);
                self.search_state.no_answer.scan = Some((scan, None, 0));
                return;
            }
            Loaded::Ready => {}
        }
        let messages = self.cache.get(&k).map(|st| st.messages.as_slice()).unwrap_or_default();
        let end = at.saturating_add(UNIT_MESSAGES).min(messages.len());
        for m in messages.get(at..end).unwrap_or_default() {
            scan.take(m, sent);
        }
        at = end;
        if at >= messages.len() {
            scan.folders.remove(0);
            at = 0;
        }
        self.search_state.no_answer.scan = Some((scan, None, at));
    }

    /// Mail that arrived answers some of it: those leave the list at once, without waiting for
    /// the next scan.
    pub(in crate::session) fn no_answer_arrived(&mut self, arrived: &[Summary]) {
        if self.search_state.no_answer.found.is_empty() {
            return;
        }
        let mine: HashSet<String> = self.my_addresses().into_iter().collect();
        let answers: HashSet<&str> = arrived
            .iter()
            .filter(|m| !mine.contains(&m.from.email.trim().to_lowercase()))
            .flat_map(|m| std::iter::once(&m.in_reply_to).chain(&m.references))
            .map(String::as_str)
            .filter(|id| !id.is_empty())
            .collect();
        self.search_state.no_answer.found.retain(|m| !answers.contains(m.message_id.as_str()));
    }

    /// The message `key`, one the user sent: from the list, else from its folder.
    fn sent_message(&mut self, key: &MessageKey) -> Result<SharedSummary, CmdError> {
        let listed = self.search_state.no_answer.found.iter().find(|m| m.key == *key).cloned();
        let m = match listed {
            Some(m) => m,
            None => self
                .folder_state(&key.account, &key.folder)
                .messages
                .iter()
                .find(|m| m.key == *key)
                .cloned()
                .ok_or_else(|| CmdError::Params("no such message".into()))?,
        };
        let from = m.from.email.trim().to_lowercase();
        if from.is_empty() || !self.my_addresses().iter().any(|a| a.trim().eq_ignore_ascii_case(&from)) {
            return Err(CmdError::Disabled("only a message you sent".into()));
        }
        Ok(m)
    }

    /// Follow up message `key`, one the user sent: it's shown, and a reply to all of its
    /// recipients begun.
    pub fn follow_up_no_answer(&mut self, key: &MessageKey) -> CmdResult {
        self.sent_message(key)?;
        self.open_message(key);
        // Read on another thread in the window: the reply needs it now.
        self.finish_pending_body();
        self.reply_compose(ReplyMode::ReplyAll)
    }

    /// Message `key`, one the user sent, no longer listed (nor found by `is:awaiting`), for
    /// good: remembered by its Message-ID with the settings.
    pub fn dismiss_no_answer(&mut self, key: &MessageKey) -> CmdResult {
        let m = self.sent_message(key)?;
        if m.message_id.is_empty() {
            return Err(CmdError::Disabled("the message has no Message-ID to remember it by".into()));
        }
        if !self.prefs.no_answer_dismissed.contains(&m.message_id) {
            let excess = (self.prefs.no_answer_dismissed.len() + 1).saturating_sub(DISMISSED_MAX);
            self.prefs.no_answer_dismissed.drain(..excess);
            self.prefs.no_answer_dismissed.push(m.message_id.clone());
        }
        self.search_state.no_answer.found.retain(|x| x.message_id != m.message_id);
        Ok(serde_json::json!({ "dismissed": key.to_id() }))
    }

    /// Show the No answer yet search folder: the same messages, from every account.
    pub fn open_no_answer_folder(&mut self) -> CmdResult {
        let n = self.search_as("is:awaiting", SearchScope::All, Some(NO_ANSWER_FOLDER.to_string()));
        Ok(serde_json::json!({ "messages": n }))
    }

    /// After how many days unanswered mail comes back (1 to [`NO_ANSWER_DAYS_MAX`]).
    pub fn set_no_answer_days(&mut self, days: u64) -> CmdResult {
        let max = u64::from(NO_ANSWER_DAYS_MAX);
        if !(1..=max).contains(&days) {
            return Err(CmdError::Params(format!("days: 1 to {max}")));
        }
        self.prefs.no_answer_days = u32::try_from(days).unwrap_or(NO_ANSWER_DAYS);
        Ok(serde_json::json!({ "days": self.prefs.no_answer_days }))
    }
}
