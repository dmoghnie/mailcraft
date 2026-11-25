//! Options' Automations card: the jobs MailCraft's AI does by itself, once there's a model,
//! each with when it runs and how its last run went; each turned on or off, changed, run now,
//! its chat opened, or deleted; and one added.

use super::*;
use crate::dialogs::{AutomationForm, time_shown};
use mailcraft_engine::model::dates;
use mailcraft_engine::{Automation, AutomationWhen};

/// Every day of the week, Monday the lowest bit.
const EVERY_DAY: u8 = 0x7f;

pub(super) fn automations(s: &Session, t: &Tokens, ui: &mut Ui, acts: &mut Vec<Action>) {
    if s.prefs.ai.profiles.is_empty() {
        return;
    }
    let about = tl!(
        "Jobs MailCraft's AI does by itself: each day at a time, or as mail arrives. Each runs in a chat of its own, where a change it calls for waits for your approval"
    );
    let list = s.ai.assistant.automations();
    card(ui, t, tl!("Automations"), Some(about), |ui| {
        for (i, a) in list.iter().enumerate() {
            automation(s, t, ui, i == 0, a, acts);
        }
        setting(ui, t, list.is_empty(), tl!("Add an automation"), Some(tl!("A job in your words, and when it's done")), |ui| {
            if ai::button(ui, tl!("Automations"), tl!("Add…"), true).clicked() {
                acts.push(Action::Dialog(Dialog::Automation(Box::default())));
            }
        });
    });
}

/// When an automation runs, said.
fn when_said(s: &Session, when: &AutomationWhen) -> String {
    match when {
        AutomationWhen::Daily { at, days } if days & EVERY_DAY == EVERY_DAY => tf!("Each day at {time}", time = time_shown(s, *at)),
        AutomationWhen::Daily { at, days } => {
            let names: Vec<&str> =
                (0..7u8).filter(|d| days & (1 << d) != 0).map(|d| tl!(dates::weekday_short(crate::dialogs::A_MONDAY + i64::from(d)))).collect();
            tf!("At {time} on {days}", time = time_shown(s, *at), days = names.join(", "))
        }
        AutomationWhen::Arrives { query, .. } if query.is_empty() => tl!("As mail arrives in the Inbox").to_string(),
        AutomationWhen::Arrives { query, .. } => tf!("As mail matching {query} arrives in the Inbox", query = query),
    }
}

/// An automation: its name, when it runs and how its last run went; its buttons, and the
/// switch that turns it on.
fn automation(s: &Session, t: &Tokens, ui: &mut Ui, first: bool, a: &Automation, acts: &mut Vec<Action>) {
    let mut lines = vec![when_said(s, &a.when)];
    if let Some(r) = &a.last {
        let when = crate::i18n::date(&dates::list_time(r.at, &s.clock));
        lines.push(match (&r.chat, &r.error) {
            (None, Some(why)) => tf!("Couldn't run {when}: {why}", when = when, why = why),
            _ => tf!("Last ran {when}", when = when),
        });
    }
    let about = lines.join("\n");
    setting(ui, t, first, &a.name, Some(&about), |ui| {
        if ai::button(ui, &a.name, tl!("Delete"), true).clicked() {
            acts.push(Action::Run("automation.delete", json!({"name": a.name})));
        }
        if ai::button(ui, &a.name, tl!("Change…"), true).clicked() {
            acts.push(Action::Dialog(Dialog::Automation(Box::new(AutomationForm::of(a)))));
        }
        if let Some(chat) = a.last.as_ref().and_then(|r| r.chat.as_ref())
            && ai::button(ui, &a.name, tl!("Open its chat"), true).clicked()
        {
            acts.push(Action::OpenChat(chat.clone()));
        }
        if ai::button(ui, &a.name, tl!("Run now"), true).clicked() {
            acts.push(Action::Run("automation.run", json!({"name": a.name})));
        }
        let mut on = !a.off;
        if widgets::switch(ui, t, &mut on, &a.name).changed() {
            acts.push(Action::Run("automation.turn", json!({"name": a.name, "on": on})));
        }
    });
}
