//! The tools the assistant's model may call: what each is called, what it does, what it takes
//! (a JSON Schema), and which of them only read (run at once) or change something (proposed,
//! for the person to approve).

use mailcraft_model::{AiTool, AiToolCall, MessageKey};
use serde_json::{Value, json};

use super::*;
use crate::Session;

/// How a call went, as soon as it's made.
pub(super) enum Outcome {
    /// Read: what it gave (the text, for the model; the data, for a script), and the
    /// messages, events and people in it.
    Ran { what: String, messages: Vec<String>, found: Vec<Found>, result: String, data: Value },
    /// It couldn't be done: why.
    Failed { what: String, result: String },
    /// It reads a message still to download.
    Waiting { what: String, key: MessageKey },
    /// A script, waiting for a message one of its calls reads.
    Scripting { what: String },
    /// A change, for the person to approve.
    Proposed { what: String, messages: Vec<String> },
}

/// The tools `profile` is told of: [`tools`]; and, when it may run scripts, `run_script`,
/// `keep_script` and the scripts `kept` that are on.
pub(super) fn tools_for(profile: &AiProfile, kept: &[script::KeptScript]) -> Vec<AiTool> {
    let mut all = tools();
    if profile.scripts {
        all.push(script::tool(kept));
        all.push(script::keep_tool());
        all.extend(script::kept_tools(kept));
    }
    all
}

