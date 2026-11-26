//! The suggestions MailCraft's AI made for mail as it arrived, newest first, for the person to
//! look over and apply or dismiss: each one chosen until they leave it out. Nothing changes
//! until they apply one; the list is the engine's, as it stands each frame.

use std::collections::HashSet;

use mailcraft_engine::{InboxGroup, TriageSuggestion};

use super::*;

/// The list's height at most: past it, it scrolls.
const LIST_HEIGHT: f32 = 360.0;
/// The longest subject shown, in characters.
const SUBJECT_SHOWN: usize = 80;

/// What the dialog holds while it's open.
#[derive(Clone, Debug, Default)]
pub struct SuggestionsForm {
    /// The messages (their keys) whose suggestions are left out.
    pub left_out: HashSet<String>,
}

/// What a suggestion would do, a piece each: its category, pinning, its sender's group.
pub(crate) fn suggestion_said(s: &Session, w: &TriageSuggestion) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(c) = &w.category {
        out.push(tf!("Category {name}", name = c));
    }
    if w.pin {
        out.push(tl!("Pin it").to_string());
    }
    if let Some(g) = w.group {
        out.push(tf!("Mail from {sender} always in {group}", sender = w.from, group = group_name(s, g)));
    }
    out
}

/// A group as the Inbox names it: with the Focused Inbox alone, Focused or Other.
fn group_name(s: &Session, g: InboxGroup) -> &'static str {
    match (s.prefs.inbox_groups, g) {
        (true, g) => tl!(g.label()),
        (false, InboxGroup::Primary) => tl!("Focused"),
        (false, _) => tl!("Other"),
    }
}

pub(crate) fn draw(ui: &mut Ui, f: &mut SuggestionsForm, s: &Session, t: &Tokens, acts: &mut Vec<Action>) -> bool {
    ui.set_width(560.0);
    title(ui, t, tl!("Suggestions"));
    let all = s.triage_suggestions();
    if all.is_empty() {
        ui.label(RichText::new(tl!("Nothing is waiting")).color(t.faint));
    }
    egui::ScrollArea::vertical().max_height(LIST_HEIGHT).show(ui, |ui| {
        for w in all.iter().rev() {
            let mut on = !f.left_out.contains(&w.id);
            let subject: String = w.subject.chars().take(SUBJECT_SHOWN).collect();
            let r = ui.checkbox(&mut on, RichText::new(format!("{} · {subject}", w.from)).color(t.text));
            if r.changed() {
                if on {
                    f.left_out.remove(&w.id);
                } else {
                    f.left_out.insert(w.id.clone());
                }
            }
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.add(egui::Label::new(RichText::new(suggestion_said(s, w).join(" · ")).color(t.faint).small()).wrap());
            });
            ui.add_space(4.0);
        }
    });
    let chosen: Vec<&str> = all.iter().filter(|w| !f.left_out.contains(&w.id)).map(|w| w.id.as_str()).collect();
    let mut open = true;
    ui.add_space(10.0);
    button_row(ui, |ui| {
        if widgets::primary_button(ui, tl!("Apply"), t, !chosen.is_empty()).clicked() {
            acts.push(Action::Run("mail.applySuggestions", json!({"ids": chosen})));
        }
        if ui.add_enabled(!chosen.is_empty(), egui::Button::new(tl!("Dismiss"))).clicked() {
            acts.push(Action::Run("mail.dismissSuggestions", json!({"ids": chosen})));
        }
        if ui.button(tl!("Close")).clicked() {
            open = false;
        }
    });
    open
}
