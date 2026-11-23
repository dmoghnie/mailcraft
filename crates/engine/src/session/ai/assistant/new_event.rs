//! The event the assistant proposes adding to the person's calendar: checked as the model asks
//! for it, said in a line for its card, and added only once the person approves it (made again
//! then from what the model asked, so what's added is what the card said). It's the person's
//! alone: no one is invited, so nothing is sent. It goes only to a calendar of an account whose
//! mail the chat's model may read.

use mailcraft_model::ical::{self, CalEvent};
use mailcraft_model::pim::{Pim, is_web_calendar};
use serde_json::{Value, json};

use super::changes::Change;
use super::links::Found;
use super::looking::said;
use super::*;
use crate::Session;

/// A new event's reminder when the model doesn't say: the window's, as Outlook sets a new
/// appointment's.
const REMINDER_AT_FIRST: i64 = 15;
/// What the model may say of the event, and the field of the window's New event each is.
const FIELDS: [(&str, &str); 5] = [("summary", "summary"), ("start", "start"), ("end", "end"), ("location", "location"), ("notes", "description")];

impl Session {
    /// The event `add_event` asks for with `input`, checked; why not, for the model.
    pub(super) fn event_change(&self, chat: &Chat, input: &Value) -> Result<Change, String> {
        let text = |k: &str| input.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
        let profile = self.assistant_profile(chat).map_err(|e| said(&e))?;
        let account = match text("account") {
            Some(a) => match self.unreadable(a, &profile) {
                Some(why) => return Err(why),
                None => a.to_string(),
            },
            // The first account it may read whose calendar can take one.
            None => self
                .readable_accounts(&profile)
                .iter()
                .map(|a| a.config.id.clone())
                .find(|a| self.calendar_for(a, None).is_ok())
                .ok_or_else(|| format!("no calendar can take an event: no account whose mail may go to {} has one", profile.name))?,
        };
        let calendar = self.calendar_for(&account, text("calendar"))?;
        text("summary").ok_or("`summary` (its title) is required")?;
        text("start").ok_or("`start` is required")?;
        // Made as the window's New event makes one, from the same fields read the same way.
        let mut params = json!({"reminder": input.get("reminder").cloned().unwrap_or(json!(REMINDER_AT_FIRST))});
        for (from, to) in FIELDS {
            if let Some(v) = text(from) {
                params[to] = json!(v);
            }
        }
        let event = crate::cmd::event_from(self, &params, CalEvent::default()).map_err(|e| said(&e))?;
        crate::session::pim::check_event(&event).map_err(|e| said(&e))?;
        Ok(Change::Event { account, calendar, event: Box::new(event) })
    }

    /// The calendar of `account` an event is added to, by its id: `wanted` (its id or name),
    /// else the one new events go to; one that can be changed.
    fn calendar_for(&self, account: &str, wanted: Option<&str>) -> Result<String, String> {
        let fresh;
        let p = match self.pim.get(account) {
            Some(p) => p,
            // Its calendar is made when something's first added to it.
            None => {
                let mut p = Pim::default();
                p.ensure_defaults();
                fresh = p;
                &fresh
            }
        };
        let cal = match wanted {
            Some(w) => p.calendars.iter().find(|c| c.id == w || c.name.eq_ignore_ascii_case(w)).ok_or_else(|| {
                let names: Vec<&str> = p.calendars.iter().filter(|c| !c.read_only && !is_web_calendar(&c.id)).map(|c| c.name.as_str()).collect();
                format!(
                    "account {account} has no calendar `{}`: those an event can go in are {}",
                    w.chars().take(80).collect::<String>(),
                    names.join(", ")
                )
            })?,
            None => p.default_calendar().ok_or_else(|| format!("account {account} has no calendar an event can go in"))?,
        };
        if cal.read_only || is_web_calendar(&cal.id) {
            return Err(format!("{} can't be changed", cal.name));
        }
        Ok(cal.id.clone())
    }

    /// What adding `e` to `calendar` of `account` does, in a line for its card.
    pub(super) fn event_what(&self, account: &str, calendar: &str, e: &CalEvent) -> String {
        let whose = match self.account(account) {
            Some(a) if self.accounts.len() > 1 => format!(" ({})", a.config.email),
            _ => String::new(),
        };
        let (start, end) = shown(self, e);
        let mut line = format!(
            "Add “{}” to {}{whose}: {}",
            e.summary.chars().take(80).collect::<String>(),
            self.calendar_name_of(account, calendar),
            self.when_text(e.all_day, start, end)
        );
        if !e.location.trim().is_empty() {
            line.push_str(&format!(", at {}", e.location.trim().chars().take(80).collect::<String>()));
        }
        line.push_str(&match ical::reminder_minutes(e) {
            Some(0) => "; a reminder as it starts".to_string(),
            Some(m) => format!("; a reminder {} before", how_long(m)),
            None => "; no reminder".to_string(),
        });
        line
    }

    /// `event`, approved, added to `calendar` of `account`: what was done, and the event, for
    /// the answer to link.
    pub(super) fn add_event(&mut self, account: &str, calendar: &str, event: CalEvent) -> Result<(String, Found), String> {
        let (start, _) = shown(self, &event);
        let occurrence = event.start;
        let added = self.create_event(Some(account), Some(calendar), event).map_err(|e| said(&e))?;
        let uid = added.get("uid").and_then(Value::as_str).ok_or("the event wasn't added")?.to_string();
        let found = Found::Event { account: account.to_string(), calendar: calendar.to_string(), uid, occurrence, start };
        Ok((format!("Done: added to {}, as {}.", self.calendar_name_of(account, calendar), found.name()), found))
    }
}

/// When `e` is, as the calendar shows it: an all-day one from the person's midnight.
fn shown(s: &Session, e: &CalEvent) -> (i64, i64) {
    if e.all_day { (s.clock.utc(e.start), s.clock.utc(e.end)) } else { (e.start, e.end) }
}

/// Minutes, said: `15 minutes`, `1 hour`, `2 days`.
fn how_long(minutes: i64) -> String {
    let (n, unit) = match minutes {
        m if m % 1440 == 0 => (m / 1440, "day"),
        m if m % 60 == 0 => (m / 60, "hour"),
        m => (m, "minute"),
    };
    if n == 1 { format!("1 {unit}") } else { format!("{n} {unit}s") }
}
