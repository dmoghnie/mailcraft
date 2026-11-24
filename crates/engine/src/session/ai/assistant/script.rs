//! The assistant's scripts (code mode): a script the model writes in Rhai, run here, calling
//! the tools that find and read as functions that give data rather than text, so that what
//! would take the model many steps (a call, its answer read, the next call) takes one. Its
//! calls are the tools' own, with their checks (an account's mail goes only to a model it may
//! go to), and each shows on the chat under the script's card. A read of a message still to
//! download waits for it; the script then runs again from its start with what its calls were
//! answered so far ([`run`]). A model runs scripts only when the person let it (File › Options
//! › AI).

mod run;
#[cfg(test)]
mod tests;
mod values;

use mailcraft_model::{AiTool, AiToolCall, MessageKey};
use serde_json::{Value, json};

use self::run::{Answer, CALLS_MOST, Call, OPS_ALL, Ran, Runner};
use super::context::cut;
use super::exchange::run_of;
use super::looking::said;
use super::tools::{Outcome, reads};
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

/// The tools a script calls, as functions of its own: those that find and read.
const SCRIPT_TOOLS: [&str; 8] =
    ["list_folders", "find_messages", "read_message", "find_events", "find_people", "find_tasks", "find_notes", "selected_messages"];

const DESCRIPTION: &str = "Run a script, in Rhai (a small language like Rust and JavaScript), that calls the tools that find and read as functions, to do in one step what would take many calls. Each takes a map of what the tool takes, or nothing, and gives data (maps and arrays) rather than text: `let found = find_messages(#{query: \"is:unread\", limit: 50}); for m in found.messages { if m.from.email.ends_with(\"@acme.example\") { print(`${m.when}: ${m.subject}`); } }`. read_message(id) gives a message with its text. Messages, events, people, tasks and notes have the ids the tools take; dates are Unix seconds, with `when` as the person reads them; now() is the time now. What it prints, and the value it ends with, come back to you. A tool that fails throws an error (try/catch). It can't change anything: propose changes with the tools, as ever. At most 100 calls; no files, network or modules.";

/// `run_script`, as the model is told it.
pub(super) fn tool() -> AiTool {
    AiTool {
        name: RUN_SCRIPT.into(),
        description: DESCRIPTION.into(),
        schema: json!({"type": "object", "properties": {
            "script": {"type": "string", "description": "The script, in Rhai"},
            "what": {"type": "string", "description": "What it does, in a few words, for the person (\"Find who I haven't answered\")"}}, "required": ["script"]}),
    }
}

/// A script as it runs: its call, and what its calls were answered so far.
struct Script {
    chat: String,
    call: AiToolCall,
    /// What `now()` gives: the time it began.
    now: i64,
    log: Vec<(Call, Answer)>,
    /// The operations its runs took, in all.
    ops: u64,
}

