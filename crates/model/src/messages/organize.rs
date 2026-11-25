//! Organising mail: categories (colour-coded keywords), inbox rules, snoozing, and the
//! Focused/Other split of the Inbox. Pure data and matching: the engine applies them.

use serde::{Deserialize, Serialize};

use crate::dates::Clock;
use crate::{FolderRole, Summary, keyword_for};

mod conditions;
pub mod keywords;
mod rule_schema;
pub mod server_rules;

pub use conditions::{CONDITION_DEPTH, CONDITION_TESTS, Condition, Importance, QUESTION_CHARS, RULE_QUESTIONS, TextOp};
pub use rule_schema::{RuleAnswer, WORDS_DEPTH, rule_answer_schema};
pub use server_rules::{NO_SERVER_RULES, ServerRule, ServerRuleOp, ServerRules};

/// A category: a name and colour shown on messages, stored on the server as a keyword so other
/// clients (and other computers) see it too.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub keyword: String,
    /// An index into the UI's category palette.
    #[serde(default)]
    pub color: u8,
}

impl Category {
    pub fn new(name: &str, color: u8) -> Category {
        Category { name: name.trim().to_string(), keyword: keyword_for(name), color }
    }
}

/// The categories a new profile starts with. Their keywords are the ones Thunderbird uses for its
/// five default tags, so tags made there show here and back.
pub fn default_categories() -> Vec<Category> {
    [("Important", "$label1", 0), ("Work", "$label2", 1), ("Personal", "$label3", 2), ("To do", "$label4", 3), ("Later", "$label5", 4)]
        .into_iter()
        .map(|(n, k, c)| Category { name: n.into(), keyword: k.into(), color: c })
        .collect()
}

/// What a rule does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "do", content = "value", rename_all = "camelCase")]
pub enum RuleAction {
    /// Move to a folder (its path) of the same account.
    MoveTo(String),
    /// Move to the folder with this role (Archive, Junk…).
    MoveToRole(FolderRole),
    MarkRead,
    Flag,
    Pin,
    /// Add a category (its name).
    Categorize(String),
    /// Move to Deleted Items.
    Delete,
}

/// A rule, run on mail as it arrives in an Inbox, or (`sent`) on mail the user sends as its copy
/// reaches Sent (the client applies it, like Outlook's client-side rules, issue #202): what it
/// looks at (`where`, a query's) and what it does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "RuleRead")]
pub struct Rule {
    pub name: String,
    pub enabled: bool,
    /// Only this account's mail (`None`: every account).
    pub account: Option<String>,
    #[serde(rename = "where")]
    pub filter: Condition,
    pub actions: Vec<RuleAction>,
    /// Don't run later rules on a message this one matched.
    pub stop: bool,
    /// On mail the user sends (its copy in Sent), not mail arriving, as Outlook's rules can be.
    pub sent: bool,
}

/// A rule as it's read: as written now (`where`), or as rules were before their conditions
/// combined (a list, all or any of it).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleRead {
    name: String,
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default)]
    account: Option<String>,
    #[serde(default, rename = "where")]
    filter: Option<Condition>,
    #[serde(default)]
    conditions: Vec<Condition>,
    #[serde(default = "yes")]
    match_all: bool,
    actions: Vec<RuleAction>,
    #[serde(default)]
    stop: bool,
    #[serde(default)]
    sent: bool,
}

impl From<RuleRead> for Rule {
    fn from(r: RuleRead) -> Rule {
        let of = r.conditions;
        let filter = r.filter.unwrap_or(if r.match_all { Condition::All { of } } else { Condition::Any { of } });
        Rule { name: r.name, enabled: r.enabled, account: r.account, filter, actions: r.actions, stop: r.stop, sent: r.sent }
    }
}

fn yes() -> bool {
    true
}

impl Rule {
    /// Whether it acts on `m`, a question it asks MailCraft's AI taken as answered no.
    pub fn matches(&self, m: &Summary) -> bool {
        self.decides(m, Some(false)) == Some(true)
    }

    /// Whether it acts on `m`, given `answer` to the question it asks MailCraft's AI (none yet):
    /// `None` when that answer, not yet had, decides it.
    pub fn decides(&self, m: &Summary, answer: Option<bool>) -> Option<bool> {
        // One that looks at nothing, or past the limits, never matches (a text it looks for
        // left empty doesn't match by itself, as before rules combined their conditions).
        if !self.enabled || self.actions.is_empty() || self.filter.tests().is_empty() || self.filter.past_limits().is_some() {
            return Some(false);
        }
        if self.account.as_ref().is_some_and(|a| *a != m.key.account) {
            return Some(false);
        }
        self.filter.decides(m, answer)
    }
}

