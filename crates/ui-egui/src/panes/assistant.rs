//! MailCraft's AI assistant beside the mail (Home › Ask AI): a chat with a model, what it read
//! and what it proposes, each change approved or declined here, and the messages and text the
//! person points at going with what they say. The engine runs the chat (`assistant.*`); this
//! pane shows it and says what the person does.

use std::collections::HashMap;

use egui::{Margin, RichText, Ui};
use mailcraft_engine::{AiProfile, Chat, ContextItem, Session};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};
use crate::{Action, UiState, widgets};

pub(crate) mod about;
mod composer;
mod markdown;
#[cfg(test)]
mod tests;
mod turns;

/// The assistant's pane as the window keeps it between frames.
#[derive(Default)]
pub(crate) struct AssistantUi {
    /// The chat shown; none: a new one, begun by what's said next.
    pub(crate) chat: Option<String>,
    /// What's typed, and what it points at.
    pub(crate) draft: String,
    pub(crate) context: Vec<ContextItem>,
    /// The model a new chat asks (none: the one chat work goes to).
    pub(crate) profile: Option<String>,
    /// The text box takes the keyboard next frame.
    pub(crate) focus: bool,
    /// Delete all chats was pressed once: pressed again, they go.
    delete_all: bool,
    /// The answers' formatting, laid out.
    markdown: egui_commonmark::CommonMarkCache,
    /// What the things pointed at are called, by [`key`], as they were first found (none: a
    /// message no longer here).
    names: HashMap<String, Option<String>>,
}

impl AssistantUi {
    /// Point at `items` in what's said next (each once), and give the text box the keyboard.
    pub(crate) fn point_at(&mut self, items: Vec<ContextItem>) {
        for item in items {
            if !self.context.contains(&item) && self.context.len() < mailcraft_engine::CONTEXT_ITEMS_MOST {
                self.context.push(item);
            }
        }
        self.focus = true;
    }

    /// What was typed went to the chat: the box is empty again, and keeps the keyboard.
    pub(crate) fn sent(&mut self) {
        self.draft.clear();
        self.context.clear();
        self.focus = true;
    }

    /// What `item` is called, as found.
    fn name(&self, item: &ContextItem) -> Option<&str> {
        self.names.get(&key(item)).and_then(Option::as_deref)
    }
}

/// An item pointed at, as its name is kept by.
fn key(item: &ContextItem) -> String {
    match item {
        ContextItem::Message { id } => id.clone(),
        ContextItem::Quote { id, text } => format!("{id}\u{1}{}", text.len()),
        ContextItem::Folder { account, folder } => format!("{account}\u{1}{folder}\u{1}"),
    }
}

/// The pane at the window's end (its left in a right-to-left interface), when it's shown and the
/// window has room for it beside the `least` the mail needs. Dragged wider or narrower, its
/// width is kept.
pub fn panel(s: &mut Session, st: &mut UiState, ui: &mut Ui, least: f32, acts: &mut Vec<Action>) {
    let pane = s.prefs.assistant_pane;
    let range = mailcraft_engine::ASSISTANT_WIDTH;
    let (narrowest, widest) = (f32::from(*range.start()), f32::from(*range.end()));
    let room = ui.available_width() - least;
    if !pane.shown || room < narrowest {
        return;
    }
    let t = st.tokens;
    let side = if crate::i18n::current().rtl() { egui::Panel::left("assistant") } else { egui::Panel::right("assistant") };
    let shown = side
        .resizable(true)
        .default_size(f32::from(pane.width).min(room))
        .size_range(narrowest..=widest.min(room))
        .frame(egui::Frame::new().fill(t.surface).stroke(egui::Stroke::new(1.0, t.border)))
        .show(ui, |ui| show(s, st, ui, acts));
    st.panes.push((crate::Pane::Assistant, shown.response.rect));
    let width = shown.response.rect.width().round().clamp(narrowest, widest);
    if (width - f32::from(pane.width)).abs() >= 1.0 && width < room && !ui.input(|i| i.pointer.any_down()) {
        acts.push(Action::Run("assistant.pane", json!({"width": width})));
    }
}

