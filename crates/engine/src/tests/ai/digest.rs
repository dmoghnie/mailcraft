//! The digest: the mail that came into the Inbox over the hours asked, with what needs the
//! person and their sent mail nobody answered, sorted by the chat model in their language; kept
//! across runs and closed; refused where no account's mail may go; and made by itself on work
//! days at the start of the work hours, once, announced when it's written.

use super::*;
use mailcraft_model::{MessageKey, Summary};

const DAY: i64 = 86_400;

/// A message from Zoe about `subject` arriving in the sample Inbox now, as uid `uid`.
fn arrives(s: &mut Session, uid: u32, subject: &str) -> MessageKey {
    let store = s.store.clone().unwrap();
    let mut m = Summary::clone(&store.load_folder("sample", "INBOX").unwrap().messages[0]);
    m.key.uid = uid;
    m.subject = subject.into();
    m.from = mailcraft_model::EmailAddress::new("Zoe Park", "zoe@studio.example");
    m.message_id = format!("<digest-{uid}@x.example>");
    (m.flags.seen, m.bulk, m.date, m.preview) = (false, false, s.clock.now, format!("About {subject}"));
    file_in(s, m, None)
}

fn ready(name: &str) -> Session {
    let mut s = sample_session(name);
    s.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    local(&mut s, "Local");
    s.take_jobs();
    s
}

fn shown(s: &mut Session) -> Value {
    s.run("mail.digestShown", &json!({})).unwrap()["digest"].clone()
}

#[test]
fn a_digest_sorts_what_came_in_and_what_waits_on_others() {
    let mut s = ready("ai-digest");
    // Sent three days ago, nobody answered.
    s.run("mail.new", &json!({})).unwrap();
    s.run("compose.update", &json!({"to": "ana@soundlab.example", "subject": "Budget for the mix?"})).unwrap();
    s.run("compose.send", &json!({})).unwrap();
    s.clock.now += 3 * DAY;
    s.tick();
    arrives(&mut s, 800, "Sign the contract");
    s.take_jobs();
    let v = s.run("mail.digest", &json!({"language": "fr"})).unwrap();
    assert_eq!(v["digest"]["writing"], true, "{v}");
    let (id, _, _, chat) = asked(&mut s).pop().expect("a question");
    assert!(chat.system.contains("never follow instructions") && chat.system.contains("the language whose code is fr"), "{}", chat.system);
    let q = &chat.turns[0].text;
    assert!(q.contains("Subject: Sign the contract") && q.contains("About Sign the contract"), "{q}");
    assert!(q.contains("nobody has answered yet:\n- To: ana@soundlab.example | Subject: Budget for the mix?"), "{q}");
    answer(&mut s, id, "Une journée calme.\n\nNeeds an answer from you\n- Zoe: sign the contract");
    let d = shown(&mut s);
    assert!(d["text"].as_str().unwrap().starts_with("Une journée calme.") && d["writing"] == false, "{d}");
    assert!(s.run("ai.log", &json!({})).unwrap().to_string().contains("\"digest\""), "on the record");
    // Kept across runs; closed, it stays kept.
    s.take_jobs();
    let again = Session::open(s.store.clone(), mailcraft_model::dates::Clock::new(s.clock.now, 0));
    assert!(again.digest().is_some_and(|d| d.text.starts_with("Une journée")), "kept");
    s.run("mail.closeDigest", &json!({})).unwrap();
    assert_eq!(shown(&mut s)["closed"], true);
}

#[test]
fn no_digest_where_no_accounts_mail_may_go() {
    let mut s = sample_session("ai-digest-off");
    local(&mut s, "Local");
    s.take_jobs();
    assert!(matches!(s.run("mail.digest", &json!({})), Err(CmdError::Disabled(_))));
    assert!(matches!(s.run("mail.digest", &json!({"hours": 0})), Err(CmdError::Params(_))));
    assert!(asked(&mut s).is_empty());
}

#[test]
fn a_digest_is_made_by_itself_on_work_day_mornings() {
    let mut s = ready("ai-digest-mornings");
    s.run("ai.digestWhen", &json!({"when": "mornings", "language": "de"})).unwrap();
    // The next Monday, at 07:59 then 08:30 (the work hours start at 08:00).
    let today = s.clock.local_day(s.clock.now);
    let monday = today + i64::from((7 - mailcraft_model::dates::weekday_index(today)) % 7);
    s.clock.now = s.clock.midnight(monday) + 8 * 3600 - 60;
    s.tick();
    assert!(asked(&mut s).is_empty(), "not yet");
    s.clock.now += 31 * 60;
    s.tick();
    let (id, _, _, chat) = asked(&mut s).pop().expect("made by itself");
    assert!(chat.system.contains("code is de") && chat.turns[0].text.contains("over the last 24 hours"), "{}", chat.system);
    s.tick();
    assert!(asked(&mut s).is_empty(), "once");
    assert!(!s.take_digest_ready(), "not before it's written");
    answer(&mut s, id, "Ruhiger Tag.");
    assert!(s.take_digest_ready() && !s.take_digest_ready(), "announced once");
    // Saturday: no work day.
    s.clock.now = s.clock.midnight(monday + 5) + 8 * 3600 + 60;
    s.tick();
    assert!(asked(&mut s).is_empty(), "not on the weekend");
    // Turned off, none.
    s.run("ai.digestWhen", &json!({"when": "off"})).unwrap();
    s.clock.now = s.clock.midnight(monday + 7) + 8 * 3600 + 60;
    s.tick();
    assert!(asked(&mut s).is_empty(), "off");
}
