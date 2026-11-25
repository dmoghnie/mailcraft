//! The assistant's scripts (code mode): a script the model writes in Rhai, run here, calling
//! the tools that find and read as functions that give data rather than text, so that what
//! would take the model many steps (a call, its answer read, the next call) takes one. Its
//! calls are the tools' own, with their checks (an account's mail goes only to a model it may
//! go to), and each shows on the chat under the script's card. A change it calls for is
//! proposed, as the model's own are: the script is told so, and the person approves it or not
//! once the script ended. A read of a message still to download waits for it; the script then
//! runs again from its start with what its calls were answered so far ([`run`]). It finds and
//! calls the tools of the MCP servers the person added, waiting for each answer as for a
//! message; one that may change something is proposed ([`mcp`]). A model runs scripts only when
//! the person let it (File › Options › AI).

mod agents;
mod asking;
mod kept;
mod mcp;
mod run;
mod strings;
#[cfg(test)]
mod tests;
mod values;

use mailcraft_model::{AiTool, AiToolCall, MessageKey};
use serde_json::{Value, json};

pub(super) use self::asking::ASK;
pub(crate) use self::kept::commands as script_commands;
pub(super) use self::kept::{KEEP_SCRIPT, keep_tool, kept_tools};
pub use self::kept::{KeptScript, ScriptParam};
use self::mcp::FIND_TOOLS;
pub(super) use self::mcp::MCP;
use self::run::{Answer, CALLS_MOST, Call, OPS_ALL, RHAI, Ran, Runner, With};
use super::context::cut;
use super::exchange::run_of;
use super::looking::said;
use super::tools::{Outcome, reads, tools};
use super::*;
use crate::Session;

/// The tool's name.
pub(super) const RUN_SCRIPT: &str = "run_script";
/// The longest script run, in characters.
const SOURCE_MOST: usize = 20_000;
/// The most of what a script came to given back, in characters.
const VALUE_MOST: usize = 8_000;
/// What a script's card says it does when the model didn't say.
const WHAT_AT_FIRST: &str = "Run a script";
/// The most changes a script proposes: each is a card to decide, and approved, a write opens
/// a message.
const PROPOSALS_MOST: usize = 20;
/// What a change says when its script failed after proposing it.
const WITHDRAWN: &str = "not made: the script failed before it ended";

/// The tools a script calls, as functions of its own: every one the model has (not scripts).
fn script_tools() -> Vec<String> {
    tools().into_iter().map(|t| t.name).collect()
}

