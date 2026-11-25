//! Engine tests: Sweep, a sender's mail in a folder cleared out in one step.

use mailcraft_model::organize::{Condition, RuleAction};

use crate::session::sweep::{SWEEP_DAYS, SweepChoice};

use super::*;

const DAY: i64 = 86_400;
const PAT: &str = "pat@lists.example";
/// The same person writing from home: not swept with the address above.
const PAT_HOME: &str = "pat.lee@home.example";

/// A message from `email` filed in the sample Inbox as uid `uid`, written `days` days ago.
fn from_pat(s: &mut Session, uid: u32, email: &str, days: i64) -> mailcraft_model::MessageKey {
    let store = s.store.clone().unwrap();
    let mut m = mailcraft_model::Summary::clone(&store.load_folder("sample", "INBOX").unwrap().messages[0]);
    m.key.uid = uid;
    m.subject = format!("Pat's note {uid}");
    m.from = mailcraft_model::EmailAddress::new("Pat Lee", email);
    m.message_id = format!("<pat-{uid}@lists.example>");
    m.date = s.clock.now - days * DAY;
    m.flags.seen = false;
    file_in(s, m, None)
}

/// The sample with Pat's mail in the Inbox: three from the list address (a day, twelve days and
/// thirty days old) and one from home. The Inbox open, a list message selected.
fn with_pat(name: &str) -> (Session, mailcraft_model::MessageKey) {
    let mut s = sample_session(name);
    let newest = from_pat(&mut s, 900, PAT, 1);
    from_pat(&mut s, 901, PAT, 12);
    from_pat(&mut s, 902, PAT, 30);
    from_pat(&mut s, 903, PAT_HOME, 2);
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    s.run("list.select", &json!({"key": newest.to_id()})).unwrap();
    (s, newest)
}

/// The days old of the messages from `email` in `folder`, youngest first.
fn ages(s: &mut Session, folder: &str, email: &str) -> Vec<i64> {
    s.run("folder.open", &json!({"account": "sample", "folder": folder})).unwrap();
    let mut out: Vec<i64> = s.list.iter().filter(|m| m.from.email == email).map(|m| (s.clock.now - m.date) / DAY).collect();
    out.sort_unstable();
    out
}

/// Sweep `key`'s sender by `choice`, with `key` selected in its folder (as the window has it).
fn sweep(s: &mut Session, key: &mailcraft_model::MessageKey, choice: &str) -> Result<Value, CmdError> {
    s.run("folder.open", &json!({"account": key.account, "folder": key.folder})).unwrap();
    s.run("list.select", &json!({"key": key.to_id()})).unwrap();
    s.run("mail.sweep", &json!({"choice": choice, "key": key.to_id()}))
}

#[test]
fn all_of_a_senders_mail_goes_and_undo_brings_it_back() {
    let (mut s, key) = with_pat("sweep-all");
    let of = s.sweep_of(Some(&key)).unwrap();
    assert_eq!(of.sender, PAT);
    // What each choice would remove, said before it's chosen.
    assert_eq!(s.sweep_counts(&of), [3, 3, 2, 2]);
    let r = sweep(&mut s, &key, "all").unwrap();
    assert_eq!(r["moved"], 3);
    assert!(ages(&mut s, "INBOX", PAT).is_empty());
    // Pat's other address is someone else to Sweep.
    assert_eq!(ages(&mut s, "INBOX", PAT_HOME), [2], "mail from another address was swept");
    assert_eq!(ages(&mut s, "Trash", PAT), [1, 12, 30]);
    // No rule, nothing each day: only what was there.
    assert!(s.rules.is_empty() && s.prefs.sweeps.is_empty());
    // One Undo puts all three back.
    s.run("edit.undo", &json!({})).unwrap();
    assert_eq!(ages(&mut s, "INBOX", PAT), [1, 12, 30]);
    assert!(ages(&mut s, "Trash", PAT).is_empty());
}

