//! The assistant's tools that look in the calendar, the contacts, the tasks and the notes: run
//! at once, changing nothing. Each gives only what's of an account whose mail may go to the
//! chat's model, and names each event, person, task and note found, so an answer can link to it
//! (and a change can name it).

use mailcraft_model::dates;
use mailcraft_model::pim::{Task, TaskPriority, TaskStatus};
use serde_json::Value;

use super::context::cut;
use super::links::Found;
use super::tools::Outcome;
use super::*;
use crate::{EventHit, EventWhen, Session};

/// The most events or people a look gives at once, and when the model doesn't say.
const FOUND_MOST: usize = 50;
const FOUND_AT_FIRST: usize = 20;
/// The most of an event's notes given, in characters.
const NOTES_CHARS: usize = 300;
/// The most attendees named for an event.
const ATTENDEES_MOST: usize = 12;
/// The most of a note's text given, in characters.
const NOTE_CHARS: usize = 500;

/// How many to give: what the model asked, within bounds.
fn how_many(input: &Value) -> usize {
    input.get("limit").and_then(Value::as_u64).map_or(FOUND_AT_FIRST, |n| usize::try_from(n).unwrap_or(FOUND_MOST).clamp(1, FOUND_MOST))
}

fn text<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty())
}

impl Session {
    /// `find_events`: events in the calendars shown, as the calendar's search box finds them;
    /// with no query, the next ones.
    pub(super) fn tool_find_events(&self, chat: &mut Chat, profile: &AiProfile, input: &Value) -> Outcome {
        let query = text(input, "query").unwrap_or_default();
        let what = if query.is_empty() {
            "List the next events".to_string()
        } else {
            format!("Find events “{}”", query.chars().take(80).collect::<String>())
        };
        let when = match text(input, "when") {
            Some(w) => match EventWhen::from_code(w) {
                Some(w) => w,
                None => return Outcome::failed(&what, format!("`when` is all, upcoming or past, not `{}`", w.chars().take(20).collect::<String>())),
            },
            None if query.is_empty() => EventWhen::Upcoming,
            None => EventWhen::All,
        };
        let account = text(input, "account");
        if let Some(why) = account.and_then(|a| self.unreadable(a, profile)) {
            return Outcome::failed(&what, why);
        }
        let most = how_many(input);
        // Every account's found, then those the model may see: the order is the search's.
        let all = self.find_events(query, when, account, None, crate::EVENT_HITS_MAX);
        let total = all.iter().filter(|h| self.prefs.ai.allows(profile, Some(&h.account))).count();
        let hits: Vec<&EventHit> = all.iter().filter(|h| self.prefs.ai.allows(profile, Some(&h.account))).take(most).collect();
        let found: Vec<Found> = hits.iter().map(|h| event_found(h)).collect();
        let mut out: Vec<String> = hits.iter().zip(&found).map(|(h, f)| self.event_row(h, f)).collect();
        for h in &hits {
            chat.holds(&h.account);
        }
        out.push(match total {
            0 => "No events found.".to_string(),
            n if n > hits.len() => format!("{n} found in all; these are the first {}.", hits.len()),
            n => format!("{n} found."),
        });
        Outcome::listed(&what, found, out.join("\n"))
    }

    /// An event found, in a line: its name first, then when, what, where, and who.
    fn event_row(&self, h: &EventHit, found: &Found) -> String {
        let mut row = format!("[{}] {} | {}", found.name(), self.when_text(h.all_day, h.start, h.end), h.summary);
        if !h.location.trim().is_empty() {
            row.push_str(&format!(" | Where: {}", h.location.trim()));
        }
        row.push_str(&format!(" | Calendar: {}", self.calendar_name_of(&h.account, &h.calendar)));
        if h.recurring {
            row.push_str(" | repeats");
        }
        if !h.upcoming {
            row.push_str(" | past");
        }
        if let Some(e) = self.event_at(h) {
            if let Some(o) = &e.organizer {
                row.push_str(&format!(" | Organizer: {}", who(o)));
            }
            if !e.attendees.is_empty() {
                let names: Vec<String> = e.attendees.iter().take(ATTENDEES_MOST).map(who).collect();
                let more = e.attendees.len().saturating_sub(ATTENDEES_MOST);
                let more = if more > 0 { format!(" and {more} more") } else { String::new() };
                row.push_str(&format!(" | Attendees: {}{more}", names.join(", ")));
            }
            let notes = e.description.split_whitespace().collect::<Vec<_>>().join(" ");
            if !notes.is_empty() {
                row.push_str(&format!(" | Notes: {}", cut(&notes, NOTES_CHARS)));
            }
        }
        row
    }

