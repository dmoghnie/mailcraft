//! A model's answer, formatted from its Markdown. What it says comes partly from mail others
//! wrote, so: a picture is shown as a link to it (nothing is fetched, from the web or from this
//! computer), a link opens as one in a message does (a web page in the browser, `mailto:` a new
//! message, anything else not at all), and a very long answer is shown cut short. A link to a
//! message (`mailcraft:`) goes to it, if the chat has seen it (`assistant.open`).

use egui::Ui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use pulldown_cmark::{Event, Options, Parser, Tag};

use mailcraft_engine::ASSISTANT_LINK;
use serde_json::json;

use crate::Action;

/// The most of an answer shown, in bytes.
const SHOWN_MOST: usize = 200 << 10;

/// `text` of chat `chat`, formatted.
pub(super) fn show(ui: &mut Ui, cache: &mut CommonMarkCache, id: egui::Id, chat: &str, text: &str, acts: &mut Vec<Action>) {
    let text = pictures_as_links(cut(text));
    let before = ui.ctx().output(|o| o.commands.len());
    ui.scope(|ui| {
        // Links in the links' colour: the window's text colour, set over all text
        // (`theme::apply`), would paint them as text. Plain text keeps it, as egui's own.
        ui.visuals_mut().override_text_color = None;
        // A line of text as high as its words: a link's are put at the top of the line
        // (egui_commonmark), the words around it in its middle, so a line taller than they are
        // (a button's, as a list item starts) had the link stand above them.
        ui.spacing_mut().interact_size.y = ui.text_style_height(&egui::TextStyle::Body);
        CommonMarkViewer::new().show_with_id(id, ui, cache, &text);
    });
    // The links clicked: MailCraft opens them, not egui.
    let urls = ui.ctx().output_mut(|o| {
        let mut urls = Vec::new();
        let mut i = 0;
        o.commands.retain(|c| {
            i += 1;
            match c {
                egui::OutputCommand::OpenUrl(open) if i > before => {
                    urls.push(open.url.clone());
                    false
                }
                _ => true,
            }
        });
        urls
    });
    acts.extend(urls.into_iter().map(|url| {
        if url.starts_with(ASSISTANT_LINK) { Action::Run("assistant.open", json!({"chat": chat, "link": url})) } else { Action::OpenUrl(url) }
    }));
}

/// `text`, at most [`SHOWN_MOST`] bytes of it (on a character's edge).
fn cut(text: &str) -> &str {
    text.get(..text.floor_char_boundary(SHOWN_MOST)).unwrap_or(text)
}

/// `text` with each picture (`![what](where)`) made a link to it (`[what](where)`): the `!`
/// before each, where the Markdown has a picture (not in code, nor an escaped one), made a
/// zero-width space. Taken out, a `!` before it would start a picture again (`!![a](b)`).
pub(super) fn pictures_as_links(text: &str) -> String {
    let bangs: Vec<usize> = Parser::new_ext(text, Options::all())
        .into_offset_iter()
        .filter_map(|(event, range)| matches!(event, Event::Start(Tag::Image { .. })).then_some(range.start))
        .filter(|&at| text.as_bytes().get(at) == Some(&b'!'))
        .collect();
    if bangs.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    for at in bangs {
        out.push_str(text.get(from..at).unwrap_or_default());
        out.push('\u{200B}');
        from = at + 1;
    }
    out.push_str(text.get(from..).unwrap_or_default());
    out
}