/// A script waiting for a message to download: the call that reads it.
pub(crate) struct Paused {
    script: Script,
    waits: Call,
    key: MessageKey,
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
        let script = Script { chat: chat.id.clone(), call: call.clone(), now: self.clock.now, log: Vec::new(), ops: 0 };
        self.script_go(chat, profile, script)
    }

    /// Run `script` as far as it goes: to its end, or to a message one of its calls waits for
    /// (kept then, for [`Session::script_body`]).
    fn script_go(&mut self, chat: &mut Chat, profile: &AiProfile, mut script: Script) -> Outcome {
        let what = what_of(&script.call.input);
        let source = script.call.input.get("script").and_then(Value::as_str).unwrap_or_default();
        let mut runner = match Runner::new(source, &SCRIPT_TOOLS, script.now) {
            Ok(r) => r,
            Err(why) => return Outcome::failed(&what, why),
        };
        // Each turn answers one more call, or ends: the calls and the operations are bounded.
        loop {
            let (ran, ops) = runner.run(&script.log, OPS_ALL.saturating_sub(script.ops));
            script.ops = script.ops.saturating_add(ops);
            let call = match ran {
                Ran::Done { printed, value } => return Outcome::ran(&what, Vec::new(), came_to(&printed, &value), Value::Null),
                Ran::Failed(why) => return Outcome::failed(&what, why),
                Ran::Needs(call) => call,
            };
            match self.script_call(chat, profile, &script, &call) {
                Ok(answer) => script.log.push((call, answer)),
                Err(key) => {
                    let at = (script.chat.clone(), script.call.id.clone());
                    self.ai.assistant.scripts.insert(at, Paused { script, waits: call, key });
                    return Outcome::Scripting { what };
                }
            }
        }
    }

    /// Make a script's call (its next): what it was answered, or the message it waits for. Its
    /// run shows under the script's card, and holds what it found, as the model's own do.
    fn script_call(&mut self, chat: &mut Chat, profile: &AiProfile, script: &Script, call: &Call) -> Result<Answer, MessageKey> {
        let id = format!("{}#{}", script.call.id, script.log.len().saturating_add(1));
        let made = AiToolCall { id, name: call.name.clone(), input: call.input.clone() };
        let outcome = if reads(&call.name) {
            self.tool_call(chat, profile, &made)
        } else {
            Outcome::failed(&call.name, format!("a script can't call `{}`", call.name.chars().take(64).collect::<String>()))
        };
        let answer = match &outcome {
            Outcome::Ran { data, .. } => Ok(Answer::Gave(data.clone())),
            Outcome::Failed { result, .. } => Ok(Answer::Failed(result.clone())),
            Outcome::Waiting { key, .. } => Err(key.clone()),
            Outcome::Proposed { .. } | Outcome::Scripting { .. } => Ok(Answer::Failed("a script can't do that".into())),
        };
        let run = ToolRun { part_of: Some(script.call.id.clone()), ..run_of(&made, outcome) };
        match chat.run_mut(&run.call) {
            Some(r) => *r = run,
            None => chat.runs.push(run),
        }
        answer
    }

    /// A message came, or couldn't: the scripts waiting for it go on.
    pub(super) fn script_body(&mut self, key: &MessageKey, failed: Option<&str>) {
        let waiting: Vec<(String, String)> = self.ai.assistant.scripts.iter().filter(|(_, p)| &p.key == key).map(|(at, _)| at.clone()).collect();
        for at in waiting {
            let Some(Paused { mut script, waits, .. }) = self.ai.assistant.scripts.remove(&at) else { continue };
            let Ok(mut chat) = self.take_chat(&script.chat) else { continue };
            // Stopped meanwhile: let go.
            if !chat.run(&script.call.id).is_some_and(|r| r.state == RunState::Waiting) {
                self.put_chat(chat);
                continue;
            }
            let outcome = match self.assistant_profile(&chat) {
                Err(e) => Outcome::failed(&what_of(&script.call.input), said(&e)),
                Ok(profile) => {
                    let answer = match failed {
                        Some(e) => Answer::Failed(format!("the message couldn't be downloaded: {e}")),
                        None => self
                            .script_call(&mut chat, &profile, &script, &waits)
                            .unwrap_or_else(|_| Answer::Failed("the message came, but couldn't be read".into())),
                    };
                    if let Answer::Failed(why) = &answer
                        && let Some(r) = chat.run_mut(&format!("{}#{}", script.call.id, script.log.len().saturating_add(1)))
                    {
                        r.state = RunState::Failed;
                        r.result = why.clone();
                    }
                    script.log.push((waits, answer));
                    self.script_go(&mut chat, &profile, script)
                }
            };
            let call = at.1;
            if !matches!(outcome, Outcome::Scripting { .. })
                && let Some(made) = chat.open_calls().into_iter().find(|c| c.id == call)
                && let Some(r) = chat.run_mut(&call)
            {
                *r = run_of(&made, outcome);
            }
            let id = chat.id.clone();
            self.put_chat(chat);
            self.assistant_go_on(&id);
        }
    }

    /// Chat `chat`'s calls were settled: its scripts waiting stop, and so do their reads.
    pub(super) fn scripts_stopped(&mut self, chat: &mut Chat) {
        self.ai.assistant.scripts.retain(|(c, _), _| *c != chat.id);
        for r in chat.runs.iter_mut().filter(|r| r.part_of.is_some() && r.state == RunState::Waiting) {
            r.state = RunState::Failed;
            r.result = "stopped before the message came".into();
        }
    }
}

/// The fuzzer's: `source` run as the assistant runs a script, its calls answered in turn by the
/// JSON values `answers` holds, one after another (past them, a call fails). What it came to,
/// as the model would be told.
#[doc(hidden)]
pub fn run_script_against(source: &str, answers: &[u8]) -> String {
    let answers: Vec<Value> =
        serde_json::Deserializer::from_slice(answers).into_iter::<Value>().map_while(Result::ok).take(CALLS_MOST.saturating_add(1)).collect();
    let mut runner = match Runner::new(source, &SCRIPT_TOOLS, 0) {
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
