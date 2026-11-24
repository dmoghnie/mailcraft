//! The changes the assistant proposes to the calendar, the contacts, the tasks and the notes:
//! each checked as the model asks for it, said in a line for its card, and made only once the
//! person approves it (checked and made again then from what the model asked, so what's made is
//! what the card said). Each is of an account whose mail the chat's model may read, and what it
//! changes or deletes is something the chat has found. Nothing is sent: no one is invited, and a
//! meeting with others is changed or deleted in the calendar, where they're told.

use std::borrow::Cow;

use mailcraft_model::ical::CalEvent;
use mailcraft_model::pim::{Pim, Task};
use mailcraft_model::vcard::Card;
use serde_json::Value;

use super::changes::Change;
use super::links::Found;
use super::looking::said;
use super::*;
use crate::Session;

mod events;
mod people;
mod todo;

/// The most a deletion names.
const DELETE_MOST: usize = 50;
/// The most of them its card names.
const NAMED_MOST: usize = 5;
/// The most characters of a title, a name or a text a card quotes.
const QUOTED_CHARS: usize = 80;

/// A change to the calendar, the contacts, the tasks or the notes, checked. What's changed or
/// deleted is named as the chat found it.
pub(super) enum PimChange {
    AddEvent { account: String, calendar: String, event: CalEvent },
    ChangeEvent { found: Found, event: CalEvent },
    AddPerson { account: String, card: Card },
    ChangePerson { found: Found, card: Card },
    AddTask { account: String, task: Task },
    ChangeTask { found: Found, task: Task, done: Option<bool> },
    AddNote { account: String, text: String },
    ChangeNote { found: Found, text: String },
    Delete { items: Vec<Found>, series: bool },
}

/// A string of `input`, trimmed; none when it's empty.
fn text<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty())
}

/// `noun` with its article: `an event`, `a note`.
fn a(noun: &str) -> String {
    if noun.starts_with(['a', 'e', 'i', 'o', 'u']) { format!("an {noun}") } else { format!("a {noun}") }
}

/// `s` as a card quotes it: its first characters.
fn quoted(s: &str) -> String {
    let s = s.trim();
    let cut: String = s.chars().take(QUOTED_CHARS).collect();
    if cut.len() < s.len() { format!("{cut}…") } else { cut }
}

impl Session {
    /// The change tool `name` asks for with `input`, checked and said in a line for its card;
    /// none when it isn't one of these.
    pub(super) fn pim_change_of(&self, chat: &Chat, name: &str, input: &Value) -> Option<Result<Change, String>> {
        let checked = match name {
            "add_event" => self.event_to_add(chat, input),
            "change_event" => self.event_to_change(chat, input),
            "add_person" => self.person_to_add(chat, input),
            "change_person" => self.person_to_change(chat, input),
            "add_task" => self.task_to_add(chat, input),
            "change_task" => self.task_to_change(chat, input),
            "add_note" => self.note_to_add(chat, input),
            "change_note" => self.note_to_change(chat, input),
            "delete_items" => self.items_to_delete(chat, input),
            _ => return None,
        };
        Some(checked.map(|(what, change)| Change::Pim { what, change: Box::new(change) }))
    }

    /// Make `change`, approved: what was done, and what it made or changed, for the answer to
    /// link.
    pub(super) fn apply_pim(&mut self, change: PimChange) -> Result<(String, Vec<Found>), String> {
        match change {
            PimChange::AddEvent { account, calendar, event } => self.add_event(&account, &calendar, event),
            PimChange::ChangeEvent { found, event } => self.change_found_event(found, event),
            PimChange::AddPerson { account, card } => self.add_person(&account, card),
            PimChange::ChangePerson { found, card } => self.change_found_person(found, card),
            PimChange::AddTask { account, task } => self.add_new_task(&account, task),
            PimChange::ChangeTask { found, task, done } => self.change_found_task(found, task, done),
            PimChange::AddNote { account, text } => self.add_new_note(&account, &text),
            PimChange::ChangeNote { found, text } => self.change_found_note(found, &text),
            PimChange::Delete { items, series } => self.delete_found(&items, series).map(|done| (done, Vec::new())),
        }
    }

