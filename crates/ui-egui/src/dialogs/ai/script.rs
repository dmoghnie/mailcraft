//! Add or change a script MailCraft's AI keeps (File › Options › AI): its name, what it does,
//! what it takes, and its code, in Rhai, coloured as it's written ([`highlight`]). Saved through
//! `script.save`, which checks it compiles; tried first, as it is, through `script.run`.

use mailcraft_engine::KeptScript;
use serde_json::Value;

use super::*;

mod highlight;

/// The most characters of what trying a script gave that the dialog shows.
const TRIED_MOST: usize = 10_000;

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
    /// What to try it with, one a line: the name of one it takes, then `:` and its value.
    pub try_with: String,
    /// What trying it gave: what it printed and its value, or why it failed.
    pub tried: Option<Result<String, String>>,
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
            ..ScriptForm::default()
        }
    }

    fn ready(&self) -> bool {
        [&self.name, &self.about, &self.code].iter().all(|f| !f.trim().is_empty())
    }

    /// Whether it takes anything.
    fn takes(&self) -> bool {
        self.params.lines().any(|l| !l.trim().is_empty())
    }

    /// `script.save`'s params.
    fn params(&self) -> Value {
        let params: Vec<Value> = self
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

    /// `script.run`'s params to try the code as it is: what it takes, from [`ScriptForm::try_with`],
    /// each value read as JSON (`7`, `true`, `["a"]`), else as the text it is.
    fn try_params(&self) -> Value {
        let given: serde_json::Map<String, Value> = self
            .try_with
            .lines()
            .filter_map(|l| {
                let (name, value) = l.split_once(':')?;
                let (name, value) = (name.trim(), value.trim());
                let value = serde_json::from_str(value).unwrap_or_else(|_| json!(value));
                (!name.is_empty()).then(|| (name.to_string(), value))
            })
            .collect();
        json!({"code": self.code, "params": given})
    }

    /// What `script.run` gave, to show: what the script printed, then its value; or why it
    /// failed.
    pub(crate) fn take_tried(&mut self, ran: Result<Value, String>) {
        let shown = ran.map(|v| {
            let printed = v["printed"].as_str().unwrap_or_default().trim_end();
            let value = match &v["value"] {
                Value::String(s) => s.clone(),
                Value::Null => "()".to_string(),
                other => serde_json::to_string_pretty(other).unwrap_or_default(),
            };
            let value = tf!("Its value: {value}", value = value);
            if printed.is_empty() { value } else { format!("{printed}\n{value}") }
        });
        let cut = |s: String| if s.chars().count() > TRIED_MOST { s.chars().take(TRIED_MOST).chain(std::iter::once('…')).collect() } else { s };
        self.tried = Some(shown.map(cut).map_err(cut));
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
    let mut coloured = |ui: &Ui, code: &dyn egui::TextBuffer, wrap: f32| highlight::layout(ui, code.as_str(), wrap, t);
    egui::ScrollArea::vertical().id_salt("script-code").max_height(280.0).show(ui, |ui| {
        ui.add(egui::TextEdit::multiline(&mut f.code).code_editor().desired_rows(12).desired_width(w).layouter(&mut coloured));
    });
    if f.takes() {
        field(ui, tl!("Try it with"));
        ui.add(egui::TextEdit::multiline(&mut f.try_with).hint_text("days: 7").desired_rows(2).desired_width(w));
        note(ui, tl!("One a line: the name of one it takes, then its value"));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.add_enabled(!f.code.trim().is_empty(), egui::Button::new(tl!("Try"))).clicked() {
            acts.push(Action::TryScript(f.try_params()));
        }
        note(ui, tl!("Trying it only reads: nothing is changed, and no model is asked"));
    });
    if let Some(tried) = &f.tried {
        let (text, colour) = match tried {
            Ok(text) => (text.as_str(), t.text),
            Err(why) => (why.as_str(), t.danger),
        };
        egui::ScrollArea::vertical().id_salt("script-tried").max_height(140.0).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).monospace().color(colour)).wrap());
        });
    }
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
