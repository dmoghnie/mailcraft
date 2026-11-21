//! AI agents through the socket (MCP, `mailcraft mcp`): the stdio bridge an agent's client
//! starts, and the running MailCraft's end, which hands each message to the window to answer
//! from its session (`mailcraft_engine::mcp`, which answers nothing while agents aren't let in).
//!
//! The bridge reaches the window as a later launch does: the socket in the user's private
//! folder, and the key beside it. Its first line is `{"key": …, "mcp": true}`; once it's
//! answered `ok`, the connection is the agent's, one JSON-RPC message a line each way (MCP's
//! stdio transport), on a thread of its own, so launches are still answered meanwhile. Each
//! message waits for the window's answer, a [`ANSWER_WAIT`] at most.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;

use super::*;

/// Longest message read (a command's params, an attachment's bytes among them, fit).
const MAX_MESSAGE: u64 = 16 << 20;
/// How long a message waits for the window: one minimized still answers each frame it's woken
/// for, so only a window that stopped answering takes this long.
const ANSWER_WAIT: Duration = Duration::from_secs(60);
/// Most agents connected at once, and messages waiting for the window.
const MAX_AGENTS: usize = 4;
const MAX_ASKS: usize = 64;

/// Why an agent's client gets nothing but errors from the bridge.
const NOT_RUNNING: &str = "MailCraft isn't running on this data folder: open it, then connect again";
const NOT_LET_IN: &str = "the running MailCraft didn't take the connection (one from before agents could connect?)";
const NO_ANSWER: &str = "MailCraft's window didn't answer in time";

/// One message from an agent, for the window to answer.
pub struct AgentAsk {
    message: String,
    reply: mpsc::Sender<Option<String>>,
}

impl AgentAsk {
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Send the answer back (`None`: a notification, which has none).
    pub fn answer(self, reply: Option<String>) {
        let _ = self.reply.send(reply);
    }
}

/// The agents' part of the running MailCraft's asks.
#[derive(Default)]
pub(super) struct AgentAsks {
    waiting: Mutex<VecDeque<AgentAsk>>,
    connected: AtomicUsize,
}

impl Server {
    /// The agents' messages waiting since the last call, in order.
    pub fn agent_asks(&self) -> Vec<AgentAsk> {
        self.asks.agents.waiting.lock().unwrap_or_else(PoisonError::into_inner).drain(..).collect()
    }
}

/// The first line of an agent's connection, with `key`.
pub(super) fn request(key: &str) -> String {
    let mut line = serde_json::json!({"key": key, "mcp": true}).to_string();
    line.push('\n');
    line
}

/// Whether `line` is an agent's first line with `key`.
pub(super) fn is_request(line: &str, key: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim_end()) else { return false };
    let sent = value.get("key").and_then(serde_json::Value::as_str);
    value.get("mcp") == Some(&serde_json::Value::Bool(true)) && !key.is_empty() && sent.is_some_and(|s| keys_match(s.as_bytes(), key.as_bytes()))
}

/// Take an agent's connection (its first line read): `ok`, then its messages on a thread of its
/// own. One past [`MAX_AGENTS`] is closed unanswered.
pub(super) fn take(stream: &UnixStream, asks: &Arc<Asks>) -> std::io::Result<()> {
    if asks.agents.connected.fetch_add(1, Ordering::AcqRel) >= MAX_AGENTS {
        asks.agents.connected.fetch_sub(1, Ordering::AcqRel);
        return Err(std::io::Error::other("too many agents connected"));
    }
    let leave = |asks: &Asks| asks.agents.connected.fetch_sub(1, Ordering::AcqRel);
    let started = stream.try_clone().and_then(|own| {
        let mut writer = &own;
        writer.write_all(OK.as_bytes())?;
        let asks = Arc::clone(asks);
        std::thread::Builder::new().name("mcp-agent".into()).spawn(move || {
            if let Err(e) = converse(&own, &asks) {
                log::info!("an agent's connection ended ({e})");
            }
            leave(&asks);
        })
    });
    if started.is_err() {
        leave(asks);
    }
    started.map(drop)
}

/// An agent's messages, answered in turn until it goes.
fn converse(stream: &UnixStream, asks: &Asks) -> std::io::Result<()> {
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let mut reader = BufReader::new(stream);
    loop {
        let Some(message) = read_message(&mut reader)? else { return Ok(()) };
        if message.trim().is_empty() {
            continue;
        }
        let reply = ask_window(asks, message.trim().to_string());
        if let Some(reply) = reply.unwrap_or_else(|why| refusal(&message, why)) {
            let mut writer = stream;
            writer.write_all(reply.as_bytes())?;
            writer.write_all(b"\n")?;
        }
    }
}

