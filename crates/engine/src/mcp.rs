//! The Model Context Protocol (MCP) as MailCraft answers it: the JSON-RPC 2.0 messages an AI
//! agent sends (through `mailcraft mcp`, which hands them to the running window), answered from
//! the command registry. Two tools: `list_commands` (what can be run, with its params) and
//! `run_command` (run one, as the ribbon, a shortcut or `--cmd` would). Nothing is answered but
//! an error while the user hasn't let agents in (`Prefs::agents`, File › Options › Safety); let
//! in, an agent runs only the commands that read and search ([`READS`]) until the user lets it
//! act too (`Prefs::agents_act`): send, reply, delete, move, change settings.
//!
//! Pure: no I/O, no clock, so it's tested here and runs wherever the engine does.

use serde_json::{Value, json};

use crate::{CmdResult, Session};

/// What runs an agent's commands: the session itself, or a window that runs them as its own
/// clicks are (showing what they show).
pub trait Runner {
    fn session(&self) -> &Session;
    fn run(&mut self, id: &str, params: &Value) -> CmdResult;
}

impl Runner for Session {
    fn session(&self) -> &Session {
        self
    }
    fn run(&mut self, id: &str, params: &Value) -> CmdResult {
        Session::run(self, id, params)
    }
}

/// The protocol versions answered, newest first: one asked for is agreed to, any other gets the
/// newest.
pub const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// JSON-RPC's own error codes, and the one for "agents aren't let in".
pub(crate) const PARSE_ERROR: i64 = -32700;
pub(crate) const INVALID_REQUEST: i64 = -32600;
pub(crate) const METHOD_NOT_FOUND: i64 = -32601;
pub(crate) const INVALID_PARAMS: i64 = -32602;
pub(crate) const AGENTS_OFF: i64 = -32000;

/// Most messages in one batch answered.
pub(crate) const MAX_BATCH: usize = 64;

/// Why nothing is answered while agents aren't let in.
pub const AGENTS_OFF_SAID: &str =
    "MailCraft doesn't let AI agents control it: turn on File › Options › Safety › Let AI agents control MailCraft, then connect again";

/// The commands an agent may run while it may only read: they open, list, read and search, as the
/// user would, and send, answer, delete, move and change nothing (an opened message is marked
/// read as the user's reading options say, as when the user opens it). Named one by one, so a
/// command added later isn't an agent's until it's put here.
pub const READS: &[&str] = &[
    "account.importStatus",
    "account.list",
    "calendar.events",
    "calendar.findFree",
    "calendar.search",
    "category.list",
    "compose.list",
    "compose.rewriting",
    "contact.list",
    "contact.searchDirectory",
    "contacts.suggest",
    "files.list",
    "files.show",
    "folder.noAnswer",
    "folder.open",
    "folder.vip",
    "invite.findWaiting",
    "list.first",
    "list.last",
    "list.messages",
    "list.next",
    "list.open",
    "list.pageDown",
    "list.pageUp",
    "list.prev",
    "mail.noAnswer",
    "mail.summary",
    "mail.translation",
    "meetings.list",
    "message.read",
    "note.list",
    "people.vips",
    "quickStep.list",
    "reminders.list",
    "rules.list",
    "search.clear",
    "search.run",
    "search.suggest",
    "searchFolder.list",
    "searchFolder.open",
    "script.list",
    "script.run",
    "task.list",
];

/// The commands an agent never runs, even let to act: adding or changing an MCP server, or
/// turning one on, starts a program on this computer, and signing in to one gives MailCraft
/// the person's own account there: theirs to choose alone.
pub const PERSON_ONLY: &[&str] = &["mcpServer.save", "mcpServer.turn", "mcpServer.signIn"];

/// Why a command isn't run for any agent.
pub const PERSON_ONLY_SAID: &str = "Only the person using MailCraft adds an MCP server, turns one on or signs in to one (File › Options › AI): it starts a program on their computer, or uses their own account";

