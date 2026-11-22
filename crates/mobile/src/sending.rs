//! A message on its way, followed from Send to where it went, for the shells to say so: sending,
//! held back for Undo, sent, saved in the Outbox, or refused (then it's open again, with why).
//! The engine's own events say each step (`Sent`, `Queued`, `SendFailed`).

use super::*;
use mailcraft_engine::model::Event as EngineEvent;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    #[default]
    Sending,
    /// Waiting for its moment (Undo Send): it can still be taken back until `until`.
    Held,
    Sent,
    /// In the Outbox: offline, or the server asked to try later; it goes when it can.
    Queued,
    /// Refused; the message is open again with the reason.
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outgoing {
    pub id: u64,
    pub stage: Stage,
    /// When a held message goes (Unix seconds).
    pub until: Option<i64>,
    pub error: Option<String>,
}

/// What `compose.send` answered: the message is on its way, or held for Undo.
pub(crate) fn started(model: &mut Model, answer: &Value) {
    let id = |k: &str| answer.get(k).and_then(Value::as_u64);
    model.outgoing = if let Some(id) = id("sending") {
        Some(Outgoing { id, stage: Stage::Sending, ..Default::default() })
    } else if let Some(id) = id("queued") {
        let held = answer.get("undo").and_then(Value::as_bool) == Some(true);
        Some(Outgoing {
            id,
            stage: if held { Stage::Held } else { Stage::Queued },
            until: answer.get("sendAfter").and_then(Value::as_i64),
            error: None,
        })
    } else {
        id("sent").map(|id| Outgoing { id, stage: Stage::Sent, ..Default::default() })
    };
}

/// The engine's events about the message followed, before the session applies them.
pub(crate) fn heard(model: &mut Model, events: &[EngineEvent]) {
    let Some(o) = model.outgoing.as_mut() else { return };
    for e in events {
        match e {
            EngineEvent::Sent { id, .. } if *id == o.id => {
                o.stage = Stage::Sent;
            }
            EngineEvent::Queued { id, .. } if *id == o.id => {
                o.stage = Stage::Queued;
            }
            EngineEvent::SendFailed { id, error, .. } if *id == o.id => {
                o.stage = Stage::Failed;
                o.error = Some(error.clone());
            }
            _ => {}
        }
    }
}

/// After the events were applied: a refused message is open again, so the person is taken back
/// to it, to fix and send again.
pub(crate) fn settle(model: &mut Model) {
    let failed = model.outgoing.as_ref().filter(|o| o.stage == Stage::Failed).map(|o| o.id);
    if let Some(id) = failed
        && model.session.as_ref().and_then(|s| s.compose.as_ref()).is_some_and(|c| c.id == id)
        && model.place.screen != Screen::Compose
    {
        model.place.screen = Screen::Compose;
    }
}

/// Undo Send: the held message back into the compose screen.
pub(crate) fn undo(model: &mut Model) {
    let Some(o) = model.outgoing.clone().filter(|o| o.stage == Stage::Held) else { return };
    if run(model, "outbox.undoSend", json!({"id": o.id})).is_some() {
        model.outgoing = None;
        model.place.screen = Screen::Compose;
    }
}

/// The accounts and addresses a message can go from: each account's own address, then its
/// others (`account.setAddress`), as the desktop's From list.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sender {
    pub account: String,
    /// Empty for the account's own address (what `compose.update`'s `from` takes for it).
    pub from: String,
    pub address: String,
    pub name: String,
}

pub(crate) fn senders(s: &Session) -> Vec<Sender> {
    let mut out = Vec::new();
    for a in s.accounts.iter().filter(|a| !a.is_data_file()) {
        let c = &a.config;
        out.push(Sender { account: c.id.clone(), from: String::new(), address: c.email.clone(), name: c.name.clone() });
        for i in c.identities.iter().filter(|i| !i.email.eq_ignore_ascii_case(&c.email)) {
            out.push(Sender { account: c.id.clone(), from: i.email.clone(), address: i.email.clone(), name: i.name.clone() });
        }
    }
    out
}
