//! Ask MailCraft's AI about the text selected in a message: a button beside where the selection
//! ends, while it's in view and not being dragged. What's selected is read in the frame after
//! the button is pressed, as only then are the runs it's in laid out again.

use egui::text::CCursor;
use egui::{Id, Rect};

use super::Doc;

/// Where the button goes, and what it asked for.
#[derive(Default)]
pub(crate) struct Ask {
    /// The text whose selection the button is beside, and where on the screen the selection ends.
    at: Option<(Id, Rect)>,
    /// The button was pressed: the text's selection is read when it's next drawn.
    wanted: Option<Id>,
    /// What was selected, read.
    read: Option<(Id, String)>,
}

impl Ask {
    /// Where the selection in text `id` ends, if there's one to ask about.
    pub(crate) fn at(&self, id: Id) -> Option<Rect> {
        self.at.filter(|(i, _)| *i == id).map(|(_, r)| r)
    }

    /// The button beside text `id`'s selection was pressed.
    pub(crate) fn want(&mut self, id: Id) {
        self.wanted = Some(id);
    }

    /// What was selected in text `id`, once read.
    pub(crate) fn take(&mut self, id: Id) -> Option<String> {
        self.read.take_if(|(i, _)| *i == id).map(|(_, text)| text)
    }
}

/// Text `id` (`d`) drawn this frame: what's selected read if it was asked for, and where the
/// button goes.
pub(super) fn note(ask: &mut Ask, id: Id, d: &Doc, whole: Option<&str>) {
    if ask.wanted == Some(id) {
        ask.wanted = None;
        let text = d.selection().map(|_| d.text(whole)).filter(|t| !t.trim().is_empty());
        ask.read = text.map(|t| (id, t));
    }
    if ask.at.is_some_and(|(i, _)| i == id) {
        ask.at = None;
    }
    if d.dragging || d.window {
        return;
    }
    // The end of the last run laid out with some of it selected.
    let end = (0..d.runs.len()).rev().find_map(|i| Some((d.runs.get(i)?, d.range_of(i).filter(|r| !r.is_empty())?)));
    let Some((run, range)) = end else { return };
    let caret = run.caret(CCursor::new(range.end));
    if d.view.is_some_and(|v| v.intersects(caret)) {
        ask.at = Some((id, caret));
    }
}
