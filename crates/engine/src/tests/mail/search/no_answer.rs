//! Engine tests: sent mail nobody answered, brought back under the Inbox's No answer yet.

use mailcraft_model::{EmailAddress, Flags, MessageKey, Summary};

use super::*;

const DAY: i64 = 86_400;

/// Send a message from the sample to `to` (and `cc`) about `subject`: its copy in Sent.
fn send(s: &mut Session, to: &str, cc: &str, subject: &str) -> Summary {
    s.run("mail.new", &json!({})).unwrap();
    s.run("compose.update", &json!({"to": to, "cc": cc, "subject": subject})).unwrap();
    s.run("compose.send", &json!({})).unwrap();
    s.run("folder.open", &json!({"account": "sample", "folder": "Sent"})).unwrap();
    s.list.iter().find(|m| m.subject == subject).map(|m| Summary::clone(m)).unwrap()
}

/// An answer to `mine` from `from`, come into `folder` as uid `uid`.
fn answer(s: &mut Session, mine: &Summary, from: (&str, &str), folder: &str, uid: u32) {
    let d = mailcraft_mime::Draft {
        to: vec![EmailAddress::new("Riley", "riley@studio.example")],
        subject: format!("Re: {}", mine.subject),
        body: "Got it.".into(),
        in_reply_to: mine.message_id.clone(),
        references: vec![mine.message_id.clone()],
        ..Default::default()
    };
    let raw = mailcraft_mime::build(&d, &EmailAddress::new(from.0, from.1), s.clock.now, &format!("<answer-{uid}@x.example>")).unwrap();
    let key = MessageKey { uid, ..next_key(s, "sample", folder) };
    file_in(s, mailcraft_mime::summary_from_full(&raw, key, Flags::default(), 0, s.clock.now), None);
}

/// The sample's own order to the print shop, sent two days before its clock: the shop's answer
/// doesn't say it answers it, so it comes back too once it's three days old. Left out here.
const SAMPLE_ORDER: &str = "Festival poster order";

/// What `mail.noAnswer` lists, by subject.
fn listed(s: &mut Session) -> Vec<String> {
    let v = s.run("mail.noAnswer", &json!({})).unwrap();
    let mut out: Vec<String> = v["messages"].as_array().unwrap().iter().map(|m| m["subject"].as_str().unwrap().to_string()).collect();
    out.retain(|m| m != SAMPLE_ORDER);
    out.sort();
    out
}

/// What the Inbox's No answer yet shows, after enough ticks for its scan to end.
fn shown(s: &mut Session) -> Vec<String> {
    for _ in 0..200 {
        s.tick();
    }
    s.no_answer_shown().iter().map(|m| m.subject.clone()).filter(|m| m != SAMPLE_ORDER).collect()
}

#[test]
fn sent_mail_nobody_answered_comes_back_after_three_days() {
    let mut s = sample_session("no-answer");
    send(&mut s, "ana@soundlab.example", "", "Budget for the mix?");
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    assert!(shown(&mut s).is_empty(), "just sent");
    s.clock.now += 2 * DAY;
    assert!(listed(&mut s).is_empty(), "two days on: not yet");
    s.clock.now += DAY;
    // The Inbox shows it, worked out a few steps a tick (the last look was a day ago).
    assert_eq!(shown(&mut s), ["Budget for the mix?"]);
    // Not over another folder, a search or the Focused Inbox's Other.
    s.run("folder.open", &json!({"account": "sample", "folder": "Sent"})).unwrap();
    assert!(s.no_answer_shown().is_empty());
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    s.run("search.run", &json!({"query": "budget"})).unwrap();
    assert!(s.no_answer_shown().is_empty());
    s.run("search.clear", &json!({})).unwrap();
    s.run("view.focused", &json!({"on": true})).unwrap();
    s.run("list.focusTab", &json!({"value": "other"})).unwrap();
    assert!(s.no_answer_shown().is_empty());
    s.run("list.focusTab", &json!({"value": "focused"})).unwrap();
    assert!(s.no_answer_shown().iter().any(|m| m.subject == "Budget for the mix?"));
    // The search folder lists the same, for an agent too.
    assert_eq!(listed(&mut s), ["Budget for the mix?"]);
    s.run("folder.noAnswer", &json!({})).unwrap();
    let found: Vec<&str> = s.list.iter().map(|m| m.subject.as_str()).filter(|m| *m != SAMPLE_ORDER).collect();
    assert_eq!(found, ["Budget for the mix?"]);
    assert!(crate::mcp::READS.contains(&"mail.noAnswer") && crate::mcp::READS.contains(&"folder.noAnswer"));
    // After the days the user chose.
    s.run("options.noAnswerDays", &json!({"days": 5})).unwrap();
    assert!(listed(&mut s).is_empty(), "five days asked for");
    for bad in [json!(0), json!(15), json!("x")] {
        assert!(matches!(s.run("options.noAnswerDays", &json!({"days": bad})), Err(CmdError::Params(_))), "{bad}");
    }
    assert_eq!(s.prefs.no_answer_days, 5);
}

