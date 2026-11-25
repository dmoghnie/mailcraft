//! A conversation summed up by MailCraft's AI (`mail.summarize`), in a card at the top of the
//! reading pane: what's written as it comes, the model that writes it named, and Close; why
//! there's none, and Try again. Its text is formatted as the assistant's answers are (links
//! open as a message's do, pictures aren't fetched).

use egui::{Margin, RichText, Sense, Ui, vec2};
use mailcraft_engine::Session;
use mailcraft_engine::model::MessageKey;
use serde_json::json;

use crate::icons::{self, Icon};
use crate::{Action, UiState, widgets};

/// The summary asked for of message `key`'s conversation, if one was.
pub(crate) fn card(s: &Session, st: &mut UiState, ui: &mut Ui, key: &MessageKey, acts: &mut Vec<Action>) {
    let Some(sum) = s.thread_summary(key) else { return };
    let t = st.tokens;
    let text = match sum.asking.and_then(|id| s.ai.part(id)) {
        Some((part, _)) => part.to_string(),
        None => sum.text.clone(),
    };
    let by = s.prefs.ai.profile(&sum.profile).map(super::where_it_is);
    egui::Frame::new().fill(t.accent_soft).corner_radius(8).inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Sparkle, t.accent);
            ui.label(RichText::new(tl!("Summary")).strong().color(t.text));
            if let Some(by) = &by {
                ui.label(RichText::new(by).small().color(t.dim));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::action_icon(ui, Icon::Close, "Close the summary", &t).clicked() {
                    acts.push(Action::Run("mail.summarize", json!({"key": key.to_id(), "close": true})));
                }
            });
        });
        if sum.asking.is_some() && text.trim().is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tl!("Summarizing…")).color(t.dim));
            });
        }
        if !text.trim().is_empty() {
            let id = ui.make_persistent_id(("summary", key.to_id()));
            super::markdown::show(ui, &mut st.assistant.markdown, id, "", &text, acts);
        }
        if let Some(e) = &sum.error {
            ui.label(RichText::new(tf!("It couldn't be summarized: {error}", error = e)).color(t.danger));
            if ui.button(tl!("Try again")).clicked() {
                acts.push(Action::Run("mail.summarize", json!({"key": key.to_id(), "language": crate::i18n::current().code()})));
            }
        }
        if sum.cut {
            ui.label(RichText::new(tl!("The answer was cut short at the longest answer set for this model.")).small().color(t.faint));
        }
        if sum.left_out > 0 {
            ui.label(RichText::new(tf!("Older messages left out for room: {count}", count = sum.left_out)).small().color(t.faint));
        }
    });
    ui.add_space(12.0);
}
