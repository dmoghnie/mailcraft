//! The assistant's tools that read: run at once, as the model calls them, changing nothing.
//! Each reads only mail of an account allowed to go to the chat's model; reading it puts that
//! account's mail in the chat. Finding never touches the window's list or search.

use mailcraft_model::{Flag, FolderRole, Job, MessageKey, Summary};
use serde_json::Value;

use super::context::{MESSAGE_CHARS, cut};
use super::links::message_name;
use super::tools::Outcome;
use super::*;
use crate::session::AccountState;
use crate::{CmdError, Session};

/// The most messages a search gives at once, and when the model doesn't say.
const FOUND_MOST: usize = 50;
const FOUND_AT_FIRST: usize = 20;
/// The most of a message's text a read gives, in characters.
const READ_CHARS_MOST: usize = 40_000;
/// The most folders listed for an account.
const FOLDERS_MOST: usize = 300;
/// The longest preview shown for a message found, in characters.
const PREVIEW_CHARS: usize = 160;

impl Session {
    /// A message's summary by its key: in the list shown, or in its folder's.
    pub(crate) fn shared_of(&mut self, key: &MessageKey) -> Option<SharedSummary> {
        if let Some(m) = self.index_of(key).and_then(|i| self.list.get(i)) {
            return Some(m.clone());
        }
        self.folder_state(&key.account, &key.folder).messages.iter().find(|m| &m.key == key).cloned()
    }

    /// A message's text as it reads, when its source is kept here.
    pub(super) fn message_text(&self, key: &MessageKey) -> Option<String> {
        let raw = self.body_raw(key)?;
        let p = mailcraft_mime::parse(&raw)?;
        let files: Vec<&str> = p.attachments.iter().map(|a| a.name.as_str()).filter(|n| !n.is_empty()).collect();
        let files = if files.is_empty() { String::new() } else { format!("\n\n[Files: {}]", files.join(", ")) };
        Some(format!("{}{files}", p.text.trim()))
    }

    /// Who wrote a message to whom, when, about what, and where it is.
    pub(super) fn message_head(&self, m: &Summary) -> String {
        let list = |l: &[mailcraft_model::EmailAddress]| l.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
        let mut head = format!("From: {}\nTo: {}", m.from, list(&m.to));
        if !m.cc.is_empty() {
            head.push_str(&format!("\nCc: {}", list(&m.cc)));
        }
        head.push_str(&format!(
            "\nDate: {}\nSubject: {}\nFolder: {}",
            mailcraft_model::dates::full_time(m.date, &self.clock),
            m.subject,
            self.folder_name(&m.key.account, &m.key.folder)
        ));
        head
    }

    pub(super) fn folder_name(&self, account: &str, folder: &str) -> String {
        self.folder(account, folder).map_or_else(|| folder.to_string(), |f| f.display_name())
    }

    /// A message found, in a line: its name first, as the tools take it.
    fn message_row(&self, m: &Summary) -> String {
        let to: Vec<String> = m.to.iter().take(3).map(ToString::to_string).collect();
        let more = if m.to.len() > 3 { format!(" and {} more", m.to.len() - 3) } else { String::new() };
        let mut marks = Vec::new();
        if !m.flags.seen {
            marks.push("unread".to_string());
        }
        if Flag::Flagged.get(&m.flags) {
            marks.push("flagged".into());
        }
        if m.has_attachments {
            marks.push("files".into());
        }
        marks.extend(self.prefs.categories.iter().filter(|c| m.flags.has(&c.keyword)).map(|c| format!("category {}", c.name)));
        let marks = if marks.is_empty() { String::new() } else { format!(" | {}", marks.join(", ")) };
        let preview: String = m.preview.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(PREVIEW_CHARS).collect();
        format!(
            "[{}] {} | From: {} | To: {}{more} | Subject: {} | {}{marks} | {preview}",
            message_name(&m.key.to_id()),
            mailcraft_model::dates::full_time(m.date, &self.clock),
            m.from,
            to.join(", "),
            m.subject,
            self.folder_name(&m.key.account, &m.key.folder)
        )
    }

    /// The accounts whose mail may go to `profile`.
    pub(super) fn readable_accounts(&self, profile: &AiProfile) -> Vec<&AccountState> {
        self.accounts.iter().filter(|a| self.prefs.ai.allows(profile, Some(&a.config.id))).collect()
    }

