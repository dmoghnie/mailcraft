//! Scripts kept to be run again: one that proved useful is kept, with a name, what it does and
//! what it takes, once the person approves its card (`keep_script`), or as the person writes it
//! (`script.save`). Each kept script that's on is a function every script can call by its name
//! (`waiting_on_me(#{days: 7})`), and a tool of its own for a model that may run scripts, run as
//! `run_script` runs, its changes proposed. They're kept in the data folder (`ai-scripts.json`),
//! which starts with MailCraft's own, copies the person can change, turn off or delete.

use mailcraft_model::{AiTool, AiToolCall};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::run::{Runner, With};
use super::*;
use crate::{CmdError, CmdResult, CommandSpec, p};

/// Where the kept scripts are, in the data folder.
const SCRIPTS_DOC: &str = "ai-scripts.json";
/// The tool that keeps a script.
pub(in crate::session::ai::assistant) const KEEP_SCRIPT: &str = "keep_script";
/// The most scripts kept: each is a tool a model is told of.
const KEPT_MOST: usize = 40;
/// The longest name, in characters.
const NAME_MOST: usize = 40;
/// The longest a script's or a parameter's description is, in characters.
const ABOUT_MOST: usize = 500;
/// The most parameters a script takes.
const PARAMS_MOST: usize = 8;
/// The names a kept script can't take: the functions every script has besides the tools.
const TAKEN: &[&str] = &["ask", "now", "print", "debug", "params", "trim", "replace", RUN_SCRIPT, KEEP_SCRIPT];
/// Where the commands are.
const WHERE: &str = "AI assistant";

/// A script kept to be run again.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct KeptScript {
    /// What it's called, by other scripts and as a tool (`waiting_on_me`).
    pub name: String,
    /// What it does and gives, for the model and the person.
    pub about: String,
    /// What it takes, each read from the map `params` it's called with (`params.days`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ScriptParam>,
    /// Its code, in Rhai: the body of a function taking `params`.
    pub code: String,
    /// Turned off: kept, but neither a tool nor a function.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub off: bool,
}

/// Something a kept script takes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptParam {
    pub name: String,
    pub about: String,
}

