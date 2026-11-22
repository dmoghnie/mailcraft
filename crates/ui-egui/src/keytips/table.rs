//! Which letters reach what: the ribbon's tabs, the search box, and each tab's buttons, as
//! Outlook (classic) shows them when Alt is pressed (watched from its window, clean room). A
//! button is known by its label in English, as the ribbon names it.

use crate::RibbonTab;

/// Where the first letters go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Top {
    /// The File page.
    File,
    /// A ribbon tab: its buttons' letters follow.
    Tab(RibbonTab),
    /// The search box.
    Search,
}

/// The letters on the tabs and the search box, after Alt.
pub const TOPS: &[(&str, Top)] = &[
    ("F", Top::File),
    ("H", Top::Tab(RibbonTab::Home)),
    ("JS", Top::Tab(RibbonTab::SendReceive)),
    ("V", Top::Tab(RibbonTab::View)),
    ("Q", Top::Search),
];

/// Each tab's buttons: their letters after the tab's, and their labels. Two buttons that take
/// each other's place (Mark read, Mark unread) share their letters.
pub const BUTTONS: &[(RibbonTab, &str, &str)] = &[
    (RibbonTab::Home, "N", "New mail"),
    (RibbonTab::Home, "D", "Delete"),
    (RibbonTab::Home, "O", "Archive"),
    (RibbonTab::Home, "R1", "Report junk"),
    (RibbonTab::Home, "R1", "Not junk"),
    (RibbonTab::Home, "MV", "Move to"),
    (RibbonTab::Home, "QS", "Quick steps"),
    (RibbonTab::Home, "RP", "Reply"),
    (RibbonTab::Home, "RA", "Reply all"),
    (RibbonTab::Home, "FW", "Forward"),
    (RibbonTab::Home, "W", "Mark read"),
    (RibbonTab::Home, "W", "Mark unread"),
    (RibbonTab::Home, "G", "Categorize"),
    (RibbonTab::Home, "U", "Flag"),
    (RibbonTab::Home, "L", "Filter"),
    (RibbonTab::Home, "Y", "More commands"),
    (RibbonTab::SendReceive, "S", "Send / Receive all folders"),
    (RibbonTab::SendReceive, "A", "Send all"),
    (RibbonTab::SendReceive, "D", "Update folder"),
    (RibbonTab::SendReceive, "W", "Work offline"),
    (RibbonTab::View, "W", "Focused Inbox"),
    (RibbonTab::View, "AB", "Arrange by"),
    (RibbonTab::View, "RS", "Reverse sort"),
    (RibbonTab::View, "U", "Compact"),
    (RibbonTab::View, "Y2", "Reading pane"),
];

/// The letters of the commands added to the simplified ribbon's Home besides its own, in order,
/// after Home's (as Outlook numbers what's added to its toolbar).
pub const ADDED: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];

/// The letters that reach `top`.
pub fn letters(top: Top) -> Option<&'static str> {
    TOPS.iter().find_map(|(letters, t)| (*t == top).then_some(*letters))
}

/// The letters that open `tab`.
pub fn tab_letters(tab: RibbonTab) -> Option<&'static str> {
    letters(Top::Tab(tab))
}

/// The whole sequence of a button on `tab` labelled `label`, in its two parts: its tab's letters,
/// then its own.
pub fn button_letters(tab: RibbonTab, label: &str) -> Option<(&'static str, &'static str)> {
    let own = BUTTONS.iter().find_map(|(t, letters, l)| (*t == tab && *l == label).then_some(*letters))?;
    Some((tab_letters(tab)?, own))
}

/// The whole sequence of the `n`th command added to the simplified ribbon (from 0), on `tab`.
pub fn added_letters(tab: RibbonTab, n: usize) -> Option<(&'static str, &'static str)> {
    if tab != RibbonTab::Home {
        return None;
    }
    Some((tab_letters(tab)?, ADDED.get(n).copied()?))
}

/// What a whole sequence reaches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Top(Top),
    /// A button: the sequence that names it.
    Button(String),
}

/// Every whole sequence, in two parts (a button's: its tab's letters, then its own), and the
/// top it reaches (none: a button); `added`: how many commands the simplified ribbon has besides
/// Home's.
fn all(added: usize) -> impl Iterator<Item = (&'static str, &'static str, Option<Top>)> {
    let tops = TOPS.iter().map(|(letters, top)| (*letters, "", Some(*top)));
    let buttons = BUTTONS.iter().filter_map(|(tab, letters, _)| Some((tab_letters(*tab)?, *letters, None)));
    let numbered = (0..added).filter_map(|n| added_letters(RibbonTab::Home, n)).map(|(head, own)| (head, own, None));
    tops.chain(buttons).chain(numbered)
}

/// What `sequence` reaches, if it's whole (`added`: as in [`all`]).
pub fn lookup(sequence: &str, added: usize) -> Option<Entry> {
    let (_, _, top) = all(added).find(|(head, tail, _)| sequence.strip_prefix(head) == Some(tail))?;
    Some(top.map_or_else(|| Entry::Button(sequence.to_string()), Entry::Top))
}

/// Whether a longer sequence starts with `prefix` (more letters are waited for).
pub fn has_deeper(prefix: &str, added: usize) -> bool {
    all(added).any(|(head, tail, _)| head.len() + tail.len() > prefix.len() && starts_with(head, tail, prefix))
}

/// Whether `head` then `tail` starts with `prefix`.
fn starts_with(head: &str, tail: &str, prefix: &str) -> bool {
    match prefix.strip_prefix(head) {
        Some(rest) => tail.starts_with(rest),
        None => head.starts_with(prefix),
    }
}