/// The example the model is shown: it runs, as a test checks.
pub(super) const EXAMPLE: &str = r#"let found = find_messages(#{query: "from:acme.example", limit: 50});
let unread = found.messages.filter(|m| m.unread);
for m in unread { print(`${m.when}: ${m.subject}`); }
let ids = unread.map(|m| m.id);
if ids.len() > 0 { print(move_messages(#{ids: ids, folder: "archive"}).proposed); }
ids.len()"#;

/// The tool, as the model is told it, with the kept scripts named as functions and the MCP
/// servers `servers` its scripts may call.
fn description(kept: &[KeptScript], servers: &[String]) -> String {
    let names: Vec<&str> = kept.iter().filter(|k| !k.off).map(|k| k.name.as_str()).collect();
    let mut said = described();
    if !names.is_empty() {
        said.push_str(&format!(" The scripts kept are functions too, each taking one map, as their tools do: {}.", names.join(", ")));
    }
    said.push_str(&mcp::told(servers));
    said
}

/// The tool, as the model is told it.
fn described() -> String {
    format!(
        "Run a script, in Rhai, that calls the tools as functions, to do in one step what would take many calls. Each tool takes ONE map, written #{{key: value}}, or nothing: find_messages(#{{query: \"is:unread\", limit: 50}}), move_messages(#{{ids: ids, folder: \"archive\"}}); read_message(id) takes an id. {RHAI} The tools give data (maps and arrays) rather than text, for example:\n{EXAMPLE}\nA message has id, account, folder, date, when, from, to, cc, subject, unread, flagged, answered, bulk, files, categories and preview: from is #{{name, email}}, to and cc are arrays of those, bulk says it came from a list or a mass sender, and files says whether it has any (read_message's lists their names, and adds its text). Messages, events, people, tasks and notes have the ids the tools take; dates are Unix seconds, with `when` as the person reads them; now() is the time now. What it prints, and the value it ends with, come back to you. ask(question) or ask(question, text) asks the language model a question, about the text when given, and gives its answer as a string: for what code can't judge (does this message ask for a quote?); at most 20, each taking the model's time. A tool that fails throws an error (try/catch). A tool that changes (move_messages, add_task…) proposes the change and gives #{{proposed: what it does}}: nothing changes until the person approves it, after the script ended, and you're told what they decided. At most 100 calls and 20 changes; no files, network or modules."
    )
}

/// `run_script`, as the model is told it.
pub(super) fn tool(kept: &[KeptScript], servers: &[String]) -> AiTool {
    AiTool {
        name: RUN_SCRIPT.into(),
        description: description(kept, servers),
        schema: json!({"type": "object", "properties": {
            "script": {"type": "string", "description": "The script, in Rhai"},
            "what": {"type": "string", "description": "What it does, in a few words, for the person (\"Find who I haven't answered\")"}}, "required": ["script"]}),
    }
}

/// A script as it runs: its call, what its card says it does, its code (and what it was called
/// with, a kept script), and what its calls were answered so far.
struct Script {
    chat: String,
    call: AiToolCall,
    what: String,
    source: String,
    params: Option<Value>,
    /// What `now()` gives: the time it began.
    now: i64,
    log: Vec<(Call, Answer)>,
    /// The operations its runs took, in all.
    ops: u64,
}

/// What a script waits for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Wait {
    /// A message to download.
    Message(MessageKey),
    /// The answer to question `q`, which it asked.
    Answer(u64),
    /// What call `id` to an MCP server gives.
    Mcp(u64),
}

/// What came for a script waiting.
pub(super) enum Came {
    /// The message it waits for (or why it couldn't be downloaded).
    Body { failed: Option<String> },
    /// The answer to its question (or why none came).
    Answer(Result<String, String>),
    /// What its call to an MCP server gave (or why it failed).
    Mcp(Result<Value, String>),
}

/// A script waiting: the call that waits, and for what.
pub(crate) struct Paused {
    script: Script,
    waits: Call,
    on: Wait,
}

/// What a script's card says it does.
fn what_of(input: &Value) -> String {
    let what = input.get("what").and_then(Value::as_str).map(str::trim).filter(|w| !w.is_empty());
    what.map_or_else(|| WHAT_AT_FIRST.to_string(), |w| w.chars().take(80).collect())
}

/// What a script that ran gives back: what it printed, and what it came to.
fn came_to(printed: &str, value: &Value) -> String {
    let mut out = printed.trim_end().to_string();
    if !value.is_null() {
        let v = match value {
            Value::String(s) => s.clone(),
            v => v.to_string(),
        };
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("It came to: {}", cut(&v, VALUE_MOST)));
    }
    if out.is_empty() { "It ran, and printed nothing.".into() } else { out }
}

impl Session {
    /// `run_script`: the script run, to its end or to a message one of its calls waits for.
    pub(super) fn tool_run_script(&mut self, chat: &mut Chat, profile: &AiProfile, call: &AiToolCall) -> Outcome {
        let what = what_of(&call.input);
        if !profile.scripts {
            return Outcome::failed(&what, format!("{} may not run scripts: the person decides that in File › Options › AI", profile.name));
        }
        let source = call.input.get("script").and_then(Value::as_str).unwrap_or_default();
        if source.trim().is_empty() {
            return Outcome::failed(&what, "`script` is the script to run".into());
        }
        if source.chars().count() > SOURCE_MOST {
            return Outcome::failed(&what, format!("a script is at most {SOURCE_MOST} characters"));
        }
        let script = Script {
            chat: chat.id.clone(),
            call: call.clone(),
            what,
            source: source.to_string(),
            params: None,
            now: self.clock.now,
            log: Vec::new(),
            ops: 0,
        };
        self.script_go(chat, profile, script)
    }

    /// Run `script` as far as it goes: to its end, or to a message one of its calls waits for
    /// (kept then, for [`Session::script_body`]).
    fn script_go(&mut self, chat: &mut Chat, profile: &AiProfile, mut script: Script) -> Outcome {
        let what = script.what.clone();
        let names = script_tools();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        self.kept_load();
        let library = kept::library(self.kept_loaded());
        let with = With { library: &library, params: script.params.as_ref() };
        let mut runner = match Runner::with(&script.source, with, &names, script.now) {
            Ok(r) => r,
            Err(why) => return Outcome::failed(&what, why),
        };
        // Each turn answers one more call, or ends: the calls and the operations are bounded.
        loop {
            let (ran, ops) = runner.run(&script.log, OPS_ALL.saturating_sub(script.ops));
            script.ops = script.ops.saturating_add(ops);
            let call = match ran {
                Ran::Done { printed, value } => return Outcome::ran(&what, Vec::new(), came_to(&printed, &value), Value::Null),
                Ran::Failed(why) => {
                    withdraw(chat, &script.call.id);
                    return Outcome::failed(&what, why);
                }
                Ran::Needs(call) => call,
            };
            match self.script_call(chat, profile, &script, &call) {
                Ok(answer) => script.log.push((call, answer)),
                Err(on) => {
                    let at = (script.chat.clone(), script.call.id.clone());
                    self.ai.assistant.scripts.insert(at, Paused { script, waits: call, on });
                    return Outcome::Scripting { what };
                }
            }
        }
    }

    /// Make a script's call (its next): what it was answered, or what it waits for. Its run
    /// shows under the script's card, and holds what it found, as the model's own do; a change
    /// proposed holds what it was called with, to be made from once approved.
    fn script_call(&mut self, chat: &mut Chat, profile: &AiProfile, script: &Script, call: &Call) -> Result<Answer, Wait> {
        let id = format!("{}#{}", script.call.id, script.log.len().saturating_add(1));
        let made = AiToolCall { id, name: call.name.clone(), input: call.input.clone() };
        if call.name == ASK {
            return self.script_ask(chat, profile, script, made);
        }
        if call.name == MCP {
            return self.script_mcp(chat, profile, script, made);
        }
        let proposed = chat.runs.iter().filter(|r| r.part_of.as_deref() == Some(script.call.id.as_str()) && !r.input.is_null()).count();
        let outcome = if call.name == FIND_TOOLS {
            self.script_find_tools(profile, &call.input)
        } else if !script_tools().contains(&call.name) {
            Outcome::failed(&call.name, format!("a script can't call `{}`", call.name.chars().take(64).collect::<String>()))
        } else if proposed >= PROPOSALS_MOST && !reads(&call.name) {
            Outcome::failed(&call.name, format!("a script proposes at most {PROPOSALS_MOST} changes: propose the rest after it"))
        } else {
            self.tool_call(chat, profile, &made)
        };
        let (answer, input) = match &outcome {
            Outcome::Ran { data, .. } => (Ok(Answer::Gave(data.clone())), Value::Null),
            Outcome::Failed { result, .. } => (Ok(Answer::Failed(result.clone())), Value::Null),
            Outcome::Waiting { key, .. } => (Err(Wait::Message(key.clone())), Value::Null),
            Outcome::Proposed { what, .. } => (Ok(Answer::Gave(json!({"proposed": what}))), call.input.clone()),
            Outcome::Scripting { .. } => (Ok(Answer::Failed("a script can't run a script".into())), Value::Null),
        };
        let run = ToolRun { part_of: Some(script.call.id.clone()), input, ..run_of(&made, outcome) };
        match chat.run_mut(&run.call) {
            Some(r) => *r = run,
            None => chat.runs.push(run),
        }
        answer
    }

    /// A message came, or couldn't: the scripts waiting for it go on.
    pub(super) fn script_body(&mut self, key: &MessageKey, failed: Option<&str>) {
        let on = Wait::Message(key.clone());
        let waiting: Vec<(String, String)> = self.ai.assistant.scripts.iter().filter(|(_, p)| p.on == on).map(|(at, _)| at.clone()).collect();
        for at in waiting {
            self.script_resume(&at, Came::Body { failed: failed.map(str::to_string) });
        }
    }

    /// What script `at` (its chat and call) waited for came: it goes on from there, and its chat
    /// with it once it ends.
    pub(super) fn script_resume(&mut self, at: &(String, String), came: Came) {
        let Some(Paused { mut script, waits, .. }) = self.ai.assistant.scripts.remove(at) else { return };
        let Ok(mut chat) = self.take_chat(&script.chat) else { return };
        // Stopped meanwhile: let go.
        if !chat.run(&script.call.id).is_some_and(|r| r.state == RunState::Waiting) {
            self.put_chat(chat);
            return;
        }
        let part = format!("{}#{}", script.call.id, script.log.len().saturating_add(1));
        let outcome = match self.assistant_profile(&chat) {
            Err(e) => {
                withdraw(&mut chat, &script.call.id);
                Outcome::failed(&script.what, said(&e))
            }
            Ok(profile) => {
                let answer = match came {
                    Came::Body { failed: Some(e) } => Answer::Failed(format!("the message couldn't be downloaded: {e}")),
                    Came::Body { failed: None } => self
                        .script_call(&mut chat, &profile, &script, &waits)
                        .unwrap_or_else(|_| Answer::Failed("the message came, but couldn't be read".into())),
                    Came::Answer(Ok(text)) => {
                        if let Some(r) = chat.run_mut(&part) {
                            r.state = RunState::Ran;
                            r.result = cut(text.trim(), VALUE_MOST);
                        }
                        Answer::Gave(Value::String(text.trim().to_string()))
                    }
                    Came::Answer(Err(e)) => Answer::Failed(format!("the model couldn't answer: {e}")),
                    Came::Mcp(Ok(value)) => {
                        if let Some(r) = chat.run_mut(&part) {
                            r.state = RunState::Ran;
                            r.result = cut(value.as_str().map_or_else(|| value.to_string(), str::to_string).trim(), VALUE_MOST);
                        }
                        Answer::Gave(value)
                    }
                    Came::Mcp(Err(e)) => Answer::Failed(format!("the MCP server couldn't answer: {e}")),
                };
                if let Answer::Failed(why) = &answer
                    && let Some(r) = chat.run_mut(&part)
                {
                    r.state = RunState::Failed;
                    r.result = why.clone();
                }
                script.log.push((waits, answer));
                self.script_go(&mut chat, &profile, script)
            }
        };
        if !matches!(outcome, Outcome::Scripting { .. })
            && let Some(made) = chat.open_calls().into_iter().find(|c| c.id == at.1)
            && let Some(r) = chat.run_mut(&at.1)
        {
            *r = run_of(&made, outcome);
        }
        let id = chat.id.clone();
        self.put_chat(chat);
        self.assistant_go_on(&id);
    }

    /// Chat `chat`'s calls were settled: its scripts waiting stop, and so do their reads and
    /// questions.
    pub(super) fn scripts_stopped(&mut self, chat: &mut Chat) {
        self.scripts_let_go(&chat.id);
        for r in chat.runs.iter_mut().filter(|r| r.part_of.is_some() && r.state == RunState::Waiting) {
            r.state = RunState::Failed;
            r.result = match r.tool.as_str() {
                ASK => "stopped before the answer came",
                MCP => "stopped before the server answered",
                _ => "stopped before the message came",
            }
            .into();
        }
    }

    /// Chat `id`'s scripts waiting are let go, and the questions and calls to MCP servers they
    /// made with them, and the calls approved: their answers are dropped as they come.
    pub(super) fn scripts_let_go(&mut self, id: &str) {
        let mine = |at: &(String, String)| at.0 == id;
        let waits: Vec<Wait> = self.ai.assistant.scripts.iter().filter(|(at, _)| mine(at)).map(|(_, p)| p.on.clone()).collect();
        self.ai.assistant.scripts.retain(|at, _| !mine(at));
        self.ai.assistant.mcp_approved.retain(|_, at| !mine(at));
        for on in waits {
            match on {
                Wait::Answer(q) => {
                    self.ai.assistant.script_asks.remove(&q);
                    self.assistant_let_go(q);
                }
                Wait::Mcp(call) => {
                    self.ai.assistant.script_mcps.remove(&call);
                }
                Wait::Message(_) => {}
            }
        }
    }
}

/// Script `call` failed: the changes it proposed are not to be made, as only a run that ends
/// is heard.
fn withdraw(chat: &mut Chat, call: &str) {
    for r in chat.runs.iter_mut().filter(|r| r.part_of.as_deref() == Some(call) && r.state == RunState::Proposed) {
        r.state = RunState::Failed;
        r.result = WITHDRAWN.into();
    }
}

/// What became of the changes script `call` proposed, for the model, after what it came to:
/// nothing when it proposed none.
pub(super) fn decided(chat: &Chat, call: &str) -> String {
    let lines: Vec<String> = chat
        .runs
        .iter()
        .filter(|r| r.part_of.as_deref() == Some(call) && !r.input.is_null())
        .map(|r| {
            let went = match r.state {
                RunState::Done => format!("approved. {}", r.result),
                RunState::Declined => format!("the person declined: nothing changed. {}", r.result),
                RunState::Proposed => "not decided".into(),
                // A call to an MCP server approved, whose answer didn't come.
                RunState::Waiting => "approved, but stopped before the server answered: it may have been done".into(),
                _ => format!("couldn't be done: {}", r.result),
            };
            format!("- {}: {}", r.what, went.trim_end())
        })
        .collect();
    if lines.is_empty() { String::new() } else { format!("\n\nWhat it proposed:\n{}", lines.join("\n")) }
}

/// The fuzzer's: `source` run as the assistant runs a script, its calls answered in turn by the
/// JSON values `answers` holds, one after another (past them, a call fails). What it came to,
/// as the model would be told.
#[doc(hidden)]
pub fn run_script_against(source: &str, answers: &[u8]) -> String {
    let answers: Vec<Value> =
        serde_json::Deserializer::from_slice(answers).into_iter::<Value>().map_while(Result::ok).take(CALLS_MOST.saturating_add(1)).collect();
    let names = script_tools();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut runner = match Runner::new(source, &names, 0) {
        Ok(r) => r,
        Err(why) => return why,
    };
    let (mut log, mut ops) = (Vec::new(), 0u64);
    // Each turn answers one more call, or ends: the replay stops past the calls a script makes.
    loop {
        let (ran, used) = runner.run(&log, OPS_ALL.saturating_sub(ops));
        ops = ops.saturating_add(used);
        match ran {
            Ran::Done { printed, value } => return came_to(&printed, &value),
            Ran::Failed(why) => return why,
            Ran::Needs(call) => {
                let answer = answers.get(log.len()).cloned().map_or_else(|| Answer::Failed("no answer".into()), Answer::Gave);
                log.push((call, answer));
            }
        }
    }
}
