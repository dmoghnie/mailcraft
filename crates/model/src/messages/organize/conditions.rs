//! What a rule looks at, as a query's `where` does: tests of a message's fields (who it's from
//! and to, its subject and text, its size, importance, attachments, whether it's list mail),
//! combined with all, any and not, within limits a damaged file or a model's answer can't pass.

use serde::{Deserialize, Serialize};

use crate::Summary;

/// The deepest a rule's conditions nest, and the most it looks at: what's past them (a damaged or
/// hostile rules file, an answer of a model) is refused, and never matches.
pub const CONDITION_DEPTH: usize = 6;
pub const CONDITION_TESTS: usize = 50;

/// What a rule looks at, as a query's `where` does: tests of a message's fields, combined with
/// all, any and not. Text tests ignore case. In JSON each is an object tagged `if`; the tests
/// alone are what rules said before they combined (`{"if": "from", "value": …}`), read as ever.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "if", rename_all = "camelCase")]
pub enum Condition {
    /// Every one of these.
    All {
        of: Vec<Condition>,
    },
    /// At least one of these.
    Any {
        of: Vec<Condition>,
    },
    /// Not this.
    Not {
        of: Box<Condition>,
    },
    /// The sender's name or address.
    From {
        value: String,
        #[serde(default, skip_serializing_if = "TextOp::is_contains")]
        op: TextOp,
    },
    /// Any To or Cc recipient's name or address.
    To {
        value: String,
        #[serde(default, skip_serializing_if = "TextOp::is_contains")]
        op: TextOp,
    },
    Subject {
        value: String,
        #[serde(default, skip_serializing_if = "TextOp::is_contains")]
        op: TextOp,
    },
    /// The subject, the sender or a recipient.
    AnyText {
        value: String,
        #[serde(default, skip_serializing_if = "TextOp::is_contains")]
        op: TextOp,
    },
    /// The start of its text, as the list shows it under the subject.
    Preview {
        value: String,
        #[serde(default, skip_serializing_if = "TextOp::is_contains")]
        op: TextOp,
    },
    HasAttachment,
    /// Newsletters, lists, automatic mail.
    Bulk,
    /// Marked by its sender as of high or low importance.
    Importance {
        value: Importance,
    },
    /// Larger than this many kilobytes.
    Larger {
        kb: u32,
    },
    /// Smaller than this many kilobytes.
    Smaller {
        kb: u32,
    },
}

/// How a text test compares, ignoring case.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextOp {
    #[default]
    Contains,
    Is,
    StartsWith,
    EndsWith,
}

impl TextOp {
    pub const ALL: [TextOp; 4] = [TextOp::Contains, TextOp::Is, TextOp::StartsWith, TextOp::EndsWith];

    pub fn is_contains(&self) -> bool {
        *self == TextOp::Contains
    }

    /// Its name in JSON.
    pub fn code(self) -> &'static str {
        match self {
            TextOp::Contains => "contains",
            TextOp::Is => "is",
            TextOp::StartsWith => "startsWith",
            TextOp::EndsWith => "endsWith",
        }
    }

    /// Whether `hay` holds `needle` as it says (an empty needle never does).
    pub fn test(self, hay: &str, needle: &str) -> bool {
        let needle = needle.trim().to_lowercase();
        if needle.is_empty() {
            return false;
        }
        let hay = hay.trim().to_lowercase();
        match self {
            TextOp::Contains => hay.contains(&needle),
            TextOp::Is => hay == needle,
            TextOp::StartsWith => hay.starts_with(&needle),
            TextOp::EndsWith => hay.ends_with(&needle),
        }
    }
}

/// The importance a sender can mark a message with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Importance {
    High,
    Low,
}

impl Condition {
    /// Every one of `of`: the form rules take when they don't combine.
    pub fn all(of: Vec<Condition>) -> Condition {
        Condition::All { of }
    }

    /// A text test that something contains `value`.
    pub fn from(value: &str) -> Condition {
        Condition::From { value: value.into(), op: TextOp::Contains }
    }

    pub fn subject(value: &str) -> Condition {
        Condition::Subject { value: value.into(), op: TextOp::Contains }
    }

    /// Whether message `m` is one this says, nested no deeper than [`CONDITION_DEPTH`]
    /// (deeper never matches). All and any of nothing never match.
    pub fn matches(&self, m: &Summary) -> bool {
        self.matches_at(m, 0)
    }

