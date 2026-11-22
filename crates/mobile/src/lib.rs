//! MailCraft's phone app core: the engine's [`Session`] behind a [Crux](crux_core) app.
//!
//! The Android and iOS shells draw what [`ViewModel`] says and send back what the person did,
//! as [`Action`]s in JSON. Everything else is here, once for both phones: which screen is
//! showing, what it lists, and the engine commands that each action runs.
//!
//! **Sans-I/O, as the engine is.** [`Mail::update`] runs commands on the session and hands the
//! jobs they queue to [`Effect::Engine`]; the native library (`apps/mailcraft-mobile`) gives them
//! to `mailcraft-net`'s worker pool and comes back with [`Event::Engine`]. The shells see only
//! [`Effect::Render`], so no `Job` and no password ever crosses into Kotlin or Swift through here.
//! The clock comes from outside too ([`Event::Clock`]): portable code doesn't read it.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use crux_core::capability::Operation;
use crux_core::macros::effect;
use crux_core::render::{RenderOperation, render};
use crux_core::{App, Command};
use mailcraft_engine::Session;
use mailcraft_engine::model::Job;
#[cfg(test)]
use mailcraft_engine::model::dates::Clock;
use serde::Deserialize;
use serde_json::{Value, json};

mod adding;
mod view;

pub use view::{Screen, ViewModel};

/// The person's actions, as the shells send them: `{"type": "openFolder", "account": …}`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Action {
    /// The accounts and their folders.
    Folders,
    OpenFolder {
        account: String,
        folder: String,
    },
    /// Every account's inbox together.
    UnifiedInbox,
    OpenMessage {
        key: String,
    },
    /// One screen back: a message to its list, a list to the folders, a message being written
    /// to what was under it (kept in Drafts).
    Back,
    /// Send / Receive: every account checked now.
    Refresh,
    /// The next messages of a long folder.
    LoadMore,
    /// A row's own action: `delete`, `flag` or `read` (each toggles but delete).
    Row {
        key: String,
        action: String,
    },
    /// `new`, `reply`, `replyAll` or `forward` (the last three of the open message).
    Compose {
        kind: String,
    },
    /// What the person typed so far; the fields left out are kept.
    Edit {
        to: Option<String>,
        cc: Option<String>,
        subject: Option<String>,
        body: Option<String>,
    },
    Send,
    /// The message being written thrown away.
    Discard,
    /// The add-account screen.
    StartAdding,
    /// An account: its servers looked up from the address unless given (`host:port`).
    AddAccount {
        email: String,
        password: String,
        name: Option<String>,
        imap: Option<String>,
        smtp: Option<String>,
    },
    RemoveAccount {
        account: String,
    },
    Search {
        query: String,
    },
    ClearSearch,
    DismissNotice,
}

/// What reaches [`Mail::update`]: the person's [`Action`]s and what the native side reports.
pub enum Event {
    Shell(Action),
    /// The cache opened: the session that reads it (the native side opens it in the app's folder).
    Opened(Box<Session>),
    /// What the worker pool came back with, in the order it did.
    Engine(Vec<mailcraft_engine::model::Event>),
    /// Now and the local offset (seconds east of UTC): each second or so while the app is open
    /// (the native side's driver), as the desktop sets it each frame.
    Clock {
        now: i64,
        offset: i32,
    },
    /// The phone's time zone (`Europe/Paris`), at start and when the person changes it.
    Zone(String),
}

/// Jobs for the network: done by the native library, never by a shell.
#[derive(Debug, Default)]
pub struct Jobs(pub Vec<Job>);

impl Operation for Jobs {
    type Output = ();
}

#[effect]
#[derive(Debug)]
pub enum Effect {
    Render(RenderOperation),
    Engine(Jobs),
}

/// Where the person is, newest last (the folders screen is under everything).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Place {
    pub screen: Screen,
    /// The folder listed (`account`, `path`), or none for the unified inbox.
    pub folder: Option<(String, String)>,
    pub message: Option<String>,
}

#[derive(Default)]
pub struct Model {
    pub(crate) session: Option<Box<Session>>,
    pub(crate) place: Place,
    /// What the screens show, as the engine's commands answered: kept here because a view
    /// can't run a command (that needs the session mutable).
    pub(crate) accounts: Value,
    pub(crate) list: Value,
    pub(crate) reading: Value,
    pub(crate) limit: u64,
    pub(crate) query: Option<String>,
    pub(crate) adding: adding::Adding,
    /// The last thing that went wrong, until dismissed.
    pub(crate) problem: Option<String>,
}

/// How many rows a folder lists at first, and how many more each Load More adds.
const PAGE: u64 = 100;

#[derive(Default)]
pub struct Mail;

impl App for Mail {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: Event, model: &mut Model) -> Command<Effect, Event> {
        match event {
            Event::Opened(s) => {
                model.session = Some(s);
                model.limit = PAGE;
            }
            Event::Clock { now, offset } => {
                if let Some(s) = model.session.as_mut() {
                    s.set_clock(now, offset);
                    // Snoozed mail that's due, and the engine's other timed work.
                    s.tick();
                }
            }
            Event::Zone(name) => {
                if let Some(s) = model.session.as_mut() {
                    s.set_zone(Some(name.as_str()).filter(|n| !n.is_empty()));
                }
            }
            Event::Engine(events) => {
                if let Some(s) = model.session.as_mut() {
                    for e in events {
                        s.apply(e);
                    }
                    s.tick();
                }
                adding::advance(model);
            }
            Event::Shell(a) => act(model, a),
        }
        refresh(model);
        let jobs = model.session.as_mut().map(|s| s.take_jobs()).unwrap_or_default();
        if jobs.is_empty() { render() } else { Command::all([render(), Command::notify_shell(Jobs(jobs)).into()]) }
    }

    fn view(&self, model: &Model) -> ViewModel {
        view::view(model)
    }
}

