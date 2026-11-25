//! MailCraft's MCP client against a server on the web of the tests' own, answering JSON and
//! events, and (where there's a shell) a program of the tests' own.

use super::*;
use crate::fakes::fake_mcp::{McpServer, SESSION};

fn web(url: &str) -> McpTarget {
    McpTarget::Web { url: url.into() }
}

/// Agreed, a server's tools are listed over their pages, one with a name the protocol doesn't
/// allow left out; a call gives the tool's structured answer, a tool that failed and a tool
/// there isn't are errors; the session, version and token go with each request after the first.
#[test]
fn a_web_servers_tools_are_listed_and_called() {
    for events in [false, true] {
        let server = McpServer::start(events);
        let mut c = McpConn::connect(&web(&server.url), Some("tok")).unwrap();
        let tools = c.tools().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["echo", "file_away"], "events: {events}");
        assert!(tools[0].read_only && !tools[1].read_only);
        assert_eq!((tools[1].title.as_str(), tools[0].about.as_str()), ("File away", "Gives back what it's given"));
        assert_eq!(c.call("echo", &json!({"text": "hi"})).unwrap(), json!({"echo": {"text": "hi"}}));
        let e = c.call("fail", &json!({})).unwrap_err().to_string();
        assert!(e.contains("it broke"), "{e}");
        assert!(c.call("none", &json!({})).unwrap_err().to_string().contains("no such tool"));
        let seen = server.seen();
        assert_eq!(seen[0].body["method"], "initialize");
        assert_eq!(seen[0].header("Mcp-Session-Id"), None);
        // After the server's ping, when it sent one first.
        assert!(seen.iter().skip(1).take(2).any(|s| s.body["method"] == "notifications/initialized"), "{seen:?}");
        let later = seen.last().unwrap();
        assert_eq!((later.header("Mcp-Session-Id"), later.header("MCP-Protocol-Version")), (Some(SESSION), Some("2025-06-18")));
        assert!(seen.iter().all(|s| s.header("Authorization") == Some("Bearer tok")));
        if events {
            // Its ping, answered with a request of MailCraft's own.
            assert!(seen.iter().any(|s| s.body["id"] == "srv-1" && s.body["result"] == json!({})), "{seen:?}");
        }
    }
}

/// A token goes over https only, or to this computer; plain http elsewhere isn't reached.
#[test]
fn a_web_server_elsewhere_is_reached_over_https_only() {
    let e = McpConn::connect(&web("http://mcp.example/mcp"), None).err().unwrap().to_string();
    assert!(e.contains("https"), "{e}");
}

/// What servers send is hostile: events and JSON read into messages, odd ones let go; a long
/// text cut; a tool's schema too large kept as any object.
#[test]
fn what_a_server_sends_is_read_with_care() {
    let events = "event: message\r\ndata: {\"id\": 1,\r\ndata: \"result\": {}}\r\n\r\ndata: not json\n\ndata: [1]\n\n";
    assert_eq!(from_events(events), [json!({"id": 1, "result": {}})]);
    assert_eq!(from_json("[{\"id\": 1}, 2]"), [json!({"id": 1})]);
    assert!(from_json("junk").is_empty());
    let long = "é".repeat(RESULT_MOST + 10);
    let text = text_of(&json!({"content": [{"type": "text", "text": long}, {"type": "image", "data": "x"}]}));
    assert_eq!(text.chars().count(), RESULT_MOST + 1);
    let big = json!({"name": "big", "inputSchema": {"type": "object", "description": "x".repeat(SCHEMA_MOST)}});
    assert_eq!(tool_of(&big).unwrap().schema, json!({"type": "object"}));
    assert!(tool_of(&json!({"name": "x".repeat(NAME_MOST + 1)})).is_none());
}

/// A program of the tests' own (a shell script), given its key in the variable named: agreed
/// over its input and output, a line not a message let go, its ping answered, its tools listed.
#[cfg(unix)]
#[test]
fn a_programs_tools_are_listed() {
    let script = r#"
read hello
echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{},"serverInfo":{"name":"sh","version":"1"}}}'
read initialized
read list
echo 'starting up'
echo '{"jsonrpc":"2.0","id":"p","method":"ping"}'
read pong
case "$pong" in *'"id":"p"'*'"result":{}'*) ;; *) exit 1 ;; esac
echo '{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}'
echo '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"look","description":"'"$MCP_TEST_KEY"'","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}}]}}'
read never
"#;
    let target = McpTarget::Program { command: "sh".into(), args: vec!["-c".into(), script.into()], key_env: Some("MCP_TEST_KEY".into()) };
    let mut c = McpConn::connect(&target, Some("k1")).unwrap();
    let tools = c.tools().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].about, "k1", "the key in its variable");
    assert!(tools[0].read_only && !c.tools_changed, "listed after it said they changed");
    let e = McpConn::connect(&McpTarget::Program { command: "/no/such/program".into(), args: Vec::new(), key_env: None }, None).err().unwrap();
    assert!(e.to_string().contains("couldn't be started"), "{e}");
}
