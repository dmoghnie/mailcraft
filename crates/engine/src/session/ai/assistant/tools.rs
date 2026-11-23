//! The tools the assistant's model may call: what each is called, what it does, what it takes
//! (a JSON Schema), and which of them only read (run at once) or change something (proposed,
//! for the person to approve).

use mailcraft_model::{AiTool, AiToolCall, MessageKey};
use serde_json::{Value, json};

use super::*;
use crate::Session;

/// How a call went, as soon as it's made.
pub(super) enum Outcome {
    /// Read: what it gave.
    Ran { what: String, messages: Vec<String>, result: String },
    /// It couldn't be done: why.
    Failed { what: String, result: String },
    /// It reads a message still to download.
    Waiting { what: String, key: MessageKey },
    /// A change, for the person to approve.
    Proposed { what: String, messages: Vec<String> },
}

/// The tools, as the model is told them.
pub(super) fn tools() -> Vec<AiTool> {
    let ids = json!({"type": "array", "items": {"type": "string"}, "description": "The messages' ids, as the tools give them", "maxItems": changes::IDS_MOST});
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
    ]
}

/// Whether `tool` only reads.
pub(super) fn reads(tool: &str) -> bool {
    matches!(tool, "list_folders" | "find_messages" | "read_message" | "selected_messages")
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
            "list_folders" => self.tool_list_folders(profile, &input),
            "find_messages" => self.tool_find_messages(profile, &input),
            "read_message" => self.tool_read_message(chat, profile, &input),
            "selected_messages" => self.tool_selected(profile),
            name if !reads(name) && tools().iter().any(|t| t.name == name) => match self.change_of(chat, name, &input) {
                Ok(change) => Outcome::Proposed { what: change.what(self), messages: change.ids() },
                Err(why) => Outcome::failed(name, why),
            },
            name => Outcome::failed(name, format!("there's no tool `{}`", name.chars().take(64).collect::<String>())),
        }
    }
}
