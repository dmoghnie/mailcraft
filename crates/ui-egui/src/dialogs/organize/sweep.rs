//! Sweep: a sender's mail in a folder cleared out in one of four ways, each saying how many
//! messages it removes now.

use mailcraft_engine::{SWEEP_DAYS, SweepChoice};

use super::*;

impl Dialog {
    /// Sweep for the first message selected, when it names a sender.
    pub fn sweep(s: &Session) -> Option<Dialog> {
        let key = s.selection.first()?.clone();
        s.sweep_of(Some(&key))?;
        Some(Dialog::Sweep { key, choice: SweepChoice::All })
    }
}

pub(super) fn draw(ui: &mut Ui, d: &mut Dialog, s: &Session, t: &Tokens, acts: &mut Vec<Action>) -> bool {
    let Dialog::Sweep { key, choice } = d else { return false };
    ui.set_width(460.0);
    title(ui, t, "Sweep");
    // The message gone meanwhile: nothing left to ask about.
    let Some(of) = s.sweep_of(Some(key)) else { return false };
    ui.label(RichText::new(tf!("Mail from {sender} in this folder.", sender = widgets::shorten(&of.sender, 60))).color(t.dim));
    ui.add_space(8.0);
    let names = [
        tl!("Remove all their messages").to_string(),
        tl!("Remove all their messages, and those still to come").to_string(),
        tl!("Keep only their newest message").to_string(),
        tf!("Always remove those older than {days} days", days = SWEEP_DAYS),
    ];
    for ((c, name), n) in SweepChoice::ALL.into_iter().zip(names).zip(s.sweep_counts(&of)) {
        ui.radio_value(choice, c, name);
        ui.horizontal(|ui| {
            ui.add_space(26.0);
            ui.label(RichText::new(tf!("Removed now: {n}", n = n)).color(t.faint).small());
        });
        ui.add_space(4.0);
    }
    if s.swept_each_day(&of) {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tl!("Their mail here is swept each day.")).color(t.dim).small());
            if ui.small_button(tl!("Stop sweeping")).clicked() {
                acts.push(Action::Run("mail.stopSweep", json!({"account": of.account, "folder": of.folder, "sender": of.sender})));
            }
        });
    }
    let (ok, cancel) = buttons(ui, t, "Sweep", true);
    if ok {
        acts.push(Action::Run("mail.sweep", json!({"key": key.to_id(), "choice": *choice})));
        return false;
    }
    !cancel
}
