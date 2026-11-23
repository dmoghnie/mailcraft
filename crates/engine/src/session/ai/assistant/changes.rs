//! The changes the assistant proposes: each checked as the model asks for it, said in a line
//! for the person's card, and made only once they approve it, checked again then (the messages
//! may have moved since). Each acts on the messages it names, never on the window's selection,
//! and is undone with Undo as the window's own changes are. A message written is opened for the
//! person to send.

use mailcraft_mime::ReplyMode;
use mailcraft_model::{Flag, FolderRole, MessageKey, Op, addr};
use serde_json::Value;

use super::looking::said;
use super::*;
use crate::Session;
use crate::session::MoveTarget;

/// The most messages a change names.
pub(super) const IDS_MOST: usize = 200;
/// The longest message the assistant writes, in bytes.
const BODY_MOST: usize = 256 << 10;

/// A change, checked.
pub(super) enum Change {
    Move { keys: Vec<MessageKey>, to: MoveTarget },
    Delete { keys: Vec<MessageKey> },
    Mark { keys: Vec<MessageKey>, read: Option<bool>, flagged: Option<bool> },
    Categorize { keys: Vec<MessageKey>, name: String, keyword: String, on: bool },
    Write { to: String, cc: String, subject: String, body: String, reply_to: Option<MessageKey>, account: Option<String> },
}

impl Change {
    /// The messages it changes, by id.
    pub(super) fn ids(&self) -> Vec<String> {
        match self {
            Change::Move { keys, .. } | Change::Delete { keys } | Change::Mark { keys, .. } | Change::Categorize { keys, .. } => {
                keys.iter().map(MessageKey::to_id).collect()
            }
            Change::Write { reply_to, .. } => reply_to.iter().map(MessageKey::to_id).collect(),
        }
    }

    /// What it does, in a line for its card.
    pub(super) fn what(&self, s: &mut Session) -> String {
        let which = |s: &mut Session, keys: &[MessageKey]| match keys {
            [one] => s.shared_of(one).map_or_else(|| "1 message".into(), |m| format!("“{}”", m.subject.chars().take(80).collect::<String>())),
            many => format!("{} messages", many.len()),
        };
        match self {
            Change::Move { keys, to } => {
                let place = match to {
                    MoveTarget::Role(r) => r.label().unwrap_or("its folder").to_string(),
                    MoveTarget::Folder(f) => keys.first().and_then(|k| s.folder(&k.account, f)).map_or_else(|| f.clone(), |f| f.display_name()),
                };
                format!("Move {} to {place}", which(s, keys))
            }
            Change::Delete { keys } => format!("Delete {}", which(s, keys)),
            Change::Mark { keys, read, flagged } => {
                let mut how = Vec::new();
                if let Some(r) = read {
                    how.push(if *r { "read" } else { "unread" });
                }
                if let Some(f) = flagged {
                    how.push(if *f { "flagged" } else { "not flagged" });
                }
                format!("Mark {} {}", which(s, keys), how.join(" and "))
            }
            Change::Categorize { keys, name, on: true, .. } => format!("Put {} in category {name}", which(s, keys)),
            Change::Categorize { keys, name, on: false, .. } => format!("Take {} out of category {name}", which(s, keys)),
            Change::Write { reply_to: Some(k), .. } => format!("Answer {}", which(s, std::slice::from_ref(k))),
            Change::Write { to, subject, .. } => {
                let to = to.trim().trim_end_matches(';');
                let to = if to.is_empty() { String::new() } else { format!(" to {to}") };
                let about = if subject.trim().is_empty() { String::new() } else { format!(": {}", subject.trim()) };
                format!("Write a message{to}{about}")
            }
        }
    }
}

