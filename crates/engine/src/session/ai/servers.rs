//! MCP servers the person adds (File › Options › AI): programs MailCraft starts, and servers on
//! the web, whose tools MailCraft's AI calls from its scripts. Each is off until the person
//! turns it on and, as a model is, reached with an account's mail only as far as that account
//! lets its mail go (a program, or a server on this computer or its network, is local). A
//! server's tools are listed when it's turned on, and kept (`mcp-tools.json`) so a script finds
//! them without starting every server. Adding or changing one, or turning it on, starts a
//! program: the person's to do alone, never an AI agent's (`crate::agents::PERSON_ONLY`).

use std::collections::BTreeMap;

use mailcraft_model::{AiWork, Job, McpDone, McpTarget, McpTool, McpWork, Secret};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::asking::{AiLogEntry, server_of};
use super::commands::{KEY_MOST, address, text};
use super::settings::local_url;
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const WHERE: &str = "File › Options › AI";
/// What the record of what went where says a call to a server's tool was for.
const PURPOSE: &str = "MCP tool";
/// Where the servers' tools are kept, in the data folder.
const TOOLS_DOC: &str = "mcp-tools.json";
/// The most servers kept.
const SERVERS_MOST: usize = 16;
/// The longest name, in characters.
const NAME_MOST: usize = 40;
/// The most a program is given, and the longest each one.
const ARGS_MOST: usize = 32;
const ARG_MOST: usize = 2_000;

/// An MCP server the person added.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct McpServer {
    /// `m1`, `m2`…: its key is kept in the keychain under [`McpServer::key_name`].
    pub id: String,
    /// What scripts call it by: lower-case letters, digits, `_` and `-`.
    pub name: String,
    pub target: McpTarget,
    /// Turned on: its tools listed, and called.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub on: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub has_key: bool,
}

impl McpServer {
    /// On this computer or its network: a program MailCraft starts, or a web address there.
    pub fn local(&self) -> bool {
        match &self.target {
            McpTarget::Program { .. } => true,
            McpTarget::Web { url, .. } => local_url(url),
        }
    }

    /// The name its key is kept under in the keychain (no profile's: those are `p1`, `p2`…).
    pub fn key_name(&self) -> String {
        format!("mcp-{}", self.id)
    }
}

/// What a server's tools are known to be.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ServerTools {
    pub tools: Vec<McpTool>,
    /// Why they couldn't be listed, the last time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Being listed now.
    #[serde(skip)]
    pub listing: bool,
    /// Being signed in to now, in the browser.
    #[serde(skip)]
    pub signing: bool,
}

