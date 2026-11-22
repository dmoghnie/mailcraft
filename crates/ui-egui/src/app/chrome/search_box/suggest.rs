//! The search box's suggestions as a search is typed (people, subjects, files, dates), and the
//! chips those chosen become: each stands for its syntax at the start of the box's text (which
//! can still be typed as it is), and Backspace in an empty box, or a click on it, takes it out.

use mailcraft_engine::{MONTHS, SuggestKind, Suggestion};

use super::*;

/// The most characters of a chip's text shown.
const CHIP_CHARS: usize = 28;

/// A suggestion chosen: what the box shows, and the syntax it stands for.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Chip {
    pub(crate) text: String,
    pub(crate) query: String,
}

/// The suggestions for what's typed after the chips, and the one the arrows are on.
#[derive(Clone, Debug, Default)]
pub(crate) struct Suggested {
    typed: String,
    pub(crate) list: Vec<Suggestion>,
    pick: Option<usize>,
}

impl Suggested {
    /// The arrows on none.
    pub(crate) fn unpick(&mut self) {
        self.pick = None;
    }
}

/// The chips' syntax, the start of the box's text.
fn chips_query(chips: &[Chip]) -> String {
    chips.iter().map(|c| c.query.as_str()).collect::<Vec<_>>().join(" ")
}

/// The box's text: the chips' syntax, then what's typed.
pub(super) fn joined(chips: &[Chip], typed: &str) -> String {
    let (head, rest) = (chips_query(chips), typed.trim_start());
    if head.is_empty() {
        typed.to_string()
    } else if rest.is_empty() {
        head
    } else {
        format!("{head} {rest}")
    }
}

/// What's typed after the chips. Chips the text no longer starts with go: it was changed from
/// elsewhere (cleared, a recent search picked, advanced search).
pub(super) fn typed_after(chips: &mut Vec<Chip>, text: &str) -> String {
    let head = chips_query(chips);
    if head.is_empty() {
        return text.to_string();
    }
    match text.strip_prefix(&head) {
        Some("") => String::new(),
        Some(rest) if rest.starts_with(' ') => rest.get(1..).unwrap_or_default().to_string(),
        _ => {
            chips.clear();
            text.to_string()
        }
    }
}

/// `text` cut to [`CHIP_CHARS`].
fn cut(text: &str) -> String {
    if text.chars().count() <= CHIP_CHARS {
        return text.to_string();
    }
    let head: String = text.chars().take(CHIP_CHARS.saturating_sub(1)).collect();
    format!("{}…", head.trim_end())
}

/// A period's name in the window's language: a named one, or a month and its year.
fn period(label: &str) -> String {
    match label.rsplit_once(' ') {
        Some((month, year)) if MONTHS.contains(&month) && year.parse::<i64>().is_ok() => format!("{} {year}", i18n::t(month)),
        _ => i18n::t(label).to_string(),
    }
}

/// The heading of a kind's suggestions.
fn group(kind: SuggestKind) -> &'static str {
    match kind {
        SuggestKind::Person => tl!("People"),
        SuggestKind::Subject => tl!("Subjects"),
        SuggestKind::File => tl!("Files"),
        SuggestKind::Date => tl!("Dates"),
    }
}

/// A suggestion as its row says it.
fn row_text(s: &Suggestion) -> String {
    match s.kind {
        SuggestKind::Person | SuggestKind::Subject => s.label.clone(),
        SuggestKind::File => tf!("Files with {text} in their name", text = s.label),
        SuggestKind::Date => period(&s.label),
    }
}

/// A suggestion as a screen reader hears it: its group, then its row.
fn spoken(s: &Suggestion) -> String {
    format!("{}: {}", group(s.kind), row_text(s))
}

/// A suggestion as its chip says it.
fn chip_text(s: &Suggestion) -> String {
    let short = cut(&s.short);
    match s.kind {
        SuggestKind::Person if s.query.starts_with("to:") => tf!("To: {names}", names = short),
        SuggestKind::Person => tf!("From: {name}", name = short),
        SuggestKind::Subject => tf!("Subject: {subject}", subject = short),
        SuggestKind::File => tf!("Attachment: {name}", name = short),
        SuggestKind::Date => cut(&period(&s.short)),
    }
}

impl MailApp {
    /// The chips before the box, each a button that takes it out; what's typed after them.
    pub(super) fn search_chips(&mut self, ui: &mut Ui, acts: &mut Vec<Action>) -> String {
        let typed = typed_after(&mut self.ui.search_chips, &self.ui.search_text);
        let t = self.ui.tokens;
        let mut gone = None;
        for (i, chip) in self.ui.search_chips.iter().enumerate() {
            let r = ui.add(egui::Button::new(RichText::new(format!("{}  ×", chip.text)).small().color(t.text)).fill(t.hover_row).corner_radius(10.0));
            let name = tf!("Remove {name} from the search", name = chip.text);
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
            if r.on_hover_text(&name).clicked() {
                gone = Some(i);
            }
        }
        if let Some(i) = gone {
            self.drop_chip(i, &typed, acts);
        }
        typed
    }

    /// Take chip `i` out, and search again for what's left.
    fn drop_chip(&mut self, i: usize, typed: &str, acts: &mut Vec<Action>) {
        if i < self.ui.search_chips.len() {
            self.ui.search_chips.remove(i);
        }
        self.ui.search_text = joined(&self.ui.search_chips, typed);
        self.ui.search_due = None;
        self.ui.focus_search = true;
        self.ui.search_cursor_end = true;
        acts.push(Action::Run("search.run", json!({"query": self.ui.search_text})));
    }

