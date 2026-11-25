//! Automations: a job said once, run by MailCraft's AI each morning or as mail a search matches
//! arrives, each run in a chat of its own; the changes it calls for proposed for the person, and
//! the person told as each run ends. The test answers the questions as the model would.

use super::*;
use crate::{AutomationReport, ReportSaid};
use mailcraft_model::Summary;
use mailcraft_model::dates::Clock;

/// `minutes` after midnight on the next Monday.
fn monday_at(s: &Session, minutes: i64) -> i64 {
    let today = s.clock.local_day(s.clock.now);
    let monday = today + i64::from((7 - mailcraft_model::dates::weekday_index(today)) % 7);
    s.clock.midnight(monday) + minutes * 60
}

/// A message arriving in the sample Inbox, unread, as uid `uid`, with `subject`.
fn arrives(s: &mut Session, uid: u32, subject: &str) -> MessageKey {
    let store = s.store.clone().unwrap();
    let mut m = Summary::clone(&store.load_folder("sample", "INBOX").unwrap().messages[0]);
    m.key.uid = uid;
    m.subject = subject.into();
    m.from = mailcraft_model::EmailAddress::new("Print Shop", "billing@print.example");
    m.message_id = format!("<arrived-{uid}@print.example>");
    (m.flags.seen, m.flags.answered, m.bulk, m.date) = (false, false, false, s.clock.now);
    file_in(s, m, None)
}

/// The text the model was sent last, what was pointed at with it.
fn said(chat: &AiChat) -> String {
    chat.turns.last().map(|t| t.text.clone()).unwrap_or_default()
}

fn last_run(s: &mut Session, name: &str) -> Value {
    let v = s.run("automation.list", &json!({})).unwrap();
    v["automations"].as_array().unwrap().iter().find(|a| a["name"] == name).expect("kept")["last"].clone()
}

fn told(s: &mut Session) -> Vec<AutomationReport> {
    s.tick();
    s.take_automation_reports()
}

/// "Every morning at 8, a digest of what needs me": run by itself at 8, once, in a chat of its
/// own named after it, the model finding with its tools; the person told what it answered, once.
/// Not again for that morning after MailCraft starts again; the next morning again; not when
/// more than four hours late, nor on a day it isn't for, nor turned off.
#[test]
fn every_morning_at_8_a_digest_of_what_needs_me() {
    let mut s = ready("automation-morning");
    let monday = monday_at(&s, 7 * 60);
    s.clock.now = monday;
    let v = json!({"name": "Morning digest", "job": "A digest of what needs me", "when": {"kind": "daily", "at": "08:00", "days": [0, 1, 2, 3, 4]}});
    s.run("automation.save", &v).unwrap();
    s.tick();
    assert!(questions(&mut s).is_empty(), "not before its time");
    s.clock.now = monday + 62 * 60;
    s.tick();
    let (q, asked) = question(&mut s);
    let text = said(&asked);
    assert!(text.contains("A digest of what needs me") && text.contains("run by itself at its time"), "{text}");
    assert!(text.contains("don't ask them anything") && text.contains("propose every change it needs at once"), "{text}");
    s.tick();
    assert!(questions(&mut s).is_empty(), "once for its time");
    calls(&mut s, q, "", &[("find_messages", json!({"query": "is:unread", "folder": "inbox"}))]);
    assert!(told(&mut s).is_empty(), "nothing told while it goes on");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "Two need you: Zoe's storyboard frames, and the screening room.\nThe rest can wait.", &[]);
    let r = told(&mut s);
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0].said, ReportSaid::Answered("Two need you: Zoe's storyboard frames, and the screening room.".into()));
    let c = r[0].chat.clone().expect("its chat");
    let v = chat(&mut s, &c);
    assert_eq!(v["chat"]["title"], "Morning digest");
    assert_eq!(v["chat"]["runs"][0]["tool"], "find_messages", "what it did is on its chat");
    assert!(told(&mut s).is_empty(), "told once");
    assert_eq!(last_run(&mut s, "Morning digest")["chat"], json!(c));

    // MailCraft started again later that morning (its model set up again: the window keeps
    // the settings): not again.
    s.take_jobs();
    let mut s = Session::open(s.store.clone(), Clock::new(monday + 3 * 3600, 0));
    s.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    local(&mut s, "Local");
    assert!(told(&mut s).is_empty() && questions(&mut s).is_empty(), "not again for that morning");
    // The next morning, again, in another chat.
    s.clock.now = monday + 86_400 + 61 * 60;
    s.tick();
    assert_eq!(questions(&mut s).len(), 1, "the next morning");
    assert_ne!(last_run(&mut s, "Morning digest")["chat"], json!(c));
    // More than four hours late: not.
    s.clock.now = monday + 2 * 86_400 + 6 * 3600;
    s.tick();
    assert!(questions(&mut s).is_empty(), "too late");
    // Saturday: not one of its days.
    s.clock.now = monday + 5 * 86_400 + 61 * 60;
    s.tick();
    assert!(questions(&mut s).is_empty(), "not on Saturday");
    // Turned off: not; turned on after its time: not for that morning.
    s.run("automation.turn", &json!({"name": "Morning digest", "on": false})).unwrap();
    s.clock.now = monday + 7 * 86_400 + 61 * 60;
    s.tick();
    assert!(questions(&mut s).is_empty(), "off");
    s.run("automation.turn", &json!({"name": "Morning digest", "on": true})).unwrap();
    s.tick();
    assert!(questions(&mut s).is_empty(), "turned on after its time");
}