/// Why a command isn't run for an agent that may only read.
pub const READ_ONLY_SAID: &str = "MailCraft lets AI agents only read and search: this command sends, answers, deletes, moves or changes something. Ask the user to turn on File › Options › Safety › Let AI agents send, delete and change settings";

/// Whether an agent may run command `id` now.
pub fn may_run(s: &Session, id: &str) -> bool {
    !PERSON_ONLY.contains(&id) && (s.prefs.agents_act || READS.contains(&id))
}

/// What the agent is told about MailCraft as it connects.
pub(crate) const INSTRUCTIONS: &str = "MailCraft is a desktop email client (IMAP, POP, JMAP, Exchange). Everything it does is a command with an id and JSON params, the same ones its ribbon and shortcuts run, acting on the window the user sees. \
Call list_commands (with a filter such as \"mail.\" or \"calendar\") to see ids and params, then run_command. \
Reading mail: folder.open {account, folder} or list.open {key}, list.messages {limit}, message.read {key} (when it answers pending, the message is being downloaded: call it again shortly), search.run {query}. \
Writing: mail.new (or mail.reply, mail.forward), then the compose.* commands; compose.send sends. Commands that send, delete or change settings act at once, as if the user clicked them: say what you are about to do first.";

/// The answer to one message (a line of JSON), or `None` for a notification or a response,
/// which aren't answered.
pub fn answer(s: &mut impl Runner, message: &str) -> Option<String> {
    let parsed: Value = match serde_json::from_str(message) {
        Ok(v) => v,
        Err(e) => return Some(error(&Value::Null, PARSE_ERROR, &format!("not JSON: {e}")).to_string()),
    };
    match parsed {
        Value::Array(batch) if batch.is_empty() => Some(error(&Value::Null, INVALID_REQUEST, "an empty batch").to_string()),
        Value::Array(batch) => {
            let answers: Vec<Value> = batch.iter().take(MAX_BATCH).filter_map(|m| one(s, m)).collect();
            (!answers.is_empty()).then(|| Value::Array(answers).to_string())
        }
        m => one(s, &m).map(|a| a.to_string()),
    }
}

/// One message's answer.
fn one(s: &mut impl Runner, m: &Value) -> Option<Value> {
    let Some(method) = m.get("method").and_then(Value::as_str) else {
        // A response to something MailCraft never asks, or not a message at all.
        return m.get("id").filter(|_| m.get("result").is_none() && m.get("error").is_none()).map(|id| error(id, INVALID_REQUEST, "no method"));
    };
    // A notification (`notifications/initialized`, `notifications/cancelled`…): nothing to say.
    let id = m.get("id")?;
    let params = m.get("params").cloned().unwrap_or_else(|| json!({}));
    if method == "ping" {
        return Some(result(id, json!({})));
    }
    if !s.session().prefs.agents {
        return Some(error(id, AGENTS_OFF, AGENTS_OFF_SAID));
    }
    Some(match method {
        "initialize" => result(id, initialize(s.session(), &params)),
        "tools/list" => result(id, json!({"tools": tools(s.session())})),
        "tools/call" => match call(s, &params) {
            Ok(r) => result(id, r),
            Err(e) => error(id, INVALID_PARAMS, &e),
        },
        // What MailCraft doesn't offer is said to be empty, so a client that asks anyway goes on.
        "resources/list" => result(id, json!({"resources": []})),
        "prompts/list" => result(id, json!({"prompts": []})),
        other => error(id, METHOD_NOT_FOUND, &format!("MailCraft doesn't answer {other}")),
    })
}

