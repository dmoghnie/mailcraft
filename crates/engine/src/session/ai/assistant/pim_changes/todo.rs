//! The tasks and notes the assistant proposes adding, changing or deleting. A task is marked
//! done as the window marks it: one that repeats moves on to its next time.

use mailcraft_model::dates;
use mailcraft_model::pim::{Task, task_day};
use serde_json::{Value, json};

use super::*;

/// What the model may say of a task, and the field of the window's task each is. A due day or a
/// reminder given empty (or `none`) is taken off.
const FIELDS: [&str; 5] = ["title", "notes", "due", "priority", "reminder"];

impl Session {
    /// The task `add_task` asks for with `input`, checked, and its card's line.
    pub(super) fn task_to_add(&self, chat: &Chat, input: &Value) -> Result<(String, PimChange), String> {
        let account = self.account_for_new(chat, input, |_| true, |model| format!("no task can be added: no account's mail may go to {model}"))?;
        text(input, "title").ok_or("`title` is required")?;
        let task = self.task_of(input, Task::default(), None)?;
        let what = format!("Add the task “{}”{}", quoted(&task.title), self.task_said(&Task::default(), &task, false));
        Ok((what, PimChange::AddTask { account, task }))
    }

    /// The change `change_task` asks for with `input`, checked, and its card's line.
    pub(super) fn task_to_change(&self, chat: &Chat, input: &Value) -> Result<(String, PimChange), String> {
        let found = self.found_of(chat, input, "task")?;
        let was = self.task_kept(&found)?;
        let done = input.get("done").and_then(Value::as_bool);
        if done.is_none() && !FIELDS.iter().any(|k| input.get(*k).is_some()) {
            return Err("say what changes: title, due, notes, priority, reminder or done".into());
        }
        let task = self.task_of(input, was.clone(), Some(&was))?;
        let done = done.filter(|d| *d != was.is_done());
        if task == was && done.is_none() {
            return Err("that's how it is already: nothing would change".into());
        }
        let mut how = self.task_said(&was, &task, true);
        if let Some(d) = done {
            how.push_str(if how.is_empty() { ": " } else { "; " });
            how.push_str(if d { "done" } else { "not done" });
        }
        let what = format!("Change the task “{}”{how}", quoted(&was.title));
        Ok((what, PimChange::ChangeTask { found, task, done }))
    }

    /// Deleting the task `found` names, checked: how its card names it.
    pub(super) fn task_to_delete(&self, found: &Found) -> Result<String, String> {
        Ok(format!("the task “{}”", quoted(&self.task_kept(found)?.title)))
    }

    /// The task `found` names, as kept.
    fn task_kept(&self, found: &Found) -> Result<Task, String> {
        let Found::Task { account, uid } = found else { return Err(format!("{} isn't a task", found.name())) };
        self.find_task(uid).filter(|t| t.account == *account).map(|t| t.task).ok_or_else(|| "that task isn't in the tasks any more".into())
    }

    /// `base` with what `input` says, as the window's task form makes it, checked (`was`: the
    /// task it changes).
    fn task_of(&self, input: &Value, base: Task, was: Option<&Task>) -> Result<Task, String> {
        let mut params = json!({});
        for k in FIELDS {
            if let Some(v) = input.get(k).and_then(Value::as_str) {
                params[k] = json!(v.trim());
            }
        }
        let task = crate::cmd::task_from(self, &params, base).map_err(|e| said(&e))?;
        crate::session::pim::check_task(&task, was).map_err(|e| said(&e))?;
        Ok(task)
    }

    /// What `now` has that `was` hadn't, for a card's line: after a colon, or none. `changed`:
    /// said as changes (`its title to …`), else as what it has.
    fn task_said(&self, was: &Task, now: &Task, changed: bool) -> String {
        let mut how = Vec::new();
        if changed && now.title != was.title {
            how.push(format!("its title to “{}”", quoted(&now.title)));
        }
        if now.due != was.due {
            how.push(match now.due {
                Some(d) => format!("due {}", dates::day_text(task_day(d).div_euclid(86_400), &self.clock)),
                None => "no due day".into(),
            });
        }
        if now.priority != was.priority {
            how.push(format!("{} priority", now.priority.code()));
        }
        if now.reminder != was.reminder {
            how.push(match now.reminder {
                Some(at) => format!("a reminder {}", dates::full_time(at, &self.clock)),
                None => "no reminder".into(),
            });
        }
        if now.notes != was.notes {
            how.push(format!("{} “{}”", if changed { "its notes to" } else { "notes" }, quoted(&now.notes)));
        }
        if how.is_empty() { String::new() } else { format!(": {}", how.join("; ")) }
    }

