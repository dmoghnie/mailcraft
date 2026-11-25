//! MCP servers the person adds: checked as they're saved, listed once on, their tools kept
//! across sessions, closed and forgotten with their key; signed in to, the sign-in kept while
//! the address stays; and never an AI agent's to add, turn on or sign in to.

use mailcraft_model::{AiWork, Event, Job, McpDone, McpOAuth, McpTarget, McpTool, McpWork};
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
/// Adding a server, changing one or turning it on starts a program, and signing in to one uses
/// the person's own account: an AI agent never runs them, even let to ask to run commands, nor
/// has them run without asking. It may list them.
#[test]
fn an_agent_never_adds_or_turns_on_a_server() {
    let mut s = sample_session("mcp-servers-agents");
    s.run("options.agentsRun", &json!({"on": true})).unwrap();
    for (id, params) in [
        ("mcpServer.save", json!({"name": "x", "command": "rm"})),
        ("mcpServer.turn", json!({"id": "m1", "on": true})),
        ("mcpServer.signIn", json!({"id": "m1"})),
    ] {
        let refused = s.agent_may_run(id, &params).unwrap_err();
        assert!(refused.contains("Only the person using MailCraft"), "{id}: {refused}");
        assert!(s.run("options.agentsAlways", &json!({"allow": id})).is_err(), "{id}: never without asking");
    }
    assert!(s.prefs.agents_always.is_empty());
    assert!(s.agent_may_run("mcpServer.list", &json!({})).is_ok());
}

/// A server on the web is signed in to: the sign-in's work names its address and the key the
/// sign-in is kept under; once signed in, it's listed with that key, and saving it again at the
/// same address keeps the sign-in, which another address, or a key given, replaces. A program
/// isn't signed in to, and a failed sign-in says why.
#[test]
fn a_server_on_the_web_is_signed_in_to() {
    let mut s = sample_session("mcp-servers-sign-in");
    let id =
        s.run("mcpServer.save", &json!({"name": "notes", "url": "https://mcp.example/mcp", "on": true})).unwrap()["id"].as_str().unwrap().to_string();
    s.take_jobs();
    s.run("mcpServer.signIn", &json!({"id": id})).unwrap();
    assert_eq!(mcp_jobs(&mut s), [McpWork::SignIn { server: id.clone(), url: "https://mcp.example/mcp".into(), key: format!("mcp-{id}") }]);
    assert!(s.mcp_tools()[&id].signing);
    let how = McpOAuth {
        issuer: "https://auth.example".into(),
        token_url: "https://auth.example/token".into(),
        client_id: "c-1".into(),
        resource: "https://mcp.example/mcp".into(),
        scope: "notes.read".into(),
    };
    s.apply(Event::Mcp { done: McpDone::SignedIn { server: id.clone(), oauth: Some(how.clone()), error: None } });
    assert!(!s.mcp_tools()[&id].signing && s.prefs.ai.mcp[0].has_key);
    let signed = McpTarget::Web { url: "https://mcp.example/mcp".into(), oauth: Some(how.clone()) };
    let list = McpWork::List { server: id.clone(), target: signed.clone(), key: Some(format!("mcp-{id}")) };
    assert!(mcp_jobs(&mut s).contains(&list), "listed with the sign-in");
    s.run("mcpServer.save", &json!({"id": id, "name": "notes", "url": "https://mcp.example/mcp"})).unwrap();
    assert_eq!(s.prefs.ai.mcp[0].target, signed, "the same address keeps its sign-in");
    s.run("mcpServer.save", &json!({"id": id, "name": "notes", "url": "https://mcp.example/v2"})).unwrap();
    assert!(matches!(&s.prefs.ai.mcp[0].target, McpTarget::Web { oauth: None, .. }) && !s.prefs.ai.mcp[0].has_key, "another address forgets it");
    s.apply(Event::Mcp { done: McpDone::SignedIn { server: id.clone(), oauth: None, error: Some("the sign-in wasn't finished in time".into()) } });
    assert_eq!(s.mcp_tools()[&id].error.as_deref(), Some("the sign-in wasn't finished in time"));
    let program = s.run("mcpServer.save", &json!({"name": "files", "command": "files-mcp"})).unwrap()["id"].as_str().unwrap().to_string();
    assert!(s.run("mcpServer.signIn", &json!({"id": program})).is_err());
}
