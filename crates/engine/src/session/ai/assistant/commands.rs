//! The assistant's commands: the chats kept, saying something in one, deciding on what it
//! proposed, stopping it, choosing its model, and deleting chats.

use serde_json::{Value, json};

use super::context::ITEMS_MOST;
use super::*;
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const WHERE: &str = "AI assistant";
/// The longest selected part pointed at, in bytes.
pub const QUOTE_MOST: usize = 64 << 10;

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("assistant.chats", "List the chats with MailCraft's AI", WHERE, |s, _| {
            s.assistant_load();
            let chats: Vec<Value> = s
                .ai
                .assistant
                .heads()
                .map(|h| json!({"id": h.id, "title": h.title, "created": h.created, "updated": h.updated, "asking": s.ai.assistant.asking(&h.id).is_some()}))
                .collect();
            Ok(json!({"chats": chats}))
        }),
        CommandSpec::new("assistant.pane", "Ask AI", "Home", |s, v| {
            let pane = &mut s.prefs.assistant_pane;
            pane.shown = p::bool(v, "on").unwrap_or(!pane.shown || v.get("width").is_some());
            if let Some(w) = p::u64(v, "width") {
                let (least, most) = (*ASSISTANT_WIDTH.start(), *ASSISTANT_WIDTH.end());
                pane.width = u16::try_from(w).unwrap_or(most).clamp(least, most);
            }
            Ok(json!({"shown": pane.shown, "width": pane.width}))
        })
        .params(r#"{"on"?: bool (shown or hidden; not given: the other way, or shown when a width is given), "width"?: number}"#),
        CommandSpec::new("assistant.chat", "Show a chat with MailCraft's AI", WHERE, show).params(r#"{"chat": string}"#),
        CommandSpec::new("assistant.open", "Open what an answer of MailCraft's AI links to", WHERE, open).params(r#"{"chat": string, "link": string (as the answer has it: mailcraft:m0123456789)} (an event or a person: gives {"event": {"uid", "occurrence"}} or {"person": uid} for the window to open)"#),
        CommandSpec::new("assistant.send", "Say something to MailCraft's AI", WHERE, send).params(
            r#"{"chat"?: string (none: a new chat), "text": string, "context"?: [{"kind": "message", "id": string} | {"kind": "quote", "id": string, "text": string (the part selected)} | {"kind": "folder", "account": string, "folder": string}], "profile"?: string (the model the chat asks from now on; "": the one chat work goes to)}"#,
        ),
        CommandSpec::new("assistant.decide", "Approve or decline what MailCraft's AI proposed", WHERE, |s, v| {
            let chat = p::req_str(v, "chat")?.to_string();
            let approve = p::bool(v, "approve").ok_or_else(|| CmdError::Params("`approve` (bool) is required".into()))?;
            let decided = s.assistant_decide(&chat, p::str(v, "call"), approve)?;
            Ok(json!({"decided": decided}))
        })
        .params(r#"{"chat": string, "call"?: string (none: everything it proposed that waits), "approve": bool}"#),
        CommandSpec::new("assistant.stop", "Stop MailCraft's AI in a chat", WHERE, |s, v| {
            let chat = p::req_str(v, "chat")?.to_string();
            Ok(json!({"stopped": s.assistant_stop(&chat)?}))
        })
        .params(r#"{"chat": string}"#),
        CommandSpec::new("assistant.model", "Choose the model a chat asks", WHERE, |s, v| {
            let chat = p::req_str(v, "chat")?.to_string();
            let profile = match p::req_str(v, "profile")? {
                "" => None,
                id => Some(s.prefs.ai.profile(id).map(|p| p.id.clone()).ok_or_else(|| CmdError::Params("no such model profile".into()))?),
            };
            s.assistant_chat(&chat)?.profile.clone_from(&profile);
            s.assistant_changed(&chat);
            Ok(json!({"profile": profile}))
        })
        .params(r#"{"chat": string, "profile": string ("": the one chat work goes to)}"#),
        CommandSpec::new("assistant.delete", "Delete a chat with MailCraft's AI", WHERE, |s, v| {
            let chat = p::req_str(v, "chat")?.to_string();
            s.assistant_delete(&chat)?;
            Ok(json!({"deleted": chat}))
        })
        .params(r#"{"chat": string}"#),
        CommandSpec::new("assistant.deleteAll", "Delete every chat with MailCraft's AI", WHERE, |s, _| {
            s.assistant_load();
            let ids: Vec<String> = s.ai.assistant.heads().map(|h| h.id.clone()).collect();
            for id in &ids {
                s.assistant_delete(id)?;
            }
            Ok(json!({"deleted": ids.len()}))
        }),
    ]
}

/// A chat as the window shows it: its turns, each call's card, and what's come so far of the
/// answer it waits for.
fn show(s: &mut Session, v: &Value) -> CmdResult {
    let id = p::req_str(v, "chat")?.to_string();
    let chat = s.assistant_chat(&id)?.clone();
    let asking = s.ai.assistant.asking(&id);
    let part = asking.and_then(|q| s.ai.part(q)).map(|(text, thinking)| json!({"text": text, "thinking": thinking}));
    let profile = s.assistant_profile(&chat).ok().map(|p| json!({"id": p.id, "name": p.name, "model": p.endpoint.model, "local": p.local()}));
    Ok(json!({"chat": chat, "asking": asking.is_some(), "part": part, "waitsOnYou": chat.waits_on_person(), "profile": profile}))
}

/// A message an answer links to, gone to: Mail, its folder, and it open. Only one the chat has
/// seen: a link is written by the model, from mail others wrote.
fn open(s: &mut Session, v: &Value) -> CmdResult {
    let id = p::req_str(v, "chat")?.to_string();
    let link = p::req_str(v, "link")?;
    let linked = s.assistant_chat(&id)?.linked(link);
    match linked.ok_or_else(|| CmdError::Failed("MailCraft's AI linked to something it hasn't found in this chat".into()))? {
        Linked::Message(key) => crate::cmd::go_to_message(s, &key),
        Linked::Found(Found::Event { account, uid, occurrence, start, .. }) => crate::cmd::go_to_event(s, &account, &uid, occurrence, start),
        Linked::Found(Found::Person { account, uid }) => crate::cmd::go_to_person(s, &account, &uid),
    }
}

fn send(s: &mut Session, v: &Value) -> CmdResult {
    let text = p::str(v, "text").unwrap_or_default().to_string();
    let context = match v.get("context") {
        None | Some(Value::Null) => Vec::new(),
        Some(c) => context_of(c)?,
    };
    let profile = p::str(v, "profile").map(|p| Some(p.to_string()).filter(|p| !p.is_empty()));
    let chat = s.assistant_send(p::str(v, "chat"), &text, &context, profile)?;
    Ok(json!({"chat": chat}))
}

/// The things pointed at, checked as they're given.
fn context_of(v: &Value) -> Result<Vec<ContextItem>, CmdError> {
    let items = v.as_array().ok_or_else(|| CmdError::Params("`context` must be a list".into()))?;
    if items.len() > ITEMS_MOST {
        return Err(CmdError::Params(format!("at most {ITEMS_MOST} things can be pointed at")));
    }
    items
        .iter()
        .map(|i| {
            let item: ContextItem =
                serde_json::from_value(i.clone()).map_err(|_| CmdError::Params("each thing pointed at is a message, a quote or a folder".into()))?;
            if let ContextItem::Quote { text, .. } = &item
                && (text.trim().is_empty() || text.len() > QUOTE_MOST)
            {
                return Err(CmdError::Params("a quote's `text` is what was selected, at most 64 KB".into()));
            }
            Ok(item)
        })
        .collect()
}