/// The tools, as the model is told them.
pub(super) fn tools() -> Vec<AiTool> {
    let ids = json!({"type": "array", "items": {"type": "string"}, "description": "The messages' ids, as the tools give them", "maxItems": changes::IDS_MOST});
    let limit = json!({"type": "integer", "minimum": 1, "maximum": 50, "description": "How many to give at most (20 when not said)"});
    let id = |of: &str| json!({"type": "string", "description": format!("The {of}'s id, as the tools gave it")});
    let tool = |name: &str, description: &str, schema: Value| AiTool { name: name.into(), description: description.into(), schema };
    vec![
        tool(
            "list_folders",
            "List the accounts whose mail you may read, and their folders: each folder's path, role, and how many messages it has and how many are unread.",
            json!({"type": "object", "properties": {"account": {"type": "string", "description": "Only this account's (its id)"}}}),
        ),
        tool(
            "find_messages",
            "Find messages, as MailCraft's search box does: words, and from:, to:, cc:, subject:, received:, is:unread, is:flagged, has:attachment, category:. With no query, the newest messages. Gives each message's id, date, sender, recipients, subject, folder and the start of its text.",
            json!({"type": "object", "properties": {
                "query": {"type": "string", "description": "As typed in the search box, e.g. `from:anna invoice received:this week`"},
                "account": {"type": "string", "description": "Only this account's mail (its id)"},
                "folder": {"type": "string", "description": "Only this folder: its path, its name, or its role (inbox, sent, drafts, archive, junk, trash)"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 50, "description": "How many to give at most, the newest first (20 when not said)"}}}),
        ),
        tool(
            "read_message",
            "Read a message whole: who wrote it to whom and when, its text, and its files' names.",
            json!({"type": "object", "properties": {"id": {"type": "string"}, "max_chars": {"type": "integer", "minimum": 500, "maximum": 40000}}, "required": ["id"]}),
        ),
        tool(
            "find_events",
            "Find events in the calendars shown, as the calendar's search box does: words, and location:, subject:, notes:, organizer:, attendee:, category:, is:recurring, after: and before: YYYY-MM-DD. With no query, the next events. Gives each event's id, when it is, what, where, its calendar, organizer and attendees, and the start of its notes.",
            json!({"type": "object", "properties": {
                "query": {"type": "string", "description": "As typed in the calendar's search box, e.g. `attendee:zoe after:2026-01-01`"},
                "when": {"type": "string", "enum": ["all", "upcoming", "past"], "description": "Upcoming, past or both (upcoming when there's no query, else all)"},
                "account": {"type": "string", "description": "Only this account's calendars (its id)"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 50, "description": "How many to give at most (20 when not said)"}}}),
        ),
        tool(
            "find_people",
            "Find people in the person's contacts, by name, organization or address. With no query, everyone. Gives each one's id, name, addresses, phones, title and organization.",
            json!({"type": "object", "properties": {
                "query": {"type": "string", "description": "A name, organization or address, or part of one"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 50, "description": "How many to give at most (20 when not said)"}}}),
        ),
        tool(
            "find_tasks",
            "Find the person's tasks, by words of their title, notes or categories. With no query, all of them. Gives each task's id, title, due day, how far along it is, priority and the start of its notes.",
            json!({"type": "object", "properties": {"query": {"type": "string"}, "limit": limit}}),
        ),
        tool(
            "find_notes",
            "Find the person's notes, by words in them. With no query, the newest. Gives each note's id, when it was changed and its text (its first line is its title).",
            json!({"type": "object", "properties": {"query": {"type": "string"}, "limit": limit}}),
        ),
        tool(
            "selected_messages",
            "The messages the person has selected in the window, and the one open.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "move_messages",
            "Propose moving messages to a folder. The person approves it or not; nothing moves until they do.",
            json!({"type": "object", "properties": {"ids": ids, "folder": {"type": "string", "description": "The folder's path, or a role: inbox, archive, junk, trash"}}, "required": ["ids", "folder"]}),
        ),
        tool(
            "delete_messages",
            "Propose deleting messages (to Deleted Items). The person approves it or not.",
            json!({"type": "object", "properties": {"ids": ids}, "required": ["ids"]}),
        ),
        tool(
            "mark_messages",
            "Propose marking messages read or unread, flagged or not. The person approves it or not.",
            json!({"type": "object", "properties": {"ids": ids, "read": {"type": "boolean"}, "flagged": {"type": "boolean"}}, "required": ["ids"]}),
        ),
        tool(
            "categorize_messages",
            "Propose putting messages in one of the person's categories, or taking them out of it. The person approves it or not.",
            json!({"type": "object", "properties": {"ids": ids, "category": {"type": "string", "description": "The category's name"}, "on": {"type": "boolean", "description": "false: taken out of it"}}, "required": ["ids", "category"]}),
        ),
        tool(
            "write_message",
            "Propose writing a message: approved, it opens for the person to check and send. You never send it. To answer a message, give its id as reply_to.",
            json!({"type": "object", "properties": {
                "to": {"type": "string", "description": "Who it's to: addresses, `Name <address>`, separated by commas"},
                "cc": {"type": "string"},
                "subject": {"type": "string"},
                "body": {"type": "string", "description": "Its text, as plain text"},
                "reply_to": {"type": "string", "description": "The id of the message it answers"},
                "account": {"type": "string", "description": "The account it's sent from (its id)"}}, "required": ["body"]}),
        ),
        tool(
            "add_event",
            "Propose adding an event to the person's calendar, with a reminder. The person approves it or not; nothing is added until they do. It's in their calendar alone: no one is invited (to tell others, propose a message).",
            json!({"type": "object", "properties": {
                "summary": {"type": "string", "description": "Its title"},
                "start": {"type": "string", "description": "When it starts, in the person's time: YYYY-MM-DDTHH:MM, or YYYY-MM-DD for all day"},
                "end": {"type": "string", "description": "When it ends, as start (an hour after it starts when not said; all day: its last day)"},
                "location": {"type": "string"},
                "notes": {"type": "string"},
                "reminder": {"type": "integer", "minimum": 0, "maximum": mailcraft_model::ical::REMINDER_MAX_MINUTES, "description": "How many minutes before it starts the person is reminded (15 when not said)"},
                "account": {"type": "string", "description": "The account whose calendar it goes in (its id; the first you may read when not said)"},
                "calendar": {"type": "string", "description": "Its calendar's name, when not the one new events go to"}}, "required": ["summary", "start"]}),
        ),
        tool(
            "change_event",
            "Propose changing an event: what's given changes, the rest stays. Not the time of one that repeats, nor a meeting with others but for its reminder (they'd have to be told).",
            json!({"type": "object", "properties": {
                "id": id("event"), "summary": {"type": "string"}, "start": {"type": "string", "description": "As add_event's"},
                "end": {"type": "string"}, "location": {"type": "string"}, "notes": {"type": "string"},
                "reminder": {"type": "integer", "minimum": 0, "maximum": mailcraft_model::ical::REMINDER_MAX_MINUTES, "description": "Minutes before it starts"}}, "required": ["id"]}),
        ),
        tool(
            "add_person",
            "Propose adding someone to the person's contacts.",
            json!({"type": "object", "properties": {
                "name": {"type": "string"}, "emails": {"type": "string", "description": "Their addresses, separated by commas"},
                "phones": {"type": "string", "description": "Their numbers, separated by commas"}, "organization": {"type": "string"},
                "title": {"type": "string", "description": "Their job title"}, "birthday": {"type": "string", "description": "YYYY-MM-DD"},
                "notes": {"type": "string"}, "account": {"type": "string", "description": "The account whose contacts they go in (its id)"}}, "required": ["name"]}),
        ),
        tool(
            "change_person",
            "Propose changing someone in the contacts: what's given replaces what they have (emails: all their addresses), the rest stays.",
            json!({"type": "object", "properties": {
                "id": id("person"), "name": {"type": "string"}, "emails": {"type": "string"}, "phones": {"type": "string"},
                "organization": {"type": "string"}, "title": {"type": "string"}, "birthday": {"type": "string"}, "notes": {"type": "string"}}, "required": ["id"]}),
        ),
        tool(
            "add_task",
            "Propose adding a task to the person's tasks.",
            json!({"type": "object", "properties": {
                "title": {"type": "string"}, "due": {"type": "string", "description": "YYYY-MM-DD"}, "notes": {"type": "string"},
                "priority": {"type": "string", "enum": ["low", "normal", "high"]},
                "reminder": {"type": "string", "description": "When it reminds: YYYY-MM-DDTHH:MM"},
                "account": {"type": "string", "description": "The account whose tasks it goes in (its id)"}}, "required": ["title"]}),
        ),
        tool(
            "change_task",
            "Propose changing a task, or marking it done: what's given changes, the rest stays.",
            json!({"type": "object", "properties": {
                "id": id("task"), "title": {"type": "string"}, "due": {"type": "string", "description": "YYYY-MM-DD, or \"\" for none"},
                "notes": {"type": "string"}, "priority": {"type": "string", "enum": ["low", "normal", "high"]},
                "reminder": {"type": "string", "description": "YYYY-MM-DDTHH:MM, or \"none\""}, "done": {"type": "boolean"}}, "required": ["id"]}),
        ),
        tool(
            "add_note",
            "Propose adding a note.",
            json!({"type": "object", "properties": {
                "text": {"type": "string", "description": "Its text; its first line is its title"},
                "account": {"type": "string", "description": "The account whose notes it goes in (its id)"}}, "required": ["text"]}),
        ),
        tool(
            "change_note",
            "Propose changing a note's text.",
            json!({"type": "object", "properties": {"id": id("note"), "text": {"type": "string", "description": "Its new text, all of it"}}, "required": ["id", "text"]}),
        ),
        tool(
            "delete_items",
            "Propose deleting events, contacts, tasks or notes, by the ids the tools gave (messages: delete_messages). What's deleted is gone for good.",
            json!({"type": "object", "properties": {
                "ids": {"type": "array", "items": {"type": "string"}, "maxItems": 50},
                "series": {"type": "boolean", "description": "For an event that repeats: every time, not only the one named"}}, "required": ["ids"]}),
        ),
    ]
}

