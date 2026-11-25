//! Automations: a job said once in words, which MailCraft's AI does by itself each day at a
//! time, or as mail a search matches arrives in an Inbox. Each run is a chat of its own, begun
//! with the job (and the mail that arrived, pointed at): the model works with its tools as it
//! does when the person asks, and a change it calls for is proposed there, for the person to
//! approve, never made by itself. As a run ends, the person is told ([`due`]). They're kept in
//! the data folder (`ai-automations.json`); one turned off doesn't run.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{CmdError, CmdResult, CommandSpec, Session, p};

mod due;

pub use due::{AutomationReport, ReportSaid};

/// Where the automations are, in the data folder.
const AUTOMATIONS_DOC: &str = "ai-automations.json";
/// The most automations kept.
const AUTOMATIONS_MOST: usize = 40;
/// The longest name, and the longest job, in characters.
const NAME_MOST: usize = 60;
const JOB_MOST: usize = 4_000;
/// The longest search, in characters.
const QUERY_MOST: usize = 500;
/// The minutes in a day.
const DAY_MINUTES: u16 = 24 * 60;
/// Every day of the week, Monday the lowest bit.
const EVERY_DAY: u8 = 0x7f;
/// The time a new one runs at, when not said: 8 in the morning.
const AT_FIRST: u16 = 8 * 60;
/// Where the commands are.
const WHERE: &str = "AI assistant";

/// A job MailCraft's AI does by itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Automation {
    /// What the person calls it: each one's own.
    pub name: String,
    /// What to do, in the person's words, as they'd ask MailCraft's AI.
    pub job: String,
    pub when: AutomationWhen,
    /// Turned off: kept, but not run.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub off: bool,
    /// It runs on its schedule for times after this only (Unix seconds): when it was saved or
    /// turned on, then the time of its last run, so none runs for a time before it was made,
    /// or twice for one.
    pub after: i64,
    /// Its last run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<AutomationRun>,
}

/// When an automation runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AutomationWhen {
    /// Each day at `at` (minutes after midnight, where the person is), on the days of `days`
    /// (Monday the lowest bit).
    Daily {
        #[serde(default = "at_first")]
        at: u16,
        #[serde(default = "every_day")]
        days: u8,
    },
    /// As mail arrives in an Inbox (what its rules leave there) that `query` matches, as typed
    /// in the search box (empty: all of it), of `account` (none: any).
    Arrives {
        #[serde(default)]
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account: Option<String>,
    },
}

impl Default for AutomationWhen {
    fn default() -> Self {
        AutomationWhen::Daily { at: AT_FIRST, days: EVERY_DAY }
    }
}

fn at_first() -> u16 {
    AT_FIRST
}

fn every_day() -> u8 {
    EVERY_DAY
}

/// An automation's run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AutomationRun {
    /// When it began (Unix seconds).
    pub at: i64,
    /// The chat it runs in; none when it couldn't begin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
    /// Why it couldn't begin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// How many messages that arrived it was given.
    #[serde(skip_serializing_if = "is_zero")]
    pub messages: usize,
    /// It ended, and the person was told.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub told: bool,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// The automations as they run: those kept, once read, and what's to be told of them.
#[derive(Debug, Default)]
pub struct Automations {
    list: Option<Vec<Automation>>,
    /// The file couldn't be read: it isn't saved over.
    unread: bool,
    /// How many times each ran as mail arrived, on the day it last did: (day, runs).
    runs: HashMap<String, (i64, u32)>,
    /// The runs that ended, for the window to tell of.
    ready: Vec<AutomationReport>,
}

impl Automations {
    /// The automations kept, once read (the window's tick reads them).
    pub fn list(&self) -> &[Automation] {
        self.list.as_deref().unwrap_or_default()
    }
}

/// `a` as it's kept (trimmed), or why it can't be.
fn checked(a: Automation) -> Result<Automation, String> {
    let name = a.name.trim().to_string();
    if name.is_empty() || name.chars().count() > NAME_MOST || name.chars().any(char::is_control) {
        return Err(format!("an automation's name is from 1 to {NAME_MOST} characters, on one line"));
    }
    let job = a.job.trim().to_string();
    if job.is_empty() || job.chars().count() > JOB_MOST {
        return Err(format!("say what it does, in at most {JOB_MOST} characters"));
    }
    let when = match a.when {
        AutomationWhen::Daily { at, days } => {
            if at >= DAY_MINUTES {
                return Err("its time is in the day: from 00:00 to 23:59".into());
            }
            if days & EVERY_DAY == 0 {
                return Err("it runs on one day of the week at least".into());
            }
            AutomationWhen::Daily { at, days: days & EVERY_DAY }
        }
        AutomationWhen::Arrives { query, account } => {
            let query = query.trim().to_string();
            if query.chars().count() > QUERY_MOST {
                return Err(format!("its search is at most {QUERY_MOST} characters"));
            }
            let account = account.map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
            AutomationWhen::Arrives { query, account }
        }
    };
    Ok(Automation { name, job, when, ..a })
}

