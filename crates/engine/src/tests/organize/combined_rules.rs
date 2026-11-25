//! Rules whose conditions combine as a query's `where` does (all, any, not, nested; texts
//! compared as each says): added as rules are written now and as they were before, run on the
//! Inbox, and refused past the limits.

use mailcraft_model::organize::{CONDITION_DEPTH, Condition};

use super::*;

/// The senders of the Inbox's mail, in the sample mailbox.
fn inbox_senders(s: &mut Session) -> Vec<String> {
    s.folder_state("sample", "INBOX").messages.iter().map(|m| m.from.email.clone()).collect()
}

/// A rule's `where` runs on what it says: Zoe's storyboard mail moves, Sam's reply to it (whose
/// subject starts with "Re:") stays.
#[test]
fn a_combined_rule_runs_on_what_it_says() {
    let mut s = sample_session("rule-combined");
    let rule = json!({"name": "Storyboard, not replies", "where": {"if": "all", "of": [
        {"if": "subject", "value": "storyboard"},
        {"if": "not", "of": {"if": "subject", "value": "re:", "op": "startsWith"}},
    ]}, "actions": [{"do": "moveToRole", "value": "archive"}]});
    s.run("rules.add", &json!({ "rule": rule })).unwrap();
    assert_eq!(s.run("rules.run", &json!({"index": 0, "inbox": true})).unwrap()["matched"], 1);
    let senders = inbox_senders(&mut s);
    assert!(!senders.iter().any(|e| e == "zoe@studio.example"), "Zoe's moved: {senders:?}");
    assert!(senders.iter().any(|e| e == "sam@studio.example"), "the reply stays");
    let listed = s.run("rules.list", &json!({})).unwrap();
    assert_eq!(listed[0]["where"]["of"][1]["of"]["op"], "startsWith", "{listed}");
}

/// A rule written as rules were before their conditions combined (a list, and whether all or
/// any of it) is added and runs as it did.
#[test]
fn a_rule_written_before_runs_as_it_did() {
    let mut s = sample_session("rule-before");
    let rule = json!({"name": "Sam or Ana", "conditions": [{"if": "from", "value": "sam@"}, {"if": "from", "value": "ana@"}], "matchAll": false, "actions": [{"do": "moveToRole", "value": "archive"}]});
    s.run("rules.add", &json!({ "rule": rule })).unwrap();
    assert_eq!(s.rules[0].filter, Condition::Any { of: vec![Condition::from("sam@"), Condition::from("ana@")] });
    assert_eq!(s.run("rules.run", &json!({"index": 0, "inbox": true})).unwrap()["matched"], 2);
    let senders = inbox_senders(&mut s);
    assert!(!senders.iter().any(|e| e.starts_with("sam@") || e.starts_with("ana@")), "{senders:?}");
}

/// Conditions nested past the limit are refused, and nothing's added. A rule that looks at
/// nothing is kept, as it was before, and matches nothing: rules once saved never stop the
/// rest from being saved.
#[test]
fn rules_past_the_limits_are_refused() {
    let mut s = sample_session("rule-limits");
    let mut deep = json!({"if": "bulk"});
    for _ in 0..CONDITION_DEPTH {
        deep = json!({"if": "not", "of": deep});
    }
    let refused = s.run("rules.add", &json!({"rule": {"name": "Deep", "where": deep, "actions": [{"do": "flag"}]}}));
    assert!(matches!(&refused, Err(CmdError::Params(m)) if m.contains("nest too deep")), "{refused:?}");
    assert!(s.rules.is_empty());
    s.run("rules.add", &json!({"rule": {"name": "Nothing", "conditions": [], "actions": [{"do": "flag"}]}})).unwrap();
    s.run("rules.add", &json!({"rule": {"name": "Bulk", "where": {"if": "bulk"}, "actions": [{"do": "flag"}]}})).unwrap();
    assert_eq!(s.rules.len(), 2);
    assert_eq!(s.run("rules.run", &json!({"index": 0, "inbox": true})).unwrap()["matched"], 0);
}
