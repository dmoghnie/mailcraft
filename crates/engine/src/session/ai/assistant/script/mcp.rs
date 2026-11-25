//! A script's calls to the MCP servers the person added (File › Options › AI): `find_tools`
//! finds their tools in what they listed, and `mcp(server, tool, args)` calls one, the script
//! waiting for its answer as for a message to download. A script calls only a server that's
//! on and that every account its model reads lets its mail go to (one on the web, only where
//! an account's mail may go anywhere). A tool the server says changes nothing is called at
//! once; one that may change something is proposed, as a script's other changes are, and called
//! once the person approves it, its card waiting for the server's answer. Each call is recorded
//! with what MailCraft's AI sent: its server, tool and sizes, never its content.

use super::*;
use crate::session::ai::McpServer;

/// The functions' names, as a script calls them.
pub(super) const FIND_TOOLS: &str = "find_tools";
pub(in crate::session::ai::assistant) const MCP: &str = "mcp";
/// The most calls a script makes to MCP servers: each takes a server's time.
const MCP_CALLS_MOST: usize = 20;
/// The most tools `find_tools` gives.
const FOUND_MOST: usize = 10;
/// The most a call gives a tool, in bytes of JSON.
const ARGS_MOST: usize = 64 * 1024;
/// The most of a proposed call's args its card shows, in characters.
const ARGS_SHOWN: usize = 400;

/// What the model is told of the servers' tools, when its scripts may call some.
pub(super) fn told(servers: &[String]) -> String {
    if servers.is_empty() {
        return String::new();
    }
    format!(
        " Tools of MCP servers the person added ({}) are found with find_tools(query), or find_tools() for all: each with its server, tool, about, args (a JSON Schema of the map it takes) and changes (whether it may change something). mcp(server, tool, #{{...}}) calls one that changes nothing and gives what it answered: data, or its text. One that may change something is proposed, as changes are: it gives #{{proposed: what}}, and is called once the person approves it after the script ends. At most {MCP_CALLS_MOST} calls to MCP servers.",
        servers.join(", ")
    )
}

/// A value an MCP server gave, as text: its text, or its JSON.
fn text_of(value: &Value) -> String {
    value.as_str().map_or_else(|| value.to_string(), str::to_string)
}

impl Session {
    /// The names of the MCP servers `profile`'s scripts may call, for the model.
    pub(in crate::session::ai::assistant) fn mcp_names(&self, profile: &AiProfile) -> Vec<String> {
        if !profile.scripts {
            return Vec::new();
        }
        self.mcp_usable(profile).iter().map(|m| m.name.clone()).collect()
    }

    /// What call `id` to an MCP server gave, or why it failed: the script that made it goes
    /// on, or the change approved is done.
    pub(in crate::session::ai) fn script_mcp_came(&mut self, id: u64, came: Result<Value, String>) {
        if let Some(at) = self.ai.assistant.script_mcps.remove(&id) {
            self.script_resume(&at, Came::Mcp(came));
        } else if let Some((chat, call)) = self.ai.assistant.mcp_approved.remove(&id) {
            self.approved_mcp_came(&chat, &call, came);
        }
    }

    /// The servers `profile`'s scripts may call: on, and let have the mail of every account
    /// the profile reads.
    fn mcp_usable(&self, profile: &AiProfile) -> Vec<&McpServer> {
        let ai = &self.prefs.ai;
        let read: Vec<&str> = self.accounts.iter().map(|a| a.config.id.as_str()).filter(|a| ai.allows(profile, Some(a))).collect();
        let lets = |m: &McpServer| if read.is_empty() { ai.allows_server(m, None) } else { read.iter().all(|a| ai.allows_server(m, Some(a))) };
        ai.mcp.iter().filter(|m| m.on && lets(m)).collect()
    }

