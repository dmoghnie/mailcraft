//! MailCraft's AI assistant: chats with a model that reads and searches the person's mail with
//! tools, and proposes changes they approve one by one.
//!
//! What it reads runs at once (finding, reading, listing folders); what it would change (moving,
//! deleting, marking, categorising or writing messages; adding, changing or deleting events,
//! contacts, tasks and notes) is a proposal, and nothing changes until the person approves it. A
//! message it writes is opened for the person to send: it never sends, nor invites or tells
//! anyone.
//! Mail goes only to a model each account's setting allows (File › Options › AI), and a model
//! asked about a chat must be allowed every account whose mail is in it. Chats are kept on this
//! computer, in the data folder, never in the record of what went where nor the log.

use std::collections::{BTreeSet, HashMap, HashSet};

use super::AiProfile;
use mailcraft_model::SharedSummary;
use std::fmt;

use mailcraft_model::{AiRole, AiToolCall, AiTurn};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod changes;
mod commands;
mod context;
mod exchange;
mod kept;
mod ledger;
mod links;
mod looking;
mod pim;
mod pim_changes;
mod script;
mod tools;
mod window;

pub use commands::QUOTE_MOST as CONTEXT_QUOTE_MOST;
pub(crate) use commands::commands as assistant_commands;
pub use context::{CONTEXT_AT_FIRST, ContextItem, ITEMS_MOST as CONTEXT_ITEMS_MOST};
pub use links::{Found, LINK as ASSISTANT_LINK, Linked, message_link as assistant_message_link};
#[doc(hidden)]
pub use script::run_script_against;
pub use script::{KeptScript, ScriptParam};
pub use window::{ASSISTANT_WIDTH, AssistantPane};

/// The most of steps (answers with tool calls) the assistant takes for one thing the person
/// says: past it, it stops and says so.
pub const STEPS_MOST: u32 = 24;
/// The most turns a chat keeps: a longer one is begun again in another.
const TURNS_MOST: usize = 2000;
/// The longest thing the person says in a turn, in bytes.
const SAID_MOST: usize = 64 << 10;
/// The most a chat holds, in bytes of its turns, before it takes no more (each step of an
/// answer writes it whole): a few hundred long messages read.
const CHAT_SIZE_MOST: usize = 8 << 20;

/// A chat with MailCraft's AI, kept on this computer. Its `Debug` form gives only its size: it
/// holds mail.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Chat {
    pub id: String,
    pub title: String,
    /// When it was begun, and last changed (Unix seconds).
    pub created: i64,
    pub updated: i64,
    /// The profile it asks; none: the one chat work goes to (File › Options › AI).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub turns: Vec<ChatTurn>,
    /// The tools it called, as each went, in the order called.
    pub runs: Vec<ToolRun>,
    /// The accounts whose mail went into it: a model asked about it must be allowed each one's.
    pub accounts: Vec<String>,
    /// Why its last question got no answer, or why it stopped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// What its model's server counted of the last question it answered, beside the estimate:
    /// the estimate is corrected by it when it was low.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counted: Option<Counted>,
    /// A question of it was on its way to the model when it was kept: read again after
    /// MailCraft closed, it says the answer never came.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub answer_due: bool,
}

impl fmt::Debug for Chat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Chat({}, {} turns, {} runs)", self.id, self.turns.len(), self.runs.len())
    }
}

impl Chat {
    pub fn run(&self, call: &str) -> Option<&ToolRun> {
        self.runs.iter().find(|r| r.call == call)
    }

    fn run_mut(&mut self, call: &str) -> Option<&mut ToolRun> {
        self.runs.iter_mut().find(|r| r.call == call)
    }

    /// The calls of its last turn, when that's the model's and it called tools: those still to
    /// be answered.
    fn open_calls(&self) -> Vec<AiToolCall> {
        match self.turns.last() {
            Some(t) if t.turn.role == mailcraft_model::AiRole::Assistant => t.turn.calls.clone(),
            _ => Vec::new(),
        }
    }

