//! Options' AI cards: the models MailCraft's own AI may ask, how far each account's mail may go
//! to them, the scripts it keeps, the MCP servers its scripts use, and the record of what went where. Nothing is sent until a model is added and an
//! account's mail let go to it.

use super::*;
use crate::dialogs::{AiModelForm, ScriptForm, ServerForm};
use mailcraft_engine::model::{McpTarget, dates};
use mailcraft_engine::{AiProfile, AiReach, AiTask, AiTest, KeptScript, McpServer, ServerTools};

/// The newest questions the record shows.
const RECORD_SHOWN: usize = 20;
/// The most of a test's answer shown, in characters.
const ANSWER_SHOWN: usize = 120;
/// How long chats can be kept, in days (0: until deleted).
const KEEP_CHATS: [u32; 5] = [0, 7, 30, 90, 365];

/// The models, and the switch that keeps every question on this computer or its network.
pub(super) fn models(s: &Session, t: &Tokens, ui: &mut Ui, acts: &mut Vec<Action>) {
    let ai = &s.prefs.ai;
    let about =
        tl!("A language model you add can answer questions about your mail. Nothing goes to one until you add it and let an account's mail go to it");
    card(ui, t, tl!("AI"), Some(about), |ui| {
        let local_only = Switch {
            label: tl!("Use models on this computer or its network alone"),
            about: Some(tl!("No mail goes to a cloud provider, whatever an account allows")),
            on: ai.local_only,
            cmd: "ai.localOnly",
        };
        switch_row(ui, t, true, local_only, acts);
        for p in &ai.profiles {
            profile(s, t, ui, p, acts);
        }
        let more = tl!("On this computer, such as Ollama, or at a cloud provider, with its key");
        setting(ui, t, false, tl!("Add a model"), Some(more), |ui| {
            if ui.button(tl!("Add…")).clicked() {
                acts.push(Action::Dialog(Dialog::AiModel(Box::default())));
            }
        });
        if ai.profiles.len() > 1 {
            uses(s, t, ui, acts);
        }
        if !ai.profiles.is_empty() {
            let needs_me = Switch {
                label: tl!("Mark mail that needs me"),
                about: Some(tl!(
                    "Mail arriving in the Inbox is read by the model for quick decisions, where its account's mail may go, and marked Needs me when it asks something of you"
                )),
                on: ai.needs_me,
                cmd: "ai.needsMe",
            };
            switch_row(ui, t, false, needs_me, acts);
        }
        keep_chats(t, ui, ai.chats_days, acts);
    });
}

/// How long the chats with the assistant are kept: until deleted, or so many days.
fn keep_chats(t: &Tokens, ui: &mut Ui, days: u32, acts: &mut Vec<Action>) {
    let about = tl!("Chats hold what MailCraft's AI was given and said, mail included. Older ones are deleted from this computer");
    let shown = |d: u32| if d == 0 { tl!("Always").to_string() } else { tf!("For {days} days", days = d) };
    setting(ui, t, false, tl!("Keep chats"), Some(about), |ui| {
        let r = egui::ComboBox::from_id_salt("ai-keep-chats")
            .width(160.0)
            .selected_text(shown(days))
            .show_ui(ui, |ui| {
                for d in KEEP_CHATS {
                    if ui.selectable_label(d == days, shown(d)).clicked() && d != days {
                        acts.push(Action::Run("ai.keepChats", json!({"days": d})));
                    }
                }
            })
            .response
            // The setting's name is beside it in the row, which doesn't hand it on.
            .accessible_name(tl!("Keep chats"));
        let mut chosen = days;
        crate::drop_down::keys(ui, &r, &mut chosen, &KEEP_CHATS);
        if chosen != days {
            acts.push(Action::Run("ai.keepChats", json!({"days": chosen})));
        }
    });
}