    /// Why mail of `account` can't be read for `profile`, when it can't.
    pub(super) fn unreadable(&self, account: &str, profile: &AiProfile) -> Option<String> {
        if self.account(account).is_none() {
            return Some(format!("there's no account {account}"));
        }
        (!self.prefs.ai.allows(profile, Some(account)))
            .then(|| format!("account {account}'s mail may not go to {}: the person decides that in File › Options › AI", profile.name))
    }

    /// `list_folders`: the accounts it may read, and their folders.
    pub(super) fn tool_list_folders(&mut self, profile: &AiProfile, input: &Value) -> Outcome {
        let only = input.get("account").and_then(Value::as_str).filter(|a| !a.is_empty());
        if let Some(why) = only.and_then(|a| self.unreadable(a, profile)) {
            return Outcome::failed("List folders", why);
        }
        let mut out = Vec::new();
        for a in self.readable_accounts(profile).into_iter().filter(|a| only.is_none_or(|o| o == a.config.id)) {
            out.push(format!("Account {} ({} <{}>):", a.config.id, a.config.name, a.config.email));
            for f in a.folders.iter().filter(|f| f.selectable).take(FOLDERS_MOST) {
                let role = if f.role == FolderRole::Other { String::new() } else { format!(" [{}]", f.role.code()) };
                out.push(format!("- {}{role}: {}, {} messages, {} unread", f.path, f.display_name(), f.total, f.unread));
            }
        }
        if out.is_empty() {
            out.push("No account's mail may go to this model.".into());
        }
        Outcome::ran("List folders", Vec::new(), out.join("\n"))
    }

    /// The folders a search or listing looks in: an account's (one of them, by its path, role or
    /// name), or every account's the model may read.
    fn folders_for(&self, profile: &AiProfile, account: Option<&str>, folder: Option<&str>) -> Result<Vec<(String, String)>, String> {
        let accounts: Vec<&AccountState> = match account {
            Some(a) => match self.unreadable(a, profile) {
                Some(why) => return Err(why),
                None => self.account(a).into_iter().collect(),
            },
            None => self.readable_accounts(profile),
        };
        let mut out = Vec::new();
        for a in accounts {
            let picked = a.folders.iter().filter(|f| f.selectable).filter(|f| {
                folder.is_none_or(|w| {
                    f.path == w
                        || f.display_name().eq_ignore_ascii_case(w)
                        || FolderRole::from_code(w).is_some_and(|r| r != FolderRole::Other && r == f.role)
                })
            });
            out.extend(picked.map(|f| (a.config.id.clone(), f.path.clone())));
        }
        if out.is_empty() {
            return Err(match folder {
                Some(f) => format!("no folder `{f}`: list_folders names them"),
                None => "no account's mail may go to this model".into(),
            });
        }
        Ok(out)
    }

    /// `find_messages`: a search (as the search box takes it), or the newest messages.
    pub(super) fn tool_find_messages(&mut self, profile: &AiProfile, input: &Value) -> Outcome {
        let s = |k: &str| input.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
        let query = s("query").unwrap_or_default();
        let what = if query.is_empty() {
            "List the newest messages".to_string()
        } else {
            format!("Find “{}”", query.chars().take(80).collect::<String>())
        };
        let most =
            input.get("limit").and_then(Value::as_u64).map_or(FOUND_AT_FIRST, |n| usize::try_from(n).unwrap_or(FOUND_MOST).clamp(1, FOUND_MOST));
        let folders = match self.folders_for(profile, s("account"), s("folder")) {
            Ok(f) => f,
            Err(why) => return Outcome::failed(&what, why),
        };
        let found = if query.is_empty() {
            let mut all: Vec<SharedSummary> = Vec::new();
            for (a, f) in &folders {
                all.extend(self.folder_state(a, f).messages.iter().cloned());
            }
            all.sort_by_key(|m| std::cmp::Reverse(m.date));
            let total = all.len();
            all.truncate(most);
            crate::session::FoundAside { found: all, total, unchecked: 0 }
        } else {
            self.search_aside(query, &folders, most)
        };
        let ids: Vec<String> = found.found.iter().map(|m| m.key.to_id()).collect();
        let mut out: Vec<String> = found.found.iter().map(|m| self.message_row(m)).collect();
        out.push(match found.total {
            0 => "Nothing found.".to_string(),
            n if n > found.found.len() => format!("{n} found in all; these are the newest {}.", found.found.len()),
            n => format!("{n} found."),
        });
        if found.unchecked > 0 {
            out.push(format!("{} more may match: their text isn't downloaded or indexed yet.", found.unchecked));
        }
        Outcome::ran(&what, ids, out.join("\n"))
    }

