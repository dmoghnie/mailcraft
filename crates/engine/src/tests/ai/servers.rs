//! MCP servers the person adds: checked as they're saved, listed once on, their tools kept
//! across sessions, closed and forgotten with their key; and never an outside agent's to add.

use mailcraft_model::{AiWork, Event, Job, McpDone, McpTarget, McpTool, McpWork};
use serde_json::{Value, json};

use super::*;

fn mcp_jobs(s: &mut Session) -> Vec<McpWork> {
    s.take_jobs().into_iter().filter_map(|j| if let Job::Mcp { work } = j { Some(work) } else { None }).collect()
}

fn listed(s: &mut Session) -> Vec<Value> {
    s.run("mcpServer.list", &json!({})).unwrap()["servers"].as_array().unwrap().clone()
}

/// A program and a server on the web are added, checked first; one turned on is listed with
/// its key's name, its tools kept as they come (and read again in a new session); turned off
/// it's closed; removed, its key is forgotten.
#[test]
fn servers_are_added_listed_and_removed() {
    let mut s = sample_session("mcp-servers");
    s.take_jobs();
    for (bad, why) in [
        (json!({"name": "Notes", "command": "x"}), "lower-case"),
        (json!({"name": "notes"}), "one of them"),
        (json!({"name": "notes", "command": "x", "url": "https://a.example"}), "one of them"),
        (json!({"name": "notes", "url": "http://mcp.example/mcp"}), "https"),
        (json!({"name": "notes", "command": "x", "keyEnv": "not a name"}), "environment variable"),
    ] {
        let e = s.run("mcpServer.save", &bad).unwrap_err().to_string();
        assert!(e.contains(why), "{bad}: {e}");
    }
    let notes = json!({"name": "notes", "command": "notes-mcp", "args": ["--read-only"], "keyEnv": "NOTES_KEY", "key": "k1", "on": true});
    let id = s.run("mcpServer.save", &notes).unwrap()["id"].as_str().unwrap().to_string();
    let jobs = s.take_jobs();
    assert!(jobs.iter().any(|j| matches!(j, Job::Ai { work: AiWork::SaveKey { name, key: Some(_) } } if name == "mcp-m1")), "{jobs:?}");
    let list =
        jobs.iter().find_map(|j| if let Job::Mcp { work: McpWork::List { server, target, key } } = j { Some((server, target, key)) } else { None });
    let (server, target, key) = list.expect("listed once on");
    assert_eq!((server.as_str(), key.as_deref()), (id.as_str(), Some("mcp-m1")));
    assert!(matches!(target, McpTarget::Program { args, key_env: Some(e), .. } if args == &["--read-only"] && e == "NOTES_KEY"));
    assert_eq!(listed(&mut s)[0]["listing"], true);
    let tool =
        McpTool { name: "find_note".into(), about: "Finds notes".into(), schema: json!({"type": "object"}), read_only: true, ..McpTool::default() };
    s.apply(Event::Mcp { done: McpDone::Listed { server: id.clone(), tools: vec![tool], error: None } });
    assert_eq!(listed(&mut s)[0]["tools"], json!(["find_note"]));
    // Listed again, and failing: what was known stays, with why.
    s.run("mcpServer.refresh", &json!({"id": id})).unwrap();
    s.apply(Event::Mcp { done: McpDone::Listed { server: id.clone(), tools: Vec::new(), error: Some("it ended".into()) } });
    let row = &listed(&mut s)[0];
    assert_eq!((&row["tools"], &row["error"]), (&json!(["find_note"]), &json!("it ended")));
    let web = s.run("mcpServer.save", &json!({"name": "search", "url": "https://mcp.example/mcp"})).unwrap();
    assert_eq!(web["local"], false);
    assert!(s.run("mcpServer.save", &json!({"name": "search", "command": "y"})).is_err(), "a name taken");
    let store = s.store.clone().unwrap();
    let mut again = Session::open(Some(store), mailcraft_model::dates::Clock::new(NOW, 0));
    again.prefs = s.prefs.clone();
    assert_eq!(listed(&mut again)[0]["tools"], json!(["find_note"]), "kept in the data folder");
    s.take_jobs();
    s.run("mcpServer.turn", &json!({"id": id, "on": false})).unwrap();
    assert_eq!(mcp_jobs(&mut s), [McpWork::Close { server: id.clone() }]);
    assert!(s.run("mcpServer.refresh", &json!({"id": id})).is_err(), "off");
    s.run("mcpServer.remove", &json!({"id": id})).unwrap();
    let jobs = s.take_jobs();
    assert!(jobs.iter().any(|j| matches!(j, Job::Ai { work: AiWork::SaveKey { name, key: None } } if name == "mcp-m1")));
    assert_eq!(listed(&mut s).len(), 1);
}

/// Adding a server, or turning one on, starts a program: an outside agent can't, even let to
/// act; it may list them.
#[test]
fn an_agent_never_adds_or_turns_on_a_server() {
    let mut s = sample_session("mcp-servers-agents");
    s.prefs.agents = true;
    s.prefs.agents_act = true;
    let run = |s: &mut Session, id: &str, params: Value| {
        let m =
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "run_command", "arguments": {"id": id, "params": params}}});
        serde_json::from_str::<Value>(&crate::mcp::answer(s, &m.to_string()).unwrap()).unwrap()
    };
    let a = run(&mut s, "mcpServer.save", json!({"name": "x", "command": "rm"}));
    assert_eq!(a["result"]["isError"], true, "{a}");
    assert!(s.prefs.ai.mcp.is_empty());
    s.run("mcpServer.save", &json!({"name": "notes", "command": "notes-mcp"})).unwrap();
    assert_eq!(run(&mut s, "mcpServer.turn", json!({"id": "m1", "on": true}))["result"]["isError"], true);
    assert!(!s.prefs.ai.mcp[0].on);
    assert_eq!(run(&mut s, "mcpServer.list", json!({}))["result"]["isError"], false);
}