/// What rules do to a message: these actions, or nothing yet until MailCraft's AI answers this
/// question about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RulesSay {
    Act(Vec<RuleAction>),
    Ask(String),
}

/// The actions of every enabled rule that matches `m`, in rule order, given `answers` to the
/// questions asked about it; or the question the next rule waits on, when its answer decides it.
pub fn actions_for(rules: &[Rule], m: &Summary, answers: &[(String, bool)]) -> RulesSay {
    let mut out = Vec::new();
    for r in rules {
        let question = r.filter.question();
        let answer = question.and_then(|q| answers.iter().find(|(asked, _)| asked == q)).map(|(_, yes)| *yes);
        match (r.decides(m, answer), question) {
            (Some(true), _) => {
                out.extend(r.actions.iter().cloned());
                if r.stop {
                    break;
                }
            }
            (None, Some(q)) => return RulesSay::Ask(q.to_string()),
            (Some(false) | None, _) => {}
        }
    }
    RulesSay::Act(out)
}

/// A snoozed message: it waits in the Snoozed folder and comes back to `back_to` at `until`.
/// Found again by Message-ID, since moving gives it a new UID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snooze {
    pub account: String,
    pub message_id: String,
    /// The Snoozed folder's path.
    pub folder: String,
    pub back_to: String,
    pub until: i64,
    #[serde(default)]
    pub subject: String,
}

/// Snooze choices, as the menu offers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnoozeUntil {
    /// In three hours.
    LaterToday,
    /// 8:00 tomorrow.
    Tomorrow,
    /// 8:00 next Saturday.
    Weekend,
    /// 8:00 next Monday.
    NextWeek,
}

impl SnoozeUntil {
    pub const ALL: [SnoozeUntil; 4] = [SnoozeUntil::LaterToday, SnoozeUntil::Tomorrow, SnoozeUntil::Weekend, SnoozeUntil::NextWeek];

    pub fn code(self) -> &'static str {
        match self {
            SnoozeUntil::LaterToday => "later",
            SnoozeUntil::Tomorrow => "tomorrow",
            SnoozeUntil::Weekend => "weekend",
            SnoozeUntil::NextWeek => "nextWeek",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SnoozeUntil::LaterToday => "Later today",
            SnoozeUntil::Tomorrow => "Tomorrow",
            SnoozeUntil::Weekend => "This weekend",
            SnoozeUntil::NextWeek => "Next week",
        }
    }

    pub fn from_code(s: &str) -> Option<SnoozeUntil> {
        SnoozeUntil::ALL.into_iter().find(|p| p.code().eq_ignore_ascii_case(s))
    }

    /// When, in Unix seconds, for a user at `clock` (local time).
    pub fn at(self, clock: &Clock) -> i64 {
        let day = clock.local(clock.now).div_euclid(86_400);
        // A clock at the ends of time (a damaged one) stays there rather than overflowing.
        let morning = |days_on: i64| clock.utc(day.saturating_add(days_on).saturating_mul(86_400).saturating_add(8 * 3600));
        // 0 = Monday … 6 = Sunday.
        let weekday = i64::from(crate::dates::weekday_index(day));
        match self {
            SnoozeUntil::LaterToday => {
                let t = clock.now.saturating_add(3 * 3600);
                // The next whole hour.
                t.saturating_add((3600 - t.rem_euclid(3600)).rem_euclid(3600))
            }
            SnoozeUntil::Tomorrow => morning(1),
            SnoozeUntil::Weekend => morning(if weekday < 5 { 5 - weekday } else { 7 - weekday + 5 }),
            SnoozeUntil::NextWeek => morning(7 - weekday),
        }
    }
}

