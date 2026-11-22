//! Sweep: a sender's mail in one folder cleared out in one step, by one of four choices
//! ([`SweepChoice`]). What goes, goes to Deleted Items, and Undo brings it back. The sender is
//! their address: mail they sent from another address stays.

use std::collections::HashSet;

use mailcraft_model::organize::{Condition, Rule, RuleAction};

use super::*;

/// How old the mail a standing sweep removes is, in days ([`SweepChoice::OlderThan`]).
pub const SWEEP_DAYS: i64 = 10;
const DAY: i64 = 86_400;
/// The most standing sweeps kept.
const STANDING_MAX: usize = 200;

/// What Sweep removes of a sender's mail in a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SweepChoice {
    /// All of it.
    All,
    /// All of it, and what they send later: a rule puts that in Deleted Items as it arrives.
    AllAndLater,
    /// All but their newest message.
    KeepNewest,
    /// What's older than [`SWEEP_DAYS`] days, now and each day after ([`StandingSweep`]).
    OlderThan,
}

impl SweepChoice {
    /// The choices, in the order they're offered.
    pub const ALL: [SweepChoice; 4] = [SweepChoice::All, SweepChoice::AllAndLater, SweepChoice::KeepNewest, SweepChoice::OlderThan];

    /// The choice a command names (`all`, `allAndLater`, `keepNewest`, `olderThan`).
    pub fn parse(s: &str) -> Option<SweepChoice> {
        Some(match s {
            "all" => SweepChoice::All,
            "allAndLater" => SweepChoice::AllAndLater,
            "keepNewest" => SweepChoice::KeepNewest,
            "olderThan" => SweepChoice::OlderThan,
            _ => return None,
        })
    }
}

/// A sweep that runs each day: the sender's mail in the folder older than [`SWEEP_DAYS`] days
/// goes to Deleted Items.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandingSweep {
    pub account: String,
    pub folder: String,
    /// The sender's address.
    pub sender: String,
    /// When it last ran (Unix seconds).
    #[serde(default)]
    pub ran: i64,
}

impl StandingSweep {
    fn is_of(&self, of: &SweepOf) -> bool {
        self.account == of.account && self.folder == of.folder && self.sender.eq_ignore_ascii_case(&of.sender)
    }
}

/// Whose mail Sweep acts on, and where: a message's sender (their address), in its folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SweepOf {
    pub account: String,
    pub folder: String,
    pub sender: String,
}

/// Of `msgs`, those from `sender` that `choice` removes (`before`: where "older" starts).
fn chosen(msgs: &[SharedSummary], sender: &str, choice: SweepChoice, before: i64) -> Vec<SharedSummary> {
    let theirs = msgs.iter().filter(|m| m.from.email.trim().eq_ignore_ascii_case(sender));
    match choice {
        SweepChoice::All | SweepChoice::AllAndLater => theirs.cloned().collect(),
        SweepChoice::KeepNewest => {
            let newest =
                msgs.iter().filter(|m| m.from.email.trim().eq_ignore_ascii_case(sender)).max_by_key(|m| (m.date, m.key.uid)).map(|m| m.key.clone());
            theirs.filter(|m| Some(&m.key) != newest.as_ref()).cloned().collect()
        }
        SweepChoice::OlderThan => theirs.filter(|m| m.date < before).cloned().collect(),
    }
}

impl Session {
    /// Whose mail Sweep acts on for message `key` (the first one selected when `None`): its
    /// sender, in its folder. `None` when there's no such message or it names no sender.
    pub fn sweep_of(&self, key: Option<&MessageKey>) -> Option<SweepOf> {
        let key = key.or_else(|| self.selection.first())?;
        let m = self.summary(key)?;
        let sender = m.from.email.trim();
        if sender.is_empty() {
            return None;
        }
        Some(SweepOf { account: key.account.clone(), folder: key.folder.clone(), sender: sender.to_string() })
    }

    /// How many messages each choice ([`SweepChoice::ALL`]'s order) removes now: of the folder's
    /// list as it's kept here.
    pub fn sweep_counts(&self, of: &SweepOf) -> [usize; 4] {
        let Some(state) = self.cache.get(&(of.account.clone(), of.folder.clone())) else { return [0; 4] };
        let before = self.clock.now.saturating_sub(SWEEP_DAYS * DAY);
        SweepChoice::ALL.map(|c| chosen(&state.messages, &of.sender, c, before).len())
    }

    /// Whether the sender's mail in that folder is swept each day.
    pub fn swept_each_day(&self, of: &SweepOf) -> bool {
        self.prefs.sweeps.iter().any(|w| w.is_of(of))
    }

    /// Sweep the sender of message `key` (the first selected when `None`) by `choice`: their
    /// mail in its folder to Deleted Items, as one Undo; with [`SweepChoice::AllAndLater`] a rule
    /// for what they send later, with [`SweepChoice::OlderThan`] a sweep each day.
    pub fn sweep(&mut self, key: Option<MessageKey>, choice: SweepChoice) -> CmdResult {
        let of = self.sweep_of(key.as_ref()).ok_or_else(|| CmdError::Disabled("select a message with a sender first".into()))?;
        let trash = self.account(&of.account).and_then(|a| a.folder(FolderRole::Trash)).map(|f| f.path.clone());
        if trash.as_deref() == Some(of.folder.as_str()) {
            return Err(CmdError::Disabled("it's in Deleted Items already".into()));
        }
        self.import_holds(&of.account)?;
        // What's kept for later first: when it can't be (the rules unreadable), nothing moves.
        match choice {
            SweepChoice::AllAndLater => self.sweep_rule(&of)?,
            SweepChoice::OlderThan => self.keep_sweeping(&of),
            SweepChoice::All | SweepChoice::KeepNewest => {}
        }
        let before = self.clock.now.saturating_sub(SWEEP_DAYS * DAY);
        let msgs = self.folder_state(&of.account, &of.folder).messages.clone();
        let gone = chosen(&msgs, &of.sender, choice, before);
        let moved = self.sweep_away(&of.account, &gone, true)?;
        Ok(serde_json::json!({ "moved": moved, "sender": of.sender, "eachDay": self.swept_each_day(&of) }))
    }

