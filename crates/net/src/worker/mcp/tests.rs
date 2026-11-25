use super::*;
use crate::fakes::fake_mcp::McpServer;
use crate::waiting::events_until;
use mailcraft_model::McpTool;
use serde_json::json;

fn web(url: &str) -> McpTarget {
    McpTarget::Web { url: url.into() }
}

/// What call `id` gave, or why it failed.
fn called(seen: &[Event], id: u64) -> Option<std::result::Result<serde_json::Value, String>> {
    seen.iter().find_map(|e| match e {
        Event::Mcp { done: McpDone::Called { id: i, result, error } } if *i == id => {
            Some(result.clone().ok_or_else(|| error.clone().unwrap_or_default()))
        }
        _ => None,
    })
}

/// A server's tools listed and its tools called on one connection, with its kept key; a closed
/// one is connected to again; a server whose key isn't kept isn't reached.
#[test]
fn a_servers_tools_are_listed_and_called_on_one_connection() {
    let server = McpServer::start(false);
    let creds = Arc::new(MemoryCredentials::default());
    creds.set_key("mcp-m1", "tok").unwrap();
    let mut p = Pool::new(crate::fake::store("mcp-work"), creds);
    let (target, key) = (web(&server.url), Some("mcp-m1".to_string()));
    p.submit(Job::Mcp { work: McpWork::List { server: "m1".into(), target: target.clone(), key: key.clone() } });
    let seen = events_until(&p, "the tools", |s| s.iter().any(|e| matches!(e, Event::Mcp { done: McpDone::Listed { .. } })));
    let tools: Vec<McpTool> = seen
        .iter()
        .find_map(|e| match e {
            Event::Mcp { done: McpDone::Listed { tools, error: None, .. } } => Some(tools.clone()),
            _ => None,
        })
        .expect("listed");
    assert_eq!(tools.len(), 2);
    let call = |id: u64, server: &str, key: Option<String>| Job::Mcp {
        work: McpWork::Call { id, server: server.into(), target: target.clone(), key, tool: "echo".into(), args: json!({"text": "hi"}) },
    };
    p.submit(call(1, "m1", key.clone()));
    let seen = events_until(&p, "the call", |s| called(s, 1).is_some());
    assert_eq!(called(&seen, 1), Some(Ok(json!({"echo": {"text": "hi"}}))));
    let agreed = |s: &McpServer| s.seen().iter().filter(|r| r.body["method"] == "initialize").count();
    assert_eq!(agreed(&server), 1, "one connection for both");
    p.submit(Job::Mcp { work: McpWork::Close { server: "m1".into() } });
    p.submit(call(2, "m1", key));
    let seen = events_until(&p, "the call again", |s| called(s, 2).is_some());
    assert!(called(&seen, 2).is_some_and(|r| r.is_ok()));
    assert_eq!(agreed(&server), 2, "connected again once closed");
    p.submit(call(3, "m2", Some("mcp-none".into())));
    let seen = events_until(&p, "no key", |s| called(s, 3).is_some());
    assert!(called(&seen, 3).is_some_and(|r| r.is_err_and(|e| e.contains("no key is kept"))), "{seen:?}");
}