/// "When an invoice arrives, file it and add a task": run as mail its search matches arrives,
/// given that mail and not the rest; the changes it calls for wait for the person, who's told
/// so, and are made once approved.
#[test]
fn when_an_invoice_arrives_it_is_filed_and_a_task_added_once_approved() {
    let mut s = ready("automation-invoice");
    let v =
        json!({"name": "Invoices", "job": "File it in Archive, and add a task to pay it", "when": {"kind": "arrives", "query": "subject:invoice"}});
    s.run("automation.save", &v).unwrap();
    arrives(&mut s, 801, "Lunch on Friday?");
    assert!(questions(&mut s).is_empty(), "not for mail its search doesn't match");
    let invoice = arrives(&mut s, 802, "Invoice 42 for November");
    let (q, asked) = question(&mut s);
    let text = said(&asked);
    assert!(text.contains("File it in Archive") && text.contains("as mail arrived: the mail pointed at"), "{text}");
    assert!(text.contains("Invoice 42 for November") && !text.contains("Lunch on Friday"), "given what arrived: {text}");
    let id = name_of(&invoice.to_id());
    calls(&mut s, q, "", &[("move_messages", json!({"ids": [id], "folder": "archive"})), ("add_task", json!({"title": "Pay invoice 42"}))]);
    let r = told(&mut s);
    assert_eq!(r.iter().map(|r| (&r.name, &r.said)).collect::<Vec<_>>(), [(&"Invoices".to_string(), &ReportSaid::Proposes(2))]);
    let in_inbox = |s: &mut Session| s.folder_state("sample", "INBOX").messages.iter().any(|m| m.key == invoice);
    assert!(in_inbox(&mut s) && s.tasks("Pay invoice").is_empty(), "nothing changed before it's approved");
    let c = r[0].chat.clone().unwrap();
    s.run("assistant.decide", &json!({"chat": c, "approve": true})).unwrap();
    assert!(!in_inbox(&mut s), "filed");
    assert!(s.folder_state("sample", "Archive").messages.iter().any(|m| m.subject == "Invoice 42 for November"), "in Archive");
    assert_eq!(s.tasks("Pay invoice").len(), 1, "the task added");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "Filed the invoice and added a task to pay it.", &[]);
    assert!(told(&mut s).is_empty(), "told once, as it waited for the person");
    // Another the same day: another run, in a chat of its own.
    arrives(&mut s, 803, "Invoice 43");
    question(&mut s);
    assert_ne!(last_run(&mut s, "Invoices")["chat"], json!(c));
}

