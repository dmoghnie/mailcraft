//! Needs me: mail arriving in the Inbox read by the model for quick decisions, once the person
//! turns it on and where the account's mail may go to it; bulk mail never; four questions at a
//! time, the rest waiting their turn; marked as the model says, kept across runs, and no longer
//! once answered or taken off.

use super::*;
use mailcraft_model::{MessageKey, Summary};

/// A message from Zoe arriving in the sample Inbox, unread, as uid `uid`.
fn arrives(s: &mut Session, uid: u32, bulk: bool) -> MessageKey {
    let store = s.store.clone().unwrap();
    let mut m = Summary::clone(&store.load_folder("sample", "INBOX").unwrap().messages[0]);
    m.key.uid = uid;
    m.subject = format!("Arrived {uid}");
    m.from = mailcraft_model::EmailAddress::new("Zoe Park", "zoe@studio.example");
    m.message_id = format!("<arrived-{uid}@x.example>");
    (m.flags.seen, m.flags.answered, m.bulk, m.date) = (false, false, bulk, NOW);
    file_in(s, m, None)
}

fn summary(s: &mut Session, key: &MessageKey) -> Summary {
    Summary::clone(&s.shared_of(key).expect("there"))
}

/// The questions asked about arriving mail: each one's id and the subject it held.
fn questions(s: &mut Session) -> Vec<(u64, String)> {
    asked(s)
        .into_iter()
        .map(|(id, _, _, chat)| {
            assert!(chat.system.contains("Answer with one word: YES or NO"), "{}", chat.system);
            let q = &chat.turns[0].text;
            (id, q.lines().find_map(|l| l.strip_prefix("Subject: ")).unwrap_or_default().to_string())
        })
        .collect()
}

fn ready(name: &str) -> Session {
    let mut s = sample_session(name);
    s.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    local(&mut s, "Local");
    s.take_jobs();
    s
}

#[test]
fn arriving_mail_is_read_and_marked_when_it_needs_me() {
    let mut s = ready("ai-needs-me");
    arrives(&mut s, 700, false);
    assert!(asked(&mut s).is_empty(), "off until turned on");
    assert_eq!(s.run("ai.needsMe", &json!({"on": true})).unwrap()["on"], true);
    let asks = arrives(&mut s, 701, false);
    arrives(&mut s, 702, true);
    let tells = arrives(&mut s, 703, false);
    let q = questions(&mut s);
    assert_eq!(q.iter().map(|(_, sub)| sub.as_str()).collect::<Vec<_>>(), ["Arrived 701", "Arrived 703"], "bulk mail isn't read");
    answer(&mut s, q[0].0, "YES");
    answer(&mut s, q[1].0, "No.");
    let (m1, m3) = (summary(&mut s, &asks), summary(&mut s, &tells));
    assert!(s.needs_me(&m1) && !s.needs_me(&m3));
    assert_eq!(s.run("mail.needsMe", &json!({"key": asks.to_id()})).unwrap()["needsMe"], true);
    assert!(s.run("ai.log", &json!({})).unwrap().to_string().contains("needs me"), "on the record");
    // Kept across runs.
    s.take_jobs();
    let mut again = Session::open(s.store.clone(), mailcraft_model::dates::Clock::new(s.clock.now, 0));
    let kept = summary(&mut again, &asks);
    assert!(again.needs_me(&kept), "kept");
    // Answered, it no longer needs me; taken off, neither.
    let mut answered = m1.clone();
    answered.flags.answered = true;
    assert!(!s.needs_me(&answered));
    s.run("mail.notNeeded", &json!({"key": asks.to_id()})).unwrap();
    assert!(!s.needs_me(&m1));
}

#[test]
fn four_are_asked_at_a_time_and_the_rest_wait_their_turn() {
    let mut s = ready("ai-needs-me-queue");
    s.run("ai.needsMe", &json!({"on": true})).unwrap();
    for uid in 710..716 {
        arrives(&mut s, uid, false);
    }
    let first = questions(&mut s);
    assert_eq!(first.len(), 4);
    // An answer that's neither yes nor no marks nothing, and lets the next go.
    answer(&mut s, first[0].0, "Perhaps");
    assert_eq!(questions(&mut s).iter().map(|(_, sub)| sub.clone()).collect::<Vec<_>>(), ["Arrived 714"]);
    fail(&mut s, first[1].0, "the model isn't there");
    assert_eq!(questions(&mut s).iter().map(|(_, sub)| sub.clone()).collect::<Vec<_>>(), ["Arrived 715"]);
}

#[test]
fn nothing_is_read_where_the_accounts_mail_may_not_go() {
    let mut s = ready("ai-needs-me-off");
    s.run("ai.needsMe", &json!({"on": true})).unwrap();
    s.run("ai.allow", &json!({"account": "sample", "reach": "off"})).unwrap();
    arrives(&mut s, 720, false);
    assert!(asked(&mut s).is_empty());
}
