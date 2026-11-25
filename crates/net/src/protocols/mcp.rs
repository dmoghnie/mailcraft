//! MailCraft as a client of the Model Context Protocol: the servers the person adds (File ›
//! Options › AI), whose tools MailCraft's AI calls from its scripts. A server is a program
//! MailCraft starts, spoken to over its standard input and output (a message a line), or a
//! server on the web, spoken to by Streamable HTTP (each message posted, its answer JSON or
//! server-sent events). JSON-RPC 2.0, as the engine's `mcp` module answers agents:
//! `initialize`, then `tools/list` and `tools/call`. What a server asks of MailCraft is
//! answered (`ping`) or refused; its notifications noted (`notifications/tools/list_changed`)
//! or let go. Its answers are hostile input as every server's: messages, tools and their texts
//! capped, names checked.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, channel, sync_channel};
use std::time::{Duration, Instant};

use mailcraft_model::{McpTarget, McpTool};
use serde_json::{Value, json};

use crate::http::{self, Request, Url};
use crate::{NetError, Result};

#[cfg(test)]
mod tests;

/// The protocol versions MailCraft speaks, newest first: a server's choice among them is kept.
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
/// The longest message read, in bytes.
const MESSAGE_MOST: usize = 4 << 20;
/// The most tools kept of a server: each is in the index scripts search.
pub const TOOLS_MOST: usize = 200;
/// The most pages of tools asked for.
const PAGES_MOST: usize = 20;
/// The longest tool name (the protocol's own limit) and title, in characters.
const NAME_MOST: usize = 128;
/// The longest description kept, in characters.
const ABOUT_MOST: usize = 2_000;
/// The largest input schema kept, in bytes: a larger one is kept as any object.
const SCHEMA_MOST: usize = 16 << 10;
/// The most of a call's answer kept, in characters (or bytes of its structured content).
pub const RESULT_MOST: usize = 64 << 10;
/// The longest error a server's words make, in characters.
const SAID_MOST: usize = 500;
/// How long a server may take to start and agree: a program fetched first, as `npx` does.
const STARTING: Duration = Duration::from_secs(60);
/// How long a call may take.
const CALLING: Duration = Duration::from_secs(120);
/// Windows' flag for a program started with no console window of its own.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A connection to one server, agreed.
pub struct McpConn {
    way: Way,
    /// The last request's id.
    next: u64,
    /// The server said its tools changed since they were last listed.
    pub tools_changed: bool,
}

enum Way {
    Program(Program),
    Web(Web),
}

/// A program MailCraft started: what it's sent and what it wrote, a line at a time, each on a
/// thread of its own, so a wait for a program that stopped reading or writing ends.
struct Program {
    child: Child,
    input: Sender<Vec<u8>>,
    lines: Receiver<std::result::Result<String, String>>,
}

/// A server on the web, and what it said to keep: its session, the version agreed.
struct Web {
    url: String,
    token: Option<String>,
    session: Option<String>,
    version: Option<String>,
}

impl Drop for Program {
    fn drop(&mut self) {
        // The program ends with its connection: never left running.
        if let Err(e) = self.child.kill() {
            log::debug!("an MCP server's program had ended already: {e}");
        }
        if let Err(e) = self.child.wait() {
            log::warn!("an MCP server's program couldn't be waited for: {e}");
        }
    }
}

