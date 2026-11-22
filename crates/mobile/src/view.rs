//! What the shells draw: one [`ViewModel`] for the whole app, in JSON.
//!
//! The lists are the engine's own answers (`account.list`, `list.messages`, `message.read`), cut
//! to the fields a phone shows, so the shells read the same names the CLI prints.

use super::*;
use serde::Serialize;

/// The screen showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Screen {
    #[default]
    Folders,
    Messages,
    Message,
    Compose,
    AddAccount,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewModel {
    pub screen: Screen,
    /// False until the cache has opened.
    pub ready: bool,
    pub accounts: Vec<AccountView>,
    /// The list's heading: the folder's name, or the search.
    pub title: String,
    pub messages: Vec<Value>,
    /// How many the folder has in all (more than `messages` when there's more to load).
    pub total: u64,
    /// The message open, as `message.read` answers (`from`, `to`, `subject`, `text`, …).
    pub message: Option<Value>,
    pub compose: Option<ComposeView>,
    pub searching: Option<String>,
    /// An add-account lookup under way.
    pub adding: bool,
    /// What went wrong last, or what the engine said (a sync that failed, a message held).
    pub notice: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    pub email: String,
    pub name: String,
    /// `online`, `connecting`, `offline`, `error`…, as `account.list` says.
    pub state: String,
    pub error: Option<String>,
    pub folders: Vec<Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeView {
    /// Which message is being written: a shell keeps what's typed while it stays the same.
    pub id: u64,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub body: String,
    pub sending: bool,
    pub error: Option<String>,
}

/// The fields of a row a phone shows; the rest of `list.messages`' answer stays behind.
const ROW: &[&str] = &["key", "from", "subject", "preview", "date", "unread", "flagged", "attachments"];

pub(crate) fn view(model: &Model) -> ViewModel {
    let Some(s) = model.session.as_deref() else { return ViewModel::default() };
    let accounts = model.accounts.as_array().map(|a| a.iter().map(account).collect()).unwrap_or_default();
    let messages = model.list.get("messages").and_then(Value::as_array).map(|m| m.iter().map(row).collect()).unwrap_or_default();
    let compose = s.compose.as_ref().map(|c| ComposeView {
        id: c.id,
        from: c.from.clone(),
        to: c.to.clone(),
        cc: c.cc.clone(),
        subject: c.subject.clone(),
        body: c.body.clone(),
        sending: c.sending,
        error: c.error.clone(),
    });
    // The engine's own last word, when it was an error and nothing of the app's is showing.
    let engine = s.notices.last().filter(|n| n.error && n.at >= s.clock.now.saturating_sub(60)).map(|n| n.text.clone());
    ViewModel {
        screen: if model.place.screen == Screen::Compose && compose.is_none() { Screen::Folders } else { model.place.screen },
        ready: true,
        accounts,
        title: model.query.clone().map(|q| format!("Search: {q}")).unwrap_or_else(|| title(model)),
        messages,
        total: model.list.get("total").and_then(Value::as_u64).unwrap_or(0),
        message: model.reading.is_object().then(|| model.reading.clone()),
        compose,
        searching: model.query.clone(),
        adding: model.adding.busy(),
        notice: model.problem.clone().or(engine),
    }
}

fn title(model: &Model) -> String {
    if let Some(v) = model.list.get("view").and_then(Value::as_str) {
        return v.to_string();
    }
    match &model.place.folder {
        Some((_, path)) => path.clone(),
        None => "Inbox".into(),
    }
}

fn account(a: &Value) -> AccountView {
    let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let status = a.get("status").cloned().unwrap_or(Value::Null);
    AccountView {
        id: s("account"),
        email: s("email"),
        name: s("name"),
        state: status.get("state").and_then(Value::as_str).unwrap_or("offline").to_string(),
        error: status.get("error").or_else(|| status.get("message")).and_then(Value::as_str).map(str::to_string),
        folders: a.get("folders").and_then(Value::as_array).cloned().unwrap_or_default(),
    }
}

fn row(m: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for k in ROW {
        if let Some(v) = m.get(*k) {
            out.insert((*k).to_string(), v.clone());
        }
    }
    Value::Object(out)
}
