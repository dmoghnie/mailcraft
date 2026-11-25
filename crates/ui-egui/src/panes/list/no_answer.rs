//! The Inbox's No answer yet, over its list: sent mail nobody answered after a few days, each to
//! follow up (a reply to all it went to) or dismiss; Show all opens them as a search folder.

use super::*;

/// How many are listed; Show all opens every one.
const SHOWN: usize = 3;
/// Its key among what's folded away (`Prefs::collapsed`).
const FOLDED: &str = "noAnswer";

pub(super) fn section(s: &Session, t: &theme::Tokens, ui: &mut Ui, acts: &mut Vec<Action>) {
    let found = s.no_answer_shown();
    if found.is_empty() {
        return;
    }
    let open = !s.prefs.collapsed.contains(FOLDED);
    let top = ui.available_rect_before_wrap();
    ui.painter().line_segment([top.left_top(), top.right_top()], egui::Stroke::new(1.0, t.border));
    egui::Frame::new().inner_margin(egui::Margin { left: 8, right: 10, top: 4, bottom: 6 }).show(ui, |ui| {
        ui.with_layout(crate::bidi::column(), |ui| {
            heading(t, ui, found.len(), open, acts);
            if !open {
                return;
            }
            for m in found.iter().take(SHOWN) {
                item(s, t, ui, m, acts);
            }
            if found.len() > SHOWN && ui.link(tf!("Show all {count}", count = found.len())).clicked() {
                acts.push(Action::Run("folder.noAnswer", json!({})));
            }
        });
    });
}

/// Its heading, as the list's own: an arrow, and a click folds it away or shows it.
fn heading(t: &theme::Tokens, ui: &mut Ui, count: usize, open: bool, acts: &mut Vec<Action>) {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 4.0, t.hover_row);
    }
    let m = crate::bidi::Mirror::new(r);
    let arrow = Rect::from_center_size(m.pos(pos2(r.min.x + 9.0, r.center().y)), vec2(10.0, 10.0));
    icons::paint(ui.painter(), arrow, if open { Icon::ChevronDown } else { m.closed() }, t.dim);
    let text = tf!("No answer yet ({count})", count = count);
    ui.painter().text(m.pos(pos2(r.min.x + 20.0, r.center().y)), m.align(egui::Align2::LEFT_CENTER), &text, theme::bold(12.5), t.dim);
    widgets::named(ui, &resp, &text, true, Some(open));
    let tip = if open { tl!("Hide sent mail nobody answered") } else { tl!("Show sent mail nobody answered") };
    if resp.on_hover_text(tip).clicked() {
        acts.push(Action::Run("view.collapse", json!({"key": FOLDED})));
    }
}

/// One message: whom it went to and its subject (a click shows it), when, then Write a
/// follow-up and Dismiss.
fn item(s: &Session, t: &theme::Tokens, ui: &mut Ui, m: &Summary, acts: &mut Vec<Action>) {
    let subject = if m.subject.is_empty() { tl!("(no subject)") } else { m.subject.as_str() };
    let text = format!("{} · {subject}", tf!("To: {names}", names = addr::display_list(&m.to)));
    let when = crate::i18n::date(&dates::list_time(m.when(), &s.clock));
    let (shown, act) = egui::containers::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            let r = ui.add(egui::Label::new(RichText::new(&text).color(t.text)).sense(Sense::click()));
            r.on_hover_text(&text).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
        },
        |ui| {
            let dismiss = widgets::tiny_icon_button(ui, Icon::Close, "Dismiss", t).clicked();
            let follow = ui.small_button(tl!("Write a follow-up")).clicked();
            ui.label(RichText::new(when).color(t.dim).small());
            if dismiss { Some("mail.noAnswerDismiss") } else { follow.then_some("mail.noAnswerFollowUp") }
        },
    );
    let key = m.key.to_id();
    if shown {
        acts.push(Action::Run("list.open", json!({"key": key})));
    }
    // A follow-up's text is written by MailCraft's AI too, once a model is set up (where the
    // account's mail may go to it): a proposal, put in from its card.
    let write = !s.prefs.ai.profiles.is_empty();
    if let Some(id) = act {
        acts.push(Action::Run(id, json!({"key": key, "write": write && id == "mail.noAnswerFollowUp"})));
    }
}