    /// No longer sweep `sender`'s mail in `folder` each day.
    pub fn stop_sweep(&mut self, account: &str, folder: &str, sender: &str) -> CmdResult {
        let of = SweepOf { account: account.to_string(), folder: folder.to_string(), sender: sender.to_string() };
        let before = self.prefs.sweeps.len();
        self.prefs.sweeps.retain(|w| !w.is_of(&of));
        if self.prefs.sweeps.len() == before {
            return Err(CmdError::Params("that mail isn't swept each day".into()));
        }
        Ok(serde_json::json!({ "stopped": true }))
    }

    /// A rule sending what `of`'s sender sends later to Deleted Items (once: one there is kept).
    fn sweep_rule(&mut self, of: &SweepOf) -> Result<(), CmdError> {
        let from = Condition::From(of.sender.clone());
        let has = |r: &Rule| r.account.as_deref() == Some(of.account.as_str()) && r.conditions == [from.clone()] && r.actions == [RuleAction::Delete];
        if self.rules.iter().any(has) {
            return Ok(());
        }
        let rule = Rule {
            name: of.sender.clone(),
            enabled: true,
            account: Some(of.account.clone()),
            conditions: vec![from],
            match_all: true,
            actions: vec![RuleAction::Delete],
            stop: true,
            sent: false,
        };
        let rules: Vec<Rule> = self.rules.iter().cloned().chain(std::iter::once(rule)).collect();
        self.set_rules(rules).map(|_| ())
    }

    /// Sweep `of` each day from now (once: one there already is kept).
    fn keep_sweeping(&mut self, of: &SweepOf) {
        if self.swept_each_day(of) || self.prefs.sweeps.len() >= STANDING_MAX {
            return;
        }
        let now = self.clock.now;
        self.prefs.sweeps.push(StandingSweep { account: of.account.clone(), folder: of.folder.clone(), sender: of.sender.clone(), ran: now });
    }

    /// The sweeps due each day (from [`Session::tick`]): each one's old mail to Deleted Items,
    /// with no Undo (it isn't what the user just did). One whose account an import holds waits.
    pub(in crate::session) fn sweeps_due(&mut self) {
        let now = self.clock.now;
        let due = |w: &StandingSweep| now >= w.ran.saturating_add(DAY);
        if !self.prefs.sweeps.iter().any(due) {
            return;
        }
        let ready: Vec<StandingSweep> = self.prefs.sweeps.iter().filter(|w| due(w) && !self.importing.held(&w.account)).cloned().collect();
        let before = now.saturating_sub(SWEEP_DAYS * DAY);
        for w in ready {
            // Marked run first: one that fails isn't tried again each frame.
            if let Some(x) = self.prefs.sweeps.iter_mut().find(|x| **x == w) {
                x.ran = now;
            }
            if self.folder(&w.account, &w.folder).is_none() {
                continue;
            }
            let msgs = self.folder_state(&w.account, &w.folder).messages.clone();
            let gone = chosen(&msgs, &w.sender, SweepChoice::OlderThan, before);
            if let Err(e) = self.sweep_away(&w.account, &gone, false) {
                log::warn!("a sweep couldn't run: {e}");
            }
        }
    }

    /// `gone` (all of `account`) to its Deleted Items, as one Undo when `undo`. How many moved.
    fn sweep_away(&mut self, account: &str, gone: &[SharedSummary], undo: bool) -> Result<usize, CmdError> {
        let trash = self
            .account(account)
            .and_then(|a| a.folder(FolderRole::Trash))
            .map(|f| f.path.clone())
            .ok_or_else(|| CmdError::Failed("this account has no Deleted Items folder".into()))?;
        let gone: Vec<&SharedSummary> = gone.iter().filter(|m| m.key.folder != trash).collect();
        if gone.is_empty() {
            return Ok(0);
        }
        let keys: Vec<MessageKey> = gone.iter().map(|m| m.key.clone()).collect();
        // Undo finds them again by Message-ID (those without one can't come back).
        let back: Vec<(String, String)> =
            gone.iter().filter(|m| !m.message_id.is_empty()).map(|m| (m.message_id.clone(), m.key.folder.clone())).collect();
        let set: HashSet<&MessageKey> = keys.iter().collect();
        let shown = self.reading.as_ref().is_some_and(|r| set.contains(&r.key)) || self.selection.iter().any(|k| set.contains(k));
        let next = if shown { self.successor(&keys) } else { None };
        let n = keys.len();
        self.apply_op_checked(account, Op::Move { keys, to: trash.clone() }).map_err(CmdError::Failed)?;
        if undo && !back.is_empty() {
            self.moved_at = self.clock.now;
            self.remember(Undo::Move { moves: vec![(account.to_string(), trash, back)] });
        }
        if shown {
            self.after_removal(next);
        }
        Ok(n)
    }
}
