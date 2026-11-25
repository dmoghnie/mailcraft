//! An MCP server on the web of the tests' own (Streamable HTTP), so MailCraft's MCP client is
//! tested without a network: it agrees on a version and gives a session, lists two tools over
//! two pages (`echo`, read-only, and `file_away`), answers `echo` with what it was given (its
//! text, and as structured content) and `fail` as a tool that failed, and keeps every request.
//! When `events`, it answers as server-sent events, a ping of its own asked first.

use std::io::BufReader;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

/// The session the server gives.
pub(crate) const SESSION: &str = "s-123";

/// A request the server was sent: its headers and its message.
#[derive(Clone, Debug)]
pub(crate) struct Seen {
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

pub(crate) struct McpServer {
    /// Its address: `http://127.0.0.1:<port>/mcp`.
    pub url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl McpServer {
    pub fn start(events: bool) -> McpServer {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/mcp", l.local_addr().unwrap().port());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        std::thread::spawn(move || {
            for conn in l.incoming() {
                let Ok(mut s) = conn else { continue };
                let Ok(reading) = s.try_clone() else { continue };
                let Some(r) = super::http::read_request(&mut BufReader::new(reading)) else { continue };
                let body: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
                kept.lock().unwrap().push(Seen { headers: r.headers.clone(), body: body.clone() });
                let Some(answer) = answer(&body) else {
                    super::http::respond(&mut s, "202 Accepted", &[("Connection", "close".into())], b"");
                    continue;
                };
                let session = ("Mcp-Session-Id", SESSION.to_string());
                if events {
                    let ping = json!({"jsonrpc": "2.0", "id": "srv-1", "method": "ping"});
                    let pieces = [format!("event: message\ndata: {ping}\n\n"), format!("event: message\ndata: {answer}\n\n")];
                    let headers = [("Content-Type", "text/event-stream".to_string()), session, ("Connection", "close".into())];
                    super::http::respond_chunked(&mut s, "200 OK", &headers, &pieces, std::time::Duration::ZERO);
                } else {
                    let headers = [("Content-Type", "application/json".to_string()), session, ("Connection", "close".into())];
                    super::http::respond(&mut s, "200 OK", &headers, answer.to_string().as_bytes());
                }
            }
        });
        McpServer { url, seen }
    }

    /// Every request sent so far, in order.
    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

/// The answer to a request; none to a notification or an answer.
fn answer(m: &Value) -> Option<Value> {
    let id = m.get("id")?.clone();
    let method = m.get("method")?.as_str()?;
    let ok = |result: Value| Some(json!({"jsonrpc": "2.0", "id": id, "result": result}));
    match method {
        "initialize" => ok(
            json!({"protocolVersion": "2025-06-18", "capabilities": {"tools": {"listChanged": true}}, "serverInfo": {"name": "fake", "version": "1"}}),
        ),
        "tools/list" if m.pointer("/params/cursor").is_none() => ok(json!({
            "tools": [{"name": "echo", "description": "Gives back what it's given", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}, "annotations": {"readOnlyHint": true}}],
            "nextCursor": "2",
        })),
        "tools/list" => ok(json!({"tools": [
            {"name": "file_away", "title": "File away", "description": "Files something", "inputSchema": {"type": "object"}},
            {"name": "bad name!", "description": "Not a name the protocol allows"},
        ]})),
        "tools/call" => match m.pointer("/params/name").and_then(Value::as_str) {
            Some("echo") => {
                let args = m.pointer("/params/arguments").cloned().unwrap_or(Value::Null);
                ok(json!({"content": [{"type": "text", "text": args.to_string()}], "structuredContent": {"echo": args}}))
            }
            Some("fail") => ok(json!({"content": [{"type": "text", "text": "it broke"}], "isError": true})),
            _ => Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": "no such tool"}})),
        },
        _ => Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "no such method"}})),
    }
}
