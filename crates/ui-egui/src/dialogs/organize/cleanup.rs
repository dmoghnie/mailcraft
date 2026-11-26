//! Clean-up suggestions (Home › Delete): what could leave the Inboxes, worked out on this
//! computer when the dialog opens. Each is acted on only when the person chooses it: a list
//! left through Unsubscribe's own question, a sender's mail swept but for its newest, an
//! account's old read mail archived. What's done is marked so.

use std::collections::HashSet;

use mailcraft_engine::{Cleanup, OLD_DAYS};

use super::*;

/// The list's height at most: past it, it scrolls.
const LIST_HEIGHT: f32 = 380.0;

/// What the dialog holds while it's open: what was found, once its first frame has looked
/// (the ribbon has every Inbox read first, with `mail.cleanupSuggestions`), and what's been done
/// of it (a sender's address and what was done, or an account's id).
#[derive(Clone, Debug, Default)]
pub struct CleanupForm {
    pub found: Option<Cleanup>,
    pub done: HashSet<String>,
}

/// A button that's `whose`'s, so a screen reader tells one row's from another's: whether it
/// was clicked.
fn button(ui: &mut Ui, whose: &str, label: &str) -> bool {
    let r = ui.button(label);
    widgets::named(ui, &r, &format!("{whose}: {label}"), true, None);
    r.clicked()
}

/// An account's name, as the folder pane shows it.
fn account_name(s: &Session, id: &str) -> String {
    s.accounts.iter().find(|a| a.config.id == id).map(|a| a.config.name.clone()).unwrap_or_else(|| id.to_string())
}

pub(crate) fn draw(ui: &mut Ui, f: &mut CleanupForm, s: &Session, t: &Tokens, acts: &mut Vec<Action>) -> bool {
    ui.set_width(600.0);
    title(ui, t, tl!("Clean-up suggestions"));
    let note = tl!("Worked out on this computer from the mail in your Inbox, with nothing sent anywhere. Nothing changes until you choose");
    ui.add(egui::Label::new(RichText::new(note).color(t.faint).small()).wrap());
    ui.add_space(8.0);
    let found = f.found.get_or_insert_with(|| s.cleanup());
    if found.senders.is_empty() && found.old.is_empty() {
        ui.label(RichText::new(tl!("Nothing to clean up")).color(t.faint));
    }
    let done = |ui: &mut Ui| {
        ui.label(RichText::new(tl!("Done")).color(t.faint));
    };
    let mut opened = None;
    egui::ScrollArea::vertical().max_height(LIST_HEIGHT).show(ui, |ui| {
        for c in &found.senders {
            let said = if c.unread == c.count {
                tf!("{n} messages, none read", n = c.count)
            } else {
                tf!("{n} messages, {unread} unread", n = c.count, unread = c.unread)
            };
            ui.label(RichText::new(&c.name).color(t.text));
            ui.label(RichText::new(said).color(t.faint).small());
            ui.horizontal_wrapped(|ui| {
                // The list's way out read from its newest message only when asked for: Unsubscribe
                // asks first, in a dialog of its own.
                if c.leave
                    && button(ui, &c.name, tl!("Unsubscribe"))
                    && let Some(key) = mailcraft_engine::model::MessageKey::from_id(&c.newest)
                    && let Some(way) = s.leaving(&key)
                {
                    opened = Some(crate::Dialog::Unsubscribe { key, way });
                }
                let swept = format!("{}\u{0}sweep", c.sender);
                if c.sweep {
                    if f.done.contains(&swept) {
                        done(ui);
                    } else if button(ui, &c.name, tl!("Sweep all but the newest")) {
                        acts.push(Action::Run("mail.sweepPile", json!({"key": c.newest})));
                        f.done.insert(swept);
                    }
                }
            });
            ui.add_space(6.0);
        }
        for o in &found.old {
            let name = account_name(s, &o.account);
            ui.label(RichText::new(tf!("Read mail older than {days} days in {account}", days = OLD_DAYS, account = name)).color(t.text));
            ui.label(RichText::new(tf!("{n} messages", n = o.count)).color(t.faint).small());
            if f.done.contains(&o.account) {
                done(ui);
            } else if button(ui, &name, tl!("Archive them")) {
                acts.push(Action::Run("mail.archiveOld", json!({"account": o.account})));
                f.done.insert(o.account.clone());
            }
            ui.add_space(6.0);
        }
    });
    if let Some(d) = opened {
        acts.push(Action::Dialog(d));
    }
    let mut open = true;
    ui.add_space(10.0);
    button_row(ui, |ui| {
        if ui.button(tl!("Close")).clicked() {
            open = false;
        }
    });
    open
}
