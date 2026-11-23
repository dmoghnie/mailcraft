//! The box to say something in: what's pointed at above it (each taken off with its ×), the
//! text (Enter sends, Shift+Enter starts a new line), the selected messages added with +, and
//! Send, or Stop while the model answers.

use egui::{Key, Modifiers, RichText, Ui};
use mailcraft_engine::{ContextItem, Session};
use serde_json::json;

use super::turns::chip;
use crate::icons::Icon;
use crate::theme;
use crate::{Action, UiState, widgets};

/// The box's rows, at first.
const ROWS: usize = 3;

pub(super) fn show(s: &Session, st: &mut UiState, ui: &mut Ui, acts: &mut Vec<Action>) {
    let t = st.tokens;
    let chat = st.assistant.chat.clone();
    let asking = chat.as_deref().is_some_and(|c| s.ai.assistant.asking(c).is_some());
    if !st.assistant.context.is_empty() {
        let mut off = None;
        ui.horizontal_wrapped(|ui| {
            for (i, item) in st.assistant.context.iter().enumerate() {
                ui.horizontal(|ui| {
                    chip(ui, &t, &st.assistant, item);
                    if widgets::tiny_icon_button(ui, Icon::Close, "Remove", &t).clicked() {
                        off = Some(i);
                    }
                });
            }
        });
        if let Some(i) = off {
            st.assistant.context.remove(i);
        }
        ui.add_space(4.0);
    }
    let id = ui.make_persistent_id("assistant-draft");
    // Enter sends, before the box takes it as a new line; with Shift it is one.
    let has_focus = ui.memory(|m| m.has_focus(id));
    let can_send = !asking && !st.assistant.draft.trim().is_empty();
    let enter = has_focus && can_send && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
    let typed = ui.add(
        egui::TextEdit::multiline(&mut st.assistant.draft)
            .id(id)
            .hint_text(tl!("Ask about your mail…"))
            .desired_rows(ROWS)
            .desired_width(f32::INFINITY)
            .font(theme::regular(13.5)),
    );
    if std::mem::take(&mut st.assistant.focus) {
        typed.request_focus();
    }
    ui.horizontal(|ui| {
        let add = widgets::tiny_icon_button(ui, Icon::Plus, "Add the selected messages", &t);
        if add.clicked() && !s.selection.is_empty() {
            st.assistant.point_at(s.selection.iter().map(|k| ContextItem::Message { id: k.to_id() }).collect());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if asking {
                if widgets::primary_button(ui, tl!("Stop"), &t, true).clicked()
                    && let Some(c) = &chat
                {
                    acts.push(Action::Run("assistant.stop", json!({"chat": c})));
                }
            } else if widgets::primary_button(ui, tl!("Send"), &t, can_send).clicked() || enter {
                let text = st.assistant.draft.trim().to_string();
                let context = st.assistant.context.clone();
                acts.push(send(st, &text, &context));
            }
            ui.label(RichText::new(tl!("Shift+Enter for a new line")).small().color(t.faint));
        });
    });
}

/// Say `text` in the chat shown (a new one when none is), pointing at `context`: a new chat asks
/// the model picked for it.
pub(super) fn send(st: &UiState, text: &str, context: &[ContextItem]) -> Action {
    let a = &st.assistant;
    let v = match (&a.chat, &a.profile) {
        (Some(c), _) => json!({"chat": c, "text": text, "context": context}),
        (None, Some(p)) => json!({"profile": p, "text": text, "context": context}),
        (None, None) => json!({"text": text, "context": context}),
    };
    Action::Run("assistant.send", v)
}
