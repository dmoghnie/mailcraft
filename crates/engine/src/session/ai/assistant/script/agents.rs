//! A script run for the CLI (`script.run`): a kept script by its name, or code of its own, run
//! at once against what's on this computer. It reads every account, whatever MailCraft's AI may
//! read. It only reads: there's no card to approve a change on, so a change it calls for fails,
//! as a question to a model does; a message not downloaded yet is fetched, for the next run.

use mailcraft_model::AiToolCall;
use serde_json::{Value, json};

use super::run::{Answer, Call, OPS_ALL, Ran, Runner, With};
use super::*;
use crate::{CmdError, CmdResult, CommandSpec, p};

/// Where the command is.
const WHERE: &str = "AI assistant";

/// Who an agent's script reads as: no model the person added, but every account.
fn agent() -> AiProfile {
    AiProfile { id: "agent".into(), name: "an outside agent".into(), scripts: true, agent: true, ..AiProfile::default() }
}

/// `script.run`.
pub(super) fn command() -> CommandSpec {
    CommandSpec::new("script.run", "Run a script MailCraft's AI keeps, or one of your own, to read", WHERE, run).params(
        r#"{"name"?: string (a kept script that's on), "params"?: object (what it takes), "code"?: string (Rhai, as run_script takes: a script of your own, in place of a name)} → {"value", "printed"}: it only reads"#,
    )
}

fn run(s: &mut Session, v: &Value) -> CmdResult {
    s.kept_load();
    let (source, params) = match (p::str(v, "name"), p::str(v, "code")) {
        (Some(name), None) => {
            if s.kept_on(name).is_none() {
                return Err(CmdError::Params(format!("no script named {} is kept and on", name.chars().take(64).collect::<String>())));
            }
            let params = v.get("params").filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({}));
            (format!("{name}(params)"), Some(params))
        }
        (None, Some(code)) => (code.to_string(), None),
        _ => return Err(CmdError::Params("give `name`, a kept script, or `code`, a script of your own".into())),
    };
    let library = kept::library(s.kept_loaded());
    let names = script_tools();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let with = With { library: &library, params: params.as_ref() };
    let mut runner = Runner::with(&source, with, &names, s.clock.now).map_err(CmdError::Failed)?;
    let (reader, mut chat) = (agent(), Chat::default());
    let (mut log, mut ops) = (Vec::new(), 0u64);
    // Each turn answers one more call, or ends: the calls and the operations are bounded.
    loop {
        let (ran, used) = runner.run(&log, OPS_ALL.saturating_sub(ops));
        ops = ops.saturating_add(used);
        match ran {
            Ran::Done { printed, value } => return Ok(json!({"value": value, "printed": printed})),
            Ran::Failed(why) => return Err(CmdError::Failed(why)),
            Ran::Needs(call) => {
                let answer = s.agent_call(&mut chat, &reader, log.len(), &call);
                log.push((call, answer));
            }
        }
    }
}

impl Session {
    /// An agent's script's call: what a read gave, or why it fails.
    fn agent_call(&mut self, chat: &mut Chat, reader: &AiProfile, n: usize, call: &Call) -> Answer {
        let name: String = call.name.chars().take(64).collect();
        if call.name == ASK {
            return Answer::Failed("a script run for an agent has no model of MailCraft's to ask".into());
        }
        if call.name == super::mcp::MCP || call.name == super::mcp::FIND_TOOLS {
            return Answer::Failed("a script run for an agent calls no MCP server: MailCraft's own AI's scripts do, as the person set it up".into());
        }
        if !script_tools().contains(&call.name) {
            return Answer::Failed(format!("a script can't call `{name}`"));
        }
        if !reads(&call.name) {
            return Answer::Failed(format!(
                "`{name}` changes something, and a script run for an agent only reads: MailCraft's AI proposes changes, for the person to approve"
            ));
        }
        let made = AiToolCall { id: format!("agent#{n}"), name: call.name.clone(), input: call.input.clone() };
        match self.tool_call(chat, reader, &made) {
            Outcome::Ran { data, .. } => Answer::Gave(data),
            Outcome::Failed { result, .. } => Answer::Failed(result),
            Outcome::Waiting { .. } => Answer::Failed("the message is being downloaded: run the script again in a moment".into()),
            Outcome::Proposed { .. } | Outcome::Scripting { .. } => Answer::Failed("a script run for an agent only reads".into()),
        }
    }
}
