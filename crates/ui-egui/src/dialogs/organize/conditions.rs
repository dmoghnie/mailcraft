//! A rule's conditions as the Rules dialog shows them: in words however they combine (all of
//! them joined by "and", any by "or", what's nested in brackets), and the one test the rule
//! editor shows and changes, when the rule's conditions are ones it can show.

use mailcraft_engine::model::organize::{Condition, Importance, TextOp};

use super::*;

/// The deepest a description goes (a rule is refused past `CONDITION_DEPTH`; one read from a
/// damaged file that still holds more is cut short, never followed without end).
const SAID_DEPTH: usize = 8;

/// The test the editor shows of `filter` and changes: its first, when they're all tests (none
/// nested, none "not") of all or any, and that first is one the editor offers. `None`: the
/// editor shows the conditions in words and keeps them as they are.
pub(super) fn first_shown(filter: &Condition) -> Option<(RuleTest, String)> {
    let test = |c: &Condition| !matches!(c, Condition::All { .. } | Condition::Any { .. } | Condition::Not { .. });
    let first = match filter {
        Condition::All { of } | Condition::Any { of } if of.iter().all(test) => of.first()?,
        Condition::All { .. } | Condition::Any { .. } | Condition::Not { .. } => return None,
        test => test,
    };
    Some(match first {
        Condition::From { value, op: TextOp::Contains } => (RuleTest::From, value.clone()),
        Condition::To { value, op: TextOp::Contains } => (RuleTest::To, value.clone()),
        Condition::Subject { value, op: TextOp::Contains } => (RuleTest::Subject, value.clone()),
        Condition::AnyText { value, op: TextOp::Contains } => (RuleTest::AnyText, value.clone()),
        Condition::HasAttachment => (RuleTest::HasAttachment, String::new()),
        Condition::Bulk => (RuleTest::Bulk, String::new()),
        _ => return None,
    })
}

/// `filter`'s first test made `new`: the first of all (or any) of them, or all of `new` alone.
pub(super) fn put_first_test(filter: &mut Condition, new: Condition) {
    match filter {
        Condition::All { of } | Condition::Any { of } => put_first(of, new),
        other => *other = Condition::all(vec![new]),
    }
}

/// `c` in words, as "If it …" goes on.
pub(super) fn said(c: &Condition) -> String {
    said_at(c, 0)
}

fn said_at(c: &Condition, depth: usize) -> String {
    if depth > SAID_DEPTH {
        return "…".into();
    }
    // A combination inside another goes in brackets.
    let part = |c: &Condition| match c {
        Condition::All { of } | Condition::Any { of } if of.len() > 1 => format!("({})", said_at(c, depth + 1)),
        _ => said_at(c, depth + 1),
    };
    // The comparison holds its text: languages put it before or after their verb.
    let text = |field: &str, op: TextOp, value: &str| {
        let op = match op {
            TextOp::Contains => tf!("contains “{text}”", text = value),
            TextOp::Is => tf!("is “{text}”", text = value),
            TextOp::StartsWith => tf!("starts with “{text}”", text = value),
            TextOp::EndsWith => tf!("ends with “{text}”", text = value),
        };
        tf!("has {field} that {op}", field = field, op = op)
    };
    match c {
        Condition::All { of } => of.iter().map(part).collect::<Vec<_>>().join(&format!(" {} ", tl!("and"))),
        Condition::Any { of } => of.iter().map(part).collect::<Vec<_>>().join(&format!(" {} ", tl!("or"))),
        Condition::Not { of } => tf!("not ({condition})", condition = said_at(of, depth + 1)),
        Condition::From { value, op: TextOp::Contains } => tf!("is from “{text}”", text = value),
        Condition::To { value, op: TextOp::Contains } => tf!("is sent to “{text}”", text = value),
        Condition::Subject { value, op: TextOp::Contains } => tf!("has “{text}” in its subject", text = value),
        Condition::AnyText { value, op: TextOp::Contains } => tf!("mentions “{text}”", text = value),
        Condition::From { value, op } => text(tl!("a sender"), *op, value),
        Condition::To { value, op } => text(tl!("a recipient"), *op, value),
        Condition::Subject { value, op } => text(tl!("a subject"), *op, value),
        Condition::AnyText { value, op } => text(tl!("a sender, recipient or subject"), *op, value),
        Condition::Preview { value, op } => text(tl!("text"), *op, value),
        Condition::HasAttachment => tl!("has attachments").to_string(),
        Condition::Bulk => tl!("is list mail").to_string(),
        Condition::Importance { value: Importance::High } => tl!("is of high importance").to_string(),
        Condition::Importance { value: Importance::Low } => tl!("is of low importance").to_string(),
        Condition::Larger { kb } => tf!("is larger than {size} KB", size = kb),
        Condition::Smaller { kb } => tf!("is smaller than {size} KB", size = kb),
        Condition::Asks { question } => tf!("gets a yes from MailCraft's AI to “{question}”", question = question),
    }
}