/// The account document an import of an Outlook data file leaves the days its flagged messages
/// are due in (local days, by [`Summary::follow_up_id`]), for the engine to take into the
/// preferences' own as the import ends: IMAP has nowhere for them, so they're kept there.
pub const FOLLOW_UP_DOC: &str = "follow-up.json";
/// Beside it, the days its messages marked complete were completed on (`PidTagFlagCompleteTime`),
/// taken into the preferences' own (`Prefs::follow_up_done`) the same way.
pub const FOLLOW_UP_DONE_DOC: &str = "follow-up-done.json";
/// Beside them, when its flagged messages remind (Unix seconds: `PidLidReminderSignalTime`, as
/// Outlook set them), taken into the preferences' own (`Prefs::mail_reminders`) the same way.
pub const FOLLOW_UP_REMINDER_DOC: &str = "follow-up-reminders.json";
/// The most due days kept, in the preferences and in an import's [`FOLLOW_UP_DOC`]; and the most
/// days completed, in each of theirs.
pub const MAX_FOLLOW_UP: usize = 10_000;
/// The longest a due day's key is kept (a Message-ID's header line is at most 998 bytes).
pub const MAX_FOLLOW_UP_ID: usize = 998;
/// The days a follow-up can be due on: 1900-01-01 to 4500-12-31 (Outlook's 4501-01-01 is its
/// "no date").
pub const FOLLOW_UP_DAYS: std::ops::RangeInclusive<i64> = -25_567..=924_428;

/// `from`'s flag reminders put in `into` where it has none for that message (one set here stays),
/// up to [`MAX_FOLLOW_UP`] in all; keys too long, or times not on a day a follow-up may have
/// ([`FOLLOW_UP_DAYS`]), are left out. How many were added.
pub fn merge_reminders(into: &mut std::collections::BTreeMap<String, i64>, from: std::collections::BTreeMap<String, i64>) -> usize {
    let days: std::collections::BTreeMap<String, i64> = from.iter().map(|(id, at)| (id.clone(), at.div_euclid(86_400))).collect();
    let mut kept = std::collections::BTreeMap::new();
    merge_follow_up(&mut kept, days);
    let mut added = 0;
    for (id, at) in from.into_iter().filter(|(id, _)| kept.contains_key(id)) {
        if into.len() >= MAX_FOLLOW_UP {
            break;
        }
        if let std::collections::btree_map::Entry::Vacant(e) = into.entry(id) {
            e.insert(at);
            added += 1;
        }
    }
    added
}

/// `from`'s due days put in `into` where it has none for that message (a day set here stays),
/// up to [`MAX_FOLLOW_UP`] in all; keys too long or days out of [`FOLLOW_UP_DAYS`] are left
/// out. How many were added.
pub fn merge_follow_up(into: &mut std::collections::BTreeMap<String, i64>, from: std::collections::BTreeMap<String, i64>) -> usize {
    let mut added = 0;
    for (id, day) in from {
        if into.len() >= MAX_FOLLOW_UP {
            break;
        }
        if id.trim().is_empty() || id.len() > MAX_FOLLOW_UP_ID || !FOLLOW_UP_DAYS.contains(&day) || into.contains_key(&id) {
            continue;
        }
        into.insert(id, day);
        added += 1;
    }
    added
}