impl McpConn {
    /// Start or reach `target` and agree on the protocol. `key`: the value of the program's
    /// environment variable, or the web server's token.
    pub fn connect(target: &McpTarget, key: Option<&str>) -> Result<McpConn> {
        let way = match target {
            McpTarget::Program { command, args, key_env } => {
                let env = key_env.as_deref().filter(|e| !e.is_empty()).zip(key);
                Way::Program(Program::start(command, args, env)?)
            }
            McpTarget::Web { url } => Way::Web(Web { url: address(url)?, token: key.map(str::to_string), session: None, version: None }),
        };
        let mut c = McpConn { way, next: 0, tools_changed: false };
        let hello = json!({
            "protocolVersion": VERSIONS[0],
            "capabilities": {},
            "clientInfo": {"name": "MailCraft", "version": env!("CARGO_PKG_VERSION")},
        });
        let agreed = c.request("initialize", hello, STARTING)?;
        let version = agreed
            .get("protocolVersion")
            .and_then(Value::as_str)
            .filter(|v| VERSIONS.contains(v))
            .ok_or_else(|| NetError::Protocol("the server speaks no version of MCP MailCraft does".into()))?;
        if let Way::Web(w) = &mut c.way {
            w.version = Some(version.to_string());
        }
        c.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}), STARTING)?;
        Ok(c)
    }

    /// The server's tools, every page of them up to [`TOOLS_MOST`]; one whose name or schema
    /// can't be used is left out.
    pub fn tools(&mut self) -> Result<Vec<McpTool>> {
        let mut tools: Vec<McpTool> = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..PAGES_MOST {
            let params = cursor.as_ref().map_or_else(|| json!({}), |c| json!({"cursor": c}));
            let page = self.request("tools/list", params, CALLING)?;
            for t in tools_of(&page) {
                if !tools.iter().any(|k| k.name == t.name) {
                    tools.push(t);
                }
            }
            cursor = page.get("nextCursor").and_then(Value::as_str).filter(|c| !c.is_empty() && c.len() <= 1024).map(str::to_string);
            if cursor.is_none() || tools.len() >= TOOLS_MOST {
                break;
            }
        }
        tools.truncate(TOOLS_MOST);
        self.tools_changed = false;
        Ok(tools)
    }

    /// Call `tool` with `args`: its structured answer when it gives one that fits, else its
    /// text; an error when the tool said it failed.
    pub fn call(&mut self, tool: &str, args: &Value) -> Result<Value> {
        let args = if args.is_object() { args.clone() } else { json!({}) };
        let r = self.request("tools/call", json!({"name": tool, "arguments": args}), CALLING)?;
        result_of(&r)
    }

    /// Ask `method` and wait up to `wait` for its answer, answering what the server asks
    /// meanwhile.
    fn request(&mut self, method: &str, params: Value, wait: Duration) -> Result<Value> {
        self.next = self.next.saturating_add(1);
        let id = self.next;
        let came = self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}), wait)?;
        let until = Instant::now() + wait;
        let mut came = came.into_iter();
        loop {
            let m = match (came.next(), &mut self.way) {
                (Some(m), _) => m,
                (None, Way::Program(p)) => p.read(until)?,
                (None, Way::Web(_)) => return Err(NetError::Protocol(format!("the server didn't answer {method}"))),
            };
            if let Some(answer) = self.heard(m, id, wait)? {
                return answer;
            }
        }
    }

    /// Send `m`: what came back with it (a web server's answer), else nothing yet.
    fn send(&mut self, m: &Value, wait: Duration) -> Result<Vec<Value>> {
        match &mut self.way {
            Way::Program(p) => p.write(m).map(|()| Vec::new()),
            Way::Web(w) => w.post(m, wait),
        }
    }

    /// A message the server sent: the answer to request `id` (its result, or its error), or
    /// none (a request of the server's answered, a notification noted).
    fn heard(&mut self, m: Value, id: u64, wait: Duration) -> Result<Option<Result<Value>>> {
        let method = m.get("method").and_then(Value::as_str);
        match (method, m.get("id")) {
            (Some(method), Some(asked)) => {
                let answer = if method == "ping" {
                    json!({"jsonrpc": "2.0", "id": asked, "result": {}})
                } else {
                    json!({"jsonrpc": "2.0", "id": asked, "error": {"code": -32601, "message": "MailCraft doesn't answer that"}})
                };
                self.send(&answer, wait)?;
                Ok(None)
            }
            (Some(method), None) => {
                self.tools_changed |= method == "notifications/tools/list_changed";
                Ok(None)
            }
            (None, Some(v)) if v.as_u64() == Some(id) => Ok(Some(match m.get("error") {
                Some(e) => {
                    let said = e.get("message").and_then(Value::as_str).unwrap_or("no reason given");
                    Err(NetError::No(cut(said, SAID_MOST)))
                }
                None => Ok(m.get("result").cloned().unwrap_or(Value::Null)),
            })),
            _ => Ok(None),
        }
    }
}

