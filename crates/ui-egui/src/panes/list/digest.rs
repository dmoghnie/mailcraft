//! The digest over the Inbox: MailCraft's AI's sorting of the mail that came in (what needs you,
//! what's waiting on others, what's worth a look, what can be ignored), shown as it's written,
//! with the model that wrote it, Refresh and Close; folded away as No answer yet is.

use super::*;

/// Its key among what's folded away (`Prefs::collapsed`).
const FOLDED: &str = "digest";
/// The most of the list it takes, in points: past it, it scrolls.
const TALLEST: f32 = 260.0;

pub(super) fn section(s: &Session, t: &theme::Tokens, ui: &mut Ui, acts: &mut Vec<Action>) {
    let Some(d) = s.digest().filter(|d| !d.closed) else { return };
    if s.search.is_some() || s.view_role() != Some(FolderRole::Inbox) {
        return;
    }
    let text = match d.asking.and_then(|id| s.ai.part(id)) {
        Some((part, _)) => part.to_string(),
        None => d.text.clone(),
    };
    let open = !s.prefs.collapsed.contains(FOLDED);
    let top = ui.available_rect_before_wrap();
    ui.painter().line_segment([top.left_top(), top.right_top()], egui::Stroke::new(1.0, t.border));
    egui::Frame::new().inner_margin(egui::Margin { left: 8, right: 10, top: 4, bottom: 6 }).show(ui, |ui| {
        ui.with_layout(crate::bidi::column(), |ui| {
            heading(s, t, ui, d.at, open, acts);
            if !open {
                return;
            }
            if d.asking.is_some() && text.trim().is_empty() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(tl!("Writing the digest…")).color(t.dim));
                });
            }
            if !text.trim().is_empty() {
                egui::ScrollArea::vertical().id_salt("digest").max_height(TALLEST).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(text).color(t.text)).selectable(true).wrap());
                });
            }
            if let Some(e) = &d.error {
                ui.label(RichText::new(tf!("The digest couldn't be written: {error}", error = e)).color(t.danger));
            }
            if d.cut {
                ui.label(RichText::new(tl!("The answer was cut short at the longest answer set for this model.")).small().color(t.faint));
            }
            if d.left_out > 0 {
                ui.label(RichText::new(tf!("Older messages left out for room: {count}", count = d.left_out)).small().color(t.faint));
            }
            ui.horizontal(|ui| {
                if let Some(by) = s.prefs.ai.profile(&d.profile).map(crate::panes::assistant::where_it_is) {
                    ui.label(RichText::new(by).small().color(t.dim));
                }
                if d.asking.is_none() && ui.small_button(tl!("Refresh")).clicked() {
                    acts.push(Action::Run("mail.digest", json!({"hours": d.hours, "language": crate::i18n::current().code()})));
                }
            });
        });
    });
}

/// Its heading: an arrow (a click folds it away or shows it), Digest and when it was made, and
/// Close.
fn heading(s: &Session, t: &theme::Tokens, ui: &mut Ui, at: i64, open: bool, acts: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width() - 28.0, 24.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 4.0, t.hover_row);
        }
        let m = crate::bidi::Mirror::new(r);
        let arrow = Rect::from_center_size(m.pos(pos2(r.min.x + 9.0, r.center().y)), vec2(10.0, 10.0));
        icons::paint(ui.painter(), arrow, if open { Icon::ChevronDown } else { m.closed() }, t.dim);
        let when = crate::i18n::date(&dates::list_time(at, &s.clock));
        let text = format!("{} · {when}", tl!("Digest"));
        ui.painter().text(m.pos(pos2(r.min.x + 20.0, r.center().y)), m.align(egui::Align2::LEFT_CENTER), &text, theme::bold(12.5), t.dim);
        widgets::named(ui, &resp, &text, true, Some(open));
        if resp.clicked() {
            acts.push(Action::Run("view.collapse", json!({"key": FOLDED})));
        }
        if widgets::action_icon(ui, Icon::Close, "Close the digest", t).clicked() {
            acts.push(Action::Run("mail.closeDigest", json!({})));
        }
    });
}
