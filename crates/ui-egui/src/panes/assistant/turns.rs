//! A chat's turns: what the person said, with what they pointed at; the model's answers,
//! formatted, its thinking folded; a card for each tool it called (what it read, or the change
//! it proposes, approved or declined here); and the answer still coming.

use egui::{Margin, RichText, Stroke, Ui};
use mailcraft_engine::model::AiRole;
use mailcraft_engine::{Chat, ChatTurn, ContextItem, RunState, Session, ToolRun};
use serde_json::json;

use super::{AssistantUi, markdown};
use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::{Action, UiState, widgets};

/// The most of the model's thinking shown while it's still coming (its end).
const THINKING_SHOWN: usize = 600;

/// The chat, scrolled to its end as it grows.
pub(super) fn show(s: &Session, st: &mut UiState, ui: &mut Ui, chat: &Chat, acts: &mut Vec<Action>) {
    let t = st.tokens;
    let asking = s.ai.assistant.asking(&chat.id);
    egui::ScrollArea::vertical().id_salt("assistant-chat").auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
        ui.set_width(ui.available_width());
        for (i, turn) in chat.turns.iter().enumerate() {
            match turn.turn.role {
                AiRole::User => said(ui, &t, &st.assistant, turn),
                AiRole::Assistant => answered(ui, &t, &mut st.assistant, chat, i, turn, acts),
            }
        }
        if let Some(q) = asking {
            coming(ui, &t, &mut st.assistant, chat, s.ai.part(q), acts);
            // The answer comes in a few times a second.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
        }
        if let Some(e) = &chat.error {
            trouble(ui, &t, e);
        }
        decide_all(ui, &t, chat, acts);
        ui.add_space(8.0);
    });
}

/// What the person said, and what they pointed at; nothing for a turn that only takes back what
/// the tools gave.
fn said(ui: &mut Ui, t: &Tokens, a: &AssistantUi, turn: &ChatTurn) {
    if turn.turn.text.trim().is_empty() && turn.context.is_empty() {
        return;
    }
    ui.add_space(6.0);
    egui::Frame::new().fill(t.accent_soft).corner_radius(8).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if !turn.context.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for item in &turn.context {
                    chip(ui, t, a, item);
                }
            });
        }
        if !turn.turn.text.trim().is_empty() {
            ui.add(egui::Label::new(RichText::new(&turn.turn.text).color(t.text)).wrap().selectable(true));
        }
    });
}

/// A thing pointed at, as a small label: what it is, and its name.
pub(super) fn chip(ui: &mut Ui, t: &Tokens, a: &AssistantUi, item: &ContextItem) -> egui::Response {
    let (icon, name) = match item {
        ContextItem::Message { .. } => (Icon::Inbox, a.name(item).map_or_else(|| tl!("A message no longer here").to_string(), name_of)),
        ContextItem::Quote { text, .. } => (Icon::Conversation, format!("“{}”", widgets::shorten(text.trim(), 40))),
        ContextItem::Folder { folder, .. } => (Icon::Folder, a.name(item).unwrap_or(folder).to_string()),
    };
    egui::Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                icons::paint(ui.painter(), r, icon, t.dim);
                ui.label(RichText::new(widgets::shorten(&name, 40)).small().color(t.text));
            });
        })
        .response
}

/// A message's subject as a chip names it.
fn name_of(subject: &str) -> String {
    if subject.trim().is_empty() { tl!("(no subject)").to_string() } else { subject.to_string() }
}

/// The model's answer: its thinking folded, its text formatted, and a card for each tool it called.
fn answered(ui: &mut Ui, t: &Tokens, a: &mut AssistantUi, chat: &Chat, i: usize, turn: &ChatTurn, acts: &mut Vec<Action>) {
    ui.add_space(6.0);
    if !turn.thinking.trim().is_empty() {
        egui::CollapsingHeader::new(RichText::new(tl!("Reasoning")).small().color(t.dim)).id_salt(("assistant-thought", &chat.id, i)).show(
            ui,
            |ui| {
                ui.add(egui::Label::new(RichText::new(turn.thinking.trim()).small().color(t.dim)).wrap().selectable(true));
            },
        );
    }
    if !turn.turn.text.trim().is_empty() {
        markdown::show(ui, &mut a.markdown, ui.make_persistent_id(("assistant-answer", &chat.id, i)), &chat.id, &turn.turn.text, acts);
    }
    if turn.cut {
        ui.label(RichText::new(tl!("The answer was cut short at the longest answer set for this model.")).small().color(t.faint));
    }
    // A change put off is proposed again, with the others, in a card of its own.
    for call in &turn.turn.calls {
        if let Some(run) = chat.run(&call.id).filter(|r| r.state != RunState::PutOff) {
            card(ui, t, &chat.id, run, acts);
        }
    }
}

