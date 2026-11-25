//! Rules in plain words, in the Rules dialog: what the person says a rule should do, written by
//! MailCraft's AI as one of MailCraft's own rules, shown in words with the model that wrote it,
//! and added (Add rule) or let go. Offered once a model is set up; nothing changes until added.

use mailcraft_engine::RuleProposal;

use super::*;

/// The section under the rules: the words, Write rule, and the rule written (or why not).
pub(super) fn section(ui: &mut Ui, s: &Session, t: &Tokens, acts: &mut Vec<Action>) {
    if s.prefs.ai.profiles.is_empty() {
        return;
    }
    ui.add_space(10.0);
    ui.label(RichText::new(tl!("Write a rule in your own words")).strong());
    let id = egui::Id::unique("rule-words");
    let mut words: String = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
    let proposal = s.rule_proposal();
    let writing = proposal.is_some_and(RuleProposal::writing);
    ui.horizontal(|ui| {
        let hint = tl!("Receipts from the print shop go to Finance and get the Receipt category");
        let edit = ui.add(egui::TextEdit::singleline(&mut words).hint_text(hint).desired_width((ui.available_width() - 110.0).max(120.0)));
        let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let ready = !writing && !words.trim().is_empty();
        let clicked = ui.add_enabled(ready, egui::Button::new(tl!("Write rule"))).clicked();
        if ready && (clicked || entered) {
            acts.push(Action::Run("rules.fromWords", json!({"words": words.trim()})));
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, words));
    if let Some(p) = proposal {
        written(ui, s, t, p, acts);
    }
}

/// The rule asked for: being written, written (in words, with Add rule), or why it couldn't be.
fn written(ui: &mut Ui, s: &Session, t: &Tokens, p: &RuleProposal, acts: &mut Vec<Action>) {
    let by = s.prefs.ai.profile(&p.profile).map(crate::panes::assistant::where_it_is);
    egui::Frame::new().fill(t.accent_soft).corner_radius(8).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if p.writing() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tl!("MailCraft's AI is writing the rule…")).color(t.dim));
            });
            return;
        }
        if let Some(r) = &p.rule {
            ui.label(RichText::new(&r.name).strong().color(t.text));
            ui.add(egui::Label::new(RichText::new(describe(s, r)).color(t.text)).wrap());
        } else if let Some(e) = &p.error {
            ui.add(egui::Label::new(RichText::new(tf!("No rule was written: {error}", error = e)).color(t.danger)).wrap());
        }
        if let Some(by) = by {
            ui.label(RichText::new(tf!("Written by {model}", model = by)).small().color(t.dim));
        }
        ui.horizontal(|ui| {
            if p.rule.is_some() && ui.button(tl!("Add rule")).clicked() {
                acts.push(Action::Run("rules.addProposed", json!({})));
            }
            if ui.button(tl!("Let it go")).clicked() {
                acts.push(Action::Run("rules.dropProposed", json!({})));
            }
        });
    });
}
