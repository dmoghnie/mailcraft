//! An MCP server on the web of the tests' own (Streamable HTTP), so MailCraft's MCP client is
//! tested without a network: it agrees on a version and gives a session, lists two tools over
//! two pages (`echo`, read-only, and `file_away`), answers `echo` with what it was given (its
//! text, and as structured content) and `fail` as a tool that failed, and keeps every request.
//! When `events`, it answers as server-sent events, a ping of its own asked first.
//!
//! [`McpServer::start_signed`] wants a sign-in (MCP's authorization) and is its own
//! authorization server on the same port.

use std::collections::HashMap;
use std::io::BufReader;
use std::net::{TcpListener, TcpStream};
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

    /// A server that wants a sign-in, its 401 naming its metadata, with its authorization
    /// server on the same port (`/auth`): its metadata, registration (client `c-1`), a sign-in
    /// page that sends the browser straight back with a code (and `iss`, another server's when
    /// `wrong_iss`), and tokens for the code (its PKCE checked) and for the refresh token, which
    /// turns over: `a-1` and `r-1`, then `a-2` and `r-2`. Each request is kept as its method,
    /// target and body.
    pub fn start_signed(wrong_iss: bool) -> McpServer {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let url = format!("{base}/mcp");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (kept, mcp) = (seen.clone(), url.clone());
        std::thread::spawn(move || {
            let mut challenge = String::new();
            for conn in l.incoming() {
                let Ok(mut s) = conn else { continue };
                let Ok(reading) = s.try_clone() else { continue };
                let Some(r) = super::http::read_request(&mut BufReader::new(reading)) else { continue };
                let text = String::from_utf8_lossy(&r.body).to_string();
                kept.lock().unwrap().push(Seen { headers: r.headers.clone(), body: json!({"method": r.method, "target": r.target, "body": text}) });
                let (path, query) = r.target.split_once('?').unwrap_or((r.target.as_str(), ""));
                let issuer = format!("{base}/auth");
                match (r.method.as_str(), path) {
                    ("POST", "/mcp") if matches!(r.header("Authorization"), Some("Bearer a-1" | "Bearer a-2")) => {
                        let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                        match answer(&body) {
                            Some(a) => json_to(&mut s, "200 OK", &a),
                            None => super::http::respond(&mut s, "202 Accepted", &[("Connection", "close".into())], b""),
                        }
                    }
                    ("POST", "/mcp") => {
                        let ask = format!(
                            "Bearer error=\"invalid_token\", resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp\", scope=\"notes.read\""
                        );
                        super::http::respond(&mut s, "401 Unauthorized", &[("WWW-Authenticate", ask), ("Connection", "close".into())], b"");
                    }
                    ("GET", "/.well-known/oauth-protected-resource/mcp") => {
                        let doc = json!({"resource": mcp, "authorization_servers": [issuer], "scopes_supported": ["notes.read", "notes.write"]});
                        json_to(&mut s, "200 OK", &doc);
                    }
                    ("GET", "/.well-known/oauth-authorization-server/auth") => json_to(
                        &mut s,
                        "200 OK",
                        &json!({
                            "issuer": issuer,
                            "authorization_endpoint": format!("{base}/auth/authorize"),
                            "token_endpoint": format!("{base}/auth/token"),
                            "registration_endpoint": format!("{base}/auth/register"),
                            "code_challenge_methods_supported": ["S256"],
                            "authorization_response_iss_parameter_supported": true,
                        }),
                    ),
                    ("POST", "/auth/register") => json_to(&mut s, "201 Created", &json!({"client_id": "c-1"})),
                    ("GET", "/auth/authorize") => {
                        let q = params(query);
                        let got = |k: &str| q.get(k).cloned().unwrap_or_default();
                        challenge = got("code_challenge");
                        let iss = if wrong_iss { "https://elsewhere.example".to_string() } else { issuer };
                        let back =
                            format!("{}?code=the-code&state={}&iss={}", got("redirect_uri"), crate::http::pct(&got("state")), crate::http::pct(&iss));
                        super::http::respond(&mut s, "302 Found", &[("Location", back), ("Connection", "close".into())], b"");
                    }
                    ("POST", "/auth/token") => {
                        let f = params(&text);
                        let got = |k: &str| f.get(k).cloned().unwrap_or_default();
                        let verified =
                            crate::oauth::b64url(ring::digest::digest(&ring::digest::SHA256, got("code_verifier").as_bytes()).as_ref()) == challenge;
                        let tokens = match got("grant_type").as_str() {
                            "authorization_code" if verified && got("code") == "the-code" => Some(("a-1", "r-1")),
                            "refresh_token" if got("refresh_token") == "r-1" => Some(("a-2", "r-2")),
                            _ => None,
                        };
                        match tokens {
                            Some((a, r)) => json_to(&mut s, "200 OK", &json!({"access_token": a, "refresh_token": r, "expires_in": 3600})),
                            None => json_to(&mut s, "400 Bad Request", &json!({"error": "invalid_grant"})),
                        }
                    }
                    _ => super::http::respond(&mut s, "404 Not Found", &[("Connection", "close".into())], b""),
                }
            }
        });
        McpServer { url, seen }
    }
}

/// Answer with `body`, as JSON.
fn json_to(s: &mut TcpStream, status: &str, body: &Value) {
    super::http::respond(s, status, &[("Content-Type", "application/json".into()), ("Connection", "close".into())], body.to_string().as_bytes());
}

/// A query's or a form's fields.
fn params(text: &str) -> HashMap<String, String> {
    text.split('&').filter_map(|p| p.split_once('=')).map(|(k, v)| (crate::oauth::unpct(k), crate::oauth::unpct(v))).collect()
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