    /// Whether its model can be asked again as it is (Try again): it owes the chat an answer
    /// (what the person said, or what the tools gave, came last, or an answer stopped as it
    /// came, which the new one takes the place of), and nothing waits on the person or on a
    /// download.
    pub fn can_ask_again(&self) -> bool {
        let owed = self.turns.last().is_some_and(|t| t.turn.role == AiRole::User || t.stopped);
        owed && self.proposals().is_empty() && !self.runs.iter().any(|r| r.state == RunState::Waiting)
    }

    /// How much it holds, in bytes: its turns, with their thinking and what was pointed at.
    pub fn size(&self) -> usize {
        self.turns.iter().map(|t| t.turn.size().saturating_add(t.thinking.len()).saturating_add(t.context_text.len())).fold(0, usize::saturating_add)
    }

    /// Whether it waits on the person: a change proposed, not decided yet.
    pub fn waits_on_person(&self) -> bool {
        !self.proposals().is_empty()
    }

    /// The changes proposed that wait for the person, as calls: its open calls', and those their
    /// scripts proposed (with what the script called them with).
    fn proposals(&self) -> Vec<AiToolCall> {
        let open = self.open_calls();
        let ids: HashSet<&str> = open.iter().map(|c| c.id.as_str()).collect();
        let proposed = |id: &str| self.run(id).is_some_and(|r| r.state == RunState::Proposed);
        let own = open.iter().filter(|c| proposed(&c.id)).cloned();
        let scripts = self
            .runs
            .iter()
            .filter(|r| r.state == RunState::Proposed && r.part_of.as_deref().is_some_and(|p| ids.contains(p)))
            .map(|r| AiToolCall { id: r.call.clone(), name: r.tool.clone(), input: r.input.clone() });
        own.chain(scripts).collect()
    }

    /// A message read for it is of `account`: its mail is in the chat now.
    fn holds(&mut self, account: &str) {
        if !self.accounts.iter().any(|a| a == account) {
            self.accounts.push(account.to_string());
        }
    }
}

/// The tokens a server counted in a question it was sent, and MailCraft's estimate of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counted {
    pub estimated: u64,
    pub read: u64,
}

/// A turn of a chat: what was said, as the model is sent it, and what the window shows with it.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTurn {
    #[serde(flatten)]
    pub turn: AiTurn,
    /// What the model thought before it answered (its turns).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub thinking: String,
    /// What the person pointed at as they said it: the items, and their text as it was sent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<ContextItem>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub context_text: String,
    /// Its answer was cut short at the longest answer its profile asks for.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cut: bool,
    /// The person stopped it as it was written: what came of it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stopped: bool,
    #[serde(default)]
    pub at: i64,
}

impl ChatTurn {
    /// The turn as the model is sent it: when the person sent it (`when` writes the time), what
    /// they pointed at, then what they said. The time is the turn's own, so a turn is sent the
    /// same each time, and a server can reuse what it read.
    fn sent(&self, when: &dyn Fn(i64) -> String) -> AiTurn {
        let parts: Vec<&str> = [self.context_text.as_str(), self.turn.text.as_str()].into_iter().filter(|p| !p.is_empty()).collect();
        let text = parts.join("\n\n");
        // Not a turn that only gives back what the tools gave, nor one kept before the time was.
        if self.turn.role != AiRole::User || text.is_empty() || self.at == 0 {
            return AiTurn { text, ..self.turn.clone() };
        }
        AiTurn { text: format!("(Sent {})\n\n{text}", when(self.at)), ..self.turn.clone() }
    }
}

/// How a tool call went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    /// Read, and what it gave went back to the model.
    Ran,
    /// It couldn't be done: why went back to the model.
    Failed,
    /// Waiting for a message to download.
    Waiting,
    /// A change, waiting for the person to approve it or not.
    Proposed,
    /// A change the person approved, made.
    Done,
    /// A change the person declined, or went on without deciding.
    Declined,
    /// A change proposed beside a find or a read: not shown, for the model to propose again with
    /// the others, so the person approves them together.
    PutOff,
}

