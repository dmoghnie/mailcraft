//! The buttons on new-mail notifications: Mark as read, Archive, Delete and Reply on one that
//! tells of a message, Mark all as read on one that tells of several. The system says which was
//! pressed ([`crate::Services::notices_pressed`]), and each runs its command on the messages that
//! notification told of: `notice.act`; for Reply, `notice.open`, then the ribbon's `mail.reply`
//! once the message has come.

use mailcraft_engine::model::MessageKey;
use serde_json::{Value, json};

use crate::*;

/// The notifications whose buttons are still answered: the newest this many.
pub(crate) const NOTICES_KEPT: usize = 32;

/// A notification's buttons, each an id and its label. One for mail of an account kept online
/// only names neither sender nor subject: it has no button that would archive or delete it
/// unseen.
pub(crate) fn buttons(messages: usize, private: bool) -> Vec<(&'static str, &'static str)> {
    match (messages, private) {
        (0, _) => Vec::new(),
        (1, false) => vec![("read", tl!("Mark as read")), ("archive", tl!("Archive")), ("delete", tl!("Delete")), ("reply", tl!("Reply"))],
        (1, true) => vec![("read", tl!("Mark as read")), ("reply", tl!("Reply"))],
        _ => vec![("readAll", tl!("Mark all as read"))],
    }
}

/// The command a button runs on its notification's messages; none for a button that
/// notification hasn't.
pub(crate) fn command(button: &str, keys: &[MessageKey]) -> Option<(&'static str, Value)> {
    let act = |action: &str| json!({"action": action, "keys": keys.iter().map(MessageKey::to_id).collect::<Vec<_>>()});
    match (button, keys) {
        ("read", [_]) | ("readAll", [_, _, ..]) => Some(("notice.act", act("read"))),
        ("archive", [_]) => Some(("notice.act", act("archive"))),
        ("delete", [_]) => Some(("notice.act", act("delete"))),
        ("reply", [k]) => Some(("notice.open", json!({"key": k.to_id()}))),
        _ => None,
    }
}

impl MailApp {
    /// A notification of these messages, with its buttons, kept to answer them; clicked, it
    /// shows `opens`.
    pub(crate) fn notify_mail(
        &mut self,
        ctx: &egui::Context,
        title: &str,
        body: &str,
        keys: Vec<MessageKey>,
        opens: Option<MessageKey>,
        private: bool,
    ) {
        let buttons = buttons(keys.len(), private);
        self.ui.notice_next = self.ui.notice_next.wrapping_add(1);
        let notice = self.ui.notice_next;
        self.ui.notices.push((notice, keys));
        let extra = self.ui.notices.len().saturating_sub(NOTICES_KEPT);
        self.ui.notices.drain(..extra);
        let shown = MailNotice { id: notice, title, body, buttons: &buttons, opens: opens.as_ref(), more: tl!("Options") };
        self.services.notify_mail(ctx, &shown);
    }

    /// The buttons pressed on notifications since the last pass, each done (the window
    /// minimized or hidden too). Reply brings the window forward, with the answer to write.
    pub(crate) fn notices_pressed(&mut self, ctx: &egui::Context) {
        for (notice, button) in self.services.notices_pressed() {
            let Some(keys) = self.ui.notices.iter().find(|(n, _)| *n == notice).map(|(_, k)| k.clone()) else { continue };
            let Some((id, params)) = command(&button, &keys) else { continue };
            if self.answer(id, params).is_some() && button == "reply" {
                self.ui.reply_due = keys.first().cloned();
                self.show_window(ctx);
            }
            ctx.request_repaint();
        }
        self.reply_when_ready();
    }

    /// A notification's Reply, written once its message has come (a new one may still be
    /// downloading); given up when another message is opened.
    fn reply_when_ready(&mut self) {
        let Some(key) = self.ui.reply_due.clone() else { return };
        match self.session.reading.as_ref() {
            Some(r) if r.key == key && r.parsed.is_none() => {}
            Some(r) if r.key == key => {
                self.ui.reply_due = None;
                self.answer("mail.reply", json!({}));
            }
            _ => self.ui.reply_due = None,
        }
    }
}
