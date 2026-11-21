//! The MCP answers (`crate::mcp`): agents let in by the user alone, JSON-RPC as the spec has it,
//! and the two tools on the sample mailbox.

use serde_json::{Value, json};

use super::*;
use crate::mcp::*;

fn ask(s: &mut Session, m: Value) -> Value {
    serde_json::from_str(&answer(s, &m.to_string()).unwrap()).unwrap()
}

fn request(id: i64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn call(s: &mut Session, tool: &str, arguments: Value) -> Value {
    ask(s, request(9, "tools/call", json!({"name": tool, "arguments": arguments})))
}

fn let_in(name: &str) -> Session {
    let mut s = sample_session(name);
    s.prefs.agents = true;
    s
}

/// Nothing but ping is answered, and no command runs, until the user lets agents in.
#[test]
fn agents_are_let_in_only_by_the_user() {
    let mut s = sample_session("mcp-off");
    let unread = s.run("list.messages", &json!({})).unwrap();
    for m in [
        request(1, "initialize", json!({})),
        request(2, "tools/list", json!({})),
        request(3, "tools/call", json!({"name": "run_command", "arguments": {"id": "mail.markAllRead"}})),
    ] {
        let a = ask(&mut s, m);
        assert_eq!((a["error"]["code"].as_i64(), a["error"]["message"].as_str()), (Some(AGENTS_OFF), Some(AGENTS_OFF_SAID)), "{a}");
    }
    assert_eq!(s.run("list.messages", &json!({})).unwrap(), unread, "nothing ran");
    assert_eq!(ask(&mut s, request(4, "ping", json!({})))["result"], json!({}));
    // Switched on as the other options are.
    assert_eq!(s.run("options.agents", &json!({"on": true})).unwrap(), json!({"on": true}));
    assert_eq!(ask(&mut s, request(5, "tools/list", json!({})))["result"]["tools"].as_array().map(Vec::len), Some(2));
}

#[test]
fn initialize_agrees_a_version_and_says_what_mailcraft_is() {
    let mut s = let_in("mcp-1");
    let a = ask(&mut s, request(1, "initialize", json!({"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "t"}})));
    assert_eq!((a["id"].as_i64(), a["result"]["protocolVersion"].as_str()), (Some(1), Some("2025-03-26")));
    assert_eq!(a["result"]["capabilities"], json!({"tools": {}}));
    assert!(a["result"]["instructions"].as_str().is_some_and(|i| i.contains("list_commands")));
    let a = ask(&mut s, request(2, "initialize", json!({"protocolVersion": "1999-01-01"})));
    assert_eq!(a["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
}

/// Notifications and responses get no answer; what isn't JSON-RPC gets JSON-RPC's errors.
#[test]
fn only_requests_are_answered() {
    let mut s = let_in("mcp-2");
    assert_eq!(answer(&mut s, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), None);
    assert_eq!(answer(&mut s, r#"{"jsonrpc":"2.0","id":3,"result":{}}"#), None);
    let a: Value = serde_json::from_str(&answer(&mut s, "{nope").unwrap()).unwrap();
    assert_eq!((a["id"].clone(), a["error"]["code"].as_i64()), (Value::Null, Some(PARSE_ERROR)));
    assert_eq!(ask(&mut s, json!({"jsonrpc": "2.0", "id": 4}))["error"]["code"], INVALID_REQUEST);
    assert_eq!(ask(&mut s, request(5, "sampling/createMessage", json!({})))["error"]["code"], METHOD_NOT_FOUND);
    assert_eq!(ask(&mut s, json!([]))["error"]["code"], INVALID_REQUEST);
    // A batch: each request answered in turn, notifications left out.
    let a = ask(
        &mut s,
        json!([request(6, "ping", json!({})), {"jsonrpc": "2.0", "method": "notifications/initialized"}, request(7, "tools/list", json!({}))]),
    );
    let ids: Vec<i64> = a.as_array().unwrap().iter().filter_map(|m| m["id"].as_i64()).collect();
    assert_eq!(ids, [6, 7]);
    assert_eq!(answer(&mut s, r#"[{"jsonrpc":"2.0","method":"notifications/initialized"}]"#), None);
}

/// list_commands gives what the registry has, filtered, with why one can't run now.
#[test]
fn list_commands_describes_the_registry() {
    let mut s = let_in("mcp-3");
    let all: Vec<Value> = serde_json::from_str(call(&mut s, "list_commands", json!({}))["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(all.len(), s.registry().all().len());
    let mail: Vec<Value> =
        serde_json::from_str(call(&mut s, "list_commands", json!({"filter": "COMPOSE.SEND"}))["result"]["content"][0]["text"].as_str().unwrap())
            .unwrap();
    assert!(mail.iter().any(|c| c["id"] == "compose.send"), "{mail:?}");
    assert!(
        mail.iter().all(|c| ["id", "label", "location"].iter().any(|k| c[k].as_str().is_some_and(|t| t.to_lowercase().contains("compose.send"))))
    );
    assert!(all.iter().any(|c| c.get("disabled").is_some_and(Value::is_string)), "some can't run in the sample's state");
}

/// run_command runs a command as `Session::run` does; one that fails says why as the tool's
/// result, and a call that's wrong itself is a JSON-RPC error.
#[test]
fn run_command_runs_one_as_the_window_would() {
    let mut s = let_in("mcp-4");
    let a = call(&mut s, "run_command", json!({"id": "list.messages", "params": {"limit": 2}}));
    assert_eq!(a["result"]["isError"], false);
    let listed: Value = serde_json::from_str(a["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(listed, s.run("list.messages", &json!({"limit": 2})).unwrap());
    assert!(listed["messages"].as_array().is_some_and(|m| m.len() == 2), "{listed}");

    let a = call(&mut s, "run_command", json!({"id": "no.such"}));
    assert_eq!(a["result"]["isError"], true);
    assert!(a["result"]["content"][0]["text"].as_str().is_some_and(|t| !t.is_empty()));
    assert_eq!(call(&mut s, "run_command", json!({}))["error"]["code"], INVALID_PARAMS);
    assert_eq!(call(&mut s, "delete_everything", json!({}))["error"]["code"], INVALID_PARAMS);
    assert_eq!(ask(&mut s, request(1, "tools/call", json!({})))["error"]["code"], INVALID_PARAMS);
}

/// Junk from an agent never panics: every shape of message, every tool with junk arguments.
#[test]
fn junk_never_crashes() {
    let mut s = let_in("mcp-5");
    for m in [
        "",
        "null",
        "1",
        "\"x\"",
        "[1,2]",
        "{}",
        r#"{"method":1,"id":{}}"#,
        r#"{"method":"tools/call","id":1,"params":[]}"#,
        r#"{"method":"tools/call","id":1,"params":{"name":"run_command","arguments":"x"}}"#,
    ] {
        let _ = answer(&mut s, m);
    }
    for args in [json!(null), json!([]), json!({"id": 5}), json!({"id": "list.messages", "params": "x"}), json!({"filter": 3})] {
        for tool in ["list_commands", "run_command"] {
            let _ = call(&mut s, tool, args.clone());
        }
    }
    let many: Vec<Value> = (0..500).map(|i| request(i, "ping", json!({}))).collect();
    assert_eq!(ask(&mut s, Value::Array(many)).as_array().map(Vec::len), Some(MAX_BATCH));
}

/// Every command the instructions name is one there is (a renamed one would send agents to
/// nothing).
#[test]
fn the_instructions_name_commands_there_are() {
    let s = Session::blank();
    let named: Vec<&str> = INSTRUCTIONS
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '*'))
        .map(|w| w.trim_end_matches('.'))
        .filter(|w| w.contains('.') && !w.contains('*') && w.chars().next().is_some_and(|c| c.is_ascii_lowercase()))
        .collect();
    assert!(named.len() >= 8, "{named:?}");
    for id in named {
        assert!(s.registry().get(id).is_some(), "{id} isn't a command");
    }
}
