//! What MailCraft's AI found to act on in the open message (`mail.facts`), in a card at the top
//! of the reading pane: its dates and meetings, each added to the calendar through a New event
//! filled in from it; what the person is asked to do, added to To Do through a New task; amounts
//! and tracking numbers, to read and copy. Nothing is made until the person saves the form.

use egui::{Margin, RichText, Sense, Ui, vec2};
use mailcraft_engine::model::{MessageKey, dates};
use mailcraft_engine::{AboutKind, FactEvent, FactTask, Facts, Session};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::pim_dialogs::{EventForm, TaskForm};
use crate::{Action, Dialog, UiState, widgets};

/// The card for message `key`, once it was asked for.
pub(crate) fn card(s: &Session, st: &UiState, ui: &mut Ui, key: &MessageKey, acts: &mut Vec<Action>) {
    let Some(w) = s.about_message(key, AboutKind::Facts) else { return };
    let t = st.tokens;
    let by = s.prefs.ai.profile(&w.profile).map(super::where_it_is);
    egui::Frame::new().fill(t.accent_soft).corner_radius(8).inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Sparkle, t.accent);
            ui.label(RichText::new(tl!("Found in this message")).strong().color(t.text));
            if let Some(by) = &by {
                ui.label(RichText::new(by).small().color(t.dim));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::action_icon(ui, Icon::Close, "Close what was found", &t).clicked() {
                    acts.push(Action::Run("mail.facts", json!({"key": key.to_id(), "close": true})));
                }
            });
        });
        if w.asking.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tl!("Looking for dates and tasks…")).color(t.dim));
            });
        } else if let Some(e) = &w.error {
            ui.label(RichText::new(tf!("Nothing could be found: {error}", error = e)).color(t.danger));
            if ui.button(tl!("Try again")).clicked() {
                acts.push(Action::Run("mail.facts", json!({"key": key.to_id(), "language": crate::i18n::current().code()})));
            }
        } else if let Some(found) = s.facts_found(key) {
            listed(s, &t, ui, key, &found, acts);
        }
    });
    ui.add_space(12.0);
}

/// What was found, by kind; that nothing was.
fn listed(s: &Session, t: &crate::theme::Tokens, ui: &mut Ui, key: &MessageKey, found: &Facts, acts: &mut Vec<Action>) {
    if found.is_empty() {
        ui.label(RichText::new(tl!("Nothing to act on was found.")).color(t.dim));
        return;
    }
    let subject = s.reading.as_ref().filter(|r| r.key == *key).and_then(|r| r.parsed.as_ref()).map(|p| p.subject.clone()).unwrap_or_default();
    let due = |day: i64| tf!("Due {date}", date = crate::i18n::date(&dates::day_text(day, &s.clock)));
    let heading = |ui: &mut Ui, text: &str| {
        ui.add_space(4.0);
        ui.label(RichText::new(text).small().strong().color(t.dim));
    };
    if !found.events.is_empty() {
        heading(ui, tl!("Dates"));
        for (i, e) in found.events.iter().enumerate() {
            let line = [e.title.clone(), event_when(s, e), e.place.clone()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
            row(ui, t, ("event", i), &line, tl!("Add to calendar"), || acts.push(Action::Dialog(event_form(s, e, &subject))));
        }
    }
    if !found.tasks.is_empty() {
        heading(ui, tl!("Tasks"));
        for (i, task) in found.tasks.iter().enumerate() {
            let line = task.due.map_or_else(|| task.title.clone(), |d| format!("{} · {}", task.title, due(d)));
            row(ui, t, ("task", i), &line, tl!("Add to tasks"), || acts.push(Action::Dialog(task_form(s, task, &subject))));
        }
    }
    if !found.amounts.is_empty() {
        heading(ui, tl!("Amounts"));
        for a in &found.amounts {
            let line = [a.what.clone(), a.amount.clone(), a.due.map(due).unwrap_or_default()]
                .into_iter()
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            ui.add(egui::Label::new(RichText::new(line).color(t.text)).selectable(true).wrap());
        }
    }
    if !found.tracking.is_empty() {
        heading(ui, tl!("Tracking numbers"));
        for (i, n) in found.tracking.iter().enumerate() {
            let line = [n.carrier.as_str(), n.number.as_str()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" ");
            let ctx = ui.ctx().clone();
            row(ui, t, ("tracking", i), &line, tl!("Copy"), || ctx.copy_text(n.number.clone()));
        }
    }
}

/// A line of what was found, and its button (`then` when it's clicked).
fn row(ui: &mut Ui, t: &crate::theme::Tokens, id: (&str, usize), line: &str, button: &str, then: impl FnOnce()) {
    ui.push_id(id, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(line).color(t.text));
            if ui.small_button(button).clicked() {
                then();
            }
        });
    });
}

/// When an event is, as the reading pane writes a time: a whole day by its day.
fn event_when(s: &Session, e: &FactEvent) -> String {
    let at = |when: i64| crate::i18n::date(&dates::full_time(when, &s.clock));
    let start = if e.all_day { crate::i18n::date(&dates::day_text(s.clock.local_day(e.start), &s.clock)) } else { at(e.start) };
    match e.end.filter(|end| *end > e.start && !e.all_day) {
        Some(end) if s.clock.local_day(end) == s.clock.local_day(e.start) => format!("{start} – {}", dates::hm(end, &s.clock)),
        Some(end) => format!("{start} – {}", at(end)),
        None => start,
    }
}

/// A New event filled in from `e`, its description the message's subject.
fn event_form(s: &Session, e: &FactEvent, subject: &str) -> Dialog {
    Dialog::Event(Box::new(EventForm::found(s, &e.title, (e.start, e.end), e.all_day, &e.place, subject)))
}

/// A New task filled in from `task`, its notes the message's subject.
fn task_form(s: &Session, task: &FactTask, subject: &str) -> Dialog {
    let due = task.due.map(|d| d.saturating_mul(86_400));
    Dialog::Task(Box::new(TaskForm { title: task.title.clone(), notes: subject.to_string(), ..TaskForm::new(s, due) }))
}
