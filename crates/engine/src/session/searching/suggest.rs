//! Suggestions as a search is typed: people, subjects, files' names and dates, each with the
//! search syntax it stands for (`from:`, `subject:"…"`, `attachment:`, `received:`), for the
//! search box to offer under it.

use mailcraft_model::dates::{civil_from_days, today};
use mailcraft_model::search::fold_word;
use mailcraft_store::search::{Bits, Field, ScanCursor};

use super::*;

/// What a suggestion is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SuggestKind {
    Person,
    Subject,
    File,
    Date,
}

/// One suggestion: what's shown, the search syntax it stands for, and how much of the end of
/// the text typed it takes the place of.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Suggestion {
    pub kind: SuggestKind,
    /// A person's name and address, a subject, the text a file's name has, or a period: in
    /// English for a period ([`DATE_WORDS`], a month, a year), for the window to translate.
    pub label: String,
    /// Its shortest name: a person's name (or address), the subject, the text, the period.
    pub short: String,
    /// The search syntax it stands for.
    pub query: String,
    /// The bytes at the end of the text typed it takes the place of.
    pub replaces: usize,
}

/// The most people, and subjects, suggested.
const PEOPLE: usize = 4;
const SUBJECTS: usize = 3;
/// The shortest text anything is suggested for.
const MIN_CHARS: usize = 2;
/// The longest subject suggested, in characters (cut at a word's end).
const SUBJECT_CHARS: usize = 80;
/// The most messages of a folder looked through for subjects, newest first.
const SUBJECTS_LOOKED: usize = 20_000;
/// The most pages of an account's index read to find a file named with a word: enough for
/// most, and a key typed stays quick.
const FILE_PAGES: usize = 8;

/// The periods offered by name, and the syntax each stands for.
pub const DATE_WORDS: [(&str, &str); 8] = [
    ("Today", "received:today"),
    ("Yesterday", "received:yesterday"),
    ("This week", "received:\"this week\""),
    ("Last week", "received:\"last week\""),
    ("This month", "received:\"this month\""),
    ("Last month", "received:\"last month\""),
    ("This year", "received:\"this year\""),
    ("Last year", "received:\"last year\""),
];

/// The months, as the window names them.
pub const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// `text`'s last word, and its last two (with the space between), when it has them.
fn tails(text: &str) -> (&str, Option<&str>) {
    let last_at = text.rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let last = text.get(last_at..).unwrap_or_default();
    let before = text.get(..last_at).unwrap_or_default().trim_end();
    let two = before.rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let pair = (!before.is_empty()).then(|| text.get(two..)).flatten();
    (last, pair)
}

/// A value as the search syntax takes it: quoted when it has a space, its own quotes left out.
fn quoted(value: &str) -> String {
    let v: String = value.chars().filter(|c| *c != '"').collect();
    if v.chars().any(char::is_whitespace) { format!("\"{v}\"") } else { v }
}

/// A subject cut to at most [`SUBJECT_CHARS`], at a word's end.
fn cut_subject(subject: &str) -> String {
    if subject.chars().count() <= SUBJECT_CHARS {
        return subject.to_string();
    }
    let head: String = subject.chars().take(SUBJECT_CHARS).collect();
    match head.rfind(char::is_whitespace) {
        Some(i) => head.get(..i).unwrap_or(&head).trim_end().to_string(),
        None => head,
    }
}

impl Session {
    /// Suggestions for `typed`, the text of a search being typed (after what's already chosen),
    /// in kinds: people for its last word (`from:`, or `to:` over Sent and Drafts), subjects of
    /// the mail on view with all of it, files named with its last word, and periods its last
    /// word or two begin to name.
    pub fn search_suggestions(&mut self, typed: &str) -> Vec<Suggestion> {
        let typed = typed.trim_end();
        let (last, pair) = tails(typed);
        if typed.chars().count() < MIN_CHARS || last.contains(':') || last.starts_with('-') {
            return Vec::new();
        }
        // A word too short alone can still end a period's name ("last w").
        let word = last.chars().count() >= MIN_CHARS;
        let mut out = if word { self.people_suggested(last) } else { Vec::new() };
        out.extend(self.subjects_suggested(typed));
        if word && self.file_named(last) {
            let word = quoted(last);
            out.push(Suggestion {
                kind: SuggestKind::File,
                label: last.to_string(),
                short: last.to_string(),
                query: format!("attachment:{word}"),
                replaces: last.len(),
            });
        }
        out.extend(self.dates_suggested(last, pair));
        out
    }