/// A run that can't begin (the account's mail may not go to the model) is told of, why it
/// couldn't, and leaves no chat behind.
#[test]
fn a_run_that_cant_begin_is_told_and_leaves_no_chat() {
    let mut s = ready("automation-not-allowed");
    s.run("automation.save", &json!({"name": "Everything", "job": "Sort it", "when": {"kind": "arrives"}})).unwrap();
    s.run("ai.allow", &json!({"account": "sample", "reach": "off"})).unwrap();
    let chats = |s: &mut Session| s.run("assistant.chats", &json!({})).unwrap()["chats"].as_array().unwrap().len();
    let before = chats(&mut s);
    arrives(&mut s, 810, "Anything at all");
    assert!(questions(&mut s).is_empty());
    let r = told(&mut s);
    assert!(matches!(&r[..], [AutomationReport { said: ReportSaid::Failed(why), chat: None, .. }] if why.contains("may not go to Local")), "{r:?}");
    assert_eq!(chats(&mut s), before, "no chat left");
}

/// Automations are kept, listed, renamed, turned off and deleted, and run at once when asked;
/// one that can't be is refused, and says why.
#[test]
fn automations_are_kept_changed_refused_and_run_when_asked() {
    let mut s = ready("automation-kept");
    let save = |s: &mut Session, v: Value| s.run("automation.save", &v).map_err(|e| e.to_string());
    save(&mut s, json!({"name": "Weekly", "job": "What's late", "when": {"kind": "daily", "at": "17:30", "days": [4]}})).unwrap();
    let mut again = Session::open(s.store.clone(), Clock::new(s.clock.now, 0));
    again.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    local(&mut again, "Local");
    let v = again.run("automation.list", &json!({})).unwrap();
    assert_eq!(v["automations"][0]["when"], json!({"kind": "daily", "at": 17 * 60 + 30, "days": 16}), "{v}");
    for (v, why) in [
        (json!({"name": "", "job": "x"}), "1 to 60 characters"),
        (json!({"name": "Weekly", "job": "x"}), "kept already"),
        (json!({"name": "N", "job": " "}), "say what it does"),
        (json!({"name": "N", "job": "x", "when": {"kind": "daily", "at": "25:00"}}), "a time"),
        (json!({"name": "N", "job": "x", "when": {"kind": "daily", "days": []}}), "one day of the week"),
        (json!({"name": "N", "job": "x", "when": {"kind": "daily", "days": [7]}}), "from 0 (Monday) to 6"),
        (json!({"name": "N", "job": "x", "when": {"kind": "hourly"}}), "`when` is"),
        (json!({"name": "N", "job": "x", "when": {"kind": "arrives", "account": "nobody"}}), "no account nobody"),
    ] {
        let e = save(&mut again, v.clone()).unwrap_err();
        assert!(e.contains(why), "{v}: {e}");
    }
    save(&mut again, json!({"name": "Fridays", "was": "Weekly", "job": "What's late", "when": {"kind": "daily", "at": 1050}})).unwrap();
    let e = save(&mut again, json!({"name": "Fridays", "was": "Weekly", "job": "x"})).unwrap_err();
    assert!(e.contains("no automation named Weekly"), "{e}");
    again.run("automation.turn", &json!({"name": "Fridays", "on": false})).unwrap();
    let v = again.run("automation.list", &json!({})).unwrap();
    assert_eq!((&v["automations"][0]["name"], &v["automations"][0]["off"]), (&json!("Fridays"), &json!(true)), "{v}");
    // Run now: at once, off or not, in a chat of its own.
    again.take_jobs();
    let c = again.run("automation.run", &json!({"name": "Fridays"})).unwrap()["chat"].as_str().unwrap().to_string();
    let (_, asked) = question(&mut again);
    assert!(said(&asked).contains("run now as the person asked"), "{}", said(&asked));
    assert_eq!(chat(&mut again, &c)["chat"]["title"], "Fridays");
    assert!(again.run("automation.run", &json!({"name": "Weekly"})).is_err(), "renamed");
    again.run("automation.delete", &json!({"name": "Fridays"})).unwrap();
    assert_eq!(again.run("automation.list", &json!({})).unwrap()["automations"], json!([]));
    assert!(again.run("automation.delete", &json!({"name": "Fridays"})).is_err());
}
