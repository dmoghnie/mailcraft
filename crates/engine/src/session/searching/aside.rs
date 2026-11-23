//! Search: a search done at once and kept apart from the window's, for MailCraft's AI. It finds
//! what the window's search would in the folders it's given (the summaries settle flags, dates,
//! senders and subjects; each account's index, words; a downloaded message's source, what the
//! index can't), and changes nothing the window shows: its list, selection, search, or the
//! message open.

use std::cmp::Reverse;

use super::look_up::Terms;
use super::matching::{Index, Recorder, Verdict, classify, from_source};
use super::run::query_terms;
use super::*;

/// The most of messages' sources read to settle what the index can't (not indexed yet, or a
/// phrase only their text settles), the newest first.
const SOURCES_MOST: usize = 200;
/// The most units of looking words up in an account's index (a word takes a unit for every few
/// pages it's on).
const UNITS_MOST: usize = 4096;

/// What a search aside found.
#[derive(Debug, Default)]
pub(crate) struct FoundAside {
    /// The messages found, newest first: at most the number asked for.
    pub(crate) found: Vec<SharedSummary>,
    /// How many were found in all.
    pub(crate) total: usize,
    /// Messages that may match but couldn't be checked: their text isn't indexed or downloaded
    /// yet, or is past where it was cut short.
    pub(crate) unchecked: usize,
}

impl Session {
    /// Search `folders` for `q` (as typed in the search box), at once: at most `most` of what's
    /// found, newest first. Nothing the window shows changes.
    pub(crate) fn search_aside(&mut self, q: &str, folders: &[(String, String)], most: usize) -> FoundAside {
        let query = self.query_of(q);
        let (words, phrases) = query_terms(&query);
        let awaiting = query.clauses.iter().any(|c| asks_awaiting(&c.term)).then(|| self.awaiting_replies());
        let mut accounts: Vec<&str> = folders.iter().map(|(a, _)| a.as_str()).collect();
        accounts.dedup();
        let mut found: Vec<SharedSummary> = Vec::new();
        let mut maybe: Vec<SharedSummary> = Vec::new();
        for account in accounts {
            let terms = self.looked_up_aside(account, &words, &phrases);
            for (_, folder) in folders.iter().filter(|(a, _)| a == account) {
                let messages = self.folder_state(account, folder).messages.clone();
                let reader = self.search_state.readers.get(account).and_then(|s| s.reader.as_ref());
                let index = reader.map(|r| Index::of(r, terms.as_ref(), folder));
                for m in messages {
                    let st = SummaryText::for_query(&m, &query);
                    let waits = awaiting.as_ref().is_some_and(|w| w.contains(&m.key));
                    match classify(&query, &m, &st, waits, index, None) {
                        Verdict::Yes => found.push(m),
                        Verdict::No => {}
                        Verdict::Verify(_) | Verdict::Missing | Verdict::CutShort => maybe.push(m),
                    }
                }
            }
        }
        maybe.sort_by_key(|m| Reverse(m.date));
        let mut unchecked = maybe.len().saturating_sub(SOURCES_MOST);
        for m in maybe.into_iter().take(SOURCES_MOST) {
            let Some(raw) = self.body_raw(&m.key) else {
                unchecked += 1;
                continue;
            };
            let st = SummaryText::for_query(&m, &query);
            let waits = awaiting.as_ref().is_some_and(|w| w.contains(&m.key));
            let answers = from_source(&raw, &Recorder::atoms(&query, &m, &st, waits));
            match classify(&query, &m, &st, waits, None, Some(&answers)) {
                Verdict::Yes => found.push(m),
                Verdict::No => {}
                Verdict::Verify(_) | Verdict::Missing | Verdict::CutShort => unchecked += 1,
            }
        }
        found.sort_by_key(|m| Reverse(m.date));
        let total = found.len();
        found.truncate(most);
        FoundAside { found, total, unchecked }
    }

    /// `words` and `phrases` looked up in `account`'s index, opened if it isn't: `None` when it
    /// has none, or it couldn't be read, or they took too long to look up (the messages' text
    /// then settles them, as far as it's downloaded).
    fn looked_up_aside(
        &mut self,
        account: &str,
        words: &[(mailcraft_store::search::Field, String)],
        phrases: &[(mailcraft_store::search::Field, Vec<String>)],
    ) -> Option<Terms> {
        self.open_reader(account);
        if !self.search_state.readers.get(account).is_some_and(|s| s.reader.is_some()) {
            return None;
        }
        let mut terms = Terms::new(words.to_vec(), phrases.to_vec());
        for _ in 0..UNITS_MOST {
            if terms.ready() {
                return Some(terms);
            }
            if !self.terms_step(&mut terms, account) {
                return None;
            }
        }
        terms.ready().then_some(terms)
    }
}