    /// `find_tools(query)`: the tools of the servers `profile`'s scripts may call that have
    /// the query's words in their name or description, those with most first; every one for
    /// no query.
    pub(super) fn script_find_tools(&mut self, profile: &AiProfile, input: &Value) -> Outcome {
        let query = input.get("query").and_then(Value::as_str).unwrap_or_default().trim().to_lowercase();
        let what =
            if query.is_empty() { "Find MCP tools".to_string() } else { format!("Find MCP tools: {}", query.chars().take(80).collect::<String>()) };
        let usable: Vec<(String, String)> = self.mcp_usable(profile).iter().map(|m| (m.id.clone(), m.name.clone())).collect();
        if usable.is_empty() {
            return Outcome::failed(
                &what,
                "no MCP server is on for this chat's mail: the person adds them, and says where mail may go, in File › Options › AI".into(),
            );
        }
        let words: Vec<&str> = query.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
        let known = self.mcp_tools();
        let mut found: Vec<(usize, Value)> = Vec::new();
        for (id, server) in &usable {
            for t in known.get(id).map(|k| k.tools.as_slice()).unwrap_or_default() {
                let said = format!("{} {} {}", t.name, t.title, t.about).to_lowercase();
                let hits = words.iter().filter(|w| said.contains(**w)).count();
                if words.is_empty() || hits > 0 {
                    let tool =
                        json!({"server": server, "tool": t.name, "title": t.title, "about": t.about, "args": t.schema, "changes": !t.read_only});
                    found.push((hits, tool));
                }
            }
        }
        found.sort_by_key(|(hits, _)| std::cmp::Reverse(*hits));
        let tools: Vec<Value> = found.into_iter().take(FOUND_MOST).map(|(_, t)| t).collect();
        let names: Vec<String> =
            tools.iter().map(|t| format!("{}: {}", t["server"].as_str().unwrap_or_default(), t["tool"].as_str().unwrap_or_default())).collect();
        let result = if names.is_empty() { "No tool found".to_string() } else { format!("Found {}", names.join(", ")) };
        Outcome::Ran { what, messages: Vec::new(), found: Vec::new(), result, data: Value::Array(tools) }
    }

    /// `mcp(server, tool, args)` from `script`, as call `made`: a tool that changes nothing
    /// called, the script waiting for what it gives; one that may change something proposed,
    /// to be called once approved; or why it can't be. Its run shows under the script's card.
    pub(super) fn script_mcp(&mut self, chat: &mut Chat, profile: &AiProfile, script: &Script, made: AiToolCall) -> Result<Answer, Wait> {
        let parent = script.call.id.clone();
        let server = made.input.get("server").and_then(Value::as_str).unwrap_or_default().to_string();
        let tool = made.input.get("tool").and_then(Value::as_str).unwrap_or_default().to_string();
        let mut what = format!("Call {} on {}", tool.chars().take(64).collect::<String>(), server.chars().take(40).collect::<String>());
        let mine = |r: &&ToolRun| r.part_of.as_deref() == Some(parent.as_str());
        let called = chat.runs.iter().filter(mine).filter(|r| r.tool == MCP).count();
        let proposed = chat.runs.iter().filter(mine).filter(|r| !r.input.is_null()).count();
        let args = made.input.get("args").cloned().unwrap_or_else(|| json!({}));
        let (state, result, input, wait) = match self.mcp_refused(profile, called, &server, &tool, &args) {
            Err(why) => (RunState::Failed, why, Value::Null, None),
            Ok((_, false)) if proposed >= PROPOSALS_MOST => {
                (RunState::Failed, format!("a script proposes at most {PROPOSALS_MOST} changes: propose the rest after it"), Value::Null, None)
            }
            Ok((_, false)) => {
                what = format!("{what}: {}", cut(&args.to_string(), ARGS_SHOWN));
                (RunState::Proposed, String::new(), json!({"server": server, "tool": tool, "args": args}), None)
            }
            Ok((m, true)) => {
                let id = self.mcp_call(&m, &tool, args, chat.accounts.clone());
                self.ai.assistant.script_mcps.insert(id, (chat.id.clone(), parent.clone()));
                (RunState::Waiting, String::new(), Value::Null, Some(id))
            }
        };
        let answer = match (&state, wait) {
            (_, Some(id)) => Err(Wait::Mcp(id)),
            (RunState::Proposed, None) => Ok(Answer::Gave(json!({"proposed": what}))),
            _ => Ok(Answer::Failed(result.clone())),
        };
        let run =
            ToolRun { call: made.id, tool: MCP.into(), state, what, messages: Vec::new(), found: Vec::new(), result, part_of: Some(parent), input };
        match chat.run_mut(&run.call) {
            Some(r) => *r = run,
            None => chat.runs.push(run),
        }
        answer
    }

