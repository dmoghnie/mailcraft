//! Automations run: those due on their schedule as the window's tick comes (each frame, each
//! CLI run), up to [`LATE_MOST`] late, once for each time; those whose search matches mail
//! arriving, as it arrives, up to [`RUNS_A_DAY_MOST`] times a day; and the person told of each
//! run as it ends: what it answered, the changes it proposes, or why it failed.

use std::cmp::Reverse;

use mailcraft_model::{AiRole, MessageKey, Summary, dates};
use serde::Serialize;

use super::super::looking::said;
use super::super::{CONTEXT_ITEMS_MOST, ContextItem, RunState};
use super::{Automation, AutomationRun, AutomationWhen};
use crate::{CmdError, Session};

/// How late a run on schedule may still begin (MailCraft was closed at its time).
const LATE_MOST: i64 = 4 * 3600;
/// The most times one runs as mail arrives in a day: past it, it waits for the next day.
const RUNS_A_DAY_MOST: u32 = 24;
/// The most messages that arrived one run is given (the newest).
const MESSAGES_MOST: usize = CONTEXT_ITEMS_MOST;
/// The longest an answer told of is, in characters.
const TOLD_MOST: usize = 200;

/// A run that ended, for the window to tell the person of.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationReport {
    /// The automation's name.
    pub name: String,
    /// The chat it ran in.
    pub chat: Option<String>,
    pub said: ReportSaid,
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "text")]
pub enum ReportSaid {
    /// It answered: the start of its answer.
    Answered(String),
    /// It proposes this many changes, waiting for the person.
    Proposes(usize),
    /// It couldn't begin, or stopped: why.
    Failed(String),
}

/// What the model is told as a run begins: what set it off, how to go about it, and the job.
fn said_for(a: &Automation, scheduled: bool, arrived: bool) -> String {
    let how = match (arrived, scheduled) {
        (true, _) => "run by itself as mail arrived: the mail pointed at",
        (false, true) => "run by itself at its time",
        (false, false) => "run now as the person asked",
    };
    format!(
        "(The automation “{}”, {how}. The person may not be watching: don't ask them anything. Do what it says with your tools, propose every change it needs at once, for the person to approve later, and end with a short report of what you found and what you proposed.)\n\n{}",
        a.name, a.job
    )
}

/// The first line of `text`, cut short.
fn first_line(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    let mut told: String = line.chars().take(TOLD_MOST).collect();
    if line.chars().count() > TOLD_MOST {
        told.push('…');
    }
    told
}

impl Session {
    /// The automations due on their schedule run, and the runs that ended told of.
    pub(crate) fn automations_due(&mut self) {
        self.automations_load();
        let now = self.clock.now;
        let today = self.clock.local_day(now);
        let due: Vec<(String, i64)> = self
            .ai
            .assistant
            .automations
            .list()
            .iter()
            .filter(|a| !a.off)
            .filter_map(|a| {
                let AutomationWhen::Daily { at, days } = a.when else { return None };
                // Yesterday's time too: one late past midnight.
                let time = |day: i64| {
                    let on = 1u8.checked_shl(dates::weekday_index(day)).is_some_and(|d| days & d != 0);
                    let t = self.clock.utc(day.saturating_mul(86_400).saturating_add(i64::from(at) * 60));
                    (on && t <= now && now - t <= LATE_MOST && t > a.after).then_some(t)
                };
                time(today).or_else(|| time(today.saturating_sub(1))).map(|t| (a.name.clone(), t))
            })
            .collect();
        for (name, t) in due {
            if let Err(e) = self.automation_begin(&name, &[], Some(t)) {
                log::info!("an automation couldn't run at its time: {e}");
            }
        }
        self.automations_told();
    }

    /// Mail that arrived in an Inbox (what its rules left there): each automation it matches runs,
    /// given the newest of it.
    pub(crate) fn automations_arrived(&mut self, arrived: &[Summary]) {
        if arrived.is_empty() {
            return;
        }
        self.automations_load();
        let on: Vec<(String, String, Option<String>)> = self
            .ai
            .assistant
            .automations
            .list()
            .iter()
            .filter(|a| !a.off)
            .filter_map(|a| match &a.when {
                AutomationWhen::Arrives { query, account } => Some((a.name.clone(), query.clone(), account.clone())),
                AutomationWhen::Daily { .. } => None,
            })
            .collect();
        for (name, query, account) in on {
            let theirs: Vec<Summary> = arrived.iter().filter(|m| account.as_ref().is_none_or(|a| *a == m.key.account)).cloned().collect();
            let mut matched = self.arrived_matching(&query, &theirs);
            if matched.is_empty() {
                continue;
            }
            if !self.automation_may_run(&name) {
                log::info!("an automation ran as often as it may today, as mail arrived: it waits for tomorrow");
                continue;
            }
            matched.sort_by_key(|m| Reverse(m.date));
            let keys: Vec<MessageKey> = matched.into_iter().take(MESSAGES_MOST).map(|m| m.key.clone()).collect();
            if let Err(e) = self.automation_begin(&name, &keys, None) {
                log::info!("an automation couldn't run as mail arrived: {e}");
            }
        }
    }

