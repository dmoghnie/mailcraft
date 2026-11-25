//! Add or change an MCP server MailCraft's AI may use (File › Options › AI): a program
//! MailCraft starts, or a server on the web; its name, and its key, which goes to the keychain.

use mailcraft_engine::McpServer;
use mailcraft_engine::model::{McpTarget, Secret};

use super::*;

/// What the dialog holds while it's open.
#[derive(Clone, Debug, Default)]
pub struct ServerForm {
    /// The server changed; none for a new one.
    pub id: Option<String>,
    pub name: String,
    /// A server on the web, not a program.
    pub web: bool,
    pub command: String,
    /// What the program is given, one a line.
    pub args: String,
    /// The environment variable the program's key goes in.
    pub key_env: String,
    pub url: String,
    /// A key typed in, to keep (its `Debug` is redacted); empty keeps the one kept.
    pub key: Secret,
    pub has_key: bool,
    pub forget_key: bool,
    pub on: bool,
    pub error: Option<String>,
}

impl ServerForm {
    /// A new server, on once saved.
    pub fn new() -> ServerForm {
        ServerForm { on: true, ..ServerForm::default() }
    }

    /// A server, to change.
    pub fn of(m: &McpServer) -> ServerForm {
        let mut f = ServerForm { id: Some(m.id.clone()), name: m.name.clone(), has_key: m.has_key, on: m.on, ..ServerForm::default() };
        match &m.target {
            McpTarget::Program { command, args, key_env } => {
                f.command = command.clone();
                f.args = args.join("\n");
                f.key_env = key_env.clone().unwrap_or_default();
            }
            McpTarget::Web { url, .. } => {
                f.web = true;
                f.url = url.clone();
            }
        }
        f
    }

    fn ready(&self) -> bool {
        !self.name.trim().is_empty() && !(if self.web { &self.url } else { &self.command }).trim().is_empty()
    }

    /// `mcpServer.save`'s params: a key only when one is typed or forgotten, so one kept stays.
    fn params(&self) -> serde_json::Value {
        let mut p = json!({"name": self.name.trim(), "on": self.on});
        if self.web {
            p["url"] = json!(self.url.trim());
        } else {
            let args: Vec<&str> = self.args.lines().map(str::trim).filter(|a| !a.is_empty()).collect();
            p["command"] = json!(self.command.trim());
            p["args"] = json!(args);
            p["keyEnv"] = json!(self.key_env.trim());
        }
        if let Some(id) = &self.id {
            p["id"] = json!(id);
        }
        let key = self.key.0.trim();
        if !key.is_empty() {
            p["key"] = json!(key);
        } else if self.forget_key {
            p["key"] = json!("");
        }
        p
    }
}

pub(crate) fn draw(ui: &mut Ui, f: &mut ServerForm, t: &Tokens, acts: &mut Vec<Action>) -> bool {
    ui.set_width(520.0);
    title(ui, t, if f.id.is_some() { "Change an MCP server" } else { "Add an MCP server" });
    let w = ui.available_width();
    let field = |ui: &mut Ui, label: &str| {
        ui.add_space(6.0);
        ui.label(RichText::new(label).color(t.text));
    };
    let note = |ui: &mut Ui, text: &str| {
        ui.add(egui::Label::new(RichText::new(text).color(t.faint).small()).wrap());
    };
    field(ui, tl!("Name"));
    ui.add(egui::TextEdit::singleline(&mut f.name).hint_text("notes").desired_width(w));
    note(ui, tl!("Lower-case letters, digits, _ and -: MailCraft's AI calls it by this name"));
    ui.add_space(6.0);
    ui.radio_value(&mut f.web, false, tl!("A program MailCraft starts on this computer"));
    ui.radio_value(&mut f.web, true, tl!("A server on the web"));
    if f.web {
        field(ui, tl!("Address"));
        ui.add(egui::TextEdit::singleline(&mut f.url).hint_text("https://mcp.example.com/mcp").desired_width(w));
        note(ui, tl!("https, or http to this computer alone. The key goes to it as a bearer token"));
    } else {
        field(ui, tl!("Program"));
        ui.add(egui::TextEdit::singleline(&mut f.command).hint_text("npx").desired_width(w));
        field(ui, tl!("What it's given, one a line"));
        ui.add(egui::TextEdit::multiline(&mut f.args).desired_rows(3).desired_width(w));
        field(ui, tl!("Environment variable for the key"));
        ui.add(egui::TextEdit::singleline(&mut f.key_env).hint_text("API_KEY").desired_width(w));
        note(ui, tl!("The program reads its key from it, if it takes one"));
    }
    field(ui, tl!("API key"));
    let hint = if f.has_key && !f.forget_key {
        tl!("Kept in the keychain: type to change it")
    } else {
        tl!("None needed for most servers on this computer")
    };
    ui.add(egui::TextEdit::singleline(&mut f.key.0).password(true).hint_text(hint).desired_width(w));
    if f.has_key {
        ui.checkbox(&mut f.forget_key, tl!("Forget the key kept"));
    }
    ui.add_space(6.0);
    ui.checkbox(&mut f.on, tl!("On: MailCraft's AI may use its tools"));
    let warn = if f.web {
        tl!("What MailCraft's AI sends it leaves this computer, unless the address is on this computer or its network")
    } else {
        tl!("The program runs with your permissions: add only one you trust")
    };
    ui.add_space(6.0);
    ui.add(egui::Label::new(RichText::new(warn).color(t.dim)).wrap());
    if let Some(e) = &f.error {
        ui.add_space(6.0);
        ui.add(egui::Label::new(RichText::new(e.as_str()).color(t.danger)).wrap());
    }
    let ready = f.ready();
    let (ok, cancel) = buttons(ui, t, "Save", ready);
    if ok && ready {
        f.error = None;
        acts.push(Action::Submit("mcpServer.save", f.params()));
    }
    !cancel
}
