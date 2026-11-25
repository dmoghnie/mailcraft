//! MailCraft's AI on the message being written: the bar's Rewrite with AI menu (shorter, warmer,
//! more formal, spelling and grammar fixed, or written from a few words), and over the text a
//! card with what's written as it comes, the model writing it, Replace (the text changes only
//! then), Undo while nothing was typed since, and Close.

use egui::{Margin, RichText, Sense, Ui, vec2};
use mailcraft_engine::{Compose, RewriteHow, Session};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::{Action, UiState, widgets};

/// The bar's Rewrite with AI: each way of rewriting, and writing from a few words (which asks
/// for them over the text, `words`).
pub(super) fn menu(ui: &mut Ui, t: &Tokens, c: &Compose, words: &mut Option<(u64, String)>, acts: &mut Vec<Action>) {
    let button = widgets::action_icon(ui, Icon::Sparkle, "Rewrite with AI", t);
    egui::Popup::menu(&button).show(|ui| {
        for (how, label) in
            [("shorter", tl!("Shorter")), ("warmer", tl!("Warmer")), ("formal", tl!("More formal")), ("grammar", tl!("Fix spelling and grammar"))]
        {
            if ui.button(label).clicked() {
                acts.push(Action::Run("compose.rewrite", json!({"how": how})));
                ui.close();
            }
        }
        ui.separator();
        if ui.button(tl!("Write it from a few words…")).clicked() {
            *words = Some((c.id, String::new()));
            ui.close();
        }
    });
}

/// What a rewrite of `how` is called on its card.
fn title(how: RewriteHow) -> &'static str {
    match how {
        RewriteHow::Shorter => tl!("Shorter"),
        RewriteHow::Warmer => tl!("Warmer"),
        RewriteHow::Formal => tl!("More formal"),
        RewriteHow::Grammar => tl!("Fix spelling and grammar"),
        RewriteHow::FromWords => tl!("From a few words"),
    }
}

/// Over the text: the few words asked for, while they are; the rewrite asked for, if there's one.
pub(super) fn card(s: &Session, st: &mut UiState, ui: &mut Ui, c: &Compose, acts: &mut Vec<Action>) {
    let t = st.tokens;
    asking_for_words(ui, &t, c, &mut st.rewrite_words, acts);
    let Some(r) = s.rewrite_for(c.id) else { return };
    let text = match r.asking.and_then(|id| s.ai.part(id)) {
        Some((part, _)) => part.to_string(),
        None => r.text.clone(),
    };
    let by = s.prefs.ai.profile(&r.profile).map(crate::panes::assistant::where_it_is);
    let done = r.asking.is_none() && r.error.is_none() && !r.text.is_empty();
    egui::Frame::new().fill(t.accent_soft).corner_radius(8).inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (at, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), at, Icon::Sparkle, t.accent);
            ui.label(RichText::new(title(r.how)).strong().color(t.text));
            if let Some(by) = &by {
                ui.label(RichText::new(by).small().color(t.dim));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::action_icon(ui, Icon::Close, "Close the rewrite", &t).clicked() {
                    acts.push(Action::Run("compose.closeRewrite", json!({})));
                }
            });
        });
        if r.asking.is_some() && text.trim().is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tl!("Rewriting…")).color(t.dim));
            });
        }
        if !text.trim().is_empty() {
            ui.add(egui::Label::new(RichText::new(text).color(t.text)).selectable(true));
        }
        if let Some(e) = &r.error {
            ui.label(RichText::new(tf!("It couldn't be rewritten: {error}", error = e)).color(t.danger));
        }
        if r.cut {
            ui.label(RichText::new(tl!("The answer was cut short at the longest answer set for this model.")).small().color(t.faint));
        }
        ui.horizontal(|ui| {
            if done && !r.is_put() && ui.button(tl!("Replace")).on_hover_text(tl!("Put it in place of your text")).clicked() {
                acts.push(Action::Run("compose.useRewrite", json!({})));
            }
            if r.can_undo(&c.body) && ui.button(tl!("Undo")).clicked() {
                acts.push(Action::Run("compose.undoRewrite", json!({})));
            }
            if r.error.is_some() && ui.button(tl!("Try again")).clicked() {
                match r.how {
                    RewriteHow::FromWords => st.rewrite_words = Some((c.id, String::new())),
                    how => acts.push(Action::Run("compose.rewrite", json!({"how": how.code()}))),
                }
            }
        });
    });
    ui.add_space(8.0);
}

/// The few words a message is written from, asked for over its text: Write (or Enter) asks the
/// model, Cancel lets them go.
fn asking_for_words(ui: &mut Ui, t: &Tokens, c: &Compose, words: &mut Option<(u64, String)>, acts: &mut Vec<Action>) {
    let Some((_, said)) = words.as_mut().filter(|(id, _)| *id == c.id) else { return };
    let (mut write, mut cancel) = (false, false);
    // The box takes the keyboard when it opens, once.
    let opened = egui::Id::unique(("rewrite-words-opened", c.id));
    let first = ui.ctx().data_mut(|d| d.get_temp::<bool>(opened).is_none());
    ui.horizontal(|ui| {
        let (at, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        icons::paint(ui.painter(), at, Icon::Sparkle, t.accent);
        let field = ui.add(egui::TextEdit::singleline(said).hint_text(tl!("What should it say?")).desired_width(ui.available_width() - 140.0));
        if first {
            field.request_focus();
            ui.ctx().data_mut(|d| d.insert_temp(opened, true));
        }
        write = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        write |= ui.add_enabled(!said.trim().is_empty(), egui::Button::new(tl!("Write"))).clicked();
        cancel = ui.button(tl!("Cancel")).clicked();
    });
    if write && !said.trim().is_empty() {
        acts.push(Action::Run("compose.writeFrom", json!({"words": said.trim(), "language": crate::i18n::current().code()})));
    }
    if write || cancel {
        *words = None;
        ui.ctx().data_mut(|d| d.remove::<bool>(opened));
    }
    ui.add_space(8.0);
}