impl Session {
    /// The change tool `name` asks for with `input`, checked; why not, for the model.
    pub(super) fn change_of(&mut self, chat: &Chat, name: &str, input: &Value) -> Result<Change, String> {
        let s = |k: &str| input.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
        let b = |k: &str| input.get(k).and_then(Value::as_bool);
        match name {
            "move_messages" => {
                let keys = self.keys_of(chat, input)?;
                let folder = s("folder").ok_or("`folder` is required")?;
                let to = match FolderRole::from_code(folder)
                    .filter(|r| matches!(r, FolderRole::Inbox | FolderRole::Archive | FolderRole::Junk | FolderRole::Trash))
                {
                    Some(r) => MoveTarget::Role(r),
                    None => MoveTarget::Folder(folder.to_string()),
                };
                for k in &keys {
                    let there = match &to {
                        MoveTarget::Role(r) => self.account(&k.account).and_then(|a| a.folder(*r)).is_some(),
                        MoveTarget::Folder(f) => self.folder(&k.account, f).is_some_and(|f| f.selectable),
                    };
                    if !there {
                        return Err(format!("account {} has no folder `{folder}`: list_folders names them", k.account));
                    }
                }
                Ok(Change::Move { keys, to })
            }
            "delete_messages" => {
                let keys = self.keys_of(chat, input)?;
                if let Some(k) = keys.iter().find(|k| self.in_role(k, FolderRole::Trash)) {
                    return Err(format!("{} is in Deleted Items already: MailCraft's AI doesn't delete mail for good", k.to_id()));
                }
                Ok(Change::Delete { keys })
            }
            "mark_messages" => {
                let keys = self.keys_of(chat, input)?;
                let (read, flagged) = (b("read"), b("flagged"));
                if read.is_none() && flagged.is_none() {
                    return Err("say `read` or `flagged`".into());
                }
                Ok(Change::Mark { keys, read, flagged })
            }
            "categorize_messages" => {
                let keys = self.keys_of(chat, input)?;
                let name = s("category").ok_or("`category` is required")?;
                let c = self.category(name).ok_or_else(|| format!("there's no category {name}: the person has {}", self.category_names()))?;
                Ok(Change::Categorize { keys, name: c.name.clone(), keyword: c.keyword.clone(), on: b("on").unwrap_or(true) })
            }
            "write_message" => self.write_of(chat, input),
            other => Err(format!("there's no tool `{}`", other.chars().take(64).collect::<String>())),
        }
    }

    fn write_of(&mut self, chat: &Chat, input: &Value) -> Result<Change, String> {
        let s = |k: &str| input.get(k).and_then(Value::as_str).map(str::trim).unwrap_or_default().to_string();
        let body = input.get("body").and_then(Value::as_str).unwrap_or_default().to_string();
        if body.trim().is_empty() {
            return Err("`body` is required".into());
        }
        if body.len() > BODY_MOST {
            return Err("that's too long for a message".into());
        }
        for field in ["to", "cc"] {
            let v = s(field);
            if !v.is_empty() {
                addr::parse_list(&v).map_err(|bad| format!("`{field}`: not an email address: {bad}"))?;
            }
        }
        let reply_to = match input.get("reply_to").and_then(Value::as_str).filter(|v| !v.is_empty()) {
            Some(id) => {
                let k = chat.message_named(id).ok_or("`reply_to` must be a message's id, as the tools give it")?;
                self.shared_of(&k).ok_or_else(|| format!("there's no message {id} (any more)"))?;
                Some(k)
            }
            None => None,
        };
        let account = Some(s("account")).filter(|a| !a.is_empty());
        if let Some(a) = &account
            && self.account(a).is_none()
        {
            return Err(format!("there's no account {a}"));
        }
        Ok(Change::Write { to: s("to"), cc: s("cc"), subject: s("subject"), body, reply_to, account })
    }