#[test]
fn an_answer_in_another_folder_or_from_someone_else_counts() {
    let mut s = sample_session("no-answer-answered");
    let budget = send(&mut s, "ana@soundlab.example", "", "Budget for the mix?");
    let dates = send(&mut s, "zoe@studio.example", "", "Dates for the screening?");
    s.clock.now += 3 * DAY;
    assert_eq!(listed(&mut s), ["Budget for the mix?", "Dates for the screening?"]);
    // Ana's answer, filed in Archive.
    answer(&mut s, &budget, ("Ana Lima", "ana@soundlab.example"), "Archive", 9300);
    assert_eq!(listed(&mut s), ["Dates for the screening?"]);
    // Sam answers what was asked of Zoe.
    answer(&mut s, &dates, ("Sam Okafor", "sam@studio.example"), "INBOX", 9301);
    assert!(listed(&mut s).is_empty());
}

#[test]
fn a_dismissed_message_stays_away() {
    let mut s = sample_session("no-answer-dismissed");
    let mine = send(&mut s, "ana@soundlab.example", "", "Budget for the mix?");
    s.clock.now += 3 * DAY;
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    assert_eq!(shown(&mut s), ["Budget for the mix?"]);
    s.run("mail.noAnswerDismiss", &json!({"key": mine.key.to_id()})).unwrap();
    assert!(s.no_answer_shown().iter().all(|m| m.key != mine.key), "gone at once");
    assert!(listed(&mut s).is_empty(), "and for good");
    s.clock.now += 2 * DAY;
    assert!(shown(&mut s).is_empty());
    // Nor does is:awaiting find it.
    s.run("search.run", &json!({"query": "is:awaiting", "scope": "all"})).unwrap();
    assert!(s.list.iter().all(|m| m.key != mine.key));
    // Kept with the settings.
    let back: crate::session::Prefs = serde_json::from_value(serde_json::to_value(&s.prefs).unwrap()).unwrap();
    assert_eq!(back.no_answer_dismissed, std::slice::from_ref(&mine.message_id));
    // Only mail the user sent is dismissed or followed up.
    s.run("search.clear", &json!({})).unwrap();
    let theirs = s.list.iter().find(|m| m.from.email == "zoe@studio.example").map(|m| m.key.to_id()).unwrap();
    for id in ["mail.noAnswerDismiss", "mail.noAnswerFollowUp"] {
        assert!(matches!(s.run(id, &json!({"key": theirs})), Err(CmdError::Disabled(_))), "{id}");
    }
}

#[test]
fn turned_off_nothing_comes_back() {
    let mut s = sample_session("no-answer-off");
    send(&mut s, "ana@soundlab.example", "", "Budget for the mix?");
    s.clock.now += 3 * DAY;
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    assert_eq!(shown(&mut s).len(), 1);
    assert_eq!(s.run("options.noAnswer", &json!({"on": false})).unwrap()["on"], false);
    assert!(shown(&mut s).is_empty());
    assert!(matches!(s.run("mail.noAnswer", &json!({})), Err(CmdError::Disabled(_))));
    // Kept with the settings, and on again it's back.
    let back: crate::session::Prefs = serde_json::from_value(serde_json::to_value(&s.prefs).unwrap()).unwrap();
    assert!(!back.no_answer);
    s.run("options.noAnswer", &json!({"on": true})).unwrap();
    assert_eq!(shown(&mut s).len(), 1);
}

#[test]
fn mail_to_a_list_or_to_yourself_isnt_waited_on() {
    let mut s = sample_session("no-answer-lists");
    // A mailing list's message, to its members' address.
    let store = s.store.clone().unwrap();
    let newsletter = store.load_folder("sample", "INBOX").unwrap().messages.iter().find(|m| m.bulk).map(|m| Summary::clone(m)).unwrap();
    let mut post = newsletter.clone();
    post.key = MessageKey { uid: 9400, ..next_key(&s, "sample", "INBOX") };
    post.message_id = "<post-1@gallery.example>".into();
    post.to = vec![EmailAddress::new("Members", "members@gallery.example")];
    file_in(&mut s, post, None);
    send(&mut s, "riley@studio.example", "", "Note to self");
    send(&mut s, "members@gallery.example", "", "Question for the members");
    // An answer to the newsletter, as Reply writes it.
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    s.run("list.open", &json!({"key": newsletter.key.to_id()})).unwrap();
    s.run("mail.reply", &json!({})).unwrap();
    s.run("compose.update", &json!({"subject": "Re: about the show"})).unwrap();
    s.run("compose.send", &json!({})).unwrap();
    // And one to a person, which is waited on.
    send(&mut s, "ana@soundlab.example", "", "Budget for the mix?");
    s.clock.now += 3 * DAY;
    assert_eq!(listed(&mut s), ["Budget for the mix?"]);
}

#[test]
fn follow_up_writes_to_everyone_it_went_to() {
    let mut s = sample_session("no-answer-follow-up");
    let mine = send(&mut s, "ana@soundlab.example", "zoe@studio.example", "Mix notes?");
    s.clock.now += 3 * DAY;
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    assert_eq!(shown(&mut s), ["Mix notes?"]);
    s.run("mail.noAnswerFollowUp", &json!({"key": mine.key.to_id()})).unwrap();
    let c = s.compose.clone().unwrap();
    assert!(c.to.contains("ana@soundlab.example"), "{}", c.to);
    assert!(format!("{} {}", c.to, c.cc).contains("zoe@studio.example"), "{} | {}", c.to, c.cc);
    assert!(!format!("{} {}", c.to, c.cc).contains("riley@"), "not to the user");
    assert_eq!(c.subject, "Re: Mix notes?");
    assert_eq!(c.in_reply_to, mine.message_id);
}
