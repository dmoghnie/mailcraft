use super::*;
use crate::fakes::fake_mcp::McpServer;
use crate::waiting::events_until;
use mailcraft_model::McpTool;
use serde_json::json;

fn web(url: &str) -> McpTarget {
    McpTarget::Web { url: url.into(), oauth: None }
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

/// A program that can't be started says so, in its own words: it's not a server's answer.
#[test]
fn a_program_that_cant_be_started_says_so() {
    let mut p = Pool::new(crate::fake::store("mcp-no-program"), Arc::new(MemoryCredentials::default()));
    let target = McpTarget::Program { command: "/no/such/mcp-program".into(), args: Vec::new(), key_env: None };
    p.submit(Job::Mcp { work: McpWork::List { server: "m1".into(), target, key: None } });
    let seen = events_until(&p, "the failure", |s| s.iter().any(|e| matches!(e, Event::Mcp { .. })));
    let error = seen.iter().find_map(|e| match e {
        Event::Mcp { done: McpDone::Listed { error, .. } } => error.clone(),
        _ => None,
    });
    assert!(error.as_deref().is_some_and(|e| e.starts_with("the program couldn't be started")), "{error:?}");
}

/// Signing in through the pool opens the page through the UI and keeps the sign-in in the
/// keychain; the server is then reached with its token, and once that runs out, a new one is
/// got and kept (the refresh token turned over).
#[test]
fn a_server_signed_in_to_is_reached_with_its_token() {
    let server = McpServer::start_signed(false);
    let creds = Arc::new(MemoryCredentials::default());
    let mut p = Pool::new(crate::fake::store("mcp-signed"), creds.clone());
    p.submit(Job::Mcp { work: McpWork::SignIn { server: "m1".into(), url: server.url.clone(), key: "mcp-m1".into() } });
    let mut how = None;
    events_until(&p, "the sign-in", |seen| {
        for e in seen {
            match e {
                // The browser: the page the UI is told to open, followed.
                Event::OpenUrl { url } if how.is_none() => {
                    let url = url.clone();
                    how = Some(None);
                    std::thread::spawn(move || crate::http::get(&url, 10_000));
                }
                Event::Mcp { done: McpDone::SignedIn { oauth, error, .. } } => {
                    assert_eq!(error, &None);
                    how = Some(oauth.clone());
                }
                _ => {}
            }
        }
        matches!(how, Some(Some(_)))
    });
    let how = how.flatten();
    let kept = creds.get_key("mcp-m1").and_then(|k| crate::mcp::Kept::read(&k)).expect("kept");
    assert_eq!(kept.access, "a-1");
    creds.set_key("mcp-m1", &crate::mcp::Kept { expires: 0, ..kept }.text()).unwrap();
    let target = McpTarget::Web { url: server.url.clone(), oauth: how };
    p.submit(Job::Mcp { work: McpWork::List { server: "m1".into(), target, key: Some("mcp-m1".into()) } });
    let seen = events_until(&p, "the tools", |s| s.iter().any(|e| matches!(e, Event::Mcp { done: McpDone::Listed { .. } })));
    let listed = seen.iter().find_map(|e| match e {
        Event::Mcp { done: McpDone::Listed { tools, error, .. } } => Some((tools.len(), error.clone())),
        _ => None,
    });
    assert_eq!(listed, Some((2, None)));
    let now = creds.get_key("mcp-m1").and_then(|k| crate::mcp::Kept::read(&k)).expect("kept");
    assert_eq!((now.access.as_str(), now.refresh.as_deref()), ("a-2", Some("r-2")));
}