impl Program {
    /// Start `command` with `args`, and `env` (a variable and its value) set.
    fn start(command: &str, args: &[String], env: Option<(&str, &str)>) -> Result<Program> {
        let mut cmd = Command::new(command.trim());
        cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        if let Some((name, value)) = env {
            cmd.env(name, value);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| NetError::Protocol(format!("the program couldn't be started: {e}")))?;
        let (input, output) = match (child.stdin.take(), child.stdout.take()) {
            (Some(i), Some(o)) => (i, o),
            _ => {
                if let Err(e) = child.kill() {
                    log::debug!("an MCP server's program had ended already: {e}");
                }
                return Err(NetError::Protocol("the program's input and output couldn't be had".into()));
            }
        };
        let (tx, lines) = sync_channel(16);
        let (to_write, written) = channel();
        let reading = std::thread::Builder::new().name("mail-mcp".into()).spawn(move || read_lines(output, &tx));
        let writing = std::thread::Builder::new().name("mail-mcp".into()).spawn(move || write_lines(input, &written));
        // Made first, so a thread that couldn't start ends the program as it's dropped.
        let program = Program { child, input: to_write, lines };
        reading.map_err(NetError::Io)?;
        writing.map_err(NetError::Io)?;
        Ok(program)
    }

    fn write(&mut self, m: &Value) -> Result<()> {
        let mut line = m.to_string().into_bytes();
        line.push(b'\n');
        self.input.send(line).map_err(|_| NetError::Protocol("the program ended".into()))
    }

    /// The program's next message, waited for until `until`.
    fn read(&mut self, until: Instant) -> Result<Value> {
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(Ok(line)) => match serde_json::from_str::<Value>(&line) {
                    Ok(v) if v.is_object() => return Ok(v),
                    // Not a message (a program's own output on the wrong stream): let go.
                    _ => log::debug!("an MCP server wrote a line that isn't a message"),
                },
                Ok(Err(why)) => return Err(NetError::Protocol(why)),
                Err(RecvTimeoutError::Timeout) => return Err(NetError::Protocol("the program didn't answer in time".into())),
                Err(RecvTimeoutError::Disconnected) => return Err(NetError::Protocol("the program ended".into())),
            }
        }
    }
}

/// What's sent to a program, a line at a time, until it stops reading or the connection ends.
fn write_lines(mut input: ChildStdin, lines: &Receiver<Vec<u8>>) {
    for line in lines.iter() {
        if let Err(e) = input.write_all(&line).and_then(|()| input.flush()) {
            log::debug!("an MCP server's program stopped reading: {e}");
            return;
        }
    }
}

/// What a program writes, a line at a time, each at most [`MESSAGE_MOST`] bytes, sent on until
/// it ends or no one reads.
fn read_lines(out: impl Read, tx: &SyncSender<std::result::Result<String, String>>) {
    let mut r = BufReader::new(out);
    loop {
        let mut line = Vec::new();
        let read = (&mut r).take(MESSAGE_MOST as u64 + 1).read_until(b'\n', &mut line);
        let sent = match read {
            Ok(0) => tx.send(Err("the program ended".into())).map(|()| false),
            Ok(_) if line.len() > MESSAGE_MOST => tx.send(Err("the program wrote a message longer than MailCraft reads".into())).map(|()| false),
            Ok(_) => tx.send(Ok(String::from_utf8_lossy(&line).trim().to_string())).map(|()| true),
            Err(e) => tx.send(Err(format!("the program's output couldn't be read: {e}"))).map(|()| false),
        };
        if !matches!(sent, Ok(true)) {
            return;
        }
    }
}

impl Web {
    /// Post `m`: the messages that came back (none for a notification or an answer).
    fn post(&mut self, m: &Value, wait: Duration) -> Result<Vec<Value>> {
        let body = m.to_string().into_bytes();
        let mut req = Request::post(&self.url, "application/json", body, MESSAGE_MOST).header("Accept", "application/json, text/event-stream");
        if let Some(v) = &self.version {
            req = req.header("MCP-Protocol-Version", v);
        }
        if let Some(s) = &self.session {
            req = req.header("Mcp-Session-Id", s);
        }
        if let Some(t) = &self.token {
            req = req.header("Authorization", &format!("Bearer {t}"));
        }
        let resp = http::send_waiting(&req, wait)?;
        if resp.status == 401 || resp.status == 403 {
            return Err(NetError::Auth("the server wants a key or token MailCraft wasn't given, or it was refused".into()));
        }
        if !resp.ok() {
            return Err(NetError::Protocol(format!("the server answered {}", resp.status)));
        }
        if let Some(s) = resp.header("Mcp-Session-Id").filter(|s| s.len() <= 256 && s.chars().all(|c| c.is_ascii_graphic())) {
            self.session = Some(s.to_string());
        }
        let text = String::from_utf8_lossy(&resp.body);
        let events = resp.header("Content-Type").is_some_and(|t| t.trim().to_ascii_lowercase().starts_with("text/event-stream"));
        Ok(if events { from_events(&text) } else { from_json(&text) })
    }
}