/// The pane: its head, the chat, and the box to say something in.
fn show(s: &mut Session, st: &mut UiState, ui: &mut Ui, acts: &mut Vec<Action>) {
    // The chat shown is gone (deleted, or its file can't be read): a new one in its place.
    if let Some(id) = st.assistant.chat.clone()
        && s.assistant_shown(&id).is_none()
    {
        st.assistant.chat = None;
    }
    find_names(s, st);
    let s: &Session = s;
    let t = st.tokens;
    let chat = st.assistant.chat.as_deref().and_then(|id| s.ai.assistant.chat(id));
    egui::Frame::new().inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
        head(s, st, ui, chat, acts);
        if s.prefs.ai.profiles.is_empty() {
            set_up(ui, &t, acts);
            return;
        }
        model(s, st, ui, chat, acts);
        ui.add_space(4.0);
        egui::Panel::bottom("assistant-box").frame(egui::Frame::new().inner_margin(Margin::symmetric(0, 6))).show(ui, |ui| {
            composer::show(s, st, ui, acts);
        });
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| match chat {
            Some(chat) if !chat.turns.is_empty() => turns::show(s, st, ui, chat, acts),
            _ => suggestions(s, st, ui, acts),
        });
    });
}

/// Look up what the things pointed at, in the chat and in the box, are called, once each.
fn find_names(s: &mut Session, st: &mut UiState) {
    let a = &st.assistant;
    let in_chat = a.chat.as_deref().and_then(|id| s.ai.assistant.chat(id)).into_iter().flat_map(|c| c.turns.iter().flat_map(|t| &t.context));
    let wanted: Vec<ContextItem> = in_chat.chain(&a.context).filter(|i| !a.names.contains_key(&key(i))).cloned().collect();
    for item in wanted {
        let name = s.assistant_item_name(&item);
        st.assistant.names.insert(key(&item), name);
    }
}

/// The head: the chat's title, and New chat, the chats kept, and Close.
fn head(s: &Session, st: &mut UiState, ui: &mut Ui, chat: Option<&Chat>, acts: &mut Vec<Action>) {
    let t = st.tokens;
    /// Room for the three buttons at the end.
    const BUTTONS: f32 = 84.0;
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
        icons::paint(ui.painter(), r, Icon::Sparkle, t.accent);
        let title = chat.map_or_else(|| tl!("New chat").to_string(), |c| c.title.clone());
        let w = (ui.available_width() - BUTTONS).max(40.0);
        ui.allocate_ui_with_layout(egui::vec2(w, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(title).font(theme::bold(14.0)).color(t.text)).truncate());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::tiny_icon_button(ui, Icon::Close, "Close the AI assistant", &t).clicked() {
                acts.push(Action::Run("assistant.pane", json!({"on": false})));
            }
            let chats = widgets::tiny_icon_button(ui, Icon::Conversation, "Chats", &t);
            egui::Popup::menu(&chats).show(|ui| chats_menu(s, st, ui, acts));
            if widgets::tiny_icon_button(ui, Icon::Plus, "New chat", &t).clicked() {
                st.assistant.chat = None;
                st.assistant.focus = true;
            }
        });
    });
}

/// The chats kept, newest first: each opened, or deleted; and all of them deleted, asked twice.
fn chats_menu(s: &Session, st: &mut UiState, ui: &mut Ui, acts: &mut Vec<Action>) {
    ui.set_min_width(240.0);
    let heads: Vec<_> = s.ai.assistant.heads().collect();
    if heads.is_empty() {
        ui.label(RichText::new(tl!("No chats yet")).color(st.tokens.faint));
        return;
    }
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
        for h in &heads {
            ui.horizontal(|ui| {
                if widgets::tiny_icon_button(ui, Icon::Delete, "Delete this chat", &st.tokens).clicked() {
                    acts.push(Action::Run("assistant.delete", json!({"chat": h.id})));
                }
                let on = st.assistant.chat.as_deref() == Some(h.id.as_str());
                if ui.selectable_label(on, widgets::shorten(&h.title, 48)).clicked() {
                    st.assistant.chat = Some(h.id.clone());
                    ui.close();
                }
            });
        }
    });
    ui.separator();
    let label =
        if st.assistant.delete_all { tf!("Click again to delete all {n} chats", n = heads.len()) } else { tl!("Delete all chats").to_string() };
    if ui.button(label).clicked() {
        if std::mem::take(&mut st.assistant.delete_all) {
            acts.push(Action::Run("assistant.deleteAll", json!({})));
            ui.close();
        } else {
            st.assistant.delete_all = true;
        }
    }
}

/// No model yet: where one is set up.
fn set_up(ui: &mut Ui, t: &Tokens, acts: &mut Vec<Action>) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
        icons::paint(ui.painter(), r, Icon::Sparkle, t.border);
        ui.add_space(8.0);
        ui.label(RichText::new(tl!("Add a model to ask about your mail: one on this computer, or a cloud provider's.")).color(t.dim));
        ui.add_space(8.0);
        if widgets::primary_button(ui, tl!("Set up AI"), t, true).clicked() {
            acts.push(Action::Backstage(crate::backstage::Page::Options));
        }
    });
}