    fn matches_at(&self, m: &Summary, depth: usize) -> bool {
        if depth > CONDITION_DEPTH {
            return false;
        }
        // A name or an address: "is" is either of them, the others look at both together.
        let person = |a: &crate::EmailAddress, op: TextOp, t: &str| match op {
            TextOp::Is => op.test(&a.email, t) || op.test(&a.name, t),
            _ => op.test(&format!("{} {}", a.name, a.email), t) || op.test(&a.email, t),
        };
        let anyone = |op: TextOp, t: &str| m.to.iter().chain(&m.cc).any(|a| person(a, op, t));
        match self {
            Condition::All { of } => !of.is_empty() && of.iter().all(|c| c.matches_at(m, depth + 1)),
            Condition::Any { of } => of.iter().any(|c| c.matches_at(m, depth + 1)),
            Condition::Not { of } => !of.matches_at(m, depth + 1),
            Condition::From { value, op } => person(&m.from, *op, value),
            Condition::To { value, op } => anyone(*op, value),
            Condition::Subject { value, op } => op.test(&m.subject, value),
            Condition::AnyText { value, op } => op.test(&m.subject, value) || person(&m.from, *op, value) || anyone(*op, value),
            Condition::Preview { value, op } => op.test(&m.preview, value),
            Condition::HasAttachment => m.has_attachments,
            Condition::Bulk => m.bulk,
            Condition::Importance { value: Importance::High } => m.importance > 0,
            Condition::Importance { value: Importance::Low } => m.importance < 0,
            Condition::Larger { kb } => u64::from(m.size) > u64::from(*kb) * 1024,
            Condition::Smaller { kb } => u64::from(m.size) < u64::from(*kb) * 1024,
        }
    }

    /// The tests it holds, however combined (all, any and not hold none of their own).
    pub fn tests(&self) -> Vec<&Condition> {
        match self {
            Condition::All { of } | Condition::Any { of } => of.iter().flat_map(Condition::tests).collect(),
            Condition::Not { of } => of.tests(),
            test => vec![test],
        }
    }

    /// How deep it nests: a test alone is 1.
    pub fn depth(&self) -> usize {
        match self {
            Condition::All { of } | Condition::Any { of } => 1 + of.iter().map(Condition::depth).max().unwrap_or(0),
            Condition::Not { of } => 1 + of.depth(),
            _ => 1,
        }
    }

    /// The tests of a rule that doesn't combine them otherwise: all of them, each a test (or the
    /// one test it is, or any of one). `None` when it nests, or says any of several or not.
    pub fn simple(&self) -> Option<Vec<&Condition>> {
        let test = |c: &Condition| !matches!(c, Condition::All { .. } | Condition::Any { .. } | Condition::Not { .. });
        match self {
            Condition::All { of } if of.iter().all(test) => Some(of.iter().collect()),
            Condition::Any { of } if of.len() == 1 && of.iter().all(test) => Some(of.iter().collect()),
            Condition::All { .. } | Condition::Any { .. } | Condition::Not { .. } => None,
            test => Some(vec![test]),
        }
    }