#[test]
fn all_and_later_adds_a_rule_for_what_they_send_next() {
    let (mut s, key) = with_pat("sweep-later");
    let r = sweep(&mut s, &key, "allAndLater").unwrap();
    assert_eq!(r["moved"], 3);
    assert_eq!(s.rules.len(), 1);
    let rule = &s.rules[0];
    assert_eq!(rule.account.as_deref(), Some("sample"));
    assert_eq!(rule.filter, Condition::all(vec![Condition::from(PAT)]));
    assert_eq!(rule.actions, [RuleAction::Delete]);
    // Pat writes again: to Deleted Items as it arrives.
    from_pat(&mut s, 950, PAT, 0);
    assert!(ages(&mut s, "INBOX", PAT).is_empty(), "the rule didn't take what came later");
    assert_eq!(ages(&mut s, "Trash", PAT), [0, 1, 12, 30]);
    // Undo brings back what was swept; the rule stays, for what comes later.
    s.run("edit.undo", &json!({})).unwrap();
    assert_eq!(ages(&mut s, "INBOX", PAT), [1, 12, 30]);
    assert_eq!(s.rules.len(), 1);
    // Swept so again: still one rule.
    let again = s.list.iter().find(|m| m.from.email == PAT).map(|m| m.key.clone()).unwrap();
    sweep(&mut s, &again, "allAndLater").unwrap();
    assert_eq!(s.rules.len(), 1, "a second rule for the same sender");
}

#[test]
fn keep_newest_leaves_only_their_newest() {
    let (mut s, key) = with_pat("sweep-newest");
    // Asked from an older one of theirs: the newest still stays.
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    let older = s.list.iter().find(|m| m.from.email == PAT && m.key != key).map(|m| m.key.clone()).unwrap();
    let r = sweep(&mut s, &older, "keepNewest").unwrap();
    assert_eq!(r["moved"], 2);
    assert_eq!(ages(&mut s, "INBOX", PAT), [1]);
    assert_eq!(ages(&mut s, "INBOX", PAT_HOME), [2]);
    s.run("edit.undo", &json!({})).unwrap();
    assert_eq!(ages(&mut s, "INBOX", PAT), [1, 12, 30]);
}

#[test]
fn older_than_ten_days_now_and_each_day_after() {
    let (mut s, key) = with_pat("sweep-days");
    let r = sweep(&mut s, &key, "olderThan").unwrap();
    assert_eq!(r["moved"], 2);
    assert_eq!(r["eachDay"], true);
    assert_eq!(ages(&mut s, "INBOX", PAT), [1]);
    assert_eq!(s.prefs.sweeps.len(), 1);
    // Chosen again: still one sweep each day.
    sweep(&mut s, &key, "olderThan").unwrap();
    assert_eq!(s.prefs.sweeps.len(), 1);
    // Old mail of theirs moved in later waits for the day's sweep.
    from_pat(&mut s, 960, PAT, 20);
    s.clock.now += DAY / 2;
    s.tick();
    assert_eq!(ages(&mut s, "INBOX", PAT), [1, 20], "swept before a day had passed");
    s.clock.now += DAY / 2;
    s.tick();
    assert_eq!(ages(&mut s, "INBOX", PAT), [2], "the day's sweep didn't run");
    // The one left is swept once it's old enough; no Undo for what ran by itself.
    let undos = s.undo.len();
    s.clock.now += (SWEEP_DAYS - 1) * DAY;
    s.tick();
    assert!(ages(&mut s, "INBOX", PAT).is_empty());
    assert_eq!(ages(&mut s, "INBOX", PAT_HOME), [SWEEP_DAYS + 2], "another address was swept");
    assert_eq!(s.undo.len(), undos, "a sweep that ran by itself is put on Undo");
    // Stopped: old mail stays.
    let r = s.run("mail.stopSweep", &json!({"account": "sample", "folder": "INBOX", "sender": PAT})).unwrap();
    assert_eq!(r["stopped"], true);
    assert!(s.prefs.sweeps.is_empty());
    from_pat(&mut s, 961, PAT, 40);
    s.clock.now += 2 * DAY;
    s.tick();
    assert_eq!(ages(&mut s, "INBOX", PAT), [40 + 2]);
    assert!(s.run("mail.stopSweep", &json!({"account": "sample", "folder": "INBOX", "sender": PAT})).is_err());
}

#[test]
fn deleted_items_and_a_bad_choice_are_refused() {
    let (mut s, key) = with_pat("sweep-refused");
    assert!(matches!(sweep(&mut s, &key, "most"), Err(CmdError::Params(_))));
    assert!(matches!(s.run("mail.sweep", &json!({"key": key.to_id()})), Err(CmdError::Params(_))));
    sweep(&mut s, &key, "all").unwrap();
    s.run("folder.open", &json!({"account": "sample", "folder": "Trash"})).unwrap();
    let there = s.list.iter().find(|m| m.from.email == PAT).map(|m| m.key.clone()).unwrap();
    assert!(matches!(sweep(&mut s, &there, "all"), Err(CmdError::Disabled(_))));
    assert_eq!(SweepChoice::parse("keepNewest"), Some(SweepChoice::KeepNewest));
}
