//! Add or change a script MailCraft's AI keeps (File › Options › AI): its name, what it does,
//! what it takes, and its code, in Rhai. Saved through `script.save`, which checks it compiles.

use mailcraft_engine::KeptScript;

use super::*;

/// What the dialog holds while it's open.
#[derive(Clone, Debug, Default)]
pub struct ScriptForm {
    /// The name it had; none for a new one.
    pub was: Option<String>,
    pub name: String,
    pub about: String,
    /// What it takes, one a line: its name, then `:` and what it is.
    pub params: String,
    pub code: String,
    /// Kept turned off.
    pub off: bool,
    pub error: Option<String>,
}

impl ScriptForm {
    /// A kept script, to change.
    pub fn of(k: &KeptScript) -> ScriptForm {
        let params: Vec<String> =
            k.params.iter().map(|p| if p.about.is_empty() { p.name.clone() } else { format!("{}: {}", p.name, p.about) }).collect();
        ScriptForm {
            was: Some(k.name.clone()),
            name: k.name.clone(),
            about: k.about.clone(),
            params: params.join("\n"),
            code: k.code.clone(),
            off: k.off,
            error: None,
        }
    }

    fn ready(&self) -> bool {
        [&self.name, &self.about, &self.code].iter().all(|f| !f.trim().is_empty())
    }

    /// `script.save`'s params.
    fn params(&self) -> serde_json::Value {
        let params: Vec<serde_json::Value> = self
            .params
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| {
                let (name, about) = l.split_once(':').unwrap_or((l, ""));
                json!({"name": name.trim(), "about": about.trim()})
            })
            .collect();
        let mut p = json!({"name": self.name.trim(), "about": self.about.trim(), "params": params, "code": self.code, "off": self.off});
        if let Some(was) = &self.was {
            p["was"] = json!(was);
        }
        p
    }
}

pub(crate) fn draw(ui: &mut Ui, f: &mut ScriptForm, t: &Tokens, acts: &mut Vec<Action>) -> bool {
    ui.set_width(600.0);
    title(ui, t, if f.was.is_some() { "Change a script" } else { "Add a script" });
    let w = ui.available_width();
    let field = |ui: &mut Ui, label: &str| {
        ui.add_space(6.0);
        ui.label(RichText::new(label).color(t.text));
    };
    let note = |ui: &mut Ui, text: &str| {
        ui.add(egui::Label::new(RichText::new(text).color(t.faint).small()).wrap());
    };
    field(ui, tl!("Name"));
    ui.add(egui::TextEdit::singleline(&mut f.name).hint_text("waiting_on_me").desired_width(w));
    note(ui, tl!("Lower-case letters, digits and _: other scripts call it by this name"));
    field(ui, tl!("What it does"));
    ui.add(egui::TextEdit::multiline(&mut f.about).desired_rows(2).desired_width(w));
    note(ui, tl!("MailCraft's AI reads this to know when to run it"));
    field(ui, tl!("What it takes"));
    ui.add(egui::TextEdit::multiline(&mut f.params).hint_text("days: how many days old at least").desired_rows(2).desired_width(w));
    note(ui, tl!("One a line: its name, then what it is. The script reads each from params, as params.days"));
    field(ui, tl!("Code, in Rhai"));
    egui::ScrollArea::vertical().id_salt("script-code").max_height(280.0).show(ui, |ui| {
        ui.add(egui::TextEdit::multiline(&mut f.code).code_editor().desired_rows(12).desired_width(w));
    });
    let mut on = !f.off;
    ui.add_space(6.0);
    ui.checkbox(&mut on, tl!("On: MailCraft's AI can run it"));
    f.off = !on;
    if let Some(e) = &f.error {
        ui.add_space(6.0);
        ui.add(egui::Label::new(RichText::new(e.as_str()).color(t.danger)).wrap());
    }
    let ready = f.ready();
    let (ok, cancel) = buttons(ui, t, "Save", ready);
    if ok && ready {
        f.error = None;
        acts.push(Action::Submit("script.save", f.params()));
    }
    !cancel
}
