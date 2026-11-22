//! Engine tests: leaving a mailing list from one of its messages (Unsubscribe).

use super::*;
use crate::session::Leaving;

/// Message `subject` in sample folder `folder`, its folder open.
fn key_of(s: &mut Session, folder: &str, subject: &str) -> mailcraft_model::MessageKey {
    s.run("folder.open", &json!({"account": "sample", "folder": folder})).unwrap();
    s.list.iter().find(|m| m.subject == subject).map(|m| m.key.clone()).unwrap()
}

/// The subjects of the messages in Sent to `to`.
fn sent_to(s: &mut Session, to: &str) -> Vec<String> {
    s.run("folder.open", &json!({"account": "sample", "folder": "Sent"})).unwrap();
    s.list.iter().filter(|m| m.to.iter().any(|a| a.email == to)).map(|m| m.subject.clone()).collect()
}

#[test]
fn a_list_is_left_by_the_message_it_asks_for() {
    let mut s = sample_session("unsubscribe-mail");
    let key = key_of(&mut s, "INBOX", "Open call: winter group show");
    assert!(matches!(s.leaving(&key), Some(Leaving::Mail { to, .. }) if to == "leave@gallery.example"));
    s.run("list.open", &json!({"key": key.to_id()})).unwrap();
    let r = s.run("mail.unsubscribe", &json!({})).unwrap();
    assert_eq!(r.get("to").and_then(Value::as_str), Some("leave@gallery.example"), "{r}");
    // From the account it came to, with the subject lists read; nothing opened.
    assert_eq!(sent_to(&mut s, "leave@gallery.example"), ["unsubscribe"]);
    assert!(s.open_urls.is_empty());
}

#[test]
fn nothing_is_sent_from_junk_email() {
    let mut s = sample_session("unsubscribe-junk");
    let key = key_of(&mut s, "Junk", "You may have won a camera");
    assert!(s.in_junk(&key) && s.leaving(&key).is_some(), "the prize mail names a way to leave its list");
    s.run("list.open", &json!({"key": key.to_id()})).unwrap();
    assert!(matches!(s.run("mail.unsubscribe", &json!({"key": key.to_id()})), Err(CmdError::Disabled(_))));
    assert!(sent_to(&mut s, "leave@prizes.example").is_empty(), "the junk mail's sender was answered");
}

#[test]
fn a_list_with_only_a_page_has_it_opened_in_the_browser() {
    let mut s = sample_session("unsubscribe-page");
    let d = mailcraft_mime::Draft {
        to: vec![mailcraft_model::EmailAddress::new("Riley", "riley@studio.example")],
        subject: "Weekly picks".into(),
        body: "This week's picks.".into(),
        ..Default::default()
    };
    let built = mailcraft_mime::build(&d, &mailcraft_model::EmailAddress::new("Picks", "picks@news.example"), NOW, "<picks@x>").unwrap();
    let url = "https://news.example/leave?u=riley";
    let raw = [format!("List-Unsubscribe: <{url}>\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n").as_bytes(), &built].concat();
    let key = mailcraft_model::MessageKey { uid: 9400, ..next_key(&s, "sample", "INBOX") };
    let key = file_whole(&mut s, key, &raw);
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    s.run("list.open", &json!({"key": key.to_id()})).unwrap();
    let r = s.run("mail.unsubscribe", &json!({})).unwrap();
    assert_eq!(r.get("opened").and_then(Value::as_str), Some(url), "{r}");
    assert_eq!(s.open_urls, [url]);
    assert!(sent_to(&mut s, "picks@news.example").is_empty(), "nothing is sent");
}

#[test]
fn mail_from_no_list_names_no_way_to_leave() {
    let mut s = sample_session("unsubscribe-none");
    s.run("list.open", &json!({"key": "sample:1:1:INBOX"})).unwrap();
    assert!(matches!(s.run("mail.unsubscribe", &json!({})), Err(CmdError::Disabled(_))));
}