    /// A task, approved, added to `account`'s: what was done, and it, for the answer to link.
    pub(super) fn add_new_task(&mut self, account: &str, task: Task) -> Result<(String, Vec<Found>), String> {
        let added = self.create_task(Some(account), task).map_err(|e| said(&e))?;
        let uid = added.get("uid").and_then(Value::as_str).ok_or("the task wasn't added")?.to_string();
        let found = Found::Task { account: account.to_string(), uid };
        Ok((format!("Done: added to the tasks, as {}.", found.name()), vec![found]))
    }

    /// The task `found` names, approved, changed to `task`, and marked done (or not) when `done`
    /// says.
    pub(super) fn change_found_task(&mut self, found: Found, task: Task, done: Option<bool>) -> Result<(String, Vec<Found>), String> {
        let Found::Task { uid, .. } = &found else { return Err("that isn't a task".into()) };
        self.update_task(uid, |t| *t = task).map_err(|e| said(&e))?;
        let mut said_done = String::from("Done: changed.");
        if let Some(d) = done {
            let marked = self.complete_task(uid, d).map_err(|e| said(&e))?;
            // One that repeats is due again: said, so the answer doesn't call it gone.
            if let Some(next) = marked.get("due").and_then(Value::as_i64) {
                said_done =
                    format!("Done: marked done; it repeats, so it's due again {}.", dates::day_text(task_day(next).div_euclid(86_400), &self.clock));
            }
        }
        Ok((said_done, vec![found]))
    }

    /// The note `add_note` asks for with `input`, checked, and its card's line.
    pub(super) fn note_to_add(&self, chat: &Chat, input: &Value) -> Result<(String, PimChange), String> {
        let account = self.account_for_new(chat, input, |_| true, |model| format!("no note can be added: no account's mail may go to {model}"))?;
        let text = text(input, "text").ok_or("`text` is required (its first line is its title)")?.to_string();
        Ok((format!("Add the note “{}”", note_title(&text)), PimChange::AddNote { account, text }))
    }

    /// The change `change_note` asks for with `input`, checked, and its card's line.
    pub(super) fn note_to_change(&self, chat: &Chat, input: &Value) -> Result<(String, PimChange), String> {
        let found = self.found_of(chat, input, "note")?;
        let was = self.note_kept(&found)?;
        let text = text(input, "text").ok_or("`text` is required: the note's text, all of it")?.to_string();
        if text == was.trim() {
            return Err("that's how it is already: nothing would change".into());
        }
        let what = format!("Change the note “{}” to “{}”", note_title(&was), quoted(&text.split_whitespace().collect::<Vec<_>>().join(" ")));
        Ok((what, PimChange::ChangeNote { found, text }))
    }

    /// Deleting the note `found` names, checked: how its card names it.
    pub(super) fn note_to_delete(&self, found: &Found) -> Result<String, String> {
        Ok(format!("the note “{}”", note_title(&self.note_kept(found)?)))
    }

    /// The text of the note `found` names.
    fn note_kept(&self, found: &Found) -> Result<String, String> {
        let Found::Note { account, uid } = found else { return Err(format!("{} isn't a note", found.name())) };
        self.pim
            .get(account)
            .and_then(|p| p.notes.iter().find(|n| n.note.uid == *uid))
            .map(|n| n.note.text.clone())
            .ok_or_else(|| "that note isn't in the notes any more".into())
    }

    /// A note, approved, added to `account`'s: what was done, and it, for the answer to link.
    pub(super) fn add_new_note(&mut self, account: &str, text: &str) -> Result<(String, Vec<Found>), String> {
        let added = self.create_note(Some(account), text, None).map_err(|e| said(&e))?;
        let uid = added.get("uid").and_then(Value::as_str).ok_or("the note wasn't added")?.to_string();
        let found = Found::Note { account: account.to_string(), uid };
        Ok((format!("Done: added to the notes, as {}.", found.name()), vec![found]))
    }

    /// The note `found` names, approved, given `text`.
    pub(super) fn change_found_note(&mut self, found: Found, text: &str) -> Result<(String, Vec<Found>), String> {
        let Found::Note { uid, .. } = &found else { return Err("that isn't a note".into()) };
        self.update_note(uid, Some(text), None).map_err(|e| said(&e))?;
        Ok(("Done: changed.".into(), vec![found]))
    }
}

/// A note's title: its first line with words, as the Notes list shows it.
fn note_title(text: &str) -> String {
    quoted(text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default())
}