/// What `when` says in params: `{"kind": "daily", "at": "08:00" or minutes, "days"?: [0-6] or
/// a mask}`, or `{"kind": "arrives", "query"?, "account"?}`.
fn when_of(v: Option<&Value>) -> Result<AutomationWhen, CmdError> {
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(AutomationWhen::default()) };
    let s = |k: &str| v.get(k).and_then(Value::as_str);
    match s("kind") {
        Some("daily") => {
            let at = match v.get("at") {
                None | Some(Value::Null) => AT_FIRST,
                Some(Value::String(t)) => minutes_of(t).ok_or_else(|| CmdError::Params("`at` is a time, as 08:00".into()))?,
                Some(n) => n.as_u64().and_then(|n| u16::try_from(n).ok()).ok_or_else(|| CmdError::Params("`at` is a time, as 08:00".into()))?,
            };
            let days = match v.get("days") {
                None | Some(Value::Null) => EVERY_DAY,
                Some(Value::Array(ds)) => {
                    ds.iter().try_fold(0u8, |all, d| d.as_u64().filter(|d| *d < 7).map(|d| all | (1u8 << d))).ok_or_else(days_said)?
                }
                Some(n) => n.as_u64().and_then(|n| u8::try_from(n).ok()).ok_or_else(days_said)?,
            };
            Ok(AutomationWhen::Daily { at, days })
        }
        Some("arrives") => {
            Ok(AutomationWhen::Arrives { query: s("query").unwrap_or_default().to_string(), account: s("account").map(str::to_string) })
        }
        _ => Err(CmdError::Params("`when` is {\"kind\": \"daily\", \"at\": \"08:00\"} or {\"kind\": \"arrives\", \"query\": …}".into())),
    }
}

fn days_said() -> CmdError {
    CmdError::Params("`days` are the days of the week, from 0 (Monday) to 6".into())
}

/// `08:00` in minutes after midnight.
fn minutes_of(t: &str) -> Option<u16> {
    let (h, m) = t.trim().split_once(':')?;
    let (h, m) = (h.trim().parse::<u16>().ok()?, m.trim().parse::<u16>().ok()?);
    (h < 24 && m < 60).then(|| h * 60 + m)
}

impl Session {
    /// The automations read from the data folder, the first time they're wanted.
    pub(crate) fn automations_load(&mut self) {
        let a = &mut self.ai.assistant.automations;
        if a.list.is_some() {
            return;
        }
        let read = self.store.as_ref().map(|s| s.load_doc::<Option<Vec<Automation>>>(AUTOMATIONS_DOC));
        let list = match read {
            Some(Ok(list)) => list.unwrap_or_default(),
            Some(Err(e)) => {
                log::warn!("the automations kept couldn't be read: {e}");
                a.unread = true;
                Vec::new()
            }
            None => Vec::new(),
        };
        a.list = Some(list);
    }

    /// The automation named `name`.
    fn automation(&self, name: &str) -> Option<&Automation> {
        self.ai.assistant.automations.list().iter().find(|a| a.name == name)
    }

    /// The automations as they are now, kept in the data folder.
    fn automations_put(&mut self, list: Vec<Automation>) -> Result<(), CmdError> {
        if self.ai.assistant.automations.unread {
            return Err(CmdError::Failed(
                "the automations kept couldn't be read, so they aren't saved over: try again once MailCraft starts again".into(),
            ));
        }
        if let Some(store) = &self.store {
            store.save_doc(AUTOMATIONS_DOC, &Some(&list)).map_err(|e| CmdError::Failed(format!("the automations couldn't be kept: {e}")))?;
        }
        self.ai.assistant.automations.list = Some(list);
        Ok(())
    }

    /// The automation named `name` changed by `change`, and kept; why not, logged.
    fn automation_change(&mut self, name: &str, change: impl Fn(&Automation) -> Automation) {
        let list = self.ai.assistant.automations.list().iter().map(|a| if a.name == name { change(a) } else { a.clone() }).collect();
        if let Err(e) = self.automations_put(list) {
            log::warn!("an automation's run couldn't be kept: {e}");
        }
    }

