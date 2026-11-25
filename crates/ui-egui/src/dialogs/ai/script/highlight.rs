//! A script's Rhai coloured as it's written: its keywords, its text and numbers, and its
//! comments, each in a colour of the theme's. Read by a scan of its own, not Rhai's parser, so
//! code half written is coloured as far as it reads.

use std::ops::Range;
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, Galley, Ui};

use crate::theme::Tokens;

/// What a stretch of code is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Keyword,
    /// Text in quotes, or a number.
    Literal,
    Comment,
}

/// Rhai's keywords: its statements and declarations, and `true`, `false` and `this`.
const KEYWORDS: &[&str] = &[
    "let", "const", "if", "else", "switch", "do", "while", "until", "loop", "for", "in", "continue", "break", "return", "throw", "try", "catch",
    "fn", "private", "import", "export", "as", "global", "true", "false", "this",
];

/// The stretches of `code` that aren't plain, in order, by their byte ranges.
pub(super) fn spans(code: &str) -> Vec<(Range<usize>, Kind)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(rest) = code.get(at..) {
        let Some(c) = rest.chars().next() else { break };
        let (len, kind) = if rest.starts_with("//") {
            (rest.find('\n').unwrap_or(rest.len()), Some(Kind::Comment))
        } else if rest.starts_with("/*") {
            (block_comment(rest), Some(Kind::Comment))
        } else if matches!(c, '"' | '\'' | '`') {
            (quoted(rest, c), Some(Kind::Literal))
        } else if c.is_ascii_digit() {
            (number(rest), Some(Kind::Literal))
        } else if c.is_alphabetic() || c == '_' {
            let n = rest.find(|x: char| !(x.is_alphanumeric() || x == '_')).unwrap_or(rest.len());
            (n, rest.get(..n).filter(|w| KEYWORDS.contains(w)).map(|_| Kind::Keyword))
        } else {
            (c.len_utf8(), None)
        };
        // Never less than the character read: the scan always moves on.
        let len = len.max(c.len_utf8());
        if let Some(kind) = kind {
            out.push((at..at + len, kind));
        }
        at += len;
    }
    out
}

/// How long the comment `rest` starts with is: to its `*/`, its own comments inside it (Rhai's
/// nest), or to the end.
fn block_comment(rest: &str) -> usize {
    let mut depth = 0usize;
    let mut i = 0;
    while let Some(r) = rest.get(i..).filter(|r| !r.is_empty()) {
        if r.starts_with("/*") {
            depth += 1;
            i += 2;
        } else if r.starts_with("*/") {
            depth = depth.saturating_sub(1);
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += r.chars().next().map_or(1, char::len_utf8);
        }
    }
    rest.len()
}

/// How long the text in quotes `rest` starts with is: to its closing `quote`. Backticks' run
/// over lines and take no `\`; the others end at their line when they aren't closed.
fn quoted(rest: &str, quote: char) -> usize {
    let mut escaped = false;
    for (i, c) in rest.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if c == '\\' && quote != '`' {
            escaped = true;
        } else if c == '\n' && quote != '`' {
            return i;
        } else if c == quote {
            return i + c.len_utf8();
        }
    }
    rest.len()
}

/// How long the number `rest` starts with is: its digits, letters (`0x1F`, `1e3`) and `_`, and a
/// `.` before a digit (`1.5`, where `0..9` is a range).
fn number(rest: &str) -> usize {
    let mut chars = rest.char_indices().peekable();
    let mut end = 0;
    while let Some((i, c)) = chars.next() {
        let before_digit = chars.peek().is_some_and(|(_, n)| n.is_ascii_digit());
        if c.is_ascii_alphanumeric() || c == '_' || (c == '.' && before_digit) {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    end
}

/// The colour code of `kind` is drawn in.
fn colour(kind: Option<Kind>, t: &Tokens) -> Color32 {
    match kind {
        Some(Kind::Keyword) => t.accent,
        Some(Kind::Literal) => t.ok,
        Some(Kind::Comment) => t.faint,
        None => t.text,
    }
}

/// The code box's text laid out: `code` in the code font, coloured, wrapped at `wrap`.
pub(super) fn layout(ui: &Ui, code: &str, wrap: f32, t: &Tokens) -> Arc<Galley> {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let mut add = |range: Range<usize>, kind: Option<Kind>| {
        if let Some(text) = code.get(range).filter(|s| !s.is_empty()) {
            job.append(text, 0.0, TextFormat::simple(font.clone(), colour(kind, t)));
        }
    };
    let mut at = 0;
    for (range, kind) in spans(code) {
        add(at..range.start, None);
        at = range.end;
        add(range, Some(kind));
    }
    add(at..code.len(), None);
    ui.painter().layout_job(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each coloured stretch's text and kind.
    fn coloured(code: &str) -> Vec<(&str, Kind)> {
        spans(code).into_iter().map(|(r, k)| (code.get(r).unwrap_or("?"), k)).collect()
    }

    #[test]
    fn keywords_text_numbers_and_comments_are_told_apart() {
        let code = "let days = params.days ?? 7; // at least\nfor m in find_messages(#{q: \"from:ana\"}) { print(`${m.subject}`); }";
        assert_eq!(
            coloured(code),
            [
                ("let", Kind::Keyword),
                ("7", Kind::Literal),
                ("// at least", Kind::Comment),
                ("for", Kind::Keyword),
                ("in", Kind::Keyword),
                ("\"from:ana\"", Kind::Literal),
                ("`${m.subject}`", Kind::Literal),
            ]
        );
    }

    /// A word that only starts like a keyword, or holds one, is plain; a range's dots aren't a
    /// number's; a quote's `\"` doesn't close it; Rhai's comments nest.
    #[test]
    fn what_only_looks_like_one_is_read_as_rhai_does() {
        assert!(coloured("letter + format_in + iffy").is_empty());
        assert_eq!(coloured("0..10"), [("0", Kind::Literal), ("10", Kind::Literal)]);
        assert_eq!(coloured("1.5 + 0x1F_FF"), [("1.5", Kind::Literal), ("0x1F_FF", Kind::Literal)]);
        assert_eq!(coloured(r#""say \"hi\"" + 'x'"#), [(r#""say \"hi\"""#, Kind::Literal), ("'x'", Kind::Literal)]);
        assert_eq!(coloured("/* a /* b */ c */ let"), [("/* a /* b */ c */", Kind::Comment), ("let", Kind::Keyword)]);
    }

    /// Code half written: what isn't closed runs to its line's end (a backtick's, and a
    /// comment's, to the end), and the scan never stops short or runs past a character.
    #[test]
    fn code_half_written_is_coloured_as_far_as_it_reads() {
        assert_eq!(coloured("\"open\nlet"), [("\"open", Kind::Literal), ("let", Kind::Keyword)]);
        assert_eq!(coloured("`open\nlet"), [("`open\nlet", Kind::Literal)]);
        assert_eq!(coloured("/* open\nlet"), [("/* open\nlet", Kind::Comment)]);
        for code in ["", "\"", "/", "/*", "'", "`", "é", "1.", "let é = \"ü\"; // ß", "\\\"", "日本 /* 語 */ 1"] {
            let all = spans(code);
            assert!(all.iter().all(|(r, _)| code.get(r.clone()).is_some() && r.end <= code.len()), "{code:?}: {all:?}");
            assert!(all.windows(2).all(|w| w.first().map(|a| a.0.end) <= w.get(1).map(|b| b.0.start)), "{code:?}: in order");
        }
    }
}