/// One line, at most [`MAX_MESSAGE`]: `None` at the end. A longer one ends the connection.
fn read_message(reader: &mut impl BufRead) -> std::io::Result<Option<String>> {
    let mut line = String::new();
    let n = reader.take(MAX_MESSAGE).read_line(&mut line)?;
    if n == 0 {
        return Ok(None);
    }
    if !line.ends_with('\n') && n as u64 >= MAX_MESSAGE {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "a message too long"));
    }
    Ok(Some(line))
}

/// Hand `message` to the window and wait for its answer.
fn ask_window(asks: &Asks, message: String) -> Result<Option<String>, &'static str> {
    let (reply, answered) = mpsc::channel();
    {
        let mut waiting = asks.agents.waiting.lock().unwrap_or_else(PoisonError::into_inner);
        if waiting.len() >= MAX_ASKS {
            return Err(NO_ANSWER);
        }
        waiting.push_back(AgentAsk { message, reply });
    }
    if let Some(ctx) = asks.wake.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        ctx.request_repaint();
    }
    answered.recv_timeout(ANSWER_WAIT).map_err(|_| NO_ANSWER)
}

/// The JSON-RPC error that answers `message` with `why`, when it's a request (one with an id).
fn refusal(message: &str, why: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(message.trim()).ok()?;
    let id = value.get("id").filter(|_| value.get("method").is_some())?;
    Some(serde_json::json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": why}}).to_string())
}

/// `mailcraft mcp`: an agent's client talks to the MailCraft running on `data` through
/// standard input and output, until its input ends. With none running (or one that won't take
/// it), each request is answered with why, so the client can say so.
pub fn bridge(data: &Path) {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stream = socket_path(data).ok_or(NOT_RUNNING).and_then(|path| connect(&path));
    let stream = match stream {
        Ok(s) => s,
        Err(why) => return refuse_all(&mut input, &mut std::io::stdout(), why),
    };
    let answers = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            log::warn!("agent bridge: {e}");
            return refuse_all(&mut input, &mut std::io::stdout(), NOT_LET_IN);
        }
    };
    // The window's answers, as they come.
    let out = std::thread::Builder::new().name("mcp-out".into()).spawn(move || {
        let mut reader = BufReader::new(&answers);
        let mut stdout = std::io::stdout();
        while let Ok(Some(line)) = read_message(&mut reader) {
            if stdout.write_all(line.as_bytes()).and_then(|()| stdout.flush()).is_err() {
                break;
            }
        }
    });
    let mut writer = &stream;
    while let Ok(Some(line)) = read_message(&mut input) {
        if writer.write_all(line.as_bytes()).and_then(|()| if line.ends_with('\n') { Ok(()) } else { writer.write_all(b"\n") }).is_err() {
            break;
        }
    }
    // Input ended: the agent's done. What's still being answered is written before leaving.
    let _ = stream.shutdown(std::net::Shutdown::Write);
    if let Ok(out) = out {
        let _ = out.join();
    }
}

/// The running MailCraft's socket, taken for an agent.
fn connect(path: &Path) -> Result<UnixStream, &'static str> {
    let stream = UnixStream::connect(path).map_err(|_| NOT_RUNNING)?;
    let key = read_key(path).map_err(|_| NOT_RUNNING)?;
    let said = stream.set_write_timeout(Some(TIMEOUT)).and_then(|()| (&stream).write_all(request(&key).as_bytes()));
    said.map_err(|_| NOT_LET_IN)?;
    let mut reply = String::new();
    let mut reader = BufReader::new(Patient::new(&stream)).take(16);
    reader.read_line(&mut reply).map_err(|_| NOT_LET_IN)?;
    if reply == OK { Ok(stream) } else { Err(NOT_LET_IN) }
}

/// Answer every request on `input` with `why` until it ends.
fn refuse_all(input: &mut impl BufRead, output: &mut impl Write, why: &str) {
    while let Ok(Some(line)) = read_message(input) {
        if let Some(reply) = refusal(&line, why)
            && output.write_all(format!("{reply}\n").as_bytes()).and_then(|()| output.flush()).is_err()
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
