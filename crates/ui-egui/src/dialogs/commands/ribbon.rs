//! Customize the ribbon: the simplified ribbon's buttons in order, each moved or taken off, and
//! more added from Home's own and every command that runs as it is (as Find a command lists
//! them); Reset puts back those it starts with. OK keeps them in the settings.

use mailcraft_engine::{HOME_BUTTONS, RIBBON_ADDED_MOST, RIBBON_ROW};

use super::*;
use super::{matching, ribbon_offers};

/// How many commands found are listed to add.
const FOUND: usize = 6;

impl Dialog {
    /// Customize the ribbon, from the simplified ribbon's buttons as they are.
    pub fn customize_ribbon(s: &Session) -> Dialog {
        Dialog::CustomizeRibbon { buttons: s.ribbon_row(), picked: None, filter: String::new() }
    }
}

/// A button's name in the interface's language.
fn name(s: &Session, id: &str) -> String {
    s.registry().get(id).map_or_else(|| id.to_string(), |c| tl!(c.label).to_string())
}

pub(crate) fn draw(ui: &mut Ui, d: &mut Dialog, s: &Session, t: &Tokens, acts: &mut Vec<Action>) -> bool {
    let Dialog::CustomizeRibbon { buttons, picked, filter } = d else { return false };
    ui.set_width(460.0);
    title(ui, t, "Customize the ribbon");
    ui.label(RichText::new(tl!("The simplified ribbon's buttons, in order. Home's others are in its More commands.")).color(t.dim));
    ui.add_space(6.0);
    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
        for (i, id) in buttons.iter().enumerate() {
            if ui.add(egui::Button::selectable(*picked == Some(i), name(s, id))).clicked() {
                *picked = Some(i);
            }
        }
    });
    arrange_row(ui, buttons, picked);
    ui.add_space(8.0);
    add_commands(ui, s, t, buttons, picked, filter);
    ui.add_space(10.0);
    let (mut ok, mut cancel) = (false, false);
    button_row(ui, |ui| {
        ok = widgets::primary_button(ui, "OK", t, true).clicked();
        cancel = ui.button(tl!("Cancel")).clicked();
        if ui.button(tl!("Reset")).on_hover_text(tl!("The buttons it starts with")).clicked() {
            *buttons = RIBBON_ROW.to_vec();
            *picked = None;
        }
    });
    if ok {
        acts.push(Action::Run("view.arrangeRibbon", json!({"buttons": buttons})));
    }
    !(ok || cancel)
}

/// Move up, Move down and Remove, for the button picked. The last one stays: a ribbon needs one.
fn arrange_row(ui: &mut Ui, buttons: &mut Vec<&'static str>, picked: &mut Option<usize>) {
    let at = picked.filter(|i| *i < buttons.len());
    ui.horizontal(|ui| {
        if ui.add_enabled(at.is_some_and(|i| i > 0), egui::Button::new(tl!("Move up"))).clicked()
            && let Some(i) = at
        {
            buttons.swap(i, i.saturating_sub(1));
            *picked = Some(i.saturating_sub(1));
        }
        if ui.add_enabled(at.is_some_and(|i| i.saturating_add(1) < buttons.len()), egui::Button::new(tl!("Move down"))).clicked()
            && let Some(i) = at
        {
            buttons.swap(i, i.saturating_add(1));
            *picked = Some(i.saturating_add(1));
        }
        if ui.add_enabled(at.is_some() && buttons.len() > 1, egui::Button::new(tl!("Remove"))).clicked()
            && let Some(i) = at
        {
            buttons.remove(i);
            *picked = None;
        }
    });
}

/// A box to find a command by its name, and those found that aren't on the ribbon yet, each
/// added at its end when clicked.
fn add_commands(ui: &mut Ui, s: &Session, t: &Tokens, buttons: &mut Vec<&'static str>, picked: &mut Option<usize>, filter: &mut String) {
    ui.label(RichText::new(tl!("Add a command")).strong());
    let r = ui.add(egui::TextEdit::singleline(filter).hint_text(tl!("Type a command's name")).desired_width(f32::INFINITY));
    // Named to a screen reader (its hint is only drawn while it's empty).
    ui.ctx().accesskit_node_builder(r.id, |node| node.set_label(tl!("Add a command")));
    if filter.trim().is_empty() {
        return;
    }
    let added = buttons.iter().filter(|id| !HOME_BUTTONS.contains(id)).count();
    let found: Vec<_> = matching(ribbon_offers(s), filter, &[]).into_iter().filter(|o| !buttons.contains(&o.id)).take(FOUND).collect();
    if found.is_empty() {
        ui.label(RichText::new(tl!("No command has that name.")).color(t.dim));
    }
    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
        for o in found {
            let room = HOME_BUTTONS.contains(&o.id) || added < RIBBON_ADDED_MOST;
            let what = tf!("Add {command}", command = o.name);
            let r = ui.add_enabled(room, egui::Button::new(format!("+  {}", o.name)));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, room, &what));
            let r = if room {
                r.on_hover_text(&what)
            } else {
                r.on_disabled_hover_text(tf!("At most {n} commands besides Home's own", n = RIBBON_ADDED_MOST))
            };
            if r.clicked() {
                buttons.push(o.id);
                *picked = Some(buttons.len().saturating_sub(1));
            }
        }
    });
}