    /// When an event is, from `start` to `end` as the calendar shows them (an all-day one from
    /// the person's midnight): its days, or its day and times.
    pub(super) fn when_text(&self, all_day: bool, start: i64, end: i64) -> String {
        if all_day {
            let (first, last) = (self.clock.local_day(start), self.clock.local_day(end.saturating_sub(1).max(start)));
            let first_day = dates::day_text(first, &self.clock);
            if last > first { format!("{first_day} to {}, all day", dates::day_text(last, &self.clock)) } else { format!("{first_day}, all day") }
        } else {
            format!("{} to {}", dates::full_time(start, &self.clock), dates::hm(end, &self.clock))
        }
    }

    /// The event a hit shows: its changed occurrence when it is one, else its series.
    fn event_at(&self, h: &EventHit) -> Option<&mailcraft_model::pim::ical::CalEvent> {
        let events = &self.pim.get(&h.account)?.events;
        let mine = || events.iter().filter(|e| e.calendar == h.calendar && e.event.uid == h.uid).map(|e| &e.event);
        mine().find(|e| e.recurrence_id == Some(h.occurrence)).or_else(|| mine().find(|e| e.recurrence_id.is_none()))
    }

    pub(super) fn calendar_name_of(&self, account: &str, calendar: &str) -> String {
        self.pim.get(account).and_then(|p| p.calendars.iter().find(|c| c.id == calendar)).map_or_else(|| calendar.to_string(), |c| c.name.clone())
    }

    /// `find_people`: people in the contacts, by name, organization or address; with no query,
    /// everyone (by name).
    pub(super) fn tool_find_people(&self, chat: &mut Chat, profile: &AiProfile, input: &Value) -> Outcome {
        let query = text(input, "query").unwrap_or_default();
        let what = if query.is_empty() {
            "List the contacts".to_string()
        } else {
            format!("Find people “{}”", query.chars().take(80).collect::<String>())
        };
        let most = how_many(input);
        let people: Vec<_> = self.people(query).into_iter().filter(|p| self.prefs.ai.allows(profile, Some(p.account))).collect();
        let found: Vec<Found> = people.iter().take(most).map(|p| Found::Person { account: p.account.to_string(), uid: p.card.uid.clone() }).collect();
        let mut out = Vec::new();
        for (p, f) in people.iter().zip(&found) {
            chat.holds(p.account);
            let c = p.card;
            let mut row = format!("[{}] {}", f.name(), c.display());
            let emails: Vec<&str> = c.emails.iter().map(|e| e.value.as_str()).collect();
            if !emails.is_empty() {
                row.push_str(&format!(" | {}", emails.join(", ")));
            }
            let phones: Vec<&str> = c.phones.iter().map(|e| e.value.as_str()).collect();
            if !phones.is_empty() {
                row.push_str(&format!(" | Phone: {}", phones.join(", ")));
            }
            let work: Vec<&str> = [c.title.trim(), c.org.trim()].into_iter().filter(|x| !x.is_empty()).collect();
            if !work.is_empty() {
                row.push_str(&format!(" | {}", work.join(", ")));
            }
            out.push(row);
        }
        out.push(match people.len() {
            0 => "No one found.".to_string(),
            n if n > found.len() => format!("{n} found in all; these are the first {}.", found.len()),
            n => format!("{n} found."),
        });
        Outcome::listed(&what, found, out.join("\n"))
    }