    /// The server a script's `called`th call to `server`'s `tool` goes to, and whether the
    /// tool changes nothing; or why it can't.
    fn mcp_refused(&mut self, profile: &AiProfile, called: usize, server: &str, tool: &str, args: &Value) -> Result<(McpServer, bool), String> {
        if called >= MCP_CALLS_MOST {
            return Err(format!("a script calls MCP servers at most {MCP_CALLS_MOST} times"));
        }
        if !args.is_object() || serde_json::to_vec(args).map_or(true, |a| a.len() > ARGS_MOST) {
            return Err(format!("mcp(server, tool, args) takes a map of at most {ARGS_MOST} bytes"));
        }
        self.mcp_tool(profile, server, tool)
    }

    /// Server `server`, if `profile`'s scripts may call it, and whether its `tool` changes
    /// nothing; or why it can't be called.
    fn mcp_tool(&mut self, profile: &AiProfile, server: &str, tool: &str) -> Result<(McpServer, bool), String> {
        let usable = self.mcp_usable(profile).into_iter().find(|m| m.name == server).cloned();
        let Some(m) = usable else {
            let added = self.prefs.ai.mcp.iter().find(|m| m.name == server);
            return Err(match added {
                Some(m) if !m.on => format!("the MCP server {server} is off: the person turns it on in File › Options › AI"),
                Some(_) => format!("this chat's mail doesn't go to {server}: File › Options › AI says where each account's mail may go"),
                None => format!(
                    "no MCP server is named {}: find_tools(query) finds tools, and their servers",
                    server.chars().take(40).collect::<String>()
                ),
            });
        };
        let listed = self.mcp_tools().get(&m.id).and_then(|k| k.tools.iter().find(|t| t.name == tool)).map(|t| t.read_only);
        match listed {
            None => Err(format!("{server} has no tool {}: find_tools(query) finds them", tool.chars().take(64).collect::<String>())),
            Some(read_only) => Ok((m, read_only)),
        }
    }

    /// The person approved `call`, a script's call to an MCP server's tool that may change
    /// something: it's made, if the server may still be called, and waits for its answer.
    pub(in crate::session::ai::assistant) fn mcp_approve(&mut self, chat: &Chat, call: &AiToolCall) -> (RunState, String) {
        let profile = match self.assistant_profile(chat) {
            Ok(p) => p,
            Err(e) => return (RunState::Failed, said(&e)),
        };
        let named = |k: &str| call.input.get(k).and_then(Value::as_str).unwrap_or_default();
        let (server, tool) = (named("server"), named("tool"));
        match self.mcp_tool(&profile, server, tool) {
            Err(why) => (RunState::Failed, why),
            Ok((m, _)) => {
                let args = call.input.get("args").cloned().unwrap_or_else(|| json!({}));
                let id = self.mcp_call(&m, tool, args, chat.accounts.clone());
                self.ai.assistant.mcp_approved.insert(id, (chat.id.clone(), call.id.clone()));
                (RunState::Waiting, String::new())
            }
        }
    }

    /// What the server answered `call` of chat `id`, a change the person approved: done, or
    /// why not; and the chat goes on once nothing else waits.
    fn approved_mcp_came(&mut self, id: &str, call: &str, came: Result<Value, String>) {
        let Ok(mut chat) = self.take_chat(id) else { return };
        if let Some(r) = chat.run_mut(call).filter(|r| r.state == RunState::Waiting) {
            (r.state, r.result) = match came {
                Ok(value) => (RunState::Done, cut(text_of(&value).trim(), VALUE_MOST)),
                Err(e) => (RunState::Failed, format!("the MCP server couldn't do it: {e}")),
            };
        }
        self.put_chat(chat);
        self.assistant_go_on(id);
    }
}
