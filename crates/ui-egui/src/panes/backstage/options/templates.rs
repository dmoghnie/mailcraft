//! File › Options › Mail › Templates: the message templates kept, each renamed or deleted here
//! (they're made from a message's More options).

use super::*;

pub(super) fn templates(s: &Session, t: &Tokens, ui: &mut Ui, acts: &mut Vec<Action>) {
    card(ui, t, tl!("Templates"), Some(tl!("Messages kept to start others from. Save one from a message's More options.")), |ui| {
        if s.templates().is_empty() {
            setting(ui, t, true, tl!("No templates yet"), None, |_| {});
        }
        for (i, kept) in s.templates().iter().enumerate() {
            let about = (!kept.subject.trim().is_empty()).then_some(kept.subject.as_str());
            setting(ui, t, i == 0, &kept.name, about, |ui| {
                if ui.button(tl!("Delete")).on_hover_text(tf!("Delete {name}", name = kept.name)).clicked() {
                    acts.push(Action::Run("template.delete", json!({"name": kept.name})));
                }
                if ui.button(tl!("Rename…")).on_hover_text(tf!("Rename {name}", name = kept.name)).clicked() {
                    acts.push(Action::Dialog(Dialog::rename_template(&kept.name)));
                }
            });
        }
    });
}