    /// `find_tasks`: the person's tasks, as the Tasks list finds them, in its order.
    pub(super) fn tool_find_tasks(&self, chat: &mut Chat, profile: &AiProfile, input: &Value) -> Outcome {
        let query = text(input, "query").unwrap_or_default();
        let what =
            if query.is_empty() { "List the tasks".to_string() } else { format!("Find tasks “{}”", query.chars().take(80).collect::<String>()) };
        let most = how_many(input);
        let today = dates::today(&self.clock);
        let tasks: Vec<_> = self.tasks(query).into_iter().filter(|t| self.prefs.ai.allows(profile, Some(t.account))).collect();
        let found: Vec<Found> = tasks.iter().take(most).map(|t| Found::Task { account: t.account.to_string(), uid: t.task.uid.clone() }).collect();
        let mut out = Vec::new();
        for (t, f) in tasks.iter().zip(&found) {
            chat.holds(t.account);
            let task = t.task;
            let mut row = format!("[{}] {}", f.name(), task.title.trim());
            if let Some(d) = task.due_day() {
                let late = if task.overdue(today) { " (late)" } else { "" };
                row.push_str(&format!(" | Due: {}{late}", dates::day_text(d, &self.clock)));
            }
            row.push_str(&format!(" | {}", status_said(task)));
            if task.priority != TaskPriority::Normal {
                row.push_str(&format!(" | {} priority", task.priority.code()));
            }
            if task.rrule.is_some() {
                row.push_str(" | repeats");
            }
            if let Some(at) = task.reminder {
                row.push_str(&format!(" | Reminder: {}", dates::full_time(at, &self.clock)));
            }
            if !task.categories.is_empty() {
                row.push_str(&format!(" | Categories: {}", task.categories.join(", ")));
            }
            let notes = task.notes.split_whitespace().collect::<Vec<_>>().join(" ");
            if !notes.is_empty() {
                row.push_str(&format!(" | Notes: {}", cut(&notes, NOTES_CHARS)));
            }
            out.push(row);
        }
        let hidden = if self.prefs.tasks_hide_done { " Done ones are hidden in the Tasks list, so they're left out." } else { "" };
        out.push(match tasks.len() {
            0 => format!("No tasks found.{hidden}"),
            n if n > found.len() => format!("{n} found in all; these are the first {}.{hidden}", found.len()),
            n => format!("{n} found.{hidden}"),
        });
        Outcome::listed(&what, found, out.join("\n"))
    }

    /// `find_notes`: the person's notes, by words in them, as the Notes list orders them.
    pub(super) fn tool_find_notes(&self, chat: &mut Chat, profile: &AiProfile, input: &Value) -> Outcome {
        let query = text(input, "query").unwrap_or_default();
        let what =
            if query.is_empty() { "List the notes".to_string() } else { format!("Find notes “{}”", query.chars().take(80).collect::<String>()) };
        let most = how_many(input);
        let notes: Vec<_> = self.notes(query).into_iter().filter(|n| self.prefs.ai.allows(profile, Some(n.account))).collect();
        let found: Vec<Found> = notes.iter().take(most).map(|n| Found::Note { account: n.account.to_string(), uid: n.note.uid.clone() }).collect();
        let mut out = Vec::new();
        for (n, f) in notes.iter().zip(&found) {
            chat.holds(n.account);
            let lines: Vec<&str> = n.note.text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
            let mut row =
                format!("[{}] Changed {} | {}", f.name(), dates::full_time(n.note.modified, &self.clock), cut(&lines.join(" / "), NOTE_CHARS));
            let files = n.note.kept_files().len();
            if files > 0 {
                row.push_str(&format!(" | {files} file{}", if files == 1 { "" } else { "s" }));
            }
            out.push(row);
        }
        out.push(match notes.len() {
            0 => "No notes found.".to_string(),
            n if n > found.len() => format!("{n} found in all; these are the first {}.", found.len()),
            n => format!("{n} found."),
        });
        Outcome::listed(&what, found, out.join("\n"))
    }
}

/// What names an event found: its account, calendar, series and the occurrence shown.
fn event_found(h: &EventHit) -> Found {
    Found::Event { account: h.account.clone(), calendar: h.calendar.clone(), uid: h.uid.clone(), occurrence: h.occurrence, start: h.start }
}

/// Someone at an event: their name and address.
fn who(a: &mailcraft_model::pim::ical::Attendee) -> String {
    if a.name.trim().is_empty() { a.email.clone() } else { format!("{} <{}>", a.name.trim(), a.email) }
}

/// How far along a task is, said.
fn status_said(t: &Task) -> String {
    match t.status {
        TaskStatus::NotStarted => "not started".into(),
        TaskStatus::InProgress => format!("in progress, {}%", t.percent),
        TaskStatus::Completed => "done".into(),
        TaskStatus::Waiting => "waiting on someone else".into(),
        TaskStatus::Deferred => "deferred".into(),
    }
}