    /// Whether automation `name` may run once more as mail arrives today: counted if so.
    fn automation_may_run(&mut self, name: &str) -> bool {
        let today = self.clock.local_day(self.clock.now);
        let runs = self.ai.assistant.automations.runs.entry(name.to_string()).or_insert((today, 0));
        if runs.0 != today {
            *runs = (today, 0);
        }
        if runs.1 >= RUNS_A_DAY_MOST {
            return false;
        }
        runs.1 = runs.1.saturating_add(1);
        true
    }

    /// Automation `name` run now (`automation.run`): in a chat of its own, the chat's id.
    pub(super) fn automation_run(&mut self, name: &str) -> Result<String, CmdError> {
        self.automation_begin(name, &[], None)
    }

    /// Automation `name` begun in a chat of its own, given `arrived` (the mail that set it off),
    /// for the time `slot` of its schedule: the chat's id, or why it couldn't begin (kept as its
    /// last run, and told of).
    fn automation_begin(&mut self, name: &str, arrived: &[MessageKey], slot: Option<i64>) -> Result<String, CmdError> {
        let a = self.automation_named(name)?;
        let context: Vec<ContextItem> = arrived.iter().map(|k| ContextItem::Message { id: k.to_id() }).collect();
        let text = said_for(&a, slot.is_some(), !context.is_empty());
        let chat = self.assistant_new(&a.name, None);
        let sent = self.assistant_send(Some(&chat), &text, &context, None);
        // One that couldn't be begun isn't kept: it would be empty.
        let begun = self.ai.assistant.chats.get(&chat).is_some_and(|c| !c.turns.is_empty());
        if !begun && let Err(e) = self.assistant_delete(&chat) {
            log::warn!("an automation's empty chat couldn't be deleted: {e}");
        }
        let run = AutomationRun {
            at: self.clock.now,
            chat: begun.then(|| chat.clone()),
            error: sent.as_ref().err().map(said),
            messages: context.len(),
            told: false,
        };
        self.automation_change(name, |a| Automation { last: Some(run.clone()), after: slot.map_or(a.after, |t| a.after.max(t)), ..a.clone() });
        sent.map(|_| chat)
    }

    /// The runs that ended since last looked at, told of (once each): one whose chat was
    /// deleted, silently.
    fn automations_told(&mut self) {
        let open: Vec<(String, Option<String>, Option<String>)> = self
            .ai
            .assistant
            .automations
            .list()
            .iter()
            .filter_map(|a| a.last.as_ref().filter(|r| !r.told).map(|r| (a.name.clone(), r.chat.clone(), r.error.clone())))
            .collect();
        for (name, chat, error) in open {
            let said = match &chat {
                None => Some(ReportSaid::Failed(error.unwrap_or_default())),
                Some(id) => match self.run_ended(id) {
                    Some(said) => said,
                    None => continue,
                },
            };
            self.automation_change(&name, |a| {
                let last = a.last.clone().map(|r| AutomationRun { told: true, ..r });
                Automation { last, ..a.clone() }
            });
            if let Some(said) = said {
                self.ai.assistant.automations.ready.push(AutomationReport { name, chat, said });
            }
        }
    }

    /// How the run in chat `id` ended: none while it goes on; `Some(None)` when its chat was
    /// deleted.
    fn run_ended(&mut self, id: &str) -> Option<Option<ReportSaid>> {
        if self.ai.assistant.asking(id).is_some() {
            return None;
        }
        self.assistant_load();
        if !self.ai.assistant.heads.iter().any(|h| h.id == id) {
            return Some(None);
        }
        let chat = match self.assistant_chat(id) {
            Ok(c) => c,
            Err(e) => return Some(Some(ReportSaid::Failed(said(&e)))),
        };
        if chat.runs.iter().any(|r| r.state == RunState::Waiting) {
            return None;
        }
        let proposed = chat.proposals().len();
        let said = if proposed > 0 {
            ReportSaid::Proposes(proposed)
        } else if let Some(e) = &chat.error {
            ReportSaid::Failed(e.clone())
        } else {
            let answer = chat.turns.iter().rev().find(|t| t.turn.role == AiRole::Assistant).map(|t| first_line(&t.turn.text));
            ReportSaid::Answered(answer.unwrap_or_default())
        };
        Some(Some(said))
    }

    /// The runs that ended since last asked, for the window to tell of.
    pub fn take_automation_reports(&mut self) -> Vec<AutomationReport> {
        std::mem::take(&mut self.ai.assistant.automations.ready)
    }
}