fn initialize(s: &Session, params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = asked.filter(|v| PROTOCOL_VERSIONS.contains(v)).unwrap_or(PROTOCOL_VERSIONS[0]);
    let instructions = if s.prefs.agents_act {
        INSTRUCTIONS.to_string()
    } else {
        format!("{INSTRUCTIONS} For now the user lets agents only read and search: list_commands marks what can't run.")
    };
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {}},
        "serverInfo": {"name": "mailcraft", "title": "MailCraft", "version": env!("CARGO_PKG_VERSION")},
        "instructions": instructions,
    })
}

/// The tools, as `tools/list` gives them: `run_command` changes nothing while agents may only
/// read.
fn tools(s: &Session) -> Value {
    let reads = !s.prefs.agents_act;
    json!([
        {
            "name": "list_commands",
            "title": "List MailCraft's commands",
            "description": "MailCraft's commands: id, label, where it is in the window, shortcut, its params (as a short JSON sketch) and, when it can't run now, why. A filter keeps those whose id, label or place contains it.",
            "inputSchema": {"type": "object", "properties": {"filter": {"type": "string", "description": "Part of an id, label or place, such as \"mail.\" or \"Calendar\""}}},
            "annotations": {"readOnlyHint": true, "openWorldHint": false},
        },
        {
            "name": "run_command",
            "title": "Run a MailCraft command",
            "description": "Run one of MailCraft's commands by id with its params, as clicking it would, in the window the user sees; its answer is JSON. Some send mail, delete or change settings at once.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "A command id from list_commands, such as \"list.messages\""},
                    "params": {"type": "object", "description": "Its params, as list_commands describes them"},
                },
                "required": ["id"],
            },
            "annotations": {"readOnlyHint": reads, "destructiveHint": !reads, "openWorldHint": true},
        },
    ])
}

/// A tool called: its result, or why the call itself is wrong (an unknown tool, no id). A
/// command that fails is a result that says so (`isError`), for the agent to read.
fn call(s: &mut impl Runner, params: &Value) -> Result<Value, String> {
    let name = params.get("name").and_then(Value::as_str).ok_or("`name` (string) is required")?;
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    match name {
        "list_commands" => {
            let filter = args.get("filter").and_then(Value::as_str).unwrap_or_default().trim().to_lowercase();
            Ok(text(&list(s.session(), &filter).to_string(), false))
        }
        "run_command" => {
            let id = args.get("id").and_then(Value::as_str).ok_or("`id` (string) is required")?;
            if PERSON_ONLY.contains(&id) {
                return Ok(text(PERSON_ONLY_SAID, true));
            }
            if !may_run(s.session(), id) {
                return Ok(text(READ_ONLY_SAID, true));
            }
            let command_params = args.get("params").cloned().unwrap_or_else(|| json!({}));
            Ok(match s.run(id, &command_params) {
                Ok(v) => text(&v.to_string(), false),
                Err(e) => text(&e.to_string(), true),
            })
        }
        other => Err(format!("MailCraft has no tool {other}")),
    }
}

/// The commands whose id, label or place contain `filter` (lower case; empty: all).
fn list(s: &Session, filter: &str) -> Value {
    let registry = s.registry();
    let found = registry.all().iter().filter(|c| filter.is_empty() || [c.id, c.label, c.location].iter().any(|t| t.to_lowercase().contains(filter)));
    Value::Array(
        found
            .map(|c| {
                let mut v = json!({"id": c.id, "label": c.label, "location": c.location, "params": c.params});
                if !c.shortcut.is_empty() {
                    v["shortcut"] = json!(c.shortcut);
                }
                if PERSON_ONLY.contains(&c.id) {
                    v["disabled"] = json!("only the person adds an MCP server, turns one on or signs in to one");
                } else if !may_run(s, c.id) {
                    v["disabled"] = json!("agents may only read and search");
                } else if let Some(why) = (c.enabled)(s) {
                    v["disabled"] = json!(why);
                }
                v
            })
            .collect(),
    )
}

fn text(t: &str, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": t}], "isError": is_error})
}

fn result(id: &Value, r: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": r})
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}