    /// Backspace in the box with nothing typed after the chips: the last chip goes.
    pub(super) fn backspace_chip(&mut self, ui: &Ui, had: &str, focused: bool, acts: &mut Vec<Action>) {
        if focused && had.is_empty() && !self.ui.search_chips.is_empty() && ui.input(|i| i.key_pressed(egui::Key::Backspace)) {
            self.drop_chip(self.ui.search_chips.len().saturating_sub(1), "", acts);
        }
    }

    /// The suggestions for `typed`, asked again only when it changes.
    pub(super) fn refresh_suggestions(&mut self, typed: &str) {
        if self.ui.search_suggest.typed != typed {
            let list = self.session.search_suggestions(typed);
            self.ui.search_suggest = Suggested { typed: typed.to_string(), list, pick: None };
        }
    }

    /// Up and Down move along the suggestions and Enter takes the one they're on: taken before
    /// the box sees them (it would move its cursor, or search).
    pub(super) fn suggestion_keys(&mut self, ui: &mut Ui, acts: &mut Vec<Action>) {
        let n = self.ui.search_suggest.list.len();
        if !self.ui.search_focused || n == 0 || self.ui.search_drop != SearchDrop::Recent {
            return;
        }
        let none = egui::Modifiers::NONE;
        let (down, up) = ui.input_mut(|i| (i.consume_key(none, egui::Key::ArrowDown), i.consume_key(none, egui::Key::ArrowUp)));
        if down || up {
            let last = n.saturating_sub(1);
            let pick = match (self.ui.search_suggest.pick, down) {
                (None, true) => 0,
                (None, false) => last,
                (Some(p), true) => {
                    if p >= last {
                        0
                    } else {
                        p.saturating_add(1)
                    }
                }
                (Some(p), false) => p.checked_sub(1).unwrap_or(last),
            };
            self.ui.search_suggest.pick = Some(pick);
            if let Some(s) = self.ui.search_suggest.list.get(pick) {
                crate::a11y::announce(ui.ctx(), spoken(s), false);
            }
        }
        if let Some(p) = self.ui.search_suggest.pick
            && ui.input_mut(|i| i.consume_key(none, egui::Key::Enter))
        {
            self.choose(p, acts);
        }
    }

    /// Suggestion `i` chosen: a chip in place of the words it stands for, searched at once.
    fn choose(&mut self, i: usize, acts: &mut Vec<Action>) {
        let Some(s) = self.ui.search_suggest.list.get(i).cloned() else { return };
        let typed = self.ui.search_suggest.typed.trim_end().to_string();
        let kept = typed.get(..typed.len().saturating_sub(s.replaces)).unwrap_or_default().trim();
        self.ui.search_chips.push(Chip { text: chip_text(&s), query: s.query.clone() });
        self.ui.search_text = joined(&self.ui.search_chips, kept);
        self.ui.search_suggest = Suggested::default();
        self.ui.search_due = None;
        self.ui.focus_search = true;
        self.ui.search_cursor_end = true;
        acts.push(Action::Run("search.run", json!({"query": self.ui.search_text})));
    }

    /// The suggestions under the box in their groups, the one the arrows are on marked.
    pub(super) fn suggestion_rows(&mut self, ui: &mut Ui, acts: &mut Vec<Action>) {
        let t = self.ui.tokens;
        let mut chosen = None;
        let mut kind = None;
        for (i, s) in self.ui.search_suggest.list.iter().enumerate() {
            if kind != Some(s.kind) {
                kind = Some(s.kind);
                ui.label(RichText::new(group(s.kind)).small().color(t.dim));
            }
            let on = self.ui.search_suggest.pick == Some(i);
            let mut job = egui::text::LayoutJob::single_section(row_text(s), egui::TextFormat::simple(theme::regular(14.0), t.text));
            job.wrap =
                egui::text::TextWrapping { max_width: ui.available_width(), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
            let row = ui.add(egui::Button::selectable(on, job).min_size(vec2(ui.available_width(), 0.0)));
            let name = spoken(s);
            row.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, &name));
            if on {
                row.scroll_to_me(None);
            }
            if row.clicked() {
                chosen = Some(i);
            }
        }
        if let Some(i) = chosen {
            self.choose(i, acts);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chip(query: &str) -> Chip {
        Chip { text: query.to_string(), query: query.to_string() }
    }

    #[test]
    fn chips_stand_for_the_start_of_the_text() {
        let mut chips = vec![chip("from:ana@soundlab.example"), chip("received:today")];
        let text = joined(&chips, "  mix");
        assert_eq!(text, "from:ana@soundlab.example received:today mix");
        assert_eq!(typed_after(&mut chips, &text), "mix");
        assert_eq!(typed_after(&mut chips, "from:ana@soundlab.example received:today"), "");
        assert_eq!(chips.len(), 2);
        // Changed from elsewhere: the chips go, the text stays.
        assert_eq!(typed_after(&mut chips, "budget"), "budget");
        assert!(chips.is_empty());
        assert_eq!(joined(&chips, "budget"), "budget");
    }

    #[test]
    fn every_period_named_is_translated() {
        let names = mailcraft_engine::DATE_WORDS.iter().map(|(w, _)| *w).chain(MONTHS);
        for name in names {
            assert!(i18n::in_every_catalog(name), "{name}");
        }
        assert_eq!(cut("Storyboard review on Thursday, and the screening"), "Storyboard review on Thursd…");
    }
}