/// The model the chat asks, picked from those set up, and where it is; or why none may be asked.
fn model(s: &Session, st: &mut UiState, ui: &mut Ui, chat: Option<&Chat>, acts: &mut Vec<Action>) {
    let t = st.tokens;
    let new = Chat { profile: st.assistant.profile.clone(), ..Chat::default() };
    let asked = s.assistant_model(chat.unwrap_or(&new));
    let named = chat.map_or(&new.profile, |c| &c.profile).clone();
    let current = asked
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|_| named.as_deref().and_then(|id| s.prefs.ai.profile(id)).map_or_else(|| tl!("No model").to_string(), |p| p.name.clone()));
    ui.horizontal(|ui| {
        let picked = egui::ComboBox::from_id_salt("assistant-model").selected_text(current).show_ui(ui, |ui| pick_model(s, ui, &named));
        widgets::named(ui, &picked.response, tl!("Model"), true, None);
        if let Some(Some(id)) = picked.inner {
            match chat {
                Some(c) => acts.push(Action::Run("assistant.model", json!({"chat": c.id, "profile": id}))),
                None => st.assistant.profile = Some(id).filter(|id| !id.is_empty()),
            }
        }
        // Cut short to fit, whole on hover: a long model's name (Ollama's `owner/Name:tag`)
        // stretched the pane past its width.
        if let Ok(p) = &asked {
            ui.add(egui::Label::new(RichText::new(where_it_is(p)).small().color(t.faint)).truncate());
        }
    });
    if let Err(e) = asked {
        ui.horizontal_wrapped(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            icons::paint(ui.painter(), r, Icon::Warning, t.warn);
            ui.label(RichText::new(e.to_string()).small().color(t.dim));
        });
    }
}

/// The models to pick from: the one chat work goes to (""), then each set up. The one picked.
fn pick_model(s: &Session, ui: &mut Ui, named: &Option<String>) -> Option<String> {
    let mut picked = None;
    if ui.selectable_label(named.is_none(), tl!("Default model")).clicked() {
        picked = Some(String::new());
    }
    for p in &s.prefs.ai.profiles {
        let label = format!("{} · {}", p.name, where_it_is(p));
        if ui.selectable_label(named.as_deref() == Some(p.id.as_str()), label).clicked() {
            picked = Some(p.id.clone());
        }
    }
    picked
}

/// Where a model runs: on this computer or its network, or the cloud provider's server.
pub(crate) fn where_it_is(p: &AiProfile) -> String {
    let server = mailcraft_engine::ai_server(&p.endpoint.url);
    if p.local() { tf!("{model} at {server}", model = p.endpoint.model, server = server) } else { tf!("Cloud: {server}", server = server) }
}

/// A new chat: what it may be asked to begin with, about what's selected or open.
fn suggestions(s: &Session, st: &mut UiState, ui: &mut Ui, acts: &mut Vec<Action>) {
    let t = st.tokens;
    ui.add_space(16.0);
    ui.label(
        RichText::new(tl!("Ask about your mail: it can find, read, sum up, answer and tidy it. Nothing changes until you approve it.")).color(t.dim),
    );
    ui.add_space(12.0);
    let selected: Vec<ContextItem> = s.selection.iter().map(|k| ContextItem::Message { id: k.to_id() }).collect();
    // Each sent as it reads, in the interface's language: the model answers in it.
    let mut ask = |ui: &mut Ui, icon: Icon, text: &str, items: Vec<ContextItem>| {
        if widgets::pill_button(ui, icon, text, &t).clicked() {
            let mut context = st.assistant.context.clone();
            context.extend(items.into_iter().filter(|i| !st.assistant.context.contains(i)));
            acts.push(composer::send(st, text, &context));
        }
        ui.add_space(6.0);
    };
    if !selected.is_empty() {
        ask(ui, Icon::ListBullet, tl!("Sum up the selected messages"), selected.clone());
    }
    if let [one] = selected.as_slice() {
        ask(ui, Icon::Reply, tl!("Write a reply to this message"), vec![one.clone()]);
    }
    ask(ui, Icon::Flag, tl!("What needs my attention today?"), Vec::new());
    ask(ui, Icon::Search, tl!("Find the mail I haven't answered"), Vec::new());
}