    /// `read_message`: a message whole, as it reads; one not downloaded yet is asked for, and
    /// the call waits for it.
    pub(super) fn tool_read_message(&mut self, chat: &mut Chat, profile: &AiProfile, input: &Value) -> Outcome {
        let Some(key) = input.get("id").and_then(Value::as_str).and_then(|n| chat.message_named(n)) else {
            return Outcome::failed("Read a message", "`id` must be a message's id, as the tools give it".into());
        };
        let id = key.to_id();
        if let Some(why) = self.unreadable(&key.account, profile) {
            return Outcome::failed("Read a message", why);
        }
        let Some(m) = self.shared_of(&key) else {
            return Outcome::failed("Read a message", format!("there's no message {id} (any more)"));
        };
        let what = format!("Read “{}”", m.subject.chars().take(80).collect::<String>());
        let chars = input
            .get("max_chars")
            .and_then(Value::as_u64)
            .map_or(MESSAGE_CHARS, |n| usize::try_from(n).unwrap_or(READ_CHARS_MOST).clamp(500, READ_CHARS_MOST));
        match self.message_text(&key) {
            Some(text) => {
                chat.holds(&key.account);
                let body = if text.is_empty() { "(No text: it may be encrypted, or only pictures.)".to_string() } else { cut(&text, chars) };
                let name = message_name(&id);
                Outcome::ran(&what, vec![id], format!("<message id=\"{name}\">\n{}\n\n{body}\n</message>", self.message_head(&m)))
            }
            None if self.account(&key.account).is_some_and(AccountState::is_local) => Outcome::failed(&what, "its text isn't kept here".into()),
            None => {
                self.jobs.push(Job::FetchBody { key: key.clone() });
                Outcome::Waiting { what, key }
            }
        }
    }

    /// `selected_messages`: what the window has selected, and the message open.
    pub(super) fn tool_selected(&mut self, profile: &AiProfile) -> Outcome {
        let mut keys: Vec<MessageKey> = self.selection.clone();
        if let Some(r) = &self.reading
            && !keys.contains(&r.key)
        {
            keys.push(r.key.clone());
        }
        let mut out = Vec::new();
        let mut ids = Vec::new();
        for key in keys.iter().take(FOUND_MOST) {
            if self.unreadable(&key.account, profile).is_some() {
                out.push("(A message of an account whose mail may not go to this model.)".to_string());
                continue;
            }
            if let Some(m) = self.shared_of(key) {
                let open = if self.reading.as_ref().is_some_and(|r| &r.key == key) { " (open)" } else { "" };
                out.push(format!("{}{open}", self.message_row(&m)));
                ids.push(key.to_id());
            }
        }
        if out.is_empty() {
            out.push("Nothing is selected.".into());
        }
        Outcome::ran("See what's selected", ids, out.join("\n"))
    }
}

impl Outcome {
    pub(super) fn ran(what: &str, messages: Vec<String>, result: String) -> Outcome {
        Outcome::Ran { what: what.to_string(), messages, found: Vec::new(), result }
    }

    /// Read: the events or people `found`.
    pub(super) fn listed(what: &str, found: Vec<Found>, result: String) -> Outcome {
        Outcome::Ran { what: what.to_string(), messages: Vec::new(), found, result }
    }

    pub(super) fn failed(what: &str, why: String) -> Outcome {
        Outcome::Failed { what: what.to_string(), result: why }
    }
}

/// Why a command can't go on, for the model.
pub(super) fn said(e: &CmdError) -> String {
    match e {
        CmdError::Params(s) | CmdError::Disabled(s) | CmdError::Failed(s) => s.clone(),
        other => other.to_string(),
    }
}