/// An MCP server's web address: https, or plain http to this computer alone (a token, and what
/// MailCraft's AI sends, could be read on the way to another).
fn address(url: &str) -> Result<String> {
    let parsed = Url::parse(url.trim())?;
    if !parsed.https && !http::is_loopback(&parsed.host) {
        return Err(NetError::Protocol("an MCP server is reached over https, unless it's on this computer".into()));
    }
    Ok(url.trim().to_string())
}

/// The messages in a JSON answer: one, or a batch.
fn from_json(text: &str) -> Vec<Value> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Array(all)) => all.into_iter().filter(Value::is_object).collect(),
        Ok(v) if v.is_object() => vec![v],
        _ => Vec::new(),
    }
}

/// The messages in server-sent events: each event's data lines, joined, one message.
fn from_events(text: &str) -> Vec<Value> {
    text.replace('\r', "")
        .split("\n\n")
        .filter_map(|event| {
            let data: Vec<&str> = event.lines().filter_map(|l| l.strip_prefix("data:")).map(|d| d.strip_prefix(' ').unwrap_or(d)).collect();
            serde_json::from_str::<Value>(&data.join("\n")).ok().filter(Value::is_object)
        })
        .collect()
}

/// The tools a page of `tools/list` holds that can be used.
fn tools_of(page: &Value) -> Vec<McpTool> {
    page.get("tools").and_then(Value::as_array).into_iter().flatten().filter_map(tool_of).collect()
}

/// What a call gave: its structured answer when it gives one that fits, else its text; an
/// error when the tool said it failed.
fn result_of(r: &Value) -> Result<Value> {
    let text = text_of(r);
    if r.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(NetError::No(cut(&text, SAID_MOST)));
    }
    Ok(match r.get("structuredContent") {
        Some(v) if v.is_object() && v.to_string().len() <= RESULT_MOST => v.clone(),
        _ => Value::String(text),
    })
}

/// The fuzzer's: what a web server sends, read as MailCraft reads it, as JSON and as events,
/// each message's tools and a call's answer in it: how many tools could be used.
#[doc(hidden)]
pub fn read_as_sent(text: &str) -> usize {
    let mut tools = 0usize;
    for m in from_json(text).into_iter().chain(from_events(text)) {
        if let Some(r) = m.get("result") {
            tools = tools.saturating_add(tools_of(r).len());
            if let Err(e) = result_of(r) {
                log::debug!("{e}");
            }
        }
    }
    tools
}

/// A tool as a server listed it, checked: a name of the protocol's letters, a schema that's an
/// object (kept as any object when too large).
fn tool_of(t: &Value) -> Option<McpTool> {
    let name = t.get("name")?.as_str()?;
    let fits = (1..=NAME_MOST).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if !fits {
        return None;
    }
    let text = |k: &str, most: usize| t.get(k).and_then(Value::as_str).map(|s| cut(s, most)).unwrap_or_default();
    let schema =
        t.get("inputSchema").filter(|s| s.is_object() && s.to_string().len() <= SCHEMA_MOST).cloned().unwrap_or_else(|| json!({"type": "object"}));
    let read_only = t.pointer("/annotations/readOnlyHint").and_then(Value::as_bool).unwrap_or(false);
    Some(McpTool { name: name.to_string(), title: text("title", NAME_MOST), about: text("description", ABOUT_MOST), schema, read_only })
}

/// A call's answer as text: its text parts, and what else it held named.
fn text_of(r: &Value) -> String {
    let parts: Vec<String> = r
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|c| match c.get("type").and_then(Value::as_str) {
            Some("text") => c.get("text").and_then(Value::as_str).unwrap_or_default().to_string(),
            Some("image") => "[a picture]".into(),
            Some("audio") => "[a recording]".into(),
            Some("resource") => c.pointer("/resource/text").and_then(Value::as_str).unwrap_or("[a file]").to_string(),
            Some("resource_link") => format!("[a link: {}]", cut(c.get("uri").and_then(Value::as_str).unwrap_or_default(), 500)),
            _ => String::new(),
        })
        .collect();
    cut(&parts.join("\n"), RESULT_MOST)
}

/// `s` cut to `most` characters, with `…` when it was longer.
fn cut(s: &str, most: usize) -> String {
    match s.char_indices().nth(most) {
        Some((at, _)) => format!("{}…", s.get(..at).unwrap_or_default()),
        None => s.to_string(),
    }
}