    /// `a` kept, in place of the one named `was` (none: a new one), checked first: it runs for
    /// times from now on, and keeps its last run.
    fn automation_save(&mut self, a: Automation, was: Option<&str>) -> Result<Automation, CmdError> {
        let a = checked(a).map_err(CmdError::Params)?;
        if let AutomationWhen::Arrives { account: Some(id), .. } = &a.when
            && self.account(id).is_none()
        {
            return Err(CmdError::Params(format!("no account {id}")));
        }
        self.automations_load();
        if let Some(was) = was {
            self.automation_named(was)?;
        }
        let list = self.ai.assistant.automations.list();
        let others: Vec<Automation> = list.iter().filter(|k| Some(k.name.as_str()) != was).cloned().collect();
        if others.iter().any(|k| k.name == a.name) {
            return Err(CmdError::Params(format!("an automation named {} is kept already", a.name)));
        }
        if others.len() >= AUTOMATIONS_MOST {
            return Err(CmdError::Failed(format!("at most {AUTOMATIONS_MOST} automations are kept: delete one first")));
        }
        let old = list.iter().find(|k| Some(k.name.as_str()) == was);
        let a = Automation { after: self.clock.now, last: old.and_then(|o| o.last.clone()), ..a };
        // In its old place, renamed or changed; a new one last.
        let at = list.iter().position(|k| Some(k.name.as_str()) == was).unwrap_or(others.len());
        let mut kept = others;
        kept.insert(at.min(kept.len()), a.clone());
        self.automations_put(kept)?;
        Ok(a)
    }

    /// The automation named `name`, or why there's none.
    fn automation_named(&mut self, name: &str) -> Result<Automation, CmdError> {
        self.automations_load();
        self.automation(name).cloned().ok_or_else(|| CmdError::Params(format!("no automation named {name}")))
    }

    /// The automation named `name` deleted.
    fn automation_delete(&mut self, name: &str) -> Result<(), CmdError> {
        self.automation_named(name)?;
        let list = self.ai.assistant.automations.list().iter().filter(|a| a.name != name).cloned().collect();
        self.automations_put(list)
    }

    /// The automation named `name` turned on (for times from now on) or off.
    fn automation_turn(&mut self, name: &str, on: bool) -> Result<(), CmdError> {
        self.automation_named(name)?;
        let now = self.clock.now;
        let list = self
            .ai
            .assistant
            .automations
            .list()
            .iter()
            .map(|a| match (a.name == name, on) {
                (true, true) => Automation { off: false, after: a.after.max(now), ..a.clone() },
                (true, false) => Automation { off: true, ..a.clone() },
                _ => a.clone(),
            })
            .collect();
        self.automations_put(list)
    }
}

/// The automations' commands: listed, saved, turned on or off, deleted, and run now.
pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("automation.list", "List the automations of MailCraft's AI", WHERE, |s, _| {
            s.automations_load();
            Ok(json!({"automations": s.ai.assistant.automations.list()}))
        }),
        CommandSpec::new("automation.save", "Keep an automation for MailCraft's AI", WHERE, save).params(
            r#"{"name": string, "job": string (what to do, in words, as you'd ask MailCraft's AI), "when"?: {"kind": "daily", "at"?: "08:00", "days"?: [0-6] (Monday 0; every day when not given)} or {"kind": "arrives", "query"?: string (as typed in the search box; all mail when not given), "account"?: string}, "off"?: bool, "was"?: string (the name of the one it changes; none: a new one)}"#,
        ),
        CommandSpec::new("automation.turn", "Turn an automation on or off", WHERE, |s, v| {
            let name = p::req_str(v, "name")?.to_string();
            let on = p::bool(v, "on").ok_or_else(|| CmdError::Params("`on` (bool) is required".into()))?;
            s.automation_turn(&name, on)?;
            Ok(json!({"name": name, "on": on}))
        })
        .params(r#"{"name": string, "on": bool}"#),
        CommandSpec::new("automation.delete", "Delete an automation", WHERE, |s, v| {
            let name = p::req_str(v, "name")?.to_string();
            s.automation_delete(&name)?;
            Ok(json!({"deleted": name}))
        })
        .params(r#"{"name": string}"#),
        CommandSpec::new("automation.run", "Run an automation now", WHERE, |s, v| {
            let name = p::req_str(v, "name")?.to_string();
            let chat = s.automation_run(&name)?;
            Ok(json!({"chat": chat}))
        })
        .params(r#"{"name": string} (it runs in a chat of its own, given no mail; any change it calls for waits there for approval)"#),
    ]
}

fn save(s: &mut Session, v: &Value) -> CmdResult {
    let str_of = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let when = when_of(v.get("when"))?;
    let a = Automation { name: str_of("name"), job: str_of("job"), when, off: p::bool(v, "off").unwrap_or(false), ..Automation::default() };
    let was = v.get("was").and_then(Value::as_str).map(str::to_string);
    let kept = s.automation_save(a, was.as_deref())?;
    Ok(json!({"name": kept.name}))
}