/// Whether `name` is one a script or what it takes may be called: lower-case letters, digits
/// and `_`, starting with a letter.
fn name_ok(name: &str) -> bool {
    name.chars().count() <= NAME_MOST
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `s` as it's kept (trimmed), or why it can't be.
fn checked(s: KeptScript) -> Result<KeptScript, String> {
    let name = s.name.trim().to_string();
    if !name_ok(&name) {
        return Err(format!("a script's name is at most {NAME_MOST} lower-case letters, digits and _, starting with a letter (waiting_on_me)"));
    }
    if TAKEN.contains(&name.as_str()) || script_tools().contains(&name) {
        return Err(format!("`{name}` is a tool's or a function's name already"));
    }
    let about = s.about.trim().to_string();
    if about.is_empty() || about.chars().count() > ABOUT_MOST {
        return Err(format!("say what it does and gives, in at most {ABOUT_MOST} characters"));
    }
    if s.params.len() > PARAMS_MOST {
        return Err(format!("a script takes at most {PARAMS_MOST} things"));
    }
    let mut params: Vec<ScriptParam> = Vec::new();
    for p in s.params {
        let (name, about) = (p.name.trim().to_string(), p.about.trim().chars().take(ABOUT_MOST).collect::<String>());
        if !name_ok(&name) || params.iter().any(|q| q.name == name) {
            return Err(format!("what a script takes is named once each, as a script is (`{}`)", name.chars().take(NAME_MOST).collect::<String>()));
        }
        params.push(ScriptParam { name, about });
    }
    let code = s.code.trim().to_string();
    if code.is_empty() || code.chars().count() > SOURCE_MOST {
        return Err(format!("a script's code is from 1 to {SOURCE_MOST} characters"));
    }
    let kept = KeptScript { name, about, params, code, off: s.off };
    Runner::with("", With { library: &library(std::slice::from_ref(&kept)), params: None }, &[], 0)
        .map_err(|why| why.replacen("a kept script doesn't compile", "it doesn't compile", 1))?;
    Ok(kept)
}

/// The kept scripts that are on, as functions every script has: each its code, the body of a
/// function of its name taking `params`.
pub(super) fn library(kept: &[KeptScript]) -> String {
    kept.iter().filter(|k| !k.off).map(|k| format!("fn {}(params) {{\n{}\n}}\n", k.name, k.code)).collect()
}

/// The kept scripts that are on, as tools a model that may run scripts is told of.
pub(in crate::session::ai::assistant) fn kept_tools(kept: &[KeptScript]) -> Vec<AiTool> {
    kept.iter()
        .filter(|k| !k.off)
        .map(|k| {
            let properties: Map<String, Value> = k.params.iter().map(|p| (p.name.clone(), json!({"description": p.about}))).collect();
            AiTool {
                name: k.name.clone(),
                description: format!("{} (A script kept for this: it runs as run_script does, its changes proposed.)", k.about),
                schema: json!({"type": "object", "properties": properties}),
            }
        })
        .collect()
}

/// `keep_script`, as the model is told it.
pub(in crate::session::ai::assistant) fn keep_tool() -> AiTool {
    AiTool {
        name: KEEP_SCRIPT.into(),
        description: "Propose keeping a script that worked, to run again: a tool of its own, and a function other scripts call by its name with one map (waiting_on_me(#{days: 7})). Its code is a function's body, what it's called with being the map `params` (params.days), and it ends with what it gives. The person approves keeping it, seeing its code.".into(),
        schema: json!({"type": "object", "properties": {
            "name": {"type": "string", "description": "Lower-case letters, digits and _ (waiting_on_me)"},
            "about": {"type": "string", "description": "What it does and gives, for whoever calls it"},
            "params": {"type": "array", "description": "What it takes", "items": {"type": "object", "properties": {"name": {"type": "string"}, "about": {"type": "string"}}, "required": ["name"]}},
            "script": {"type": "string", "description": "Its code, in Rhai"}}, "required": ["name", "about", "script"]}),
    }
}

/// The scripts MailCraft starts with, copies the person can change, turn off or delete.
pub(super) fn shipped() -> Vec<KeptScript> {
    let param = |name: &str, about: &str| ScriptParam { name: name.into(), about: about.into() };
    let days = |at_first: u32| param("days", &format!("How many days old at least ({at_first} when not said)"));
    let script = |name: &str, about: &str, params: Vec<ScriptParam>, code: &str| KeptScript {
        name: name.into(),
        about: about.into(),
        params,
        code: code.trim().into(),
        off: false,
    };
    vec![
        script(
            "waiting_on_me",
            "Mail in the Inbox from people (not lists or mass senders) not answered yet, older than some days: each one's id, sender, subject and when it came.",
            vec![days(3)],
            WAITING_ON_ME,
        ),
        script(
            "waiting_on_them",
            "Mail the person sent that has no answer yet, older than some days: each one's id, who it went to, subject and when. Follow-ups can then be proposed with write_message.",
            vec![days(3)],
            WAITING_ON_THEM,
        ),
        script(
            "person_history",
            "A person's recent mail both ways, their contact card and the events they're in.",
            vec![param("who", "Their address, or a name"), param("days", "How far back, in days (90 when not said)")],
            PERSON_HISTORY,
        ),
        script("today", "The day ahead: events in the next day, tasks late or due, flagged mail, and important mail unread.", Vec::new(), TODAY),
        script("tidy", "Proposes moving newsletters and other mail from lists in the Inbox, older than some days, to Archive.", vec![days(30)], TIDY),
    ]
}

const WAITING_ON_ME: &str = r#"
let days = if "days" in params { params.days } else { 3 };
let before = now() - days * 86400;
let found = find_messages(#{folder: "inbox", limit: 50});
let waiting = found.messages.filter(|m| !m.answered && !m.bulk && m.date < before);
waiting.map(|m| #{id: m.id, from: m.from, subject: m.subject, when: m.when})
"#;

const WAITING_ON_THEM: &str = r#"
let days = if "days" in params { params.days } else { 3 };
let before = now() - days * 86400;
let found = find_messages(#{query: "is:awaiting", limit: 50});
let waiting = found.messages.filter(|m| m.date < before);
waiting.map(|m| #{id: m.id, to: m.to, subject: m.subject, when: m.when})
"#;

const PERSON_HISTORY: &str = r#"
let who = params.who;
let days = if "days" in params { params.days } else { 90 };
let since = now() - days * 86400;
let row = |m| #{id: m.id, subject: m.subject, when: m.when, files: m.files};
let from_them = find_messages(#{query: `from:${who}`, limit: 20}).messages.filter(|m| m.date >= since);
let to_them = find_messages(#{query: `to:${who}`, limit: 20}).messages.filter(|m| m.date >= since);
#{
    people: find_people(#{query: who, limit: 5}).people,
    from_them: from_them.map(row),
    to_them: to_them.map(row),
    events: find_events(#{query: `attendee:${who}`, when: "all", limit: 10}).events
}
"#;

const TODAY: &str = r#"
let soon = now() + 86400;
let events = find_events(#{when: "upcoming", limit: 20}).events.filter(|e| e.start < soon);
let tasks = find_tasks(#{limit: 50}).tasks.filter(|t| !t.done && (t.late || type_of(t.due) == "string"));
let row = |m| #{id: m.id, from: m.from, subject: m.subject, when: m.when};
#{
    events: events.map(|e| #{id: e.id, when: e.when, summary: e.summary, location: e.location}),
    tasks: tasks.map(|t| #{id: t.id, title: t.title, due: t.due, late: t.late}),
    flagged: find_messages(#{query: "is:flagged", limit: 20}).messages.map(row),
    important_unread: find_messages(#{query: "is:unread is:important", limit: 20}).messages.map(row)
}
"#;

const TIDY: &str = r#"
let days = if "days" in params { params.days } else { 30 };
let before = now() - days * 86400;
let old = find_messages(#{folder: "inbox", limit: 50}).messages.filter(|m| m.bulk && m.date < before);
if old.len() == 0 {
    return "Nothing to tidy.";
}
let ids = old.map(|m| m.id);
move_messages(#{ids: ids, folder: "archive"});
`Proposed for Archive, from lists: ${ids.len()}`
"#;

/// A kept script as `keep_script` was called with, and the code's own lines.
fn keep_input(input: &Value) -> KeptScript {
    let s = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let params = input
        .get("params")
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |ps| ps.iter().filter_map(|p| serde_json::from_value::<ScriptParam>(p.clone()).ok()).collect());
    KeptScript { name: s("name"), about: s("about"), params, code: s("script"), off: false }
}

impl Session {
    /// The kept scripts read from the data folder, the first time they're wanted (MailCraft's
    /// own when none were ever kept, or the file was damaged).
    pub(in crate::session::ai::assistant) fn kept_load(&mut self) {
        if self.ai.assistant.kept.is_some() {
            return;
        }
        let read = self.store.as_ref().map(|s| s.load_doc::<Option<Vec<KeptScript>>>(SCRIPTS_DOC));
        let kept = match read {
            Some(Ok(Some(kept))) => kept,
            Some(Err(e)) => {
                log::warn!("the scripts MailCraft's AI keeps couldn't be read: {e}");
                shipped()
            }
            _ => shipped(),
        };
        self.ai.assistant.kept = Some(kept);
    }

    /// The kept scripts, as read so far.
    pub(in crate::session::ai::assistant) fn kept_loaded(&self) -> &[KeptScript] {
        self.ai.assistant.kept.as_deref().unwrap_or_default()
    }

    /// The kept script named `name` that's on.
    pub(in crate::session::ai::assistant) fn kept_on(&self, name: &str) -> Option<&KeptScript> {
        self.kept_loaded().iter().find(|k| k.name == name && !k.off)
    }

    /// The kept scripts as they are now, kept in the data folder.
    fn kept_put(&mut self, kept: Vec<KeptScript>) -> Result<(), CmdError> {
        if let Some(store) = &self.store {
            store.save_doc(SCRIPTS_DOC, &Some(&kept)).map_err(|e| CmdError::Failed(format!("the scripts couldn't be kept: {e}")))?;
        }
        self.ai.assistant.kept = Some(kept);
        Ok(())
    }

    /// `s` kept, in place of the one named `was` (or its own name), checked first.
    pub(in crate::session::ai::assistant) fn kept_save(&mut self, s: KeptScript, was: Option<&str>) -> Result<KeptScript, CmdError> {
        let s = checked(s).map_err(CmdError::Params)?;
        self.kept_load();
        let was = was.unwrap_or(&s.name);
        let others: Vec<KeptScript> = self.kept_loaded().iter().filter(|k| k.name != was).cloned().collect();
        if others.iter().any(|k| k.name == s.name) {
            return Err(CmdError::Params(format!("a script named {} is kept already", s.name)));
        }
        if others.len() >= KEPT_MOST {
            return Err(CmdError::Failed(format!("at most {KEPT_MOST} scripts are kept: delete one first")));
        }
        // In its old place, renamed or changed; a new one last.
        let at = self.kept_loaded().iter().position(|k| k.name == was).unwrap_or(others.len());
        let mut kept = others;
        kept.insert(at.min(kept.len()), s.clone());
        self.kept_put(kept)?;
        Ok(s)
    }

    /// The kept script named `name` deleted.
    fn kept_delete(&mut self, name: &str) -> Result<(), CmdError> {
        self.kept_load();
        if !self.kept_loaded().iter().any(|k| k.name == name) {
            return Err(CmdError::Params(format!("no script named {name} is kept")));
        }
        let kept = self.kept_loaded().iter().filter(|k| k.name != name).cloned().collect();
        self.kept_put(kept)
    }

    /// The kept script named `name` turned on or off.
    fn kept_turn(&mut self, name: &str, on: bool) -> Result<(), CmdError> {
        self.kept_load();
        if !self.kept_loaded().iter().any(|k| k.name == name) {
            return Err(CmdError::Params(format!("no script named {name} is kept")));
        }
        let kept = self.kept_loaded().iter().map(|k| if k.name == name { KeptScript { off: !on, ..k.clone() } } else { k.clone() }).collect();
        self.kept_put(kept)
    }

    /// What `keep_script` proposes, checked: a new name, code that compiles.
    pub(in crate::session::ai::assistant) fn keep_of(&mut self, input: &Value) -> Result<KeptScript, String> {
        let s = checked(keep_input(input))?;
        self.kept_load();
        if self.kept_loaded().iter().any(|k| k.name == s.name) {
            return Err(format!("a script named {} is kept already: name this one otherwise", s.name));
        }
        Ok(s)
    }

    /// A kept script called as a tool: run as `run_script` runs, what it was called with its
    /// `params`.
    pub(in crate::session::ai::assistant) fn tool_run_kept(&mut self, chat: &mut Chat, profile: &AiProfile, call: &AiToolCall) -> Outcome {
        let what = self.kept_on(&call.name).map_or_else(|| call.name.clone(), |k| k.about.chars().take(80).collect());
        if !profile.scripts {
            return Outcome::failed(&what, format!("{} may not run scripts: the person decides that in File › Options › AI", profile.name));
        }
        let params = if call.input.is_object() { call.input.clone() } else { json!({}) };
        let source = format!("{}(params)", call.name);
        let script =
            Script { chat: chat.id.clone(), call: call.clone(), what, source, params: Some(params), now: self.clock.now, log: Vec::new(), ops: 0 };
        self.script_go(chat, profile, script)
    }
}

/// The scripts' commands: listed, saved, turned on or off, deleted.
pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("script.list", "List the scripts MailCraft's AI keeps", WHERE, |s, _| {
            s.kept_load();
            Ok(json!({"scripts": s.kept_loaded()}))
        }),
        CommandSpec::new("script.save", "Keep a script for MailCraft's AI", WHERE, save).params(
            r#"{"name": string (lower-case letters, digits and _), "about": string (what it does and gives), "code": string (Rhai: a function's body, taking the map `params`), "params"?: [{"name": string, "about": string}], "off"?: bool, "was"?: string (the name it had, to rename it)}"#,
        ),
        CommandSpec::new("script.turn", "Turn a kept script on or off", WHERE, |s, v| {
            let name = p::req_str(v, "name")?.to_string();
            let on = p::bool(v, "on").ok_or_else(|| CmdError::Params("`on` (bool) is required".into()))?;
            s.kept_turn(&name, on)?;
            Ok(json!({"name": name, "on": on}))
        })
        .params(r#"{"name": string, "on": bool}"#),
        CommandSpec::new("script.delete", "Delete a kept script", WHERE, |s, v| {
            let name = p::req_str(v, "name")?.to_string();
            s.kept_delete(&name)?;
            Ok(json!({"deleted": name}))
        })
        .params(r#"{"name": string}"#),
    ]
}

fn save(s: &mut Session, v: &Value) -> CmdResult {
    let str_of = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let params = match v.get("params") {
        None | Some(Value::Null) => Vec::new(),
        Some(ps) => serde_json::from_value::<Vec<ScriptParam>>(ps.clone())
            .map_err(|_| CmdError::Params("`params` is a list of {\"name\", \"about\"}".into()))?,
    };
    let script = KeptScript { name: str_of("name"), about: str_of("about"), params, code: str_of("code"), off: p::bool(v, "off").unwrap_or(false) };
    let was = v.get("was").and_then(Value::as_str).map(str::to_string);
    let kept = s.kept_save(script, was.as_deref())?;
    Ok(json!({"name": kept.name}))
}