/// The days of an account's follow-ups, as an export writes them (local days, by
/// [`Summary::follow_up_id`]): when flagged messages are due, and when the ones marked complete
/// were completed; and when flagged ones remind (Unix seconds).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowUpDays {
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub due: std::collections::BTreeMap<String, i64>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub done: std::collections::BTreeMap<String, i64>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub reminder: std::collections::BTreeMap<String, i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dates::civil_from_days;
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

    #[test]
    fn rules_match_and_stop() {
        let news = Rule {
            name: "News".into(),
            enabled: true,
            account: None,
            filter: Condition::all(vec![Condition::Bulk, Condition::from("letters")]),
            actions: vec![RuleAction::MoveTo("News".into()), RuleAction::MarkRead],
            stop: true,
            sent: false,
        };
        let flag = Rule {
            name: "Boss".into(),
            filter: Condition::all(vec![Condition::from("boss@")]),
            actions: vec![RuleAction::Flag],
            stop: false,
            ..news.clone()
        };
        let rules = vec![news.clone(), flag.clone()];
        let act = |v: Vec<RuleAction>| RulesSay::Act(v);
        assert_eq!(
            actions_for(&rules, &m("letters@shop.example", "Sale", true), &[]),
            act(vec![RuleAction::MoveTo("News".into()), RuleAction::MarkRead])
        );
        assert_eq!(actions_for(&rules, &m("letters@shop.example", "Hi", false), &[]), act(vec![]), "all conditions must hold");
        assert_eq!(actions_for(&rules, &m("BOSS@x.example", "Now", false), &[]), act(vec![RuleAction::Flag]));
        let any = Rule { filter: Condition::Any { of: vec![Condition::Bulk, Condition::from("letters")] }, ..news.clone() };
        assert!(any.matches(&m("x@y.example", "Hi", true)));
        assert!(!Rule { enabled: false, ..news.clone() }.matches(&m("letters@x", "", true)));
        assert!(!Rule { account: Some("a2".into()), ..news }.matches(&m("letters@x", "", true)));
        assert!(Condition::To { value: "me@".into(), op: TextOp::Contains }.matches(&m("a@b", "", false)));
        assert!(!Condition::subject("  ").matches(&m("a@b", "anything", false)), "an empty test matches nothing");
        // Rules round-trip through JSON (rules.json, the CLI).
        let json = serde_json::to_string(&rules).unwrap();
        assert!(json.contains("\"if\":\"bulk\""), "{json}");
        assert_eq!(serde_json::from_str::<Vec<Rule>>(&json).unwrap(), rules);
    }

    /// A rule's question to MailCraft's AI is asked only when its answer decides the rule (its
    /// other tests leave it open), and the rules after it wait for it; once answered, they run on.
    #[test]
    fn a_rules_question_is_asked_only_when_its_answer_decides() {
        let contract = Condition::Asks { question: "Is it a contract to sign?".into() };
        let ask = Rule {
            name: "Contracts".into(),
            enabled: true,
            account: None,
            filter: Condition::all(vec![Condition::from("studio"), contract.clone()]),
            actions: vec![RuleAction::Flag],
            stop: true,
            sent: false,
        };
        let pin = Rule { name: "All".into(), filter: Condition::all(vec![Condition::from("@")]), actions: vec![RuleAction::Pin], ..ask.clone() };
        let rules = vec![ask.clone(), pin];
        let studio = m("ops@studio.example", "Agreement", false);
        let q = "Is it a contract to sign?".to_string();
        assert_eq!(actions_for(&rules, &studio, &[]), RulesSay::Ask(q.clone()), "the answer decides it: the rules wait");
        assert_eq!(actions_for(&rules, &studio, &[(q.clone(), true)]), RulesSay::Act(vec![RuleAction::Flag]), "yes, and it stops there");
        assert_eq!(actions_for(&rules, &studio, &[(q.clone(), false)]), RulesSay::Act(vec![RuleAction::Pin]));
        assert_eq!(actions_for(&rules, &m("a@shop.example", "Sale", false), &[]), RulesSay::Act(vec![RuleAction::Pin]), "not asked: from elsewhere");
        let either = Rule { filter: Condition::Any { of: vec![Condition::from("studio"), contract.clone()] }, ..ask.clone() };
        assert_eq!(either.decides(&studio, None), Some(true), "not asked: it matches either way");
        assert!(!ask.matches(&studio), "unanswered is no");
        let not = Rule { filter: Condition::all(vec![Condition::from("studio"), Condition::Not { of: Box::new(contract.clone()) }]), ..ask.clone() };
        assert_eq!((not.decides(&studio, Some(false)), not.decides(&studio, Some(true))), (Some(true), Some(false)));
        // More than one question, or one too long or empty, never runs, nor is it asked.
        let two = Condition::all(vec![contract.clone(), Condition::Asks { question: "Is it urgent?".into() }]);
        assert_eq!(two.past_limits(), Some("it asks MailCraft's AI more than one question"));
        assert_eq!(Rule { filter: two, ..ask.clone() }.decides(&studio, None), Some(false));
        let long = Condition::Asks { question: "?".repeat(QUESTION_CHARS + 1) };
        assert_eq!(long.past_limits(), Some("a question it asks is too long"));
        assert_eq!(Condition::Asks { question: " ".into() }.refused(), Some("a question it asks is empty"));
        let json = serde_json::to_value(&contract).unwrap();
        assert_eq!(json, serde_json::json!({"if": "asks", "question": "Is it a contract to sign?"}));
    }

    /// Rules written before conditions combined (a list, all or any of it) read as ever, and
    /// are written back as a query's `where`.
    #[test]
    fn rules_written_before_read_as_ever() {
        let before = r#"[{"name": "News", "conditions": [{"if": "bulk"}, {"if": "from", "value": "letters"}], "matchAll": false, "actions": [{"do": "markRead"}]},
            {"name": "Boss", "enabled": false, "conditions": [{"if": "subject", "value": "now"}], "actions": [{"do": "flag"}], "stop": true}]"#;
        let rules: Vec<Rule> = serde_json::from_str(before).unwrap();
        assert_eq!(rules[0].filter, Condition::Any { of: vec![Condition::Bulk, Condition::from("letters")] });
        assert_eq!((rules[1].filter.clone(), rules[1].enabled, rules[1].stop), (Condition::all(vec![Condition::subject("now")]), false, true));
        let json = serde_json::to_string(&rules).unwrap();
        assert!(
            json.contains(r#""where":{"if":"any","of":[{"if":"bulk"},{"if":"from","value":"letters"}]}"#) && !json.contains("conditions"),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<Vec<Rule>>(&json).unwrap(), rules);
        let op: Condition = serde_json::from_str(r#"{"if": "subject", "value": "x", "op": "startsWith"}"#).unwrap();
        assert_eq!(op, Condition::Subject { value: "x".into(), op: TextOp::StartsWith });
    }

    #[test]
    fn snooze_times_are_local() {
        // Wednesday 2026-10-14 15:20 UTC, in UTC+2 (17:20 local).
        let clock = Clock::new(1_791_991_200 + 20 * 60, 7200);
        let local_day = |t: i64| civil_from_days((t + 7200).div_euclid(86_400));
        let local_hour = |t: i64| (t + 7200).rem_euclid(86_400) / 3600;
        let later = SnoozeUntil::LaterToday.at(&clock);
        assert!(later > clock.now + 3 * 3600 - 1 && later % 3600 == 0);
        let t = SnoozeUntil::Tomorrow.at(&clock);
        assert_eq!((local_day(t), local_hour(t)), ((2026, 10, 15), 8));
        let w = SnoozeUntil::Weekend.at(&clock);
        assert_eq!((local_day(w), local_hour(w)), ((2026, 10, 17), 8), "Saturday");
        let n = SnoozeUntil::NextWeek.at(&clock);
        assert_eq!((local_day(n), local_hour(n)), ((2026, 10, 19), 8), "Monday");
        // On a Sunday, "this weekend" is next Saturday and "next week" is tomorrow.
        let sunday = Clock::new(1_792_324_800, 0); // 2026-10-18 12:00 UTC
        assert_eq!(civil_from_days(sunday.now.div_euclid(86_400)), (2026, 10, 18));
        assert_eq!(civil_from_days(SnoozeUntil::NextWeek.at(&sunday).div_euclid(86_400)), (2026, 10, 19));
        assert_eq!(civil_from_days(SnoozeUntil::Weekend.at(&sunday).div_euclid(86_400)), (2026, 10, 24));
        assert_eq!(SnoozeUntil::from_code("NEXTWEEK"), Some(SnoozeUntil::NextWeek));
    }

    /// A clock at either end of time (a damaged one) gives a time, not an overflow.
    #[test]
    fn snoozing_at_the_ends_of_time() {
        for now in [i64::MAX - 5, i64::MIN + 5] {
            let clock = Clock::new(now, 0);
            for s in [SnoozeUntil::LaterToday, SnoozeUntil::Tomorrow, SnoozeUntil::Weekend, SnoozeUntil::NextWeek] {
                let _ = s.at(&clock);
            }
        }
        assert_eq!(SnoozeUntil::LaterToday.at(&Clock::new(i64::MAX - 5, 0)), i64::MAX);
    }

    #[test]
    fn default_categories_use_shared_keywords() {
        let c = default_categories();
        assert_eq!(c[0].keyword, "$label1");
        assert_eq!(Category::new(" Clients ", 6), Category { name: "Clients".into(), keyword: "clients".into(), color: 6 });
    }

    /// An import's due days go in beside the user's: a day set here stays, junk is left out, and
    /// no more than the most kept go in.
    #[test]
    fn imported_due_days_merge_under_the_users() {
        use crate::dates::days_from_civil;
        use std::collections::BTreeMap;
        assert_eq!(FOLLOW_UP_DAYS, days_from_civil(1900, 1, 1)..=days_from_civil(4501, 1, 1) - 1);
        let mut mine = BTreeMap::from([("a@x".to_string(), 20_000)]);
        let theirs = BTreeMap::from([
            ("a@x".to_string(), 20_001),
            ("b@x".to_string(), 20_002),
            ("c@x".to_string(), days_from_civil(4501, 1, 1)),
            ("d@x".to_string(), i64::MIN),
            (" ".to_string(), 20_003),
            ("e".repeat(MAX_FOLLOW_UP_ID + 1), 20_004),
        ]);
        assert_eq!(merge_follow_up(&mut mine, theirs), 1);
        assert_eq!(mine, BTreeMap::from([("a@x".to_string(), 20_000), ("b@x".to_string(), 20_002)]));
        let many: BTreeMap<String, i64> = (0..MAX_FOLLOW_UP + 5).map(|i| (format!("{i}@x"), 20_000)).collect();
        merge_follow_up(&mut mine, many);
        assert_eq!(mine.len(), MAX_FOLLOW_UP);
    }
}