/// Whether `tool` only reads.
pub(super) fn reads(tool: &str) -> bool {
    matches!(
        tool,
        "list_folders" | "find_messages" | "read_message" | "find_events" | "find_people" | "find_tasks" | "find_notes" | "selected_messages"
    )
}

impl Session {
    /// Make `call` for chat `chat`, asked of `profile`: what it read, or why it couldn't, or the
    /// change it proposes.
    pub(super) fn tool_call(&mut self, chat: &mut Chat, profile: &AiProfile, call: &AiToolCall) -> Outcome {
        let input = if call.input.is_object() { call.input.clone() } else { json!({}) };
        if !call.input.is_object() {
            return Outcome::failed(&call.name, "what was given isn't a JSON object".into());
        }
        match call.name.as_str() {
            "list_folders" => self.tool_list_folders(chat, profile, &input),
            "find_messages" => self.tool_find_messages(chat, profile, &input),
            "read_message" => self.tool_read_message(chat, profile, &input),
            "find_events" => self.tool_find_events(chat, profile, &input),
            "find_people" => self.tool_find_people(chat, profile, &input),
            "find_tasks" => self.tool_find_tasks(chat, profile, &input),
            "find_notes" => self.tool_find_notes(chat, profile, &input),
            "selected_messages" => self.tool_selected(chat, profile),
            script::RUN_SCRIPT => self.tool_run_script(chat, profile, call),
            script::KEEP_SCRIPT if profile.scripts => match self.change_of(chat, script::KEEP_SCRIPT, &input) {
                Ok(change) => Outcome::Proposed { what: change.what(self), messages: change.ids() },
                Err(why) => Outcome::failed(script::KEEP_SCRIPT, why),
            },
            name if self.kept_on(name).is_some() => self.tool_run_kept(chat, profile, call),
            name if !reads(name) && tools().iter().any(|t| t.name == name) => match self.change_of(chat, name, &input) {
                Ok(change) => Outcome::Proposed { what: change.what(self), messages: change.ids() },
                Err(why) => Outcome::failed(name, why),
            },
            name => Outcome::failed(name, format!("there's no tool `{}`", name.chars().take(64).collect::<String>())),
        }
    }
}