    /// The account something new goes in: the one `input` names, if the chat's model may read
    /// its mail; else the first it may read that `takes` it. `none`: why there's none, said with
    /// the model's name.
    fn account_for_new(&self, chat: &Chat, input: &Value, takes: impl Fn(&str) -> bool, none: impl FnOnce(&str) -> String) -> Result<String, String> {
        let profile = self.assistant_profile(chat).map_err(|e| said(&e))?;
        match text(input, "account") {
            Some(a) => match self.unreadable(a, &profile) {
                Some(why) => Err(why),
                None => Ok(a.to_string()),
            },
            None => self.readable_accounts(&profile).iter().map(|a| a.config.id.clone()).find(|a| takes(a)).ok_or_else(|| none(&profile.name)),
        }
    }

    /// What the chat found that `id` names, of an account whose mail its model may read.
    fn found_named(&self, chat: &Chat, id: &str) -> Result<Found, String> {
        let Some(found) = chat.found_named(id).cloned() else {
            return Err(if chat.message_named(id).is_some() {
                format!("{id} is a message: delete_messages deletes messages")
            } else {
                format!("`{}` isn't an id the tools gave", id.chars().take(80).collect::<String>())
            });
        };
        let profile = self.assistant_profile(chat).map_err(|e| said(&e))?;
        match self.unreadable(found.account(), &profile) {
            Some(why) => Err(why),
            None => Ok(found),
        }
    }

    /// What `input`'s `id` names: something the chat found, of kind `kind` (an event, a person,
    /// a task or a note).
    fn found_of(&self, chat: &Chat, input: &Value, kind: &str) -> Result<Found, String> {
        let id = text(input, "id").ok_or_else(|| format!("`id` is required: the {kind}'s, as the tools gave it"))?;
        let found = self.found_named(chat, id)?;
        if found.kind() != kind {
            return Err(format!("{id} is {}, not {}", a(found.kind()), a(kind)));
        }
        Ok(found)
    }

    /// `account`'s calendars and contacts; one with none yet, as it starts (made when something's
    /// first added to it).
    fn pim_or_new(&self, account: &str) -> Cow<'_, Pim> {
        match self.pim.get(account) {
            Some(p) => Cow::Borrowed(p),
            None => {
                let mut p = Pim::default();
                p.ensure_defaults();
                Cow::Owned(p)
            }
        }
    }

    /// The deletion `delete_items` asks for with `input`, checked, and its card's line.
    fn items_to_delete(&self, chat: &Chat, input: &Value) -> Result<(String, PimChange), String> {
        let ids = input.get("ids").and_then(Value::as_array).ok_or("`ids` (what's deleted: the ids the tools gave) is required")?;
        if ids.is_empty() || ids.len() > DELETE_MOST {
            return Err(format!("name between 1 and {DELETE_MOST}"));
        }
        let series = input.get("series").and_then(Value::as_bool).unwrap_or(false);
        let (mut items, mut named) = (Vec::new(), Vec::new());
        for id in ids {
            let found = self.found_named(chat, id.as_str().ok_or("each id is a string")?)?;
            if items.contains(&found) {
                continue;
            }
            named.push(match &found {
                Found::Event { .. } => self.event_to_delete(&found, series)?,
                Found::Person { .. } => self.person_to_delete(&found)?,
                Found::Task { .. } => self.task_to_delete(&found)?,
                Found::Note { .. } => self.note_to_delete(&found)?,
            });
            items.push(found);
        }
        let more = named.len().saturating_sub(NAMED_MOST);
        named.truncate(NAMED_MOST);
        let more = if more > 0 { format!("; and {more} more") } else { String::new() };
        Ok((format!("Delete {}{more}", named.join("; ")), PimChange::Delete { items, series }))
    }

    /// Delete `items`, approved: each that can be, and why any couldn't.
    fn delete_found(&mut self, items: &[Found], series: bool) -> Result<String, String> {
        let (mut deleted, mut failed) = (0, Vec::new());
        for found in items {
            let done = match found {
                Found::Event { .. } => self.delete_found_event(found, series),
                Found::Person { uid, .. } => self.delete_card(uid).map(drop).map_err(|e| said(&e)),
                Found::Task { uid, .. } => self.delete_task(uid).map(drop).map_err(|e| said(&e)),
                Found::Note { account, uid } => self.delete_note(Some(account), uid).map(drop).map_err(|e| said(&e)),
            };
            match done {
                Ok(()) => deleted += 1,
                Err(why) => failed.push(format!("{}: {why}", found.name())),
            }
        }
        match (deleted, failed.is_empty()) {
            (0, _) => Err(failed.join("; ")),
            (n, true) => Ok(format!("Done: {n} deleted.")),
            (n, false) => Ok(format!("{n} deleted; not {}.", failed.join("; "))),
        }
    }
}