/// Whether `name` is one a server may be called by.
fn name_ok(name: &str) -> bool {
    name.chars().count() <= NAME_MOST
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// Whether `name` is an environment variable's name a key may go in.
fn env_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

impl Session {
    /// The servers' tools, read from the data folder the first time.
    pub fn mcp_tools(&mut self) -> &BTreeMap<String, ServerTools> {
        self.mcp_tools_mut()
    }

    fn mcp_tools_mut(&mut self) -> &mut BTreeMap<String, ServerTools> {
        if self.ai.mcp_tools.is_none() {
            let read = self.store.as_ref().map(|s| s.load_doc::<BTreeMap<String, ServerTools>>(TOOLS_DOC));
            let tools = match read {
                Some(Ok(t)) => t,
                Some(Err(e)) => {
                    log::warn!("the tools of the MCP servers couldn't be read: {e}");
                    BTreeMap::new()
                }
                None => BTreeMap::new(),
            };
            self.ai.mcp_tools = Some(tools);
        }
        self.ai.mcp_tools.get_or_insert_default()
    }

    /// The tools kept, as they are now, kept in the data folder.
    fn mcp_tools_kept(&mut self) {
        let (Some(store), Some(tools)) = (&self.store, &self.ai.mcp_tools) else { return };
        if let Err(e) = store.save_doc(TOOLS_DOC, tools) {
            log::warn!("the tools of the MCP servers couldn't be kept: {e}");
        }
    }

    /// Server `id`'s tools listed, if it's on.
    fn mcp_list(&mut self, id: &str) {
        let Some(server) = self.prefs.ai.mcp.iter().find(|m| m.id == id && m.on).cloned() else { return };
        self.mcp_tools_mut().entry(server.id.clone()).or_default().listing = true;
        let key = server.has_key.then(|| server.key_name());
        self.jobs.push(Job::Mcp { work: McpWork::List { server: server.id, target: server.target, key } });
    }

    /// Call `server`'s `tool` with `args`, for a chat holding the mail of `accounts`, recorded
    /// as what MailCraft's AI sent (where, and its size, never what): the call's id.
    pub(crate) fn mcp_call(&mut self, server: &McpServer, tool: &str, args: Value, accounts: Vec<String>) -> u64 {
        let id = self.ai.next_id();
        let sent = serde_json::to_vec(&args).map_or(0, |a| a.len());
        let place = match &server.target {
            McpTarget::Program { .. } => "this computer".to_string(),
            McpTarget::Web { url, .. } => server_of(url),
        };
        let line = self.ai_record(AiLogEntry {
            at: self.clock.now,
            purpose: PURPOSE.into(),
            profile: server.name.clone(),
            server: place,
            model: tool.to_string(),
            account: (!accounts.is_empty()).then(|| accounts.join(", ")),
            sent: u64::try_from(sent).unwrap_or(u64::MAX),
            received: 0,
            outcome: "waiting".into(),
        });
        self.ai.mcp_calls.insert(id, line);
        let key = server.has_key.then(|| server.key_name());
        let work = McpWork::Call { id, server: server.id.clone(), target: server.target.clone(), key, tool: tool.to_string(), args };
        self.jobs.push(Job::Mcp { work });
        id
    }

    /// What a server did.
    pub(crate) fn mcp_done(&mut self, done: McpDone) {
        match done {
            McpDone::Listed { server, tools, error } => {
                if !self.prefs.ai.mcp.iter().any(|m| m.id == server) {
                    return;
                }
                let known = self.mcp_tools_mut().entry(server).or_default();
                known.listing = false;
                match error {
                    // What was listed before stays known, said to be old.
                    Some(e) => known.error = Some(e),
                    None => *known = ServerTools { tools, error: None, listing: false, signing: known.signing },
                }
                self.mcp_tools_kept();
            }
            McpDone::Called { id, result, error } => {
                if let Some(line) = self.ai.mcp_calls.remove(&id) {
                    let received = result.as_ref().and_then(|r| serde_json::to_vec(r).ok()).map_or(0, |r| r.len());
                    let outcome = if result.is_some() { "answered" } else { "failed" };
                    self.ai_settle(line, outcome, u64::try_from(received).unwrap_or(u64::MAX));
                }
                let came = result.ok_or_else(|| error.unwrap_or_else(|| "no answer came".into()));
                self.script_mcp_came(id, came);
            }
            McpDone::SignedIn { server, oauth, error } => self.mcp_signed_in(&server, oauth, error),
        }
    }

    /// Signed in to server `id` (how, from now on), or not (why): its tools are listed with
    /// the sign-in when it's on.
    fn mcp_signed_in(&mut self, id: &str, how: Option<mailcraft_model::McpOAuth>, error: Option<String>) {
        let Some(m) = self.prefs.ai.mcp.iter_mut().find(|m| m.id == id) else { return };
        let signed = match (&mut m.target, how) {
            (McpTarget::Web { oauth, .. }, Some(how)) => {
                *oauth = Some(how);
                m.has_key = true;
                true
            }
            _ => false,
        };
        let on = m.on;
        let known = self.mcp_tools_mut().entry(id.to_string()).or_default();
        known.signing = false;
        if error.is_some() {
            known.error = error;
        }
        if signed && on {
            self.mcp_list(id);
        }
    }
}

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("mcpServer.list", "List the MCP servers MailCraft's AI may use, and their tools", WHERE, |s, _| Ok(list(s))),
        CommandSpec::new("mcpServer.save", "Add or change an MCP server", WHERE, save).params(
            r#"{"id"?: string (none: a new one), "name": string (lower-case letters, digits, _ and -: scripts call it by this), "command"?: string (a program MailCraft starts), "args"?: [string] (what the program is given), "keyEnv"?: string (the environment variable its key goes in), "url"?: string (a server on the web instead: https, or http on this computer), "key"?: string (kept in the keychain; "": forgotten), "on"?: bool} (the person's alone: it starts a program)"#,
        ),
        CommandSpec::new("mcpServer.turn", "Turn an MCP server on or off", WHERE, |s, v| {
            let id = server_id(s, v)?;
            let on = p::bool(v, "on").ok_or_else(|| CmdError::Params("`on` (bool) is required".into()))?;
            if let Some(m) = s.prefs.ai.mcp.iter_mut().find(|m| m.id == id) {
                m.on = on;
            }
            if on {
                s.mcp_list(&id);
            } else {
                s.jobs.push(Job::Mcp { work: McpWork::Close { server: id.clone() } });
            }
            Ok(json!({"id": id, "on": on}))
        })
        .params(r#"{"id": string, "on": bool} (the person's alone: it starts a program)"#),
        CommandSpec::new("mcpServer.signIn", "Sign in to an MCP server on the web, in the browser", WHERE, |s, v| {
            let id = server_id(s, v)?;
            let server = s.prefs.ai.mcp.iter().find(|m| m.id == id).cloned();
            let Some((McpTarget::Web { url, .. }, key)) = server.map(|m| (m.target.clone(), m.key_name())) else {
                return Err(CmdError::Failed("only a server on the web is signed in to".into()));
            };
            s.mcp_tools_mut().entry(id.clone()).or_default().signing = true;
            s.jobs.push(Job::Mcp { work: McpWork::SignIn { server: id.clone(), url, key } });
            Ok(json!({"signingIn": id}))
        })
        .params(r#"{"id": string} (the person's alone: they sign in with their own account)"#),
        CommandSpec::new("mcpServer.refresh", "List an MCP server's tools again", WHERE, |s, v| {
            let id = server_id(s, v)?;
            if !s.prefs.ai.mcp.iter().any(|m| m.id == id && m.on) {
                return Err(CmdError::Failed("the server is off: turn it on first".into()));
            }
            s.mcp_list(&id);
            Ok(json!({"listing": id}))
        })
        .params(r#"{"id": string}"#),
        CommandSpec::new("mcpServer.remove", "Remove an MCP server and forget its key", WHERE, |s, v| {
            let id = server_id(s, v)?;
            let key = s.prefs.ai.mcp.iter().find(|m| m.id == id).map(McpServer::key_name).unwrap_or_default();
            s.prefs.ai.mcp.retain(|m| m.id != id);
            s.mcp_tools_mut().remove(&id);
            s.mcp_tools_kept();
            s.ai.key_errors.remove(&key);
            s.jobs.push(Job::Mcp { work: McpWork::Close { server: id.clone() } });
            s.jobs.push(Job::Ai { work: AiWork::SaveKey { name: key, key: None } });
            Ok(json!({"removed": id}))
        })
        .params(r#"{"id": string}"#),
    ]
}

/// The id of a server added, from `v`'s `id`.
fn server_id(s: &Session, v: &Value) -> Result<String, CmdError> {
    let id = p::req_str(v, "id")?;
    s.prefs
        .ai
        .mcp
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.id.clone())
        .ok_or_else(|| CmdError::Params(format!("no MCP server {}", id.chars().take(40).collect::<String>())))
}

/// The servers, each with what's known of its tools.
fn list(s: &mut Session) -> Value {
    let servers = s.prefs.ai.mcp.clone();
    let tools = s.mcp_tools().clone();
    let all: Vec<Value> = servers
        .iter()
        .map(|m| {
            let known = tools.get(&m.id).cloned().unwrap_or_default();
            let names: Vec<&str> = known.tools.iter().map(|t| t.name.as_str()).collect();
            json!({"id": m.id, "name": m.name, "target": m.target, "on": m.on, "hasKey": m.has_key, "local": m.local(), "tools": names, "listing": known.listing, "error": known.error})
        })
        .collect();
    json!({"servers": all})
}

fn save(s: &mut Session, v: &Value) -> CmdResult {
    let name = text(v, "name")?;
    if !name_ok(&name) {
        return Err(CmdError::Params(format!("a server's name is at most {NAME_MOST} lower-case letters, digits, _ and -, starting with a letter")));
    }
    let old = match p::str(v, "id") {
        Some(_) => Some(server_id(s, v)?).and_then(|id| s.prefs.ai.mcp.iter().find(|m| m.id == id).cloned()),
        None if s.prefs.ai.mcp.len() >= SERVERS_MOST => return Err(CmdError::Failed(format!("MailCraft keeps {SERVERS_MOST} MCP servers at most"))),
        None => None,
    };
    let id = old.as_ref().map_or_else(|| new_id(s), |o| o.id.clone());
    if s.prefs.ai.mcp.iter().any(|m| m.name == name && m.id != id) {
        return Err(CmdError::Params(format!("a server named {name} is added already")));
    }
    let mut target = target_of(v)?;
    let key = p::str(v, "key").map(str::trim);
    if key.is_some_and(|k| k.len() > KEY_MOST || k.chars().any(char::is_control)) {
        return Err(CmdError::Params(format!("`key` is longer than {KEY_MOST} bytes, or holds control characters")));
    }
    // A sign-in stays with the address it was for, unless a key is given in its place.
    if let (McpTarget::Web { url, oauth }, Some(McpTarget::Web { url: was, oauth: signed })) = (&mut target, old.as_ref().map(|o| &o.target))
        && *url == *was
        && key.is_none()
    {
        oauth.clone_from(signed);
    }
    // A key stays with where it was given for: another program or address forgets it unless
    // one is given.
    let moved = old.as_ref().is_some_and(|o| o.target != target);
    let has_key = match key {
        Some(k) => !k.is_empty(),
        None => old.as_ref().is_some_and(|o| o.has_key) && !moved,
    };
    let on = p::bool(v, "on").unwrap_or_else(|| old.as_ref().is_some_and(|o| o.on));
    let server = McpServer { id: id.clone(), name, target, on, has_key };
    match key {
        Some(k) => s.jobs.push(Job::Ai { work: AiWork::SaveKey { name: server.key_name(), key: (!k.is_empty()).then(|| Secret(k.to_string())) } }),
        None if moved => s.jobs.push(Job::Ai { work: AiWork::SaveKey { name: server.key_name(), key: None } }),
        None => {}
    }
    let local = server.local();
    match s.prefs.ai.mcp.iter_mut().find(|m| m.id == id) {
        Some(m) => *m = server,
        None => s.prefs.ai.mcp.push(server),
    }
    // Connected to again with what changed, and its tools listed again.
    s.jobs.push(Job::Mcp { work: McpWork::Close { server: id.clone() } });
    s.mcp_list(&id);
    Ok(json!({"id": id, "local": local}))
}

/// Where the server is: a program to start (`command`, `args`, `keyEnv`), or a web address
/// (`url`), not both.
fn target_of(v: &Value) -> Result<McpTarget, CmdError> {
    let given = |k: &str| p::str(v, k).is_some_and(|t| !t.trim().is_empty());
    match (given("command"), given("url")) {
        (true, false) => {
            let command = text(v, "command")?;
            let args = match v.get("args") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(a)) if a.len() <= ARGS_MOST => a
                    .iter()
                    .map(|x| x.as_str().filter(|t| t.chars().count() <= ARG_MOST && !t.chars().any(char::is_control)).map(str::to_string))
                    .collect::<Option<Vec<String>>>()
                    .ok_or_else(|| CmdError::Params(format!("each of `args` is text of at most {ARG_MOST} characters")))?,
                Some(_) => return Err(CmdError::Params(format!("`args` is a list of at most {ARGS_MOST} texts"))),
            };
            let key_env = match p::str(v, "keyEnv").map(str::trim).filter(|e| !e.is_empty()) {
                Some(e) if env_ok(e) => Some(e.to_string()),
                Some(_) => return Err(CmdError::Params("`keyEnv` is an environment variable's name: capitals, digits and _".into())),
                None => None,
            };
            Ok(McpTarget::Program { command, args, key_env })
        }
        (false, true) => Ok(McpTarget::Web { url: address(v)?, oauth: None }),
        _ => Err(CmdError::Params("give a program to start (`command`) or a web address (`url`), one of them".into())),
    }
}

/// The first `m<n>` no server has.
fn new_id(s: &Session) -> String {
    let taken = |id: &str| s.prefs.ai.mcp.iter().any(|m| m.id == id);
    (1..).map(|n: u32| format!("m{n}")).find(|id| !taken(id)).unwrap_or_default()
}