    fn people_suggested(&mut self, last: &str) -> Vec<Suggestion> {
        self.ensure_contacts();
        let field = if matches!(self.view_role(), Some(FolderRole::Sent | FolderRole::Drafts)) { "to" } else { "from" };
        let people = self.contacts.as_ref().map(|b| b.suggest(last, PEOPLE, self.clock.now)).unwrap_or_default();
        people
            .into_iter()
            .filter(|c| {
                let e = c.address.email.trim();
                e.contains('@') && !e.contains(|ch: char| ch.is_whitespace() || ch == ',' || ch == '"')
            })
            .map(|c| Suggestion {
                kind: SuggestKind::Person,
                short: if c.address.name.trim().is_empty() { c.address.email.trim().to_string() } else { c.address.name.trim().to_string() },
                label: c.address.to_string(),
                query: format!("{field}:{}", c.address.email.trim()),
                replaces: last.len(),
            })
            .collect()
    }

    /// Subjects of the mail on view with `typed` in them, newest first, each conversation once.
    fn subjects_suggested(&self, typed: &str) -> Vec<Suggestion> {
        let want = typed.trim().to_lowercase();
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for (a, f) in self.view_folders() {
            let Some(st) = self.cache.get(&(a, f)) else { continue };
            for m in st.messages.iter().take(SUBJECTS_LOOKED) {
                let base = m.base_subject();
                let subject = base.trim();
                if subject.is_empty() || !subject.to_lowercase().contains(&want) || !seen.insert(subject.to_lowercase()) {
                    continue;
                }
                let shown = cut_subject(subject);
                out.push(Suggestion {
                    kind: SuggestKind::Subject,
                    query: format!("subject:{}", quoted(&shown)),
                    short: shown.clone(),
                    label: shown,
                    replaces: typed.len(),
                });
                if out.len() >= SUBJECTS {
                    return out;
                }
            }
        }
        out
    }

    /// Whether a file's name in an account on view has a word starting with `word`, as far as
    /// [`FILE_PAGES`] of its index tell.
    fn file_named(&mut self, word: &str) -> bool {
        let Some(store) = self.store.clone() else { return false };
        let folded = fold_word(word);
        let accounts: BTreeSet<String> = self.view_folders().into_iter().map(|(a, _)| a).collect();
        for account in accounts {
            self.open_reader(&account);
            let Some(reader) = self.search_state.readers.get_mut(&account).and_then(|s| s.reader.as_mut()) else { continue };
            let mut bits = Bits::new(reader.len());
            let scanned = reader.scan_prefix(&store, Field::Names, &folded, &mut ScanCursor::default(), &mut bits, FILE_PAGES);
            if scanned.is_ok() && bits.count() > 0 {
                return true;
            }
        }
        false
    }

    /// Periods the last word, or the last two, begin to name: by name, a month (this year's, or
    /// last year's when it's still to come), a year.
    fn dates_suggested(&self, last: &str, pair: Option<&str>) -> Vec<Suggestion> {
        let begins = |name: &str, typed: &str| name.to_lowercase().starts_with(&typed.to_lowercase());
        let mut out = Vec::new();
        for (name, query) in DATE_WORDS {
            let alone = Some(last).filter(|l| l.chars().count() >= MIN_CHARS);
            let typed = [pair, alone].into_iter().flatten().find(|t| begins(name, t));
            if let Some(t) = typed {
                out.push(Suggestion {
                    kind: SuggestKind::Date,
                    label: name.to_string(),
                    short: name.to_string(),
                    query: query.to_string(),
                    replaces: t.len(),
                });
            }
        }
        let (year, month, _) = civil_from_days(today(&self.clock));
        if last.chars().count() >= 3 {
            for (i, name) in MONTHS.iter().enumerate() {
                let m = u32::try_from(i).unwrap_or(0).saturating_add(1);
                if begins(name, last) {
                    let y = if m <= month { year } else { year.saturating_sub(1) };
                    let label = format!("{name} {y}");
                    out.push(Suggestion {
                        kind: SuggestKind::Date,
                        short: label.clone(),
                        label,
                        query: format!("received:{y:04}-{m:02}"),
                        replaces: last.len(),
                    });
                }
            }
        }
        if let Ok(y) = last.parse::<i64>()
            && last.len() == 4
            && (1970..=year).contains(&y)
        {
            out.push(Suggestion {
                kind: SuggestKind::Date,
                label: y.to_string(),
                short: y.to_string(),
                query: format!("received:{y}"),
                replaces: last.len(),
            });
        }
        out
    }
}
