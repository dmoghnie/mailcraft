//! A script's calls to MCP servers: their tools found in what they listed, a tool that changes
//! nothing called and its answer waited for, recorded without what went; one that may change
//! something proposed and called once approved; a server that's off and one the chat's mail
//! can't go to refused; an answer let go once its chat stopped; and none for an outside agent's
//! script.

use super::script::{results, scripts_on};
use super::*;
use mailcraft_model::{McpDone, McpTool, McpWork};

/// A notes server (a program, so on this computer), on, its tools listed: one that finds,
/// one that adds.
fn notes_server(s: &mut Session) -> String {
    let added = s.run("mcpServer.save", &json!({"name": "notes", "command": "notes-mcp", "on": true})).unwrap();
    let id = added["id"].as_str().unwrap().to_string();
    let tool = |name: &str, about: &str, read_only: bool| McpTool {
        name: name.into(),
        about: about.into(),
        schema: json!({"type": "object"}),
        read_only,
        ..McpTool::default()
    };
    let tools = vec![tool("find_note", "Finds notes by their words", true), tool("add_note", "Adds a note", false)];
    s.apply(Event::Mcp { done: McpDone::Listed { server: id.clone(), tools, error: None } });
    s.take_jobs();
    id
}

/// The calls queued for MCP servers: each one's id, tool and args.
fn mcp_calls(s: &mut Session) -> Vec<(u64, String, Value)> {
    let jobs = s.take_jobs();
    jobs.into_iter()
        .filter_map(|j| match j {
            Job::Mcp { work: McpWork::Call { id, tool, args, .. } } => Some((id, tool, args)),
            _ => None,
        })
        .collect()
}

/// Chat `id`'s run of `tool`.
fn run_of(s: &mut Session, id: &str, tool: &str) -> Value {
    chat(s, id)["chat"]["runs"].as_array().unwrap().iter().find(|r| r["tool"] == tool).cloned().unwrap()
}

/// A script waiting for a call to the notes server's `find_note`: the chat's id and the call's.
fn calling(name: &str) -> (Session, String, u64) {
    let mut s = ready(name);
    scripts_on(&mut s);
    notes_server(&mut s);
    let id = send(&mut s, None, "What did I note about the storyboard?");
    let (q, put) = question(&mut s);
    let tool = put.tools.iter().find(|t| t.name == "run_script").unwrap();
    assert!(tool.description.contains("find_tools") && tool.description.contains("notes"), "told of the servers: {}", tool.description);
    let script = r#"let t = find_tools("find notes"); print(`${t[0].server} ${t[0].tool} ${t.len()}`); mcp(t[0].server, t[0].tool, #{words: "storyboard"}).found"#;
    calls(&mut s, q, "", &[("run_script", json!({"script": script}))]);
    let called = mcp_calls(&mut s);
    assert_eq!(called.len(), 1, "{called:?}");
    let (call, tool, args) = called[0].clone();
    assert_eq!((tool.as_str(), &args), ("find_note", &json!({"words": "storyboard"})));
    assert_eq!(run_of(&mut s, &id, "mcp")["state"], "waiting");
    (s, id, call)
}

/// A script finds a server's tool by its words and calls it, waiting for its answer and going
/// on with it; the record says where it went, never what.
#[test]
fn a_script_finds_an_mcp_tool_calls_it_and_goes_on_with_its_answer() {
    let (mut s, id, call) = calling("script-mcp");
    s.apply(Event::Mcp { done: McpDone::Called { id: call, result: Some(json!({"found": "Shot list due Friday"})), error: None } });
    let (_, back) = question(&mut s);
    let r = &results(&back)[0];
    assert!(!r.error && r.content.contains("notes find_note 1") && r.content.contains("It came to: Shot list due Friday"), "{}", r.content);
    assert_eq!(run_of(&mut s, &id, "mcp")["state"], "ran");
    let log = s.run("ai.log", &json!({})).unwrap().to_string();
    assert!(log.contains("MCP tool") && log.contains("find_note") && log.contains("this computer"), "{log}");
    assert!(!log.contains("storyboard") && !log.contains("Shot list"), "what went is never recorded: {log}");
}

