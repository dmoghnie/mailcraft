//! The events the assistant proposes adding to the person's calendar, changing or deleting, in a
//! calendar that can be changed. One added is the person's alone: no one is invited. A meeting
//! with others is changed (but for what's the person's own: its reminder) or deleted only in the
//! calendar, where they're told; one that repeats has its time changed there too.

use mailcraft_model::ical::{self, CalEvent};
use mailcraft_model::pim::is_web_calendar;
use serde_json::{Value, json};

use super::*;

/// A new event's reminder when the model doesn't say: the window's, as Outlook sets a new
/// appointment's.
const REMINDER_AT_FIRST: i64 = 15;
/// What the model may say of the event, and the field of the window's New event each is.
const FIELDS: [(&str, &str); 5] = [("summary", "summary"), ("start", "start"), ("end", "end"), ("location", "location"), ("notes", "description")];

/// `input`'s fields of an event, as the window's form names them.
fn fields(input: &Value) -> Value {
    let mut params = json!({});
    for (from, to) in FIELDS {
        if let Some(v) = text(input, from) {
            params[to] = json!(v);
        }
    }
    params
}

impl Session {
    /// The event `add_event` asks for with `input`, checked, and its card's line.
    pub(super) fn event_to_add(&self, chat: &Chat, input: &Value) -> Result<(String, PimChange), String> {
        let account = self.account_for_new(
            chat,
            input,
            |a| self.calendar_for(a, None).is_ok(),
            |model| format!("no calendar can take an event: no account whose mail may go to {model} has one"),
        )?;
        let calendar = self.calendar_for(&account, text(input, "calendar"))?;
        text(input, "summary").ok_or("`summary` (its title) is required")?;
        text(input, "start").ok_or("`start` is required")?;
        // Made as the window's New event makes one, from the same fields read the same way.
        let mut params = fields(input);
        params["reminder"] = input.get("reminder").cloned().unwrap_or(json!(REMINDER_AT_FIRST));
        let event = crate::cmd::event_from(self, &params, CalEvent::default()).map_err(|e| said(&e))?;
        crate::session::pim::check_event(&event).map_err(|e| said(&e))?;
        let what = self.event_what(&account, &calendar, &event);
        Ok((what, PimChange::AddEvent { account, calendar, event }))
    }

    /// The change `change_event` asks for with `input`, checked, and its card's line.
    pub(super) fn event_to_change(&self, chat: &Chat, input: &Value) -> Result<(String, PimChange), String> {
        let found = self.found_of(chat, input, "event")?;
        let was = self.event_kept(&found)?;
        let mut params = fields(input);
        if let Some(r) = input.get("reminder") {
            params["reminder"] = r.clone();
        }
        if params.as_object().is_none_or(serde_json::Map::is_empty) {
            return Err("say what changes: summary, start, end, location, notes or reminder".into());
        }
        if was.rrule.is_some() && (params.get("start").is_some() || params.get("end").is_some()) {
            return Err(format!(
                "“{}” repeats: its time is changed in the calendar, not here (every time it repeats would move)",
                quoted(&was.summary)
            ));
        }
        let mut event = crate::cmd::event_from(self, &params, was.clone()).map_err(|e| said(&e))?;
        // Its time not asked to change, it stays in the zone it was in (the form puts one with
        // none in the person's): the same times, and nothing for a meeting's guests to hear of.
        if params.get("start").is_none() && params.get("end").is_none() {
            event.tzid.clone_from(&was.tzid);
        }
        crate::session::pim::check_event(&event).map_err(|e| said(&e))?;
        if event == *was {
            return Err("that's how it is already: nothing would change".into());
        }
        if !event.attendees.is_empty() && !ical::same_for_guests(was, &event) {
            return Err(with_others(was, "of the change"));
        }
        let what = self.event_change_what(&found, was, &event);
        Ok((what, PimChange::ChangeEvent { found, event }))
    }

    /// Deleting the event `found` names (every time it repeats, `series`), checked: how its card
    /// names it.
    pub(super) fn event_to_delete(&self, found: &Found, series: bool) -> Result<String, String> {
        let e = self.event_kept(found)?;
        if !e.attendees.is_empty() {
            return Err(with_others(e, "it's off"));
        }
        let title = quoted(&e.summary);
        Ok(match (e.rrule.is_some(), found) {
            (true, _) if series => format!("the event “{title}”, every time it repeats"),
            (true, Found::Event { start, .. }) => {
                let end = start.saturating_add(e.end.saturating_sub(e.start));
                format!("the event “{title}”, {} (that time only: it repeats)", self.when_text(e.all_day, *start, end))
            }
            _ => {
                let (start, end) = shown(self, e);
                format!("the event “{title}”, {}", self.when_text(e.all_day, start, end))
            }
        })
    }

    /// The event `found` names, as kept (its series, for one that repeats), when it may be
    /// changed.
    fn event_kept(&self, found: &Found) -> Result<&CalEvent, String> {
        let Found::Event { account, uid, .. } = found else { return Err(format!("{} isn't an event", found.name())) };
        let p = self.pim.get(account).ok_or("that event isn't in the calendar any more")?;
        let stored =
            p.events.iter().find(|e| e.event.uid == *uid && e.event.recurrence_id.is_none()).ok_or("that event isn't in the calendar any more")?;
        if p.calendars.iter().any(|c| c.id == stored.calendar && c.read_only) || is_web_calendar(&stored.calendar) {
            return Err(format!("{} can't be changed", self.calendar_name_of(account, &stored.calendar)));
        }
        if ical::details_hidden(&stored.event) {
            return Err("only this event's time can be seen: it can't be changed".into());
        }
        Ok(&stored.event)
    }

