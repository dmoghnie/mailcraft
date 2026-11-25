//! Add or change a model MailCraft's own AI may ask (File › Options › AI): its server, the
//! format it speaks, the model, and its key, which goes to the keychain.

use mailcraft_engine::AiProfile;
use mailcraft_engine::model::{AiApi, AiEffort, Secret};

use super::*;

/// Servers whose address and format are known, to fill in: two that run on this computer, two
/// cloud providers. Their names are their own, in every language.
const KNOWN: [(&str, AiApi, &str); 4] = [
    ("Ollama", AiApi::Ollama, "http://localhost:11434"),
    ("LM Studio", AiApi::OpenAi, "http://localhost:1234/v1"),
    ("OpenAI", AiApi::OpenAi, "https://api.openai.com/v1"),
    ("Anthropic", AiApi::Anthropic, "https://api.anthropic.com/v1"),
];

/// What the dialog holds while it's open.
#[derive(Clone, Debug, Default)]
pub struct AiModelForm {
    /// The profile changed; none for a new one.
    pub id: Option<String>,
    pub name: String,
    pub api: AiApi,
    pub url: String,
    pub model: String,
    /// A key typed in, to keep (its `Debug` is redacted); empty keeps the one kept.
    pub key: Secret,
    /// A key is kept for the profile already.
    pub has_key: bool,
    /// The kept key is to be forgotten.
    pub forget_key: bool,
    /// The longest answer, in tokens, as typed.
    pub max_tokens: String,
    /// What the model takes in at once, in tokens, as typed (empty: as at first).
    pub context: String,
    /// Its temperature, as typed (empty: the model's own).
    pub temperature: String,
    /// How much it thinks; none: as the model does.
    pub effort: Option<AiEffort>,
    /// The most tokens it may use in a month, as typed (empty: no limit).
    pub budget: String,
    /// The profile asked instead when this one fails.
    pub fallback: Option<String>,
    /// The assistant may run scripts it writes with it.
    pub scripts: bool,
    pub error: Option<String>,
}

impl AiModelForm {
    /// A profile, to change.
    pub fn of(p: &AiProfile) -> AiModelForm {
        AiModelForm {
            id: Some(p.id.clone()),
            name: p.name.clone(),
            api: p.endpoint.api,
            url: p.endpoint.url.clone(),
            model: p.endpoint.model.clone(),
            has_key: p.has_key,
            max_tokens: p.max_tokens.to_string(),
            context: if p.context > 0 { p.context.to_string() } else { String::new() },
            temperature: p.temperature.map(|t| (f64::from(t) / 100.0).to_string()).unwrap_or_default(),
            effort: p.effort,
            budget: p.budget.map(|b| b.to_string()).unwrap_or_default(),
            fallback: p.fallback.clone(),
            scripts: p.scripts,
            ..AiModelForm::default()
        }
    }

    fn ready(&self) -> bool {
        [&self.name, &self.url, &self.model].iter().all(|f| !f.trim().is_empty())
    }

    /// `ai.saveProfile`'s params: a key only when one is typed or forgotten, so one kept stays.
    fn params(&self) -> serde_json::Value {
        let api = match self.api {
            AiApi::OpenAi => "openAi",
            AiApi::Anthropic => "anthropic",
            AiApi::Ollama => "ollama",
        };
        let mut p = json!({"name": self.name.trim(), "api": api, "url": self.url.trim(), "model": self.model.trim(), "fallback": self.fallback.clone().unwrap_or_default(), "scripts": self.scripts});
        if let Some(id) = &self.id {
            p["id"] = json!(id);
        }
        if let Ok(n) = self.max_tokens.trim().parse::<u64>() {
            p["maxTokens"] = json!(n);
        }
        let context = self.context.trim();
        if context.is_empty() {
            p["context"] = json!(0);
        } else if let Ok(n) = context.parse::<u64>() {
            p["context"] = json!(n);
        }
        // One that isn't a number goes as typed, for the engine to say what it should be.
        let temperature = self.temperature.trim().replace(',', ".");
        p["temperature"] = match temperature.parse::<f64>().ok().filter(|t| t.is_finite()) {
            _ if temperature.is_empty() => serde_json::Value::Null,
            Some(t) => json!(t),
            None => json!(temperature),
        };
        p["effort"] = json!(self.effort.map_or("default", AiEffort::code));
        let budget = self.budget.trim();
        p["budget"] = match budget.parse::<u64>() {
            _ if budget.is_empty() => serde_json::Value::Null,
            Ok(b) => json!(b),
            Err(_) => json!(budget),
        };
        let key = self.key.0.trim();
        if !key.is_empty() {
            p["key"] = json!(key);
        } else if self.forget_key {
            p["key"] = json!("");
        }
        p
    }
}

