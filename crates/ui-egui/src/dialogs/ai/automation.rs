//! Add or change an automation of MailCraft's AI (File › Options › AI): its name, the job in
//! the person's words, and when it runs, each day at a time on the days chosen, or as mail a
//! search matches arrives in an Inbox. Saved through `automation.save`, which checks it.

use mailcraft_engine::model::dates;
use mailcraft_engine::{Automation, AutomationWhen};
use serde_json::Value;

use super::*;

/// The minutes between the times offered.
const TIME_STEP: usize = 15;
/// The minutes in a day.
const DAY_MINUTES: u16 = 24 * 60;
/// Every day of the week, Monday the lowest bit.
const EVERY_DAY: u8 = 0x7f;
/// A Monday (5 January 1970), for the days' names.
pub(crate) const A_MONDAY: i64 = 4;

/// What the dialog holds while it's open.
#[derive(Clone, Debug)]
pub struct AutomationForm {
    /// The name it had; none for a new one.
    pub was: Option<String>,
    pub name: String,
    pub job: String,
    /// Each day at a time; else as mail arrives.
    pub daily: bool,
    /// Its time, in minutes after midnight, and its days (Monday the lowest bit).
    pub at: u16,
    pub days: u8,
    /// The search the mail arriving matches, and the account it's of (none: any).
    pub query: String,
    pub account: Option<String>,
    /// Kept turned off.
    pub off: bool,
    pub error: Option<String>,
}

impl Default for AutomationForm {
    fn default() -> Self {
        AutomationForm {
            was: None,
            name: String::new(),
            job: String::new(),
            daily: true,
            at: 8 * 60,
            days: EVERY_DAY,
            query: String::new(),
            account: None,
            off: false,
            error: None,
        }
    }
}

impl AutomationForm {
    /// An automation kept, to change.
    pub fn of(a: &Automation) -> AutomationForm {
        let base = AutomationForm { was: Some(a.name.clone()), name: a.name.clone(), job: a.job.clone(), off: a.off, ..AutomationForm::default() };
        match &a.when {
            AutomationWhen::Daily { at, days } => AutomationForm { at: *at, days: *days, ..base },
            AutomationWhen::Arrives { query, account } => AutomationForm { daily: false, query: query.clone(), account: account.clone(), ..base },
        }
    }

    fn ready(&self) -> bool {
        !self.name.trim().is_empty() && !self.job.trim().is_empty() && (!self.daily || self.days & EVERY_DAY != 0)
    }

    /// `automation.save`'s params.
    pub(crate) fn params(&self) -> Value {
        let when = if self.daily {
            json!({"kind": "daily", "at": self.at, "days": self.days})
        } else {
            let mut w = json!({"kind": "arrives", "query": self.query.trim()});
            if let Some(a) = &self.account {
                w["account"] = json!(a);
            }
            w
        };
        let mut p = json!({"name": self.name.trim(), "job": self.job.trim(), "when": when, "off": self.off});
        if let Some(was) = &self.was {
            p["was"] = json!(was);
        }
        p
    }
}

/// A time of day as the person reads times.
pub(crate) fn time_shown(s: &Session, minutes: u16) -> String {
    crate::i18n::time_box(u32::from(minutes / 60 % 24), u32::from(minutes % 60), s.clock.style)
}

pub(crate) fn draw(ui: &mut Ui, f: &mut AutomationForm, s: &Session, t: &Tokens, acts: &mut Vec<Action>) -> bool {
    ui.set_width(560.0);
    title(ui, t, if f.was.is_some() { "Change an automation" } else { "Add an automation" });
    let w = ui.available_width();
    let field = |ui: &mut Ui, label: &str| {
        ui.add_space(6.0);
        ui.label(RichText::new(label).color(t.text));
    };
    let note = |ui: &mut Ui, text: &str| {
        ui.add(egui::Label::new(RichText::new(text).color(t.faint).small()).wrap());
    };
    field(ui, tl!("Name"));
    ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(w));
    field(ui, tl!("What to do"));
    ui.add(egui::TextEdit::multiline(&mut f.job).desired_rows(4).desired_width(w));
    note(ui, tl!("In your words, as you'd ask MailCraft's AI. It works with its tools, and a change it calls for waits for your approval"));
    field(ui, tl!("When"));
    ui.radio_value(&mut f.daily, true, tl!("Each day at"));
    ui.horizontal(|ui| {
        ui.add_space(24.0);
        ui.add_enabled_ui(f.daily, |ui| {
            let r = egui::ComboBox::from_id_salt("automation-at").selected_text(time_shown(s, f.at)).width(96.0).show_ui(ui, |ui| {
                for m in (0..DAY_MINUTES).step_by(TIME_STEP) {
                    ui.selectable_value(&mut f.at, m, time_shown(s, m));
                }
            });
            widgets::named(ui, &r.response, tl!("Each day at"), f.daily, None);
            ui.spacing_mut().item_spacing.x = 2.0;
            for d in 0..7u8 {
                let bit = 1u8 << d;
                let day = A_MONDAY + i64::from(d);
                let r = ui.selectable_label(f.days & bit != 0, tl!(dates::weekday_short(day))).on_hover_text(tl!(dates::weekday_long(day)));
                if r.clicked() {
                    f.days ^= bit;
                }
            }
        });
    });
    ui.radio_value(&mut f.daily, false, tl!("As mail arrives in the Inbox"));
    ui.add_enabled_ui(!f.daily, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.label(tl!("Matching"));
            let r = ui.add(egui::TextEdit::singleline(&mut f.query).hint_text("subject:invoice").desired_width(w - 120.0));
            widgets::named(ui, &r, tl!("Matching"), !f.daily, None);
        });
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            note(ui, tl!("As typed in the search box. Nothing: all the mail the Inbox's rules leave there"));
        });
        if s.accounts.len() > 1 {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label(tl!("Account"));
                let shown = f.account.as_ref().and_then(|id| s.accounts.iter().find(|a| a.config.id == *id)).map(|a| a.config.name.clone());
                let r = egui::ComboBox::from_id_salt("automation-account")
                    .selected_text(shown.unwrap_or_else(|| tl!("Any account").to_string()))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut f.account, None, tl!("Any account"));
                        for a in &s.accounts {
                            ui.selectable_value(&mut f.account, Some(a.config.id.clone()), &a.config.name);
                        }
                    });
                widgets::named(ui, &r.response, tl!("Account"), !f.daily, None);
            });
        }
    });
    let mut on = !f.off;
    ui.add_space(6.0);
    ui.checkbox(&mut on, tl!("On: it runs by itself"));
    f.off = !on;
    if let Some(e) = &f.error {
        ui.add_space(6.0);
        ui.add(egui::Label::new(RichText::new(e.as_str()).color(t.danger)).wrap());
    }
    let ready = f.ready();
    let (ok, cancel) = buttons(ui, t, "Save", ready);
    if ok && ready {
        f.error = None;
        acts.push(Action::Submit("automation.save", f.params()));
    }
    !cancel
}
