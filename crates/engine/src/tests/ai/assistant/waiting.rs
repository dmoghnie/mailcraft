//! Reads that wait for their message to download: the model is asked again only once every
//! message it waits for came, or couldn't.

use super::*;
use mailcraft_model::{FolderRole, Summary};

/// An account online (the sample's mail is all here already), its mail allowed to the model, with
/// a message of each of `subjects` in its Inbox, none of them downloaded.
fn online(s: &mut Session, subjects: &[&str]) -> Vec<MessageKey> {
    let r = s.run("account.add", &json!({"email": "riley@online.example", "password": "pw"})).unwrap();
    let id = r["account"].as_str().unwrap().to_string();
    s.run("ai.allow", &json!({"account": id, "reach": "local"})).unwrap();
    let inbox =
        mailcraft_model::Folder { account: id.clone(), path: "INBOX".into(), role: FolderRole::Inbox, selectable: true, ..Default::default() };
    s.apply(Event::Folders { account: id.clone(), folders: vec![inbox] });
    let keys: Vec<MessageKey> =
        (1..=9).take(subjects.len()).map(|uid| MessageKey { account: id.clone(), folder: "INBOX".into(), validity: 9, uid }).collect();
    let messages = keys
        .iter()
        .zip(subjects)
        .map(|(k, subject)| Summary { key: k.clone(), subject: (*subject).into(), date: NOW - 60, ..Default::default() }.into());
    let st = mailcraft_store::FolderState { validity: 9, uid_next: 10, highest_modseq: 0, messages: messages.collect(), window: 0 };
    s.store.as_ref().unwrap().save_folder(&id, "INBOX", &st).unwrap();
    s.apply(Event::FolderChanged { account: id, folder: "INBOX".into() });
    s.take_jobs();
    keys
}

/// Two messages read in one step, neither downloaded: both are fetched, and the model waits
/// until the one came and the other couldn't, then hears of both.
#[test]
fn reads_waiting_for_their_messages_go_on_once_every_one_came() {
    let mut s = ready("assistant-waiting");
    let keys = online(&mut s, &["Quarterly figures", "Venue contract"]);
    send(&mut s, None, "Read both");
    let (q, _) = question(&mut s);
    calls(&mut s, q, "", &[("find_messages", json!({"query": "Quarterly"})), ("find_messages", json!({"query": "Venue"}))]);
    let (q, asked) = question(&mut s);
    let found = &asked.turns.last().unwrap().results;
    let (a, b) = (bracketed(&found[0].content), bracketed(&found[1].content));
    calls(&mut s, q, "", &[("read_message", json!({"id": a})), ("read_message", json!({"id": b}))]);
    let jobs = s.take_jobs();
    let fetched: Vec<&MessageKey> = jobs.iter().filter_map(|j| if let Job::FetchBody { key } = j { Some(key) } else { None }).collect();
    assert_eq!(fetched, [&keys[0], &keys[1]]);
    assert!(!jobs.iter().any(|j| matches!(j, Job::Ai { .. })), "not asked while they're on their way");
    // As the worker does: kept, then said.
    let raw = b"From: Ana <ana@online.example>\r\nSubject: Quarterly figures\r\n\r\nRevenue is up 4%.\r\n";
    s.store.as_ref().unwrap().write_body(&keys[0], raw).unwrap();
    s.apply(Event::Body { key: keys[0].clone(), raw: raw.to_vec() });
    assert!(questions(&mut s).is_empty(), "the other still on its way");
    s.apply(Event::BodyFailed { key: keys[1].clone(), error: "the server closed the connection".into() });
    let (_, asked) = question(&mut s);
    let read = &asked.turns.last().unwrap().results;
    assert!(!read[0].error && read[0].content.contains("Revenue is up 4%."), "{:?}", read[0].content);
    assert!(read[1].error && read[1].content.contains("couldn't be downloaded: the server closed the connection"), "{:?}", read[1].content);
}
