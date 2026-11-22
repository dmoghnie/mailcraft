//! Engine tests: a sent message sent again, as a new one (Send Again, Resend).

use super::*;

/// A message Riley sent, with a file, filed in the sample's Sent folder and opened.
fn sent_with_a_file(s: &mut Session) -> mailcraft_model::MessageKey {
    let d = mailcraft_mime::Draft {
        to: vec![mailcraft_model::EmailAddress::new("Print Shop", "orders@printshop.example")],
        cc: vec![mailcraft_model::EmailAddress::new("Sam Okafor", "sam@studio.example")],
        subject: "Poster files".into(),
        body: "The files for the poster.".into(),
        attachments: vec![mailcraft_mime::OutAttachment {
            name: "poster.pdf".into(),
            mime: "application/pdf".into(),
            data: b"%PDF".to_vec(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let raw = mailcraft_mime::build(&d, &mailcraft_model::EmailAddress::new("Riley Moreno", "riley@studio.example"), NOW, "<sent1@x>").unwrap();
    let key = mailcraft_model::MessageKey { uid: 9300, ..next_key(s, "sample", "Sent") };
    let key = file_whole(s, key, &raw);
    s.run("folder.open", &json!({"account": "sample", "folder": "Sent"})).unwrap();
    s.run("list.open", &json!({"key": key.to_id()})).unwrap();
    key
}

#[test]
fn a_sent_message_opens_again_as_a_new_one() {
    let mut s = sample_session("send-again");
    s.prefs.new_mail_window = false;
    let key = sent_with_a_file(&mut s);
    let r = s.run("mail.sendAgain", &json!({})).unwrap();
    assert!(r.get("compose").is_some(), "{r}");
    let c = s.compose.clone().unwrap();
    assert_eq!((c.account.as_str(), c.subject.as_str()), ("sample", "Poster files"));
    assert!(c.to.contains("orders@printshop.example") && c.cc.contains("sam@studio.example"), "{c:?}");
    assert!(c.body.contains("The files for the poster."), "{:?}", c.body);
    assert_eq!(c.attachments.iter().map(|a| (a.name.as_str(), a.data.as_slice())).collect::<Vec<_>>(), [("poster.pdf", b"%PDF".as_slice())]);
    // A new message: not the copy in Sent, not a draft yet, and nothing on its way.
    assert!(c.draft.is_none() && c.draft_id.is_empty() && c.source.is_none() && !c.sending, "{c:?}");
    assert!(!s.take_jobs().iter().any(|j| matches!(j, Job::Send { .. })), "nothing was sent");
    assert!(s.outbox.is_empty(), "nothing waits in the Outbox");
    // Named by its key, as an agent would.
    s.run("compose.discard", &json!({})).unwrap();
    s.run("mail.sendAgain", &json!({"key": key.to_id()})).unwrap();
    assert_eq!(s.compose.as_ref().map(|c| c.subject.as_str()), Some("Poster files"));
}

#[test]
fn only_a_message_you_sent_is_sent_again() {
    let mut s = sample_session("send-again-not-mine");
    s.run("list.open", &json!({"key": "sample:1:1:INBOX"})).unwrap();
    assert!(!s.sent_by_me(&mailcraft_model::MessageKey::from_id("sample:1:1:INBOX").unwrap()));
    assert!(matches!(s.run("mail.sendAgain", &json!({})), Err(CmdError::Disabled(_))));
    assert!(s.compose.is_none());
}