/// A tool the model called: what it did, or the change it proposes and whether it's made.
fn card(ui: &mut Ui, t: &Tokens, chat: &str, run: &ToolRun, acts: &mut Vec<Action>) {
    let proposed = run.state == RunState::Proposed;
    let (icon, color, state) = match run.state {
        RunState::Ran | RunState::PutOff => (tool_icon(&run.tool), t.dim, None),
        RunState::Failed => (Icon::Warning, t.warn, Some(tl!("Couldn't be done"))),
        RunState::Waiting => (Icon::Clock, t.dim, Some(tl!("Downloading the message…"))),
        RunState::Proposed => (tool_icon(&run.tool), t.accent, Some(tl!("Needs your approval"))),
        RunState::Done => (Icon::Done, t.ok, Some(tl!("Done"))),
        RunState::Declined => (Icon::Close, t.faint, Some(tl!("Declined"))),
    };
    ui.add_space(4.0);
    let frame = egui::Frame::new()
        .fill(if proposed { t.accent_soft } else { t.chrome })
        .stroke(Stroke::new(1.0, if proposed { t.accent } else { t.border }))
        .corner_radius(6)
        .inner_margin(Margin::symmetric(8, 6));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            icons::paint(ui.painter(), r, icon, color);
            ui.label(RichText::new(tool_title(&run.tool)).small().strong().color(t.text));
            if let Some(state) = state {
                ui.label(RichText::new(state).small().color(color));
            }
        });
        if !run.what.is_empty() {
            ui.add(egui::Label::new(RichText::new(&run.what).small().color(t.dim)).wrap());
        }
        if run.state == RunState::Failed && !run.result.is_empty() {
            ui.add(egui::Label::new(RichText::new(widgets::shorten(&run.result, 200)).small().color(t.faint)).wrap());
        }
        if proposed {
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, tl!("Approve"), t, true).clicked() {
                    acts.push(Action::Run("assistant.decide", json!({"chat": chat, "call": run.call, "approve": true})));
                }
                if ui.button(tl!("Decline")).clicked() {
                    acts.push(Action::Run("assistant.decide", json!({"chat": chat, "call": run.call, "approve": false})));
                }
            });
        }
    });
}

/// What a tool is called on its card.
fn tool_title(tool: &str) -> &str {
    match tool {
        "list_folders" => tl!("Looked at the folders"),
        "find_messages" => tl!("Searched the mail"),
        "read_message" => tl!("Read a message"),
        "find_events" => tl!("Searched the calendar"),
        "find_people" => tl!("Searched the contacts"),
        "find_tasks" => tl!("Searched the tasks"),
        "find_notes" => tl!("Searched the notes"),
        "selected_messages" => tl!("Looked at what's selected"),
        "move_messages" => tl!("Move"),
        "delete_messages" => tl!("Delete"),
        "mark_messages" => tl!("Mark"),
        "categorize_messages" => tl!("Categorize"),
        "write_message" => tl!("Write a message"),
        "add_event" => tl!("New event"),
        "add_person" => tl!("New contact"),
        "add_task" => tl!("New task"),
        "add_note" => tl!("New note"),
        "change_event" | "change_person" | "change_task" | "change_note" => tl!("Edit"),
        "delete_items" => tl!("Delete"),
        other => other,
    }
}

fn tool_icon(tool: &str) -> Icon {
    match tool {
        "list_folders" => Icon::Folder,
        "find_messages" => Icon::Search,
        "read_message" | "selected_messages" => Icon::Read,
        "find_events" | "add_event" | "change_event" => Icon::Calendar,
        "find_people" | "add_person" | "change_person" => Icon::People,
        "find_tasks" | "add_task" | "change_task" => Icon::Task,
        "find_notes" | "add_note" | "change_note" => Icon::Note,
        "move_messages" => Icon::Move,
        "delete_messages" | "delete_items" => Icon::Delete,
        "mark_messages" => Icon::Flag,
        "categorize_messages" => Icon::Tag,
        "write_message" => Icon::NewMail,
        _ => Icon::Steps,
    }
}

/// The answer as it comes: what it's thought (its end) until it says something, then what it's
/// said so far; and Stop.
fn coming(ui: &mut Ui, t: &Tokens, a: &mut AssistantUi, chat: &Chat, part: Option<(&str, &str)>, acts: &mut Vec<Action>) {
    ui.add_space(6.0);
    let (text, thinking) = part.unwrap_or_default();
    if !text.trim().is_empty() {
        markdown::show(ui, &mut a.markdown, ui.make_persistent_id(("assistant-coming", &chat.id)), &chat.id, text, acts);
    }
    ui.horizontal(|ui| {
        ui.spinner();
        let doing = if text.trim().is_empty() && !thinking.trim().is_empty() { tl!("Thinking…") } else { tl!("Answering…") };
        ui.label(RichText::new(doing).small().color(t.dim));
    });
    if text.trim().is_empty() && !thinking.trim().is_empty() {
        let thinking = thinking.trim();
        let from = thinking.char_indices().rev().nth(THINKING_SHOWN).map_or(0, |(i, _)| i);
        ui.add(egui::Label::new(RichText::new(thinking.get(from..).unwrap_or_default()).small().color(t.faint)).wrap());
    }
}

/// Why the chat stopped: the model couldn't be asked, or it took too many steps.
fn trouble(ui: &mut Ui, t: &Tokens, error: &str) {
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        icons::paint(ui.painter(), r, Icon::Warning, t.warn);
        ui.label(RichText::new(error).small().color(t.dim));
    });
}

/// Several changes wait: all of them approved, or declined, at once.
fn decide_all(ui: &mut Ui, t: &Tokens, chat: &Chat, acts: &mut Vec<Action>) {
    let waiting = chat.runs.iter().filter(|r| r.state == RunState::Proposed).count();
    if waiting < 2 {
        return;
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if widgets::primary_button(ui, &tf!("Approve all {n}", n = waiting), t, true).clicked() {
            acts.push(Action::Run("assistant.decide", json!({"chat": chat.id, "approve": true})));
        }
        if ui.button(tl!("Decline all")).clicked() {
            acts.push(Action::Run("assistant.decide", json!({"chat": chat.id, "approve": false})));
        }
    });
}