pub(crate) fn draw(ui: &mut Ui, f: &mut AiModelForm, s: &Session, t: &Tokens, acts: &mut Vec<Action>) -> bool {
    ui.set_width(500.0);
    title(ui, t, if f.id.is_some() { "Change a model" } else { "Add a model" });
    if f.id.is_none() {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tl!("Fill in for")).color(t.dim));
            for (name, api, url) in KNOWN {
                if ui.button(name).clicked() {
                    f.name = name.to_string();
                    f.api = api;
                    f.url = url.to_string();
                }
            }
        });
        ui.add_space(8.0);
    }
    let w = ui.available_width();
    let field = |ui: &mut Ui, label: &str| {
        ui.add_space(6.0);
        ui.label(RichText::new(label).color(t.text));
    };
    field(ui, tl!("Name"));
    ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(w));
    field(ui, tl!("Format"));
    // One a line: three in a row are wider than the dialog in some languages.
    for (api, label) in
        [(AiApi::OpenAi, tl!("OpenAI's, which most servers speak")), (AiApi::Anthropic, tl!("Anthropic's")), (AiApi::Ollama, tl!("Ollama's own"))]
    {
        ui.radio_value(&mut f.api, api, label);
    }
    field(ui, tl!("Address"));
    ui.add(egui::TextEdit::singleline(&mut f.url).hint_text("https://api.example.com/v1").desired_width(w));
    ui.label(RichText::new(tl!("Up to its version, such as /v1. Plain http only to this computer")).color(t.faint).small());
    field(ui, tl!("Model"));
    ui.add(egui::TextEdit::singleline(&mut f.model).desired_width(w));
    field(ui, tl!("API key"));
    let hint =
        if f.has_key && !f.forget_key { tl!("Kept in the keychain: type to change it") } else { tl!("None needed for a model on this computer") };
    ui.add(egui::TextEdit::singleline(&mut f.key.0).password(true).hint_text(hint).desired_width(w));
    if f.has_key {
        ui.checkbox(&mut f.forget_key, tl!("Forget the key kept"));
    }
    field(ui, tl!("Longest answer, in tokens"));
    ui.add(egui::TextEdit::singleline(&mut f.max_tokens).hint_text("4096").desired_width(120.0));
    field(ui, tl!("Takes in at once, in tokens"));
    let at_first = mailcraft_engine::CONTEXT_AT_FIRST.to_string();
    ui.add(egui::TextEdit::singleline(&mut f.context).hint_text(at_first).desired_width(120.0));
    tuning(ui, f, t);
    fallback(ui, f, s);
    ui.add_space(6.0);
    ui.checkbox(&mut f.scripts, tl!("Let it write and run scripts"));
    let scripts = tl!(
        "A script finds and reads mail, events, contacts, tasks and notes in one step rather than many. It runs inside MailCraft, with no files or network, and a change it calls for waits for your approval."
    );
    ui.add(egui::Label::new(RichText::new(scripts).color(t.faint).small()).wrap());
    let local = mailcraft_engine::ai_url_is_local(&f.url);
    let place = if f.url.trim().is_empty() {
        None
    } else if local {
        Some(tl!("On this computer or its network"))
    } else {
        Some(tl!("At a cloud provider: mail you let go to it leaves this computer"))
    };
    if let Some(place) = place {
        ui.add_space(8.0);
        ui.add(egui::Label::new(RichText::new(place).color(t.dim)).wrap());
    }
    if let Some(e) = &f.error {
        ui.add_space(6.0);
        ui.add(egui::Label::new(RichText::new(e.as_str()).color(t.danger)).wrap());
    }
    let ready = f.ready();
    let (ok, cancel) = buttons(ui, t, "Save", ready);
    if ok && ready {
        f.error = None;
        acts.push(Action::Submit("ai.saveProfile", f.params()));
    }
    !cancel
}

/// Its temperature and how much it thinks, each the model's own unless set.
fn tuning(ui: &mut Ui, f: &mut AiModelForm, t: &Tokens) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(tl!("Temperature")).color(t.text));
        ui.add(egui::TextEdit::singleline(&mut f.temperature).hint_text(tl!("Its own")).desired_width(70.0));
        ui.add_space(16.0);
        ui.label(RichText::new(tl!("Thinking")).color(t.text));
        egui::ComboBox::from_id_salt("ai-model-thinking").selected_text(thinking(f.effort)).show_ui(ui, |ui| {
            for e in std::iter::once(None).chain(AiEffort::ALL.map(Some)) {
                ui.selectable_value(&mut f.effort, e, thinking(e));
            }
        });
    });
    let note = tl!("Not every model takes these: Test shows whether it does.");
    ui.add(egui::Label::new(RichText::new(note).color(t.faint).small()).wrap());
    ui.add_space(6.0);
    ui.label(RichText::new(tl!("Tokens a month, at most")).color(t.text));
    ui.add(egui::TextEdit::singleline(&mut f.budget).hint_text(tl!("No limit")).desired_width(120.0));
    let note = tl!("As its server counts them. Past them, the model it falls back to is asked instead.");
    ui.add(egui::Label::new(RichText::new(note).color(t.faint).small()).wrap());
}

/// How much a model thinks, as the dialog names it.
fn thinking(e: Option<AiEffort>) -> &'static str {
    match e {
        None => tl!("Its own"),
        Some(AiEffort::Off) => tl!("Off"),
        Some(AiEffort::Low) => tl!("Low"),
        Some(AiEffort::Medium) => tl!("Medium"),
        Some(AiEffort::High) => tl!("High"),
    }
}

/// The model asked instead when this one fails: none, or another.
fn fallback(ui: &mut Ui, f: &mut AiModelForm, s: &Session) {
    let others: Vec<&AiProfile> = s.prefs.ai.profiles.iter().filter(|p| Some(&p.id) != f.id.as_ref()).collect();
    if others.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.label(tl!("When it fails, ask"));
    let none = tl!("No other model");
    let shown = f.fallback.as_ref().and_then(|id| others.iter().find(|p| &p.id == id)).map_or(none, |p| p.name.as_str());
    egui::ComboBox::from_id_salt("ai-fallback").width(240.0).selected_text(shown).show_ui(ui, |ui| {
        if ui.selectable_label(f.fallback.is_none(), none).clicked() {
            f.fallback = None;
        }
        for p in &others {
            if ui.selectable_label(f.fallback.as_ref() == Some(&p.id), p.name.as_str()).clicked() {
                f.fallback = Some(p.id.clone());
            }
        }
    });
}