/// An engine command, its error kept to show.
pub(crate) fn run(model: &mut Model, id: &str, params: Value) -> Option<Value> {
    let s = model.session.as_mut()?;
    match s.run(id, &params) {
        Ok(v) => Some(v),
        Err(e) => {
            model.problem = Some(e.to_string());
            None
        }
    }
}

fn act(model: &mut Model, a: Action) {
    if model.session.is_none() {
        return;
    }
    match a {
        Action::Folders => model.place = Place::default(),
        Action::OpenFolder { account, folder } => {
            if run(model, "folder.open", json!({"account": account, "folder": folder})).is_some() {
                model.limit = PAGE;
                model.place = Place { screen: Screen::Messages, folder: Some((account, folder)), message: None };
            }
        }
        Action::UnifiedInbox => {
            if run(model, "folder.unified", json!({"role": "inbox"})).is_some() {
                model.limit = PAGE;
                model.place = Place { screen: Screen::Messages, folder: None, message: None };
            }
        }
        Action::OpenMessage { key } => {
            if run(model, "list.open", json!({"key": key})).is_some() {
                model.place.screen = Screen::Message;
                model.place.message = Some(key);
            }
        }
        Action::Back => back(model),
        Action::Refresh => {
            run(model, "mail.sync", json!({}));
        }
        Action::LoadMore => {
            model.limit = model.limit.saturating_add(PAGE);
            run(model, "folder.more", json!({}));
        }
        Action::Row { key, action } => {
            if matches!(action.as_str(), "delete" | "flag" | "read") {
                run(model, "list.rowAction", json!({"key": key, "action": action}));
                if action == "delete" && model.place.message.as_deref() == Some(key.as_str()) {
                    model.place.screen = Screen::Messages;
                    model.place.message = None;
                }
            }
        }
        Action::Compose { kind } => {
            let id = match kind.as_str() {
                "new" => "mail.new",
                "reply" => "mail.reply",
                "replyAll" => "mail.replyAll",
                "forward" => "mail.forward",
                _ => return,
            };
            if run(model, id, json!({})).is_some() {
                model.place.screen = Screen::Compose;
            }
        }
        Action::Edit { to, cc, subject, body } => {
            let mut p = json!({});
            for (k, v) in [("to", to), ("cc", cc), ("subject", subject), ("body", body)] {
                if let Some(v) = v {
                    p[k] = json!(v);
                }
            }
            run(model, "compose.update", p);
        }
        Action::Send => {
            // A subject left empty is the person's choice on a phone: the Send button is
            // pressed on purpose, and there's no second dialog to ask in.
            if let Some(r) = run(model, "compose.send", json!({"allowEmptySubject": true}))
                && r.get("check").is_none()
            {
                close_compose(model);
            } else if model.problem.is_none() {
                model.problem = Some("This message needs checking before it goes: its addresses look unusual.".into());
            }
        }
        Action::Discard => {
            run(model, "compose.discard", json!({}));
            close_compose(model);
        }
        Action::StartAdding => {
            model.adding = adding::Adding::default();
            model.place.screen = Screen::AddAccount;
        }
        Action::AddAccount { email, password, name, imap, smtp } => adding::start(model, adding::Form { email, password, name, imap, smtp }),
        Action::RemoveAccount { account } => {
            if run(model, "account.remove", json!({"account": account})).is_some() {
                model.place = Place::default();
            }
        }
        Action::Search { query } => {
            if run(model, "search.run", json!({"query": query, "scope": "mailbox"})).is_some() {
                model.query = Some(query);
            }
        }
        Action::ClearSearch => {
            run(model, "search.clear", json!({}));
            model.query = None;
        }
        Action::DismissNotice => model.problem = None,
    }
}

fn back(model: &mut Model) {
    match model.place.screen {
        Screen::Compose => {
            // Kept in Drafts, as the desktop's window closing does.
            run(model, "compose.close", json!({"save": true}));
            close_compose(model);
        }
        Screen::Message => {
            model.place.screen = Screen::Messages;
            model.place.message = None;
        }
        Screen::Messages | Screen::AddAccount => model.place = Place::default(),
        Screen::Folders => {}
    }
}

/// Back to what was under the message being written: the message replied to, or its list.
fn close_compose(model: &mut Model) {
    model.place.screen = if model.place.message.is_some() {
        Screen::Message
    } else if model.place.folder.is_some() || model.list.is_object() {
        Screen::Messages
    } else {
        Screen::Folders
    };
}

/// What the screens show, asked again: after every action and every batch of events, since
/// either can change any of it (a message arriving, a flag set elsewhere).
fn refresh(model: &mut Model) {
    let Some(s) = model.session.as_mut() else { return };
    model.accounts = s.run("account.list", &json!({})).unwrap_or(Value::Null);
    if matches!(model.place.screen, Screen::Messages | Screen::Message | Screen::Compose) {
        model.list = s.run("list.messages", &json!({"limit": model.limit})).unwrap_or(Value::Null);
    }
    if let (Screen::Message | Screen::Compose, Some(key)) = (&model.place.screen, &model.place.message) {
        model.reading = s.run("message.read", &json!({"key": key})).unwrap_or(Value::Null);
    } else {
        model.reading = Value::Null;
    }
}

#[cfg(test)]
mod tests;
