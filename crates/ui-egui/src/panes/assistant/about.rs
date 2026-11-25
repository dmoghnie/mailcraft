//! What MailCraft's AI wrote about the open message, in cards at the top of the reading pane:
//! its conversation summed up (`mail.summarize`), the message translated (`mail.translate`),
//! and what's to act on in it (`facts.rs`).
//! Each shows what's written as it comes, the model that writes it, and Close; why there's
//! none, and Try again. Its text is formatted as the assistant's answers are (links open as a
//! message's do, pictures aren't fetched).

use egui::{Margin, RichText, Sense, Ui, vec2};
use mailcraft_engine::model::MessageKey;
use mailcraft_engine::{AboutKind, Session};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::{Action, UiState, widgets};

/// The cards for message `key`: its summary, then its translation, those asked for.
pub(crate) fn card(s: &Session, st: &mut UiState, ui: &mut Ui, key: &MessageKey, acts: &mut Vec<Action>) {
    for kind in [AboutKind::Summary, AboutKind::Translation] {
        one(s, st, ui, key, kind, acts);
    }
    super::facts::card(s, st, ui, key, acts);
}

/// What a card of `kind` says: its title, its Close, while it's written, why there's none, and
/// the command that asks for it.
fn words(kind: AboutKind, error: &str) -> (&'static str, &'static str, &'static str, String, &'static str) {
    match kind {
        AboutKind::Summary => {
            (tl!("Summary"), "Close the summary", tl!("Summarizing…"), tf!("It couldn't be summarized: {error}", error = error), "mail.summarize")
        }
        // In a card of its own (`facts.rs`).
        AboutKind::Translation | AboutKind::Facts => (
            tl!("Translation"),
            "Close the translation",
            tl!("Translating…"),
            tf!("It couldn't be translated: {error}", error = error),
            "mail.translate",
        ),
    }
}

fn one(s: &Session, st: &mut UiState, ui: &mut Ui, key: &MessageKey, kind: AboutKind, acts: &mut Vec<Action>) {
    let Some(w) = s.about_message(key, kind) else { return };
    let t = st.tokens;
    let (title, close, writing, failed, command) = words(kind, w.error.as_deref().unwrap_or_default());
    let text = match w.asking.and_then(|id| s.ai.part(id)) {
        Some((part, _)) => part.to_string(),
        None => w.text.clone(),
    };
    let by = s.prefs.ai.profile(&w.profile).map(super::where_it_is);
    egui::Frame::new().fill(t.accent_soft).corner_radius(8).inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Sparkle, t.accent);
            ui.label(RichText::new(title).strong().color(t.text));
            if let Some(by) = &by {
                ui.label(RichText::new(by).small().color(t.dim));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::action_icon(ui, Icon::Close, close, &t).clicked() {
                    acts.push(Action::Run(command, json!({"key": key.to_id(), "close": true})));
                }
            });
        });
        if w.asking.is_some() && text.trim().is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(writing).color(t.dim));
            });
        }
        if !text.trim().is_empty() {
            let id = ui.make_persistent_id((kind.purpose(), key.to_id()));
            super::markdown::show(ui, &mut st.assistant.markdown, id, "", &text, acts);
        }
        if w.error.is_some() {
            ui.label(RichText::new(failed).color(t.danger));
            if ui.button(tl!("Try again")).clicked() {
                acts.push(Action::Run(command, json!({"key": key.to_id(), "language": crate::i18n::current().code()})));
            }
        }
        if w.cut {
            ui.label(RichText::new(tl!("The answer was cut short at the longest answer set for this model.")).small().color(t.faint));
        }
        if w.left_out > 0 {
            ui.label(RichText::new(tf!("Older messages left out for room: {count}", count = w.left_out)).small().color(t.faint));
        }
    });
    ui.add_space(12.0);
}