/// A model: its name; what it is and where; its key and its last test; and what can be done.
fn profile(s: &Session, t: &Tokens, ui: &mut Ui, p: &AiProfile, acts: &mut Vec<Action>) {
    let mut lines = vec![tf!("{model} at {server}", model = p.endpoint.model, server = mailcraft_engine::ai_server(&p.endpoint.url))];
    lines.push(if p.local() {
        tl!("On this computer or its network").to_string()
    } else {
        tl!("At a cloud provider: mail you let go to it leaves this computer").to_string()
    });
    let used = s.ai_used(&p.id);
    match p.budget {
        Some(b) if s.ai_over_budget(p) => {
            lines.push(tf!("It has used its {budget} tokens this month: questions go to the model it falls back to", budget = b))
        }
        Some(b) => lines.push(tf!("{used} of {budget} tokens used this month", used = used, budget = b)),
        None if used > 0 => lines.push(tf!("{used} tokens used this month", used = used)),
        None => {}
    }
    if let Some(e) = s.ai.key_errors.get(&p.id) {
        lines.push(tf!("Its key couldn't be kept: {error}", error = e));
    } else if p.has_key {
        lines.push(tl!("Its key is kept in this computer's keychain").to_string());
    }
    let testing = matches!(s.ai.tests.get(&p.id), Some(AiTest::Asking));
    match s.ai.tests.get(&p.id) {
        Some(AiTest::Asking) => lines.push(tl!("Testing…").to_string()),
        Some(AiTest::Answered { text }) => {
            let text: String = text.chars().filter(|c| !c.is_control()).take(ANSWER_SHOWN).collect();
            lines.push(tf!("It answered: {text}", text = text));
        }
        Some(AiTest::Failed { error }) => lines.push(tf!("The test failed: {error}", error = error)),
        None => {}
    }
    let about = lines.join("\n");
    setting(ui, t, false, &p.name, Some(&about), |ui| {
        if button(ui, &p.name, tl!("Remove"), true).clicked() {
            acts.push(Action::Run("ai.removeProfile", json!({"id": p.id})));
        }
        if button(ui, &p.name, tl!("Change…"), true).clicked() {
            acts.push(Action::Dialog(Dialog::AiModel(Box::new(AiModelForm::of(p)))));
        }
        if button(ui, &p.name, tl!("Test"), !testing).clicked() {
            acts.push(Action::Run("ai.test", json!({"profile": p.id})));
        }
    });
}

/// One of a model's or a script's buttons, named to a screen reader with whose it is first
/// (`Ollama: Test`): each row has the same ones.
fn button(ui: &mut Ui, whose: &str, label: &str, enabled: bool) -> egui::Response {
    let resp = ui.add_enabled(enabled, egui::Button::new(label));
    widgets::named(ui, &resp, &format!("{whose}: {label}"), enabled, None);
    resp
}

/// The scripts MailCraft's AI keeps, MailCraft's own among them, once there's a model: each
/// turned on or off, changed or deleted; and one added.
pub(super) fn scripts(s: &Session, t: &Tokens, ui: &mut Ui, acts: &mut Vec<Action>) {
    if s.prefs.ai.profiles.is_empty() {
        return;
    }
    let about = tl!(
        "Scripts MailCraft's AI kept to run again, and MailCraft's own. A model you let write and run scripts can run them; a change one calls for waits for your approval"
    );
    let kept = s.ai.assistant.kept_scripts();
    card(ui, t, tl!("Scripts"), Some(about), |ui| {
        for (i, k) in kept.iter().enumerate() {
            kept_script(t, ui, i == 0, k, acts);
        }
        let more = tl!("Written in Rhai, as MailCraft's AI writes them");
        setting(ui, t, kept.is_empty(), tl!("Add a script"), Some(more), |ui| {
            if ui.button(tl!("Add…")).clicked() {
                acts.push(Action::Dialog(Dialog::Script(Box::default())));
            }
        });
    });
}

/// A kept script: its name and what it does, its buttons, and the switch that turns it on.
fn kept_script(t: &Tokens, ui: &mut Ui, first: bool, k: &KeptScript, acts: &mut Vec<Action>) {
    setting(ui, t, first, &k.name, Some(&k.about), |ui| {
        if button(ui, &k.name, tl!("Delete"), true).clicked() {
            acts.push(Action::Run("script.delete", json!({"name": k.name})));
        }
        if button(ui, &k.name, tl!("Change…"), true).clicked() {
            acts.push(Action::Dialog(Dialog::Script(Box::new(ScriptForm::of(k)))));
        }
        let mut on = !k.off;
        if widgets::switch(ui, t, &mut on, &k.name).changed() {
            acts.push(Action::Run("script.turn", json!({"name": k.name, "on": on})));
        }
    });
}