    /// The calendar of `account` an event is added to, by its id: `wanted` (its id or name),
    /// else the one new events go to; one that can be changed.
    fn calendar_for(&self, account: &str, wanted: Option<&str>) -> Result<String, String> {
        let p = self.pim_or_new(account);
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
    fn event_what(&self, account: &str, calendar: &str, e: &CalEvent) -> String {
        let whose = match self.account(account) {
            Some(a) if self.accounts.len() > 1 => format!(" ({})", a.config.email),
            _ => String::new(),
        };
        let (start, end) = shown(self, e);
        let mut line =
            format!("Add “{}” to {}{whose}: {}", quoted(&e.summary), self.calendar_name_of(account, calendar), self.when_text(e.all_day, start, end));
        if !e.location.trim().is_empty() {
            line.push_str(&format!(", at {}", quoted(&e.location)));
        }
        line.push_str(&format!("; {}", reminder_said(e)));
        line
    }

    /// What changing `was` (which `found` names) to `now` does, in a line for its card.
    fn event_change_what(&self, found: &Found, was: &CalEvent, now: &CalEvent) -> String {
        let mut how = Vec::new();
        if now.summary != was.summary {
            how.push(format!("its title to “{}”", quoted(&now.summary)));
        }
        if (now.start, now.end, now.all_day) != (was.start, was.end, was.all_day) {
            let (start, end) = shown(self, now);
            how.push(format!("to {}", self.when_text(now.all_day, start, end)));
        }
        if now.location != was.location {
            how.push(if now.location.trim().is_empty() { "no place".into() } else { format!("its place to {}", quoted(&now.location)) });
        }
        if now.description != was.description {
            how.push(format!("its notes to “{}”", quoted(&now.description)));
        }
        if ical::reminder_minutes(now) != ical::reminder_minutes(was) {
            how.push(reminder_said(now));
        }
        let when = if was.rrule.is_some() {
            " (every time it repeats)".to_string()
        } else {
            let (start, end) = shown(self, was);
            format!(", {},", self.when_text(was.all_day, start, end))
        };
        let Found::Event { account, calendar, .. } = found else { return String::new() };
        format!("Change “{}”{when} in {}: {}", quoted(&was.summary), self.calendar_name_of(account, calendar), how.join("; "))
    }

    /// `event`, approved, added to `calendar` of `account`: what was done, and the event, for
    /// the answer to link.
    pub(super) fn add_event(&mut self, account: &str, calendar: &str, event: CalEvent) -> Result<(String, Vec<Found>), String> {
        let (start, _) = shown(self, &event);
        let occurrence = event.start;
        let added = self.create_event(Some(account), Some(calendar), event).map_err(|e| said(&e))?;
        let uid = added.get("uid").and_then(Value::as_str).ok_or("the event wasn't added")?.to_string();
        let found = Found::Event { account: account.to_string(), calendar: calendar.to_string(), uid, occurrence, start };
        Ok((format!("Done: added to {}, as {}.", self.calendar_name_of(account, calendar), found.name()), vec![found]))
    }

    /// The event `found` names, approved, changed to `event`: one that doesn't repeat is named
    /// by when it is, so moved, it has a new name.
    pub(super) fn change_found_event(&mut self, found: Found, event: CalEvent) -> Result<(String, Vec<Found>), String> {
        let Found::Event { account, calendar, uid, occurrence, start } = found else { return Err("that isn't an event".into()) };
        // Changed in the account it was found in, where it was checked: one invited at two of
        // the person's addresses has the same uid in both.
        let at = account.clone();
        let now = if event.rrule.is_some() {
            Found::Event { account, calendar, uid: uid.clone(), occurrence, start }
        } else {
            Found::Event { account, calendar, uid: uid.clone(), occurrence: event.start, start: shown(self, &event).0 }
        };
        self.update_event(Some(&at), &uid, |x| *x = event).map_err(|e| said(&e))?;
        Ok((format!("Done: changed; it's {} now.", now.name()), vec![now]))
    }

    /// The event `found` names, approved, deleted: only the time it names, of one that repeats,
    /// unless `series`.
    pub(super) fn delete_found_event(&mut self, found: &Found, series: bool) -> Result<(), String> {
        let repeats = self.event_kept(found)?.rrule.is_some();
        let Found::Event { account, uid, occurrence, .. } = found else { return Err("that isn't an event".into()) };
        let only = if repeats && !series { Some(*occurrence) } else { None };
        self.delete_event(Some(account), uid, only).map(drop).map_err(|e| said(&e))
    }
}

/// Why `e`, a meeting with others, isn't changed or deleted here: they'd have to be told `what`.
fn with_others(e: &CalEvent, what: &str) -> String {
    format!(
        "“{}” is a meeting with others: they'd have to be told {what}, and MailCraft's AI tells no one; the person can do it in the calendar",
        quoted(&e.summary)
    )
}

/// `e`'s reminder, said.
fn reminder_said(e: &CalEvent) -> String {
    match ical::reminder_minutes(e) {
        Some(0) => "a reminder as it starts".into(),
        Some(m) => format!("a reminder {} before", how_long(m)),
        None => "no reminder".into(),
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