/// A call that failed is an error the script can catch; an answer that comes once the chat
/// stopped is let go.
#[test]
fn a_call_that_failed_or_came_late_is_handled() {
    let (mut s, _, call) = calling("script-mcp-failed");
    s.apply(Event::Mcp { done: McpDone::Called { id: call, result: None, error: Some("the program ended".into()) } });
    let (_, back) = question(&mut s);
    let r = &results(&back)[0];
    assert!(r.content.contains("the program ended"), "{}", r.content);
    let (mut s, id, call) = calling("script-mcp-late");
    s.run("assistant.stop", &json!({"chat": id})).unwrap();
    s.apply(Event::Mcp { done: McpDone::Called { id: call, result: Some(json!("late")), error: None } });
    assert!(questions(&mut s).is_empty(), "nothing asked once stopped");
    assert_eq!(run_of(&mut s, &id, "mcp")["result"], "stopped before the server answered");
}

/// A server on the web this chat's mail can't go to (the account's mail goes to local models
/// alone), a tool or a server there isn't, and a server turned off are refused, as errors the script catches; `find_tools` finds only what may be
/// called. An outside agent's script calls no MCP server.
#[test]
fn a_script_calls_only_what_it_may() {
    let mut s = ready("script-mcp-refused");
    scripts_on(&mut s);
    let notes = notes_server(&mut s);
    let web =
        s.run("mcpServer.save", &json!({"name": "web", "url": "https://mcp.example/mcp", "on": true})).unwrap()["id"].as_str().unwrap().to_string();
    let look = McpTool { name: "look".into(), read_only: true, ..McpTool::default() };
    s.apply(Event::Mcp { done: McpDone::Listed { server: web, tools: vec![look], error: None } });
    s.take_jobs();
    let id = send(&mut s, None, "Try them all");
    let (q, _) = question(&mut s);
    let script = r#"
        let said = [];
        for c in [["web", "look"], ["notes", "none"], ["nobody", "x"]] {
            try { mcp(c[0], c[1], #{}); } catch (e) { said.push(e); }
        }
        print(find_tools().map(|t| t.server));
        said
    "#;
    calls(&mut s, q, "", &[("run_script", json!({"script": script}))]);
    let (called, mut asked) = queued(&mut s);
    assert!(called.is_empty() && asked.len() == 1, "nothing called, one question: {called:?}");
    let (q, back) = asked.remove(0);
    let r = &results(&back)[0].content;
    for why in ["doesn't go to web", "has no tool none", "no MCP server is named nobody", r#"["notes", "notes"]"#] {
        assert!(r.contains(why), "{why}: {r}");
    }
    s.run("mcpServer.turn", &json!({"id": notes, "on": false})).unwrap();
    s.take_jobs();
    calls(&mut s, q, "", &[("run_script", json!({"script": r#"mcp("notes", "find_note", #{})"#}))]);
    let (_, back) = question(&mut s);
    assert!(results(&back)[0].content.contains("is off"), "{}", results(&back)[0].content);
    assert!(chat(&mut s, &id)["chat"]["runs"].as_array().unwrap().iter().all(|r| r["state"] != "waiting"));
    for code in [r#"mcp("notes", "find_note", #{})"#, r#"find_tools("note")"#] {
        let e = s.run("script.run", &json!({"code": code})).unwrap_err().to_string();
        assert!(e.contains("MailCraft's own AI"), "{code}: {e}");
    }
}

/// The calls queued for MCP servers, and the questions, taken together.
fn queued(s: &mut Session) -> (Vec<(u64, String, Value)>, Vec<(u64, mailcraft_model::AiChat)>) {
    let (mut called, mut asked) = (Vec::new(), Vec::new());
    for j in s.take_jobs() {
        match j {
            Job::Mcp { work: McpWork::Call { id, tool, args, .. } } => called.push((id, tool, args)),
            Job::Ai { work: mailcraft_model::AiWork::Ask { id, chat, .. } } => asked.push((id, chat)),
            _ => {}
        }
    }
    (called, asked)
}

/// A script proposing to add a note through the notes server: the chat's id.
fn proposing(name: &str) -> (Session, String) {
    let mut s = ready(name);
    scripts_on(&mut s);
    notes_server(&mut s);
    let id = send(&mut s, None, "Note that the printer needs a call");
    let (q, _) = question(&mut s);
    let script = r#"mcp("notes", "add_note", #{text: "Call the printer"}).proposed"#;
    calls(&mut s, q, "", &[("run_script", json!({"script": script}))]);
    let (called, asked) = queued(&mut s);
    assert!(called.is_empty() && asked.is_empty(), "nothing called nor asked before the person decides: {called:?}");
    let run = run_of(&mut s, &id, "mcp");
    let what = run["what"].as_str().unwrap();
    assert_eq!(run["state"], "proposed");
    assert!(what.contains("add_note on notes") && what.contains("Call the printer"), "the card shows what it would send: {what}");
    (s, id)
}

/// A tool that may change something is proposed, and called once the person approves it: its
/// card waits for the server, then the chat goes on with what it answered.
#[test]
fn an_mcp_tool_that_changes_something_is_called_once_approved() {
    let (mut s, id) = proposing("script-mcp-approved");
    s.run("assistant.decide", &json!({"chat": id, "approve": true})).unwrap();
    let (called, asked) = queued(&mut s);
    assert!(asked.is_empty(), "the chat waits for the server");
    assert_eq!(called.len(), 1, "{called:?}");
    assert_eq!((called[0].1.as_str(), &called[0].2), ("add_note", &json!({"text": "Call the printer"})));
    assert_eq!(run_of(&mut s, &id, "mcp")["state"], "waiting");
    s.apply(Event::Mcp { done: McpDone::Called { id: called[0].0, result: Some(json!("Added note 7")), error: None } });
    assert_eq!(run_of(&mut s, &id, "mcp")["state"], "done");
    let (_, back) = question(&mut s);
    let r = &results(&back)[0].content;
    assert!(r.contains("approved. Added note 7"), "{r}");
}

/// One declined is never called; one whose server was turned off before it was approved, nor;
/// and one stopped while it waited lets the server's answer go.
#[test]
fn an_mcp_tool_declined_turned_off_or_stopped_is_left() {
    let (mut s, id) = proposing("script-mcp-declined");
    s.run("assistant.decide", &json!({"chat": id, "approve": false})).unwrap();
    let (called, asked) = queued(&mut s);
    assert!(called.is_empty() && asked.len() == 1, "{called:?}");
    assert_eq!(run_of(&mut s, &id, "mcp")["state"], "declined");

    let (mut s, id) = proposing("script-mcp-off");
    let notes = s.prefs.ai.mcp[0].id.clone();
    s.run("mcpServer.turn", &json!({"id": notes, "on": false})).unwrap();
    s.take_jobs();
    s.run("assistant.decide", &json!({"chat": id, "approve": true})).unwrap();
    let (called, asked) = queued(&mut s);
    assert!(called.is_empty() && asked.len() == 1, "{called:?}");
    let run = run_of(&mut s, &id, "mcp");
    assert!(run["state"] == "failed" && run["result"].as_str().unwrap().contains("is off"), "{run}");

    let (mut s, id) = proposing("script-mcp-stopped");
    s.run("assistant.decide", &json!({"chat": id, "approve": true})).unwrap();
    let (called, _) = queued(&mut s);
    s.run("assistant.stop", &json!({"chat": id})).unwrap();
    s.take_jobs();
    s.apply(Event::Mcp { done: McpDone::Called { id: called[0].0, result: Some(json!("late")), error: None } });
    let (called, asked) = queued(&mut s);
    assert!(called.is_empty() && asked.is_empty(), "nothing asked once stopped");
    let run = run_of(&mut s, &id, "mcp");
    assert_eq!((run["state"].as_str(), run["result"].as_str()), (Some("failed"), Some("stopped before the server answered")));
}
