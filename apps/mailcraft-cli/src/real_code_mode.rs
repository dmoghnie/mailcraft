//! Code mode measured against the JSON tools, with a real model on this computer and the sample
//! mailbox, run by hand:
//!
//! ```text
//! MAILCRAFT_REAL_CODE_MODE="http://localhost:11434 gemma4:12b" \
//!   cargo test -p mailcraft-cli --release real_code_mode -- --ignored --nocapture
//! ```
//!
//! The model (through Ollama's own API) is given twenty tasks on the sample mailbox, each in a
//! chat of its own, twice: with the JSON tools alone, then with code mode on (`run_script`
//! beside them). A third word, `json` or `code`, runs one half alone (`both`: both), and a fourth, the tasks'
//! numbers (`16,18`), those alone. For each task: whether it
//! was done (the answer says what the mail says, or the change asked for is proposed), how many
//! times the model was asked, the tool calls, and the tokens its server counted. Nothing is
//! approved: a change proposed ends the task.

use super::*;
use mailcraft_engine::model::AiDone;

/// How long one task may take.
const TASK_WAIT: Duration = Duration::from_secs(10 * 60);

/// What a task must come to.
enum Want {
    /// The answer says, for each group, one of its words (case aside).
    Says(&'static [&'static [&'static str]]),
    /// A change proposed by `tool`, its line saying `says` and not `not`, for `count` messages
    /// at least, in all.
    Proposes { tool: &'static str, says: &'static str, not: &'static str, count: usize },
}

struct Task {
    ask: &'static str,
    want: Want,
}

const fn says(ask: &'static str, words: &'static [&'static [&'static str]]) -> Task {
    Task { ask, want: Want::Says(words) }
}

const fn proposes(ask: &'static str, tool: &'static str, says: &'static str, not: &'static str, count: usize) -> Task {
    Task { ask, want: Want::Proposes { tool, says, not, count } }
}

/// The tasks, on the sample mailbox (`mailcraft_engine::sample`): finding and reading first,
/// then those that look across several messages, then changes.
const TASKS: &[Task] = &[
    says("How many unread messages are in my Inbox?", &[&["3", "three"]]),
    says("Who wants me to bring revised storyboard frames, and for which scenes?", &[&["zoe"], &["4"], &["7"]]),
    says("From what time is the screening room booked for the storyboard review?", &[&["2 pm", "2pm", "14:00", "2:00", "2 p.m"]]),
    says("What is the order number of the festival poster proof?", &[&["4471"]]),
    says("How many copies of the festival poster did I order?", &[&["40", "forty"]]),
    says("When will the render farm be offline?", &[&["saturday"], &["sunday"]]),
    says("What size and finish is the festival poster proof?", &[&["a2"], &["matte"]]),
    says("Which messages in my Inbox are flagged?", &[&["storyboard"]]),
    says("What did Ana change in version 3 of the sound mix?", &[&["music", "dialogue"], &["rain"]]),
    says("What running time was agreed at the short film's kick-off?", &[&["8 min", "eight min"]]),
    says("How many lines do the Arabic subtitles have?", &[&["128"]]),
    says("What is the third shot in the shot list?", &[&["lantern"]]),
    says(
        "Until when can I submit work to the winter group show, and how many works may I submit?",
        &[&["30 november", "november 30", "end of the month"], &["3", "three"]],
    ),
    says("Who has written to me in my Inbox in the past seven days? Give their names.", &[&["zoe"], &["sam"], &["ana"], &["jun"]]),
    says("Which messages in my Inbox from people, not shops or newsletters, are still unread? Name who sent each.", &[&["zoe"], &["sam"], &["ana"]]),
    proposes("Flag Ana's message about the sound mix.", "mark_messages", "flagged", "not flagged", 1),
    proposes("Mark every unread message in my Inbox as read.", "mark_messages", "read", "unread", 3),
    proposes("Move the message about the render farm maintenance to the Archive.", "move_messages", "archive", "", 1),
    proposes("Delete the junk message about winning a camera.", "delete_messages", "", "", 1),
    proposes("Write a reply to the Print Shop saying the festival poster proof is approved.", "write_message", "", "", 0),
];

/// What one task came to.
#[derive(Default)]
struct Measured {
    done: bool,
    /// Answers the model gave: the chat's, and a script's own questions.
    asked: u32,
    calls: usize,
    scripts: usize,
    input: u64,
    output: u64,
    secs: f64,
    /// Why it wasn't done: its error, or the start of its answer.
    why: String,
}

/// The model's address and name, and the half and the tasks to run, if not all.
struct Setting {
    url: String,
    model: String,
    only: Option<String>,
    tasks: Vec<usize>,
}

fn setting() -> Option<Setting> {
    let v = std::env::var("MAILCRAFT_REAL_CODE_MODE").ok()?;
    let mut words = v.split_whitespace();
    let (url, model) = (words.next()?.to_string(), words.next()?.to_string());
    let only = words.next().map(str::to_string);
    let tasks = words.next().map(|n| n.split(',').filter_map(|n| n.trim().parse().ok()).collect()).unwrap_or_default();
    Some(Setting { url, model, only, tasks })
}

/// A copy of MailCraft on the sample mailbox, asking `model` at `url`, code mode on or not.
fn open(url: &str, model: &str, scripts: bool) -> (Session, Pool, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("mailcraft-real-code-mode-{scripts}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let c = clock();
    mailcraft_engine::sample::seed_if_empty_at(&store, c.now, i64::from(c.offset)).unwrap();
    let mut s = Session::open(Some(store.clone()), c);
    let profile = json!({"name": "Measured", "api": "ollama", "url": url, "model": model, "scripts": scripts});
    let id = s.run("ai.saveProfile", &profile).unwrap()["id"].as_str().unwrap().to_string();
    s.run("ai.use", &json!({"task": "chat", "profile": id})).unwrap();
    let account = s.accounts.first().map(|a| a.config.id.clone()).unwrap();
    s.run("ai.allow", &json!({"account": account, "reach": "local"})).unwrap();
    s.take_jobs();
    (s, Pool::new(store, Arc::new(MemoryCredentials::default())), dir)
}

/// The scripts kept (the shipped ones), offered as tools of their own in code mode.
fn kept(s: &mut Session) -> Vec<String> {
    let listed = s.run("script.list", &json!({})).unwrap_or_default();
    listed["scripts"].as_array().map(|all| all.iter().filter_map(|k| k["name"].as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// Task `t` given in a chat of its own, followed until the chat stops: its answer given, a
/// change proposed, or an error. A call to `run_script` or to a script in `kept` is a script's.
fn run(s: &mut Session, pool: &mut Pool, t: &Task, kept: &[String]) -> Measured {
    let start = Instant::now();
    let mut m = Measured::default();
    let chat = match s.run("assistant.send", &json!({"text": t.ask})) {
        Ok(v) => v["chat"].as_str().unwrap_or_default().to_string(),
        Err(e) => return Measured { why: format!("not asked: {e}"), ..m },
    };
    let mut shown = s.run("assistant.chat", &json!({"chat": chat})).unwrap_or_default();
    let mut ended = false;
    for j in s.take_jobs() {
        pool.submit(j);
    }
    while !ended && start.elapsed() < TASK_WAIT {
        let Some(e) = pool.wait(Duration::from_millis(250)) else { continue };
        if let Event::Ai { done: AiDone::Answered { answer: Some(a), .. } } = &e {
            m.asked += 1;
            m.input += a.input_tokens.unwrap_or(0);
            m.output += a.output_tokens.unwrap_or(0);
        }
        s.apply(e);
        for j in s.take_jobs() {
            pool.submit(j);
        }
        shown = s.run("assistant.chat", &json!({"chat": chat})).unwrap_or_default();
        let waiting = shown["chat"]["runs"].as_array().is_some_and(|r| r.iter().any(|r| r["state"] == "waiting"));
        ended = !shown["asking"].as_bool().unwrap_or(false) && !waiting;
    }
    m.secs = start.elapsed().as_secs_f64();
    let runs = shown["chat"]["runs"].as_array().cloned().unwrap_or_default();
    m.calls = runs.len();
    m.scripts = runs.iter().filter(|r| r["tool"] == "run_script" || kept.iter().any(|k| r["tool"] == k.as_str())).count();
    let answer = shown["chat"]["turns"]
        .as_array()
        .and_then(|turns| turns.iter().rev().find(|t| t["role"] == "assistant" && t["text"].as_str().is_some_and(|x| !x.trim().is_empty())))
        .and_then(|t| t["text"].as_str())
        .unwrap_or_default()
        .to_lowercase();
    m.done = ended
        && match &t.want {
            Want::Says(groups) => groups.iter().all(|g| g.iter().any(|w| answer.contains(w))),
            Want::Proposes { tool, says, not, count } => {
                let theirs: Vec<&Value> = runs
                    .iter()
                    .filter(|r| r["state"] == "proposed" && r["tool"] == *tool)
                    .filter(|r| {
                        let what = r["what"].as_str().unwrap_or_default().to_lowercase();
                        what.contains(says) && (not.is_empty() || !what.contains(not))
                    })
                    .collect();
                let messages: usize = theirs.iter().map(|r| r["messages"].as_array().map_or(0, Vec::len)).sum();
                !theirs.is_empty() && messages >= *count
            }
        };
    if !m.done {
        let error = shown["chat"]["error"].as_str().map(str::to_string);
        // The model's last turn: whether it was cut off, how much it thought, what it called.
        let last = shown["chat"]["turns"].as_array().and_then(|t| t.iter().rev().find(|t| t["role"] == "assistant")).cloned().unwrap_or_default();
        let calls: Vec<String> = last["calls"]
            .as_array()
            .map(|c| c.iter().map(|c| format!("{}({} bytes)", c["name"].as_str().unwrap_or_default(), c["input"].to_string().len())).collect())
            .unwrap_or_default();
        let last = format!(
            "[last turn: {}said {} chars {:?}, thought {} chars, called {}] ",
            if last["cut"].as_bool().unwrap_or(false) { "cut off, " } else { "" },
            last["text"].as_str().map_or(0, |t| t.chars().count()),
            last["text"].as_str().unwrap_or_default().chars().take(40).collect::<String>(),
            last["thinking"].as_str().map_or(0, |t| t.chars().count()),
            if calls.is_empty() { "nothing".to_string() } else { calls.join(", ") }
        );
        let proposed: Vec<&str> = runs.iter().filter(|r| r["state"] == "proposed").filter_map(|r| r["what"].as_str()).collect();
        m.why = match (ended, error) {
            (false, _) => "timed out".into(),
            (_, Some(e)) => format!("{last}error: {e}"),
            _ if !proposed.is_empty() => format!("{last}proposed: {}", proposed.join("; ")),
            _ => format!("{last}answered: {}", answer.chars().take(160).collect::<String>().replace('\n', " ")),
        };
    }
    m
}

/// Every task with code mode on or off, a line each, then the totals.
fn half(url: &str, model: &str, scripts: bool, tasks: &[usize]) {
    let mode = if scripts { "code" } else { "json" };
    let (mut s, mut pool, dir) = open(url, model, scripts);
    let kept = kept(&mut s);
    let mut all = Vec::new();
    for (n, t) in TASKS.iter().enumerate().filter(|(n, _)| tasks.is_empty() || tasks.contains(&(n + 1))) {
        let m = run(&mut s, &mut pool, t, &kept);
        println!(
            "{mode}\t{:2}\t{}\tasked {}\tcalls {}\tscripts {}\tin {}\tout {}\t{:.0} s\t{}",
            n + 1,
            if m.done { "done" } else { "NOT" },
            m.asked,
            m.calls,
            m.scripts,
            m.input,
            m.output,
            m.secs,
            m.why
        );
        all.push(m);
    }
    let done = all.iter().filter(|m| m.done).count();
    let sum = |f: &dyn Fn(&Measured) -> f64| all.iter().map(f).sum::<f64>();
    let n = all.len() as f64;
    println!(
        "{mode} {model}: {done}/{} done; per task: asked {:.1}, calls {:.1}, tokens in {:.0}, out {:.0}, {:.0} s; scripts {}",
        all.len(),
        sum(&|m| f64::from(m.asked)) / n,
        sum(&|m| m.calls as f64) / n,
        sum(&|m| m.input as f64) / n,
        sum(&|m| m.output as f64) / n,
        sum(&|m| m.secs) / n,
        all.iter().map(|m| m.scripts).sum::<usize>(),
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[ignore = "a measurement: needs a model on this computer, named in MAILCRAFT_REAL_CODE_MODE"]
fn real_code_mode() {
    let Some(set) = setting() else {
        println!("MAILCRAFT_REAL_CODE_MODE isn't set: \"<Ollama's address> <model> [json|code|both] [task numbers]\"");
        return;
    };
    for scripts in [false, true] {
        let mode = if scripts { "code" } else { "json" };
        if set.only.as_deref().is_none_or(|o| o == mode || o == "both") {
            half(&set.url, &set.model, scripts, &set.tasks);
        }
    }
}