/// A tool call as it went: what a card in the chat shows.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRun {
    /// The call it answers.
    pub call: String,
    pub tool: String,
    pub state: RunState,
    /// What it does, in a line ("Move 3 messages to Archive").
    pub what: String,
    /// The messages it reads or changes, by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<String>,
    /// The events and people it found.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub found: Vec<Found>,
    /// What it gave back to the model: once the model has it (whole, in the chat's turns), its
    /// first thousand characters, what a card shows.
    #[serde(default)]
    pub result: String,
    /// The script whose call it is (its call), shown under it: none, the model's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_of: Option<String>,
    /// A change a script proposed: what the script called it with, made from it when approved
    /// (the model's own are made from its call).
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub input: Value,
}

/// A chat as listed: its name and dates, without what was said.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChatHead {
    pub id: String,
    pub title: String,
    pub created: i64,
    pub updated: i64,
}

/// The assistant as it runs: the chats kept, those read so far, and the questions they wait on.
#[derive(Default)]
pub struct Assistant {
    /// Every chat kept, newest last; read from this computer when first wanted.
    heads: Vec<ChatHead>,
    loaded: bool,
    /// When old chats were last looked for ([`Session::assistant_expire`]).
    expired_at: i64,
    /// The chats read so far, by id.
    chats: HashMap<String, Chat>,
    /// The chats changed since they were last kept, and whether the list changed.
    changed: BTreeSet<String>,
    heads_changed: bool,
    /// The question each chat waits on, by question; the steps taken since the person last
    /// said something.
    asking: HashMap<u64, String>,
    steps: HashMap<String, u32>,
    /// Questions whose chat stopped waiting: their answers are let go as they come.
    dropped: HashSet<u64>,
    /// The scripts waiting for a message to download or an answer, by chat and call.
    scripts: HashMap<(String, String), script::Paused>,
    /// The questions scripts asked, by question: the chat and call of each.
    script_asks: HashMap<u64, (String, String)>,
    /// The calls scripts made to MCP servers, by call: the chat and call of each.
    script_mcps: HashMap<u64, (String, String)>,
    /// The calls to MCP servers the person approved, by call: the chat and run of each.
    mcp_approved: HashMap<u64, (String, String)>,
    /// The scripts kept, once read ([`Session::kept_load`]).
    kept: Option<Vec<script::KeptScript>>,
    /// How many tokens each chat's question was estimated at as it was sent, by question: set
    /// beside the server's count when the answer comes ([`Chat::counted`]).
    estimates: HashMap<u64, u64>,
}

impl Assistant {
    /// The chats kept, newest first.
    pub fn heads(&self) -> impl Iterator<Item = &ChatHead> {
        self.heads.iter().rev()
    }

    /// The scripts kept, once read ([`Session::assistant_scripts`] reads them).
    pub fn kept_scripts(&self) -> &[KeptScript] {
        self.kept.as_deref().unwrap_or_default()
    }

    /// Chat `id`, if it's been read ([`Session::assistant_shown`] reads it).
    pub fn chat(&self, id: &str) -> Option<&Chat> {
        self.chats.get(id)
    }

    /// The question chat `id` waits on, if it does.
    pub fn asking(&self, id: &str) -> Option<u64> {
        self.asking.iter().find(|(_, c)| *c == id).map(|(q, _)| *q)
    }

    /// Whether question `q` is a chat's or its script's (asked or let go).
    pub(crate) fn owns(&self, q: u64) -> bool {
        self.asking.contains_key(&q) || self.dropped.contains(&q) || self.script_asks.contains_key(&q)
    }

    /// Whether question `q` was let go: its chat stopped or deleted.
    pub(crate) fn let_go(&self, q: u64) -> bool {
        self.dropped.contains(&q)
    }
}
