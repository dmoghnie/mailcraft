//! What the start of a chat held, said when it's left out for room: what the person asked, and
//! what the tools found and changed, by name and outcome. Made from the chat itself, never asked
//! of a model: the model keeps the names of what it found (to link them, without looking again)
//! and how each change it proposed was decided (not to propose again what was declined).

use super::links::{Found, message_name};
use super::*;

/// The most names said for what one call found.
const NAMES_MOST: usize = 30;
/// The most of what the person asked said for a turn, in characters.
const ASKED_CHARS: usize = 200;
/// What the ledger begins with, said alone when there's no room for more.
const HEADING: &str = "The start of this chat was left out for room. What it held, oldest first:";

/// What `chat`'s first `dropped` turns held, in `most` bytes at most: the newest lines kept.
pub(super) fn ledger(chat: &Chat, dropped: usize, most: usize) -> String {
    let lines: Vec<String> = chat.turns.iter().take(dropped).flat_map(|t| lines_of(chat, t)).collect();
    let mut left = most.saturating_sub(HEADING.len());
    let mut kept: Vec<&str> = Vec::new();
    for line in lines.iter().rev() {
        let size = line.len().saturating_add(1);
        if size > left {
            break;
        }
        left = left.saturating_sub(size);
        kept.push(line);
    }
    kept.reverse();
    let mut text = HEADING.to_string();
    for line in kept {
        text.push('\n');
        text.push_str(line);
    }
    text
}

/// The names of what `run` found or read, for the model to link: at most [`NAMES_MOST`].
pub(super) fn found_names(run: &ToolRun) -> String {
    let names: Vec<String> = run.messages.iter().map(|id| message_name(id)).chain(run.found.iter().map(Found::name)).collect();
    let shown = names.iter().take(NAMES_MOST).map(String::as_str).collect::<Vec<_>>().join(", ");
    match names.len().saturating_sub(NAMES_MOST) {
        0 => shown,
        more => format!("{shown} and {more} more"),
    }
}

/// A turn's lines: what the person asked, or what each call the model made came to (with the
/// changes a script of it proposed).
fn lines_of(chat: &Chat, t: &ChatTurn) -> Vec<String> {
    if t.turn.role == AiRole::User {
        let asked: String = t.turn.text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(ASKED_CHARS).collect();
        return if asked.is_empty() { Vec::new() } else { vec![format!("- The person asked: \"{asked}\"")] };
    }
    t.turn
        .calls
        .iter()
        .filter_map(|c| chat.run(&c.id))
        .flat_map(|run| std::iter::once(run).chain(changes_of(chat, &run.call)))
        .filter_map(line_of)
        .collect()
}

/// The changes a script, called as `call`, proposed.
fn changes_of<'a>(chat: &'a Chat, call: &'a str) -> impl Iterator<Item = &'a ToolRun> + 'a {
    chat.runs.iter().filter(move |r| r.part_of.as_deref() == Some(call) && r.state != RunState::Ran)
}

/// What a call came to, in a line; none for one still to be decided or made.
fn line_of(run: &ToolRun) -> Option<String> {
    let came = match run.state {
        RunState::Ran => match found_names(run) {
            names if names.is_empty() => "done".to_string(),
            names => format!("found {names}"),
        },
        RunState::Failed => "failed".into(),
        RunState::Done => "approved by the person, and made".into(),
        RunState::Declined => "declined, or not decided: nothing changed".into(),
        RunState::PutOff => "put off, to be proposed again with the others".into(),
        RunState::Waiting | RunState::Proposed => return None,
    };
    let what = if run.what.is_empty() { run.tool.as_str() } else { run.what.as_str() };
    Some(format!("- {}: {came}", what.split_whitespace().collect::<Vec<_>>().join(" ")))
}