/// The MCP servers MailCraft's AI may use, once there's a model: each turned on or off,
/// changed, listed again or removed; and one added.
pub(super) fn servers(s: &Session, t: &Tokens, ui: &mut Ui, acts: &mut Vec<Action>) {
    if s.prefs.ai.profiles.is_empty() {
        return;
    }
    let about = tl!(
        "Tools of other programs and services, such as your notes or a search, for MailCraft's AI's scripts. Each is off until you turn it on, and gets an account's mail only as far as that account lets its mail go"
    );
    let servers = &s.prefs.ai.mcp;
    card(ui, t, tl!("MCP servers"), Some(about), |ui| {
        for (i, m) in servers.iter().enumerate() {
            server(s, t, ui, i == 0, m, acts);
        }
        let more = tl!("A program on this computer, or a server on the web");
        setting(ui, t, servers.is_empty(), tl!("Add an MCP server"), Some(more), |ui| {
            if ui.button(tl!("Add…")).clicked() {
                acts.push(Action::Dialog(Dialog::Server(Box::new(ServerForm::new()))));
            }
        });
    });
}

/// A server: its name; where it is, and what's known of its tools; its buttons, and the switch
/// that turns it on.
fn server(s: &Session, t: &Tokens, ui: &mut Ui, first: bool, m: &McpServer, acts: &mut Vec<Action>) {
    let known = s.ai.mcp_tools.as_ref().and_then(|k| k.get(&m.id));
    let mut lines = vec![match &m.target {
        McpTarget::Program { command, .. } => tf!("A program on this computer: {command}", command = command),
        McpTarget::Web { url, .. } => tf!("At {server}", server = mailcraft_engine::ai_server(url)),
    }];
    if !m.local() {
        lines.push(tl!("On the web: what MailCraft's AI sends it leaves this computer").to_string());
    }
    let listing = known.is_some_and(|k| k.listing);
    let (web, signed) = match &m.target {
        McpTarget::Web { oauth, .. } => (true, oauth.is_some()),
        McpTarget::Program { .. } => (false, false),
    };
    let signing = known.is_some_and(|k| k.signing);
    if signing {
        lines.push(tl!("Finish signing in in your browser").to_string());
    } else if signed {
        lines.push(tl!("Signed in").to_string());
    }
    match known {
        Some(k) if k.listing => lines.push(tl!("Listing its tools…").to_string()),
        Some(ServerTools { error: Some(e), .. }) => lines.push(tf!("Its tools couldn't be listed: {error}", error = e)),
        Some(k) if m.on => lines.push(tf!("Tools: {count}", count = k.tools.len())),
        _ => {}
    }
    if let Some(e) = s.ai.key_errors.get(&m.key_name()) {
        lines.push(tf!("Its key couldn't be kept: {error}", error = e));
    }
    let about = lines.join("\n");
    setting(ui, t, first, &m.name, Some(&about), |ui| {
        if button(ui, &m.name, tl!("Remove"), true).clicked() {
            acts.push(Action::Run("mcpServer.remove", json!({"id": m.id})));
        }
        if button(ui, &m.name, tl!("Change…"), true).clicked() {
            acts.push(Action::Dialog(Dialog::Server(Box::new(ServerForm::of(m)))));
        }
        if button(ui, &m.name, tl!("List again"), m.on && !listing).clicked() {
            acts.push(Action::Run("mcpServer.refresh", json!({"id": m.id})));
        }
        // A server on the web that asks for a sign-in: in the browser, with the person's account.
        if web && button(ui, &m.name, if signed { tl!("Sign in again…") } else { tl!("Sign in…") }, !signing).clicked() {
            acts.push(Action::Run("mcpServer.signIn", json!({"id": m.id})));
        }
        let mut on = m.on;
        if widgets::switch(ui, t, &mut on, &m.name).changed() {
            acts.push(Action::Run("mcpServer.turn", json!({"id": m.id, "on": on})));
        }
    });
}