    /// Why no rule can keep it: it nests too deep, or looks at too much.
    pub fn past_limits(&self) -> Option<&'static str> {
        match () {
            _ if self.depth() > CONDITION_DEPTH => Some("its conditions nest too deep"),
            _ if self.tests().len() > CONDITION_TESTS => Some("it looks at too much"),
            _ => None,
        }
    }

    /// Why it can't be a new rule's: nothing it tests, an empty text, or past the limits.
    pub fn refused(&self) -> Option<&'static str> {
        let tests = self.tests();
        let empty_text = tests.iter().any(|t| match t {
            Condition::From { value, .. } | Condition::To { value, .. } | Condition::Subject { value, .. } => value.trim().is_empty(),
            Condition::AnyText { value, .. } | Condition::Preview { value, .. } => value.trim().is_empty(),
            _ => false,
        });
        match () {
            _ if tests.is_empty() => Some("it looks at nothing"),
            _ if empty_text => Some("a text it looks for is empty"),
            _ => self.past_limits(),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::organize::{Rule, RuleAction};
    use crate::{EmailAddress, MessageKey};

    fn m(from: &str, subject: &str, bulk: bool) -> Summary {
        Summary {
            key: MessageKey { account: "a1".into(), ..Default::default() },
            from: EmailAddress::new("", from),
            to: vec![EmailAddress::new("Me", "me@x.example")],
            subject: subject.into(),
            bulk,
            ..Default::default()
        }
    }

    /// Conditions combine as a query's: all, any and not, nested; texts compared as each says.
    #[test]
    fn conditions_combine_and_compare() {
        let m = Summary {
            size: 3000,
            importance: 1,
            preview: "Invoice 42 attached".into(),
            ..m("Shop <orders@print.example>", "Receipt for May", false)
        };
        let receipt = Condition::all(vec![
            Condition::from("print.example"),
            Condition::Any { of: vec![Condition::subject("invoice"), Condition::subject("receipt")] },
            Condition::Not { of: Box::new(Condition::Bulk) },
        ]);
        assert!(receipt.matches(&m));
        assert!(!receipt.matches(&Summary { bulk: true, ..m.clone() }), "not bulk");
        let text = |op: TextOp, value: &str| Condition::Subject { value: value.into(), op };
        assert!(text(TextOp::StartsWith, "RECEIPT").matches(&m) && !text(TextOp::StartsWith, "May").matches(&m));
        assert!(
            text(TextOp::EndsWith, "may").matches(&m)
                && text(TextOp::Is, " receipt for may ").matches(&m)
                && !text(TextOp::Is, "receipt").matches(&m)
        );
        let from_is = |v: &str| Condition::From { value: v.into(), op: TextOp::Is };
        assert!(from_is("orders@print.example").matches(&Summary { from: EmailAddress::new("Shop", "orders@print.example"), ..m.clone() }));
        assert!(from_is("shop").matches(&Summary { from: EmailAddress::new("Shop", "orders@print.example"), ..m.clone() }), "or the name");
        assert!(!from_is("print.example").matches(&m), "is: the whole address");
        assert!(Condition::Preview { value: "invoice 42".into(), op: TextOp::Contains }.matches(&m));
        assert!(Condition::Importance { value: Importance::High }.matches(&m) && !Condition::Importance { value: Importance::Low }.matches(&m));
        assert!(Condition::Larger { kb: 2 }.matches(&m) && !Condition::Larger { kb: 3 }.matches(&m) && Condition::Smaller { kb: 3 }.matches(&m));
        assert!(!Condition::all(Vec::new()).matches(&m) && !Condition::Any { of: Vec::new() }.matches(&m), "all and any of nothing never match");
        assert_eq!(receipt.tests().len(), 4);
        assert_eq!(receipt.depth(), 3);
        assert!(receipt.simple().is_none() && Condition::all(vec![Condition::Bulk, Condition::HasAttachment]).simple().is_some_and(|t| t.len() == 2));
    }

    /// Past the limits, or looking at nothing, a rule is refused and never matches: a damaged
    /// or hostile file, or a model's answer, can't nest without end.
    #[test]
    fn conditions_past_the_limits_are_refused() {
        let mut deep = Condition::Bulk;
        for _ in 0..CONDITION_DEPTH {
            deep = Condition::Not { of: Box::new(deep) };
        }
        assert_eq!((deep.refused(), deep.past_limits()), (Some("its conditions nest too deep"), Some("its conditions nest too deep")));
        let rule = |filter: Condition| Rule {
            name: "R".into(),
            enabled: true,
            account: None,
            filter,
            actions: vec![RuleAction::Flag],
            stop: false,
            sent: false,
        };
        assert!(!rule(deep).matches(&m("a@b", "", true)));
        let many = Condition::Any { of: (0..=CONDITION_TESTS).map(|_| Condition::Bulk).collect() };
        assert_eq!(many.refused(), Some("it looks at too much"));
        assert_eq!(Condition::all(Vec::new()).refused(), Some("it looks at nothing"));
        assert_eq!(Condition::all(Vec::new()).past_limits(), None, "kept, as rules were before: it never matches");
        assert!(!rule(Condition::Not { of: Box::new(Condition::all(Vec::new())) }).matches(&m("a@b", "", true)), "not of nothing looks at nothing");
        assert_eq!(Condition::all(vec![Condition::subject(" ")]).refused(), Some("a text it looks for is empty"));
        assert_eq!(Condition::all(vec![Condition::Bulk]).refused(), None);
    }

    fn text() -> impl Strategy<Value = String> {
        "[a-zA-Z@. ]{0,12}"
    }

    /// A test of one field, any of them.
    fn test() -> impl Strategy<Value = Condition> {
        let op = || proptest::sample::select(TextOp::ALL.to_vec());
        prop_oneof![
            (text(), op()).prop_map(|(value, op)| Condition::From { value, op }),
            (text(), op()).prop_map(|(value, op)| Condition::To { value, op }),
            (text(), op()).prop_map(|(value, op)| Condition::Subject { value, op }),
            (text(), op()).prop_map(|(value, op)| Condition::AnyText { value, op }),
            (text(), op()).prop_map(|(value, op)| Condition::Preview { value, op }),
            Just(Condition::HasAttachment),
            Just(Condition::Bulk),
            prop_oneof![Just(Importance::High), Just(Importance::Low)].prop_map(|value| Condition::Importance { value }),
            any::<u32>().prop_map(|kb| Condition::Larger { kb }),
            any::<u32>().prop_map(|kb| Condition::Smaller { kb }),
        ]
    }

    /// Tests combined with all, any and not, nested past the limit at times.
    fn tree() -> impl Strategy<Value = Condition> {
        test().prop_recursive(8, 40, 4, |inner| {
            prop_oneof![
                proptest::collection::vec(inner.clone(), 0..4).prop_map(|of| Condition::All { of }),
                proptest::collection::vec(inner.clone(), 0..4).prop_map(|of| Condition::Any { of }),
                inner.prop_map(|c| Condition::Not { of: Box::new(c) }),
            ]
        })
    }

    proptest::proptest! {
        /// Any conditions are written and read back the same (rules.json, the CLI, a model's
        /// answer), and "not" says the opposite of what it holds, within the limits.
        #[test]
        fn conditions_round_trip_and_not_inverts(c in tree(), subject in text(), size in 0u32..100_000) {
            let json = serde_json::to_string(&c).unwrap();
            prop_assert_eq!(serde_json::from_str::<Condition>(&json).unwrap(), c.clone());
            let msg = Summary { size, preview: subject.clone(), ..m("Shop <orders@print.example>", &subject, size % 2 == 0) };
            if c.depth() < CONDITION_DEPTH {
                prop_assert_eq!(Condition::Not { of: Box::new(c.clone()) }.matches(&msg), !c.matches(&msg));
            }
        }
    }
}
