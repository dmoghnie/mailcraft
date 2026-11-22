//! Engine tests: the buttons on new-mail notifications, done to the messages they told of
//! wherever those are, the folder on view left as it is.

use mailcraft_model::MessageKey;

use super::*;

/// A message come into the sample's Inbox (sent in bulk: a list's, kept in Other).
fn arrive(s: &mut Session, subject: &str, bulk: bool) -> MessageKey {
    let key = next_key(s, "sample", "INBOX");
    let list = if bulk { "List-Id: <news.friends.example>\r\n" } else { "" };
    let raw = format!(
        "From: Ana Lima <ana@friends.example>\r\nTo: riley@studio.example\r\nSubject: {subject}\r\n{list}Date: Mon, 16 Nov 2026 09:00:00 +0000\r\nMessage-ID: <n{}@friends.example>\r\n\r\nHello.\r\n",
        key.uid
    );
    file_whole(s, key, raw.as_bytes())
}

/// A sample folder's messages, as the session has them: subject and whether read.
fn folder(s: &mut Session, path: &str) -> Vec<(String, bool)> {
    s.folder_state("sample", path).messages.iter().map(|m| (m.subject.clone(), m.flags.seen)).collect()
}

fn has(s: &mut Session, path: &str, subject: &str) -> bool {
    folder(s, path).iter().any(|(m, _)| m == subject)
}

#[test]
fn a_notifications_buttons_act_on_its_messages_where_they_are() {
    let mut s = sample_session("notice-act");
    let [read, kept, binned, more] = ["Read me", "Keep me", "Bin me", "And me"].map(|subject| arrive(&mut s, subject, false));
    s.run("folder.open", &json!({"account": "sample", "folder": "Sent"})).unwrap();
    let on_view = (s.view.clone(), s.list.len());

    s.run("notice.act", &json!({"action": "read", "keys": [read.to_id()]})).unwrap();
    assert!(folder(&mut s, "INBOX").contains(&("Read me".into(), true)));
    s.run("notice.act", &json!({"action": "archive", "keys": [kept.to_id()]})).unwrap();
    assert!(!has(&mut s, "INBOX", "Keep me") && has(&mut s, "Archive", "Keep me"));
    s.run("notice.act", &json!({"action": "delete", "keys": [binned.to_id()]})).unwrap();
    assert!(!has(&mut s, "INBOX", "Bin me") && has(&mut s, "Trash", "Bin me"));
    // Undone as the ribbon's are.
    s.run("edit.undo", &json!({})).unwrap();
    assert!(has(&mut s, "INBOX", "Bin me") && !has(&mut s, "Trash", "Bin me"));
    // Mark all as read: each one told of; one gone since is left out.
    let done = s.run("notice.act", &json!({"action": "read", "keys": [read.to_id(), more.to_id(), kept.to_id()]})).unwrap();
    assert_eq!(done["count"], 2);
    assert!(folder(&mut s, "INBOX").contains(&("And me".into(), true)));
    assert_eq!((s.view.clone(), s.list.len()), on_view, "the folder on view stays as it was");
    // Gone altogether: said.
    assert!(matches!(s.run("notice.act", &json!({"action": "delete", "keys": [kept.to_id()]})), Err(CmdError::Failed(_))));

    for bad in [
        json!({}),
        json!({"action": "read"}),
        json!({"action": "read", "keys": []}),
        json!({"action": "read", "keys": "sample:1:1:INBOX"}),
        json!({"action": "flag", "keys": [more.to_id()]}),
        json!({"action": "read", "keys": ["nowhere:1:1:INBOX"]}),
        json!({"action": "read", "keys": ["sample:1:1:Gone"]}),
        json!({"action": "read", "keys": ["sample:1:1:Outbox"]}),
        json!({"action": "read", "keys": [3]}),
    ] {
        assert!(matches!(s.run("notice.act", &bad), Err(CmdError::Params(_))), "{bad}");
    }
}

/// The message being read, archived from its notification, is no longer shown or selected.
#[test]
fn a_message_archived_from_its_notification_leaves_the_reading_pane() {
    let mut s = sample_session("notice-reading");
    let key = arrive(&mut s, "Read and archived", false);
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    s.run("list.open", &json!({"key": key.to_id()})).unwrap();
    s.run("notice.act", &json!({"action": "archive", "keys": [key.to_id()]})).unwrap();
    assert!(s.reading.is_none() && !s.selection.contains(&key) && s.focus.as_ref() != Some(&key));
}

/// Reply opens the message in its folder, in Mail, for the ribbon's Reply to answer: the
/// list's filter and Focused give way to show it.
#[test]
fn reply_opens_the_message_in_its_folder_to_answer_it() {
    let mut s = sample_session("notice-reply");
    let key = arrive(&mut s, "News for you", true);
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    s.run("view.focused", &json!({"on": true})).unwrap();
    s.run("list.filter", &json!({"value": "flagged"})).unwrap();
    assert!(s.summary(&key).is_none());
    s.run("view.module", &json!({"name": "calendar"})).unwrap();

    s.run("notice.open", &json!({"key": key.to_id()})).unwrap();
    assert_eq!(s.module, crate::Module::Mail);
    assert_eq!(s.reading.as_ref().map(|r| r.key.clone()), Some(key.clone()));
    s.run("mail.reply", &json!({})).unwrap();
    assert_eq!(s.compose.as_ref().map(|c| c.subject.clone()), Some("Re: News for you".into()));
    for bad in [json!({}), json!({"key": "sample:1:1:Gone"}), json!({"key": 7})] {
        assert!(matches!(s.run("notice.open", &bad), Err(CmdError::Params(_))), "{bad}");
    }
    let gone = MessageKey { uid: key.uid + 100, ..key };
    assert!(matches!(s.run("notice.open", &json!({"key": gone.to_id()})), Err(CmdError::Failed(_))));
}