/// Which model does each kind of work: the first one, unless another is chosen.
fn uses(s: &Session, t: &Tokens, ui: &mut Ui, acts: &mut Vec<Action>) {
    let ai = &s.prefs.ai;
    for (task, label, about) in [
        (AiTask::Chat, tl!("Model for questions"), tl!("Answers, summaries and drafts")),
        (AiTask::Decision, tl!("Model for quick decisions"), tl!("Short choices about mail: a small, fast model is enough")),
    ] {
        let chosen = ai.uses.get(&task).and_then(|id| ai.profile(id));
        let first = tl!("The first model");
        let shown = chosen.map_or(first, |p| p.name.as_str());
        setting(ui, t, false, label, Some(about), |ui| {
            egui::ComboBox::from_id_salt(("ai-use", task.code())).width(220.0).selected_text(shown).show_ui(ui, |ui| {
                if ui.selectable_label(chosen.is_none(), first).clicked() {
                    acts.push(Action::Run("ai.use", json!({"task": task.code(), "profile": ""})));
                }
                for p in &ai.profiles {
                    let on = chosen.is_some_and(|c| c.id == p.id);
                    if ui.selectable_label(on, p.name.as_str()).clicked() && !on {
                        acts.push(Action::Run("ai.use", json!({"task": task.code(), "profile": p.id})));
                    }
                }
            });
        });
    }
}

/// How far each account's mail may go: to no model, to those on this computer or its network,
/// or to any.
pub(super) fn accounts(s: &Session, t: &Tokens, ui: &mut Ui, acts: &mut Vec<Action>) {
    if s.prefs.ai.profiles.is_empty() || s.accounts.is_empty() {
        return;
    }
    let about = tl!("Off: no question asked holds its mail. Local models: only those on this computer or its network are asked");
    card(ui, t, tl!("Whose mail may go to a model"), Some(about), |ui| {
        let options = [tl!("Off"), tl!("Local models"), tl!("Any model")];
        for (i, a) in s.accounts.iter().enumerate() {
            let id = &a.config.id;
            let who = if a.config.email.is_empty() { a.config.name.as_str() } else { a.config.email.as_str() };
            let now = match s.prefs.ai.accounts.get(id) {
                None => 0,
                Some(AiReach::Local) => 1,
                Some(AiReach::Any) => 2,
            };
            setting(ui, t, i == 0, who, None, |ui| {
                if let Some(pick) = widgets::segmented_in(ui, t, &format!("ai-reach-{id}"), &options, who, now)
                    && pick != now
                    && let Some(reach) = ["off", "local", "any"].get(pick)
                {
                    acts.push(Action::Run("ai.allow", json!({"account": id, "reach": reach})));
                }
            });
        }
    });
}

/// What went where: the newest questions, by when, what for, to which model, whose mail and how
/// much. What they said is never kept.
pub(super) fn record(s: &Session, t: &Tokens, ui: &mut Ui) {
    if s.prefs.ai.profiles.is_empty() && s.ai.log.is_empty() {
        return;
    }
    let about = tl!("The newest questions MailCraft's AI asked: when, what for, which model, whose mail and how much. What they said isn't kept");
    card(ui, t, tl!("What was sent"), Some(about), |ui| {
        if s.ai.log.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new(tl!("Nothing sent yet.")).color(t.faint).small());
            ui.add_space(8.0);
        }
        for (i, (_, e)) in s.ai.log.iter().rev().take(RECORD_SHOWN).enumerate() {
            let when = crate::i18n::date(&dates::list_time(e.at, &s.clock));
            let purpose = match e.purpose.as_str() {
                "test" => tl!("A test").to_string(),
                "question" => tl!("A question").to_string(),
                other => other.to_string(),
            };
            let mut lines = vec![format!("{} · {}", e.profile, tf!("{model} at {server}", model = e.model, server = e.server))];
            if let Some(account) = &e.account {
                let who = s
                    .account(account)
                    .map_or(account.as_str(), |a| if a.config.email.is_empty() { a.config.name.as_str() } else { a.config.email.as_str() });
                lines.push(tf!("Mail of {account}", account = who));
            }
            lines.push(tf!(
                "{sent} sent, {received} back",
                sent = widgets::human_size(usize::try_from(e.sent).unwrap_or(usize::MAX)),
                received = widgets::human_size(usize::try_from(e.received).unwrap_or(usize::MAX))
            ));
            let outcome = match e.outcome.as_str() {
                "waiting" => tl!("Waiting"),
                "answered" => tl!("Answered"),
                _ => tl!("Failed"),
            };
            let about = lines.join("\n");
            setting(ui, t, i == 0, &format!("{when} · {purpose}"), Some(&about), |ui| {
                ui.label(RichText::new(outcome).color(if e.outcome == "failed" { t.danger } else { t.dim }));
            });
        }
    });
}
