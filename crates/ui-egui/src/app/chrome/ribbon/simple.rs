//! The simplified ribbon's Home: one row of the buttons chosen for it (Customize the ribbon), as
//! many as the window has room for, then More commands with the rest: those that didn't fit,
//! then Home's others. Each keeps its key tip, drawn or not.

use std::collections::HashMap;

use mailcraft_engine::HOME_BUTTONS;

use super::home;
use crate::*;

/// More commands' name, in English (its key tip is found by it).
const MORE: &str = "More commands";
/// What a ribbon button takes besides its label: its icon and margins (`widgets::tool_button`).
const BUTTON_PAD: f32 = 36.0;
/// What a drop-down arrow beside a button takes, with the gap before it.
const ARROW_W: f32 = 20.0;
/// The gap between buttons.
const GAP: f32 = 2.0;

/// A button on the row: the command it runs, and its place among those added besides Home's
/// (from 0: its key tip's digit, less one).
type Item = (&'static str, Option<usize>);

/// The simplified ribbon's buttons, in order, those added besides Home's numbered.
fn row(s: &Session) -> Vec<Item> {
    let mut added = 0usize;
    s.ribbon_row()
        .into_iter()
        .map(|id| {
            if HOME_BUTTONS.contains(&id) {
                return (id, None);
            }
            added = added.saturating_add(1);
            (id, Some(added.saturating_sub(1)))
        })
        .collect()
}

/// How many commands besides Home's the simplified ribbon shows, for their key tips (none when
/// it isn't shown).
pub(super) fn added(s: &Session) -> usize {
    if s.module != Module::Mail || !s.prefs.simple_ribbon {
        return 0;
    }
    row(s).iter().filter(|(_, n)| n.is_some()).count()
}

/// Where the buttons' widths are kept from one frame to the next.
fn widths_id() -> egui::Id {
    egui::Id::unique("simplified-ribbon-widths")
}

/// The room a button not drawn yet takes: its label, icon and margins, and its arrow.
fn estimate(ui: &Ui, s: &Session, t: Tokens, id: &'static str) -> f32 {
    let label = ui.painter().layout_no_wrap(tl!(home::name(s, id)).to_string(), theme::regular(13.0), t.text).size().x;
    label + BUTTON_PAD + if home::has_arrow(id) { ARROW_W } else { 0.0 }
}

/// Home on the simplified ribbon: the row's buttons while they fit (as wide as last drawn, or as
/// worked out), then More commands.
pub(super) fn home(ui: &mut Ui, s: &Session, t: Tokens, acts: &mut Vec<Action>) {
    // What doesn't fit goes in More commands; no button loses its label to make room.
    widgets::keep_ribbon_labels(ui);
    let mut widths: HashMap<String, f32> = ui.ctx().data(|d| d.get_temp(widths_id())).unwrap_or_default();
    let row = row(s);
    let rest: Vec<Item> = HOME_BUTTONS.iter().filter(|id| !row.iter().any(|(r, _)| r == *id)).map(|id| (*id, None)).collect();
    ui.with_layout(crate::bidi::row_from_start(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let more_w = if rest.is_empty() { 0.0 } else { widgets::ACTION_ICON + GAP };
        let room = ui.available_width();
        let mut used = 0.0;
        let mut over: Vec<Item> = Vec::new();
        for (i, (id, added)) in row.iter().enumerate() {
            // The last button keeps room for More commands only if Home's others go there.
            let last = i + 1 == row.len();
            let w = widths.get(*id).copied().unwrap_or_else(|| estimate(ui, s, t, id));
            let keep = if last { more_w } else { widgets::ACTION_ICON + GAP };
            if !over.is_empty() || used + w + keep > room {
                over.push((id, *added));
                continue;
            }
            let drawn = ui.scope(|ui| home::button(ui, s, t, id, *added, acts)).response.rect.width();
            widths.insert(id.to_string(), drawn);
            used += drawn + GAP;
        }
        more(ui, s, t, &over, &rest, acts);
    });
    ui.ctx().data_mut(|d| d.insert_temp(widths_id(), widths));
}

/// More commands, at the row's end: what didn't fit, then Home's others. A button among them
/// whose key tip is typed is pressed; one with a menu opens More commands.
fn more(ui: &mut Ui, s: &Session, t: Tokens, over: &[Item], rest: &[Item], acts: &mut Vec<Action>) {
    if over.is_empty() && rest.is_empty() {
        return;
    }
    let mut open = false;
    for (id, added) in over.iter().chain(rest) {
        let typed = match added {
            Some(n) => crate::keytips::pressed_added(ui, *n),
            None => crate::keytips::pressed(ui, home::name(s, id)),
        };
        if typed && home::can(s, id) {
            match home::act(s, id) {
                Some(a) => acts.push(a),
                None => open = true,
            }
        }
    }
    let mut r = widgets::more_icon(ui, MORE, &t);
    crate::keytips::apply(ui, MORE, &mut r, true, &t);
    if open {
        r.flags.insert(egui::response::Flags::FAKE_PRIMARY_CLICKED);
    }
    egui::Popup::menu(&r).show(|ui| {
        for (id, _) in over {
            home::line(ui, s, t, id, acts);
        }
        if !over.is_empty() && !rest.is_empty() {
            ui.separator();
        }
        for (id, _) in rest {
            home::line(ui, s, t, id, acts);
        }
    });
}
