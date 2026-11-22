//! Where a reply's own text stops and the original it quotes starts, for the reading pane's
//! conversation to fold what an earlier message there already shows. The quote is told by the
//! line that begins it, as mail apps write it: who sent the original and when (`From:` then
//! `Sent:` or `Date:`), a rule of underscores or "Original Message", "On … wrote:", or `>`
//! before every line left.

/// How many lines are looked at: a reply's own text comes first, and mail can be long.
pub const QUOTE_LINES: usize = 2000;

/// Whether `line` is a rule a quote starts under: underscores, or dashes around "Original
/// Message" (in any case).
fn rule(line: &str) -> bool {
    let l = line.trim();
    (l.len() >= 8 && l.chars().all(|c| c == '_')) || (l.starts_with("---") && l.to_ascii_lowercase().contains("original message"))
}

/// Whether `line` starts with `label` (`From:`), in any case, after its indent.
fn labelled(line: &str, label: &str) -> bool {
    line.trim_start().get(..label.len()).is_some_and(|h| h.eq_ignore_ascii_case(label))
}

/// Where a reply's quote starts in `lines` (its text, line by line): the line it begins on,
/// with the rules and blank lines just above it. None when there's no quote, or nothing of the
/// reply's own before it (a message that's all quote keeps it: there would be nothing to show).
pub fn quoted_line<S: AsRef<str>>(lines: &[S]) -> Option<usize> {
    let lines: Vec<&str> = lines.iter().take(QUOTE_LINES).map(AsRef::as_ref).collect();
    let line = |i: usize| lines.get(i).copied().unwrap_or_default();
    let header = |i: usize| labelled(line(i), "From:") && (1..=2).any(|d| labelled(line(i + d), "Sent:") || labelled(line(i + d), "Date:"));
    let wrote =
        |i: usize| line(i).trim().starts_with("On ") && (line(i).trim_end().ends_with("wrote:") || line(i + 1).trim_end().ends_with("wrote:"));
    let marked_to_the_end = |i: usize| {
        let rest = lines.get(i..).unwrap_or_default();
        line(i).starts_with('>') && rest.iter().all(|l| l.trim().is_empty() || l.starts_with('>'))
    };
    let first = (0..lines.len()).find(|&i| rule(line(i)) || header(i) || wrote(i) || marked_to_the_end(i))?;
    let mut start = first;
    while start > 0 && (line(start - 1).trim().is_empty() || rule(line(start - 1))) {
        start -= 1;
    }
    lines.get(..start).is_some_and(|own| own.iter().any(|l| !l.trim().is_empty())).then_some(start)
}

/// Where a reply's quote starts in `text`: the byte offset of the line it begins on (as
/// [`quoted_line`] finds it).
pub fn quoted_start(text: &str) -> Option<usize> {
    let lines: Vec<&str> = text.split_inclusive('\n').take(QUOTE_LINES).collect();
    let trimmed: Vec<&str> = lines.iter().map(|l| l.trim_end_matches(['\r', '\n'])).collect();
    let at = quoted_line(&trimmed)?;
    Some(lines.iter().take(at).map(|l| l.len()).sum())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn own(text: &str) -> Option<&str> {
        quoted_start(text).and_then(|at| text.get(..at))
    }

    #[test]
    fn the_quote_starts_where_mail_apps_begin_it() {
        // As MailCraft and Outlook write it: a rule, then who sent it and when.
        let outlook = "Works for me.\n\nSam\n\n________________________________\nFrom: Zoe Park <zoe@studio.example>\nSent: Thursday 12:14\nSubject: Storyboard\n\nHi all";
        assert_eq!(own(outlook), Some("Works for me.\n\nSam\n"));
        let header = "Thanks!\r\nFrom: Ana <ana@soundlab.example>\r\nDate: Tue\r\n\r\nThe mix";
        assert_eq!(own(header), Some("Thanks!\r\n"));
        let original = "See below.\n-----Original Message-----\nFrom: someone";
        assert_eq!(own(original), Some("See below.\n"));
        let wrote = "Agreed.\n\nOn Tue, 3 Nov 2026, Ana Lima\n<ana@soundlab.example> wrote:\n> The mix";
        assert_eq!(own(wrote), Some("Agreed.\n"));
        let marks = "Yes.\n\n> Can you come?\n>\n> Zoe\n";
        assert_eq!(own(marks), Some("Yes.\n"));
        // Formatted text, line by line: the line the quote begins on.
        assert_eq!(quoted_line(&["Fine by me.", "", "From: Zoe", "Sent: Thu", "", "Hi"]), Some(1));
    }

    #[test]
    fn text_that_isnt_a_quote_is_kept() {
        // A `>` inside the reply's own text, a From line that isn't a header, a message that's
        // all quote, and no quote at all.
        assert_eq!(quoted_start("> one\nmy answer\n> two\nmore"), None);
        assert_eq!(quoted_start("From: the studio, with love\nSee you"), None);
        assert_eq!(quoted_start("From: Zoe\nSent: Thu\n\nHi"), None);
        assert_eq!(quoted_start("Just text.\nOn time, as always.\n"), None);
        assert_eq!(quoted_start(""), None);
        assert_eq!(quoted_line::<&str>(&[]), None);
    }

    proptest! {
        /// Any text: the start is a line's start inside it, after some of the reply's own.
        #[test]
        fn the_start_is_inside_the_text(text in "(.{0,40}\n?){0,12}") {
            if let Some(at) = quoted_start(&text) {
                prop_assert!(text.is_char_boundary(at) && at <= text.len());
                prop_assert!(at == 0 || text[..at].ends_with('\n'));
                prop_assert!(!text[..at].trim().is_empty());
            }
        }
    }
}