    /// The messages a change names (`ids`), each there.
    fn keys_of(&mut self, chat: &Chat, input: &Value) -> Result<Vec<MessageKey>, String> {
        let ids = input.get("ids").and_then(Value::as_array).ok_or("`ids` (the messages' ids) is required")?;
        if ids.is_empty() || ids.len() > IDS_MOST {
            return Err(format!("name between 1 and {IDS_MOST} messages"));
        }
        let mut keys: Vec<MessageKey> = Vec::new();
        for id in ids {
            let id = id.as_str().ok_or("each id is a string")?;
            let k =
                chat.message_named(id).ok_or_else(|| format!("`{}` isn't a message's id the tools gave", id.chars().take(80).collect::<String>()))?;
            if self.shared_of(&k).is_none() {
                return Err(format!("there's no message {id} (any more)"));
            }
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        Ok(keys)
    }

    fn in_role(&self, k: &MessageKey, role: FolderRole) -> bool {
        self.account(&k.account).and_then(|a| a.folder(role)).is_some_and(|f| f.path == k.folder)
    }

    fn category_names(&self) -> String {
        let names: Vec<&str> = self.prefs.categories.iter().map(|c| c.name.as_str()).collect();
        if names.is_empty() { "none".into() } else { names.join(", ") }
    }

    /// Make `change`, approved: what was done, or why it couldn't be.
    pub(super) fn apply_change(&mut self, change: Change) -> Result<String, String> {
        match change {
            Change::Move { keys, to } => self.move_named(&keys, &to),
            Change::Delete { keys } => self.move_named(&keys, &MoveTarget::Role(FolderRole::Trash)),
            Change::Mark { keys, read, flagged } => {
                let sel = self.shared_all(&keys)?;
                if let Some(on) = read {
                    self.mark_selection(&sel, on, |m| Flag::Seen.get(&m.flags), |keys, on| Op::SetFlag { keys, flag: Flag::Seen, on });
                }
                if let Some(on) = flagged {
                    self.flag_messages(&sel, on).map_err(|e| said(&e))?;
                }
                Ok(format!("Done: {} marked.", sel.len()))
            }
            Change::Categorize { keys, name, keyword, on } => {
                let sel = self.shared_all(&keys)?;
                let kw = keyword.clone();
                self.mark_selection(&sel, on, move |m| m.flags.has(&kw), move |keys, on| Op::SetKeyword { keys, keyword: keyword.clone(), on });
                Ok(format!("Done: {} {} category {name}.", sel.len(), if on { "put in" } else { "taken out of" }))
            }
            Change::Write { to, cc, subject, body, reply_to, account } => self.write_named(to, cc, subject, body, reply_to, account),
        }
    }

    /// The messages `keys`, those still there; none: an error.
    fn shared_all(&mut self, keys: &[MessageKey]) -> Result<Vec<SharedSummary>, String> {
        let sel: Vec<SharedSummary> = keys.iter().filter_map(|k| self.shared_of(k)).collect();
        if sel.is_empty() { Err("those messages aren't there any more".into()) } else { Ok(sel) }
    }

    /// Move `keys` (one open or selected in the window gives way to the next, as when the
    /// window moves it).
    fn move_named(&mut self, keys: &[MessageKey], to: &MoveTarget) -> Result<String, String> {
        let sel = self.shared_all(keys)?;
        let in_window: Vec<MessageKey> =
            keys.iter().filter(|k| self.selection.contains(k) || self.reading.as_ref().is_some_and(|r| &r.key == *k)).cloned().collect();
        let next = if in_window.is_empty() { None } else { self.successor(&in_window) };
        let moved = self.move_messages(&sel, to).map_err(|e| said(&e))?;
        if !in_window.is_empty() && moved > 0 {
            self.after_removal(next);
        }
        Ok(match moved {
            0 => "Nothing moved: they were there already.".into(),
            n => format!("Done: {n} moved."),
        })
    }

    /// A message written, opened for the person to check and send: an answer to `reply_to`
    /// when it's given.
    fn write_named(
        &mut self,
        to: String,
        cc: String,
        subject: String,
        body: String,
        reply_to: Option<MessageKey>,
        account: Option<String>,
    ) -> Result<String, String> {
        let answered = match &reply_to {
            Some(k) => Some(self.shared_of(k).ok_or("the message it answers isn't there any more")?),
            None => None,
        };
        let account = account.or_else(|| reply_to.as_ref().map(|k| self.sending_account(k.account.clone())));
        let c = self.new_compose(account.as_deref()).map_err(|e| said(&e))?;
        let to = match (&answered, to.is_empty()) {
            (Some(m), true) => m.from.to_string(),
            _ => to,
        };
        if let Ok(list) = addr::parse_list(&to) {
            c.to = addr::format_field(&list);
        }
        if let Ok(list) = addr::parse_list(&cc) {
            c.cc = addr::format_field(&list);
        }
        c.subject = match &answered {
            Some(m) if subject.is_empty() => {
                if m.subject.to_ascii_lowercase().starts_with("re:") {
                    m.subject.clone()
                } else {
                    format!("Re: {}", m.subject)
                }
            }
            _ => subject,
        };
        if let Some(m) = &answered {
            c.in_reply_to.clone_from(&m.message_id);
            c.references = m.references.iter().cloned().chain(std::iter::once(m.message_id.clone()).filter(|id| !id.is_empty())).collect();
            c.source = Some(m.key.clone());
            c.mode = Some(ReplyMode::Reply);
        }
        let text = format!("{}\n", body.trim_end());
        let styles = mailcraft_mime::rich::Styles::plain(&text);
        mailcraft_mime::rich::splice(&mut c.body, &mut c.styles, 0..0, &text, &styles);
        Ok("Opened for the person to check and send.".into())
    }
}
