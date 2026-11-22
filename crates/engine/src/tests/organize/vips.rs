//! Engine tests: VIPs, the people whose mail always matters.

use mailcraft_model::{Folder, MessageKey, Summary};

use crate::session::vips::VIPS_MAX;

use super::*;

const ZOE: &str = "zoe@studio.example";
const GALLERY: &str = "hello@gallery.example";

/// The sample's message from `email`, selected in the Inbox.
fn select_from(s: &mut Session, email: &str) -> MessageKey {
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    let key = s.list.iter().find(|m| m.from.email == email).map(|m| m.key.clone()).unwrap();
    s.run("list.select", &json!({"key": key.to_id()})).unwrap();
    key
}

#[test]
fn people_are_made_vips_and_taken_off() {
    let mut s = sample_session("vips-mark");
    assert!(s.run("people.vip", &json!({})).is_err(), "nothing selected, nobody named");
    // The sender of the message selected.
    select_from(&mut s, ZOE);
    assert_eq!(s.run("people.vip", &json!({})).unwrap()["address"], ZOE);
    // An address given, as typed: one person whatever its case.
    s.run("people.vip", &json!({"address": "  Ana@SoundLab.example "})).unwrap();
    s.run("people.vip", &json!({"address": "ana@soundlab.example"})).unwrap();
    assert_eq!(s.run("people.vips", &json!({})).unwrap()["vips"], json!(["ana@soundlab.example", ZOE]));
    assert!(s.is_vip("Zoe@Studio.Example") && !s.is_vip("sam@studio.example"));
    for bad in ["", "zoe", "@studio.example", "zoe@", "a b@c.example", "a@b.example,c@d.example", &format!("{}@x.example", "a".repeat(400))] {
        assert!(matches!(s.run("people.vip", &json!({"address": bad})), Err(CmdError::Params(_))), "{bad:?}");
    }
    s.run("people.unvip", &json!({"address": ZOE})).unwrap();
    assert_eq!(s.vips(), ["ana@soundlab.example"]);
    // Kept with the settings.
    let back: crate::session::Prefs = serde_json::from_value(serde_json::to_value(&s.prefs).unwrap()).unwrap();
    assert_eq!(back.vips, s.prefs.vips);
    // A few people: capped.
    for n in 0..VIPS_MAX + 5 {
        let _ = s.run("people.vip", &json!({"address": format!("p{n}@x.example")}));
    }
    assert_eq!(s.vips().len(), VIPS_MAX);
    // Listing them only reads: an agent that may only read may.
    assert!(crate::mcp::READS.contains(&"people.vips") && crate::mcp::READS.contains(&"folder.vip"));
}

/// A second account, on a server, whose Inbox holds a message from Zoe.
fn second_account(s: &mut Session) -> String {
    let server = s.run("account.add", &json!({"email": "riley@home.example", "password": "pw"})).unwrap()["account"].as_str().unwrap().to_string();
    let folder = Folder {
        account: server.clone(),
        path: "INBOX".into(),
        delimiter: Some('/'),
        role: FolderRole::Inbox,
        selectable: true,
        ..Default::default()
    };
    s.apply(Event::Folders { account: server.clone(), folders: vec![folder] });
    let raw = b"From: Zoe Park <zoe@studio.example>\r\nTo: riley@home.example\r\nSubject: Weekend plans\r\nMessage-ID: <home-1@studio.example>\r\nDate: Mon, 2 Nov 2026 10:00:00 +0000\r\n\r\nSee you Saturday.\r\n";
    let key = MessageKey { account: server.clone(), folder: "INBOX".into(), validity: 9, uid: 1 };
    let m: Summary = mailcraft_mime::summary_from_full(raw, key, mailcraft_model::Flags::default(), 0, NOW);
    let st = mailcraft_store::FolderState { validity: 9, uid_next: 2, highest_modseq: 0, messages: vec![m.into()], window: 0 };
    s.store.clone().unwrap().save_folder(&server, "INBOX", &st).unwrap();
    s.apply(Event::FolderChanged { account: server.clone(), folder: "INBOX".into() });
    s.take_jobs();
    server
}

#[test]
fn the_vip_folder_holds_their_mail_from_every_account() {
    let mut s = sample_session("vips-folder");
    let home = second_account(&mut s);
    assert!(matches!(s.run("folder.vip", &json!({})), Err(CmdError::Disabled(_))), "no VIPs yet");
    s.run("people.vip", &json!({"address": ZOE})).unwrap();
    s.run("folder.vip", &json!({})).unwrap();
    assert!(s.vip_folder_shown());
    let accounts: Vec<&str> = s.list.iter().map(|m| m.key.account.as_str()).collect();
    assert!(s.list.iter().all(|m| m.from.email == ZOE), "{:?}", s.list.iter().map(|m| &m.from.email).collect::<Vec<_>>());
    assert!(accounts.contains(&"sample") && accounts.contains(&home.as_str()), "{accounts:?}");
    // Someone made a VIP while it's shown: theirs is there too.
    s.run("people.vip", &json!({"address": GALLERY})).unwrap();
    assert!(s.vip_folder_shown());
    assert!(s.list.iter().any(|m| m.from.email == GALLERY));
    // The last one taken off: the folder is gone, the list is the folder's again.
    s.run("people.unvip", &json!({"address": ZOE})).unwrap();
    s.run("people.unvip", &json!({"address": GALLERY})).unwrap();
    assert!(!s.vip_folder_shown() && s.search.is_none());
}

#[test]
fn vip_mail_always_goes_to_focused() {
    let mut s = sample_session("vips-focused");
    let key = select_from(&mut s, GALLERY);
    let newsletter = s.summary(&key).cloned().unwrap();
    s.run("view.focused", &json!({"on": true})).unwrap();
    assert!(!s.is_focused(&newsletter), "the gallery's newsletter starts in Other");
    assert!(s.list.iter().all(|m| m.key != key));
    s.run("people.vip", &json!({"address": GALLERY})).unwrap();
    assert!(s.is_focused(&newsletter), "a VIP's newsletter stayed in Other");
    assert!(s.list.iter().any(|m| m.key == key), "and Focused shows it");
    // Even when their mail was once put in Other by hand.
    s.prefs.focus_overrides.insert(GALLERY.into(), false);
    assert!(s.is_focused(&newsletter));
    s.run("people.unvip", &json!({"address": GALLERY})).unwrap();
    assert!(!s.is_focused(&newsletter));
}

/// A message from `from` arriving in the sample Inbox, unread, as uid `uid`.
fn arrives(s: &mut Session, uid: u32, from: (&str, &str)) {
    let store = s.store.clone().unwrap();
    let mut m = Summary::clone(&store.load_folder("sample", "INBOX").unwrap().messages[0]);
    m.key.uid = uid;
    m.subject = format!("Arrived {uid}");
    m.from = mailcraft_model::EmailAddress::new(from.0, from.1);
    m.message_id = format!("<arrived-{uid}@x.example>");
    m.flags.seen = false;
    m.bulk = false;
    m.date = NOW;
    file_in(s, m, None);
}

#[test]
fn new_mail_can_be_announced_from_vips_only() {
    let mut s = sample_session("vips-notify");
    s.prefs.notify = true;
    s.run("people.vip", &json!({"address": ZOE})).unwrap();
    arrives(&mut s, 700, ("Zoe Park", ZOE));
    arrives(&mut s, 701, ("Sam Okafor", "sam@studio.example"));
    let said: Vec<String> = s.take_new_mail().into_iter().map(|m| m.subject).collect();
    assert_eq!(said, ["Arrived 700", "Arrived 701"], "off: everyone's is announced");
    assert_eq!(s.run("view.notifyVipsOnly", &json!({"on": true})).unwrap()["on"], true);
    arrives(&mut s, 702, ("Zoe Park", ZOE));
    arrives(&mut s, 703, ("Sam Okafor", "sam@studio.example"));
    let said: Vec<String> = s.take_new_mail().into_iter().map(|m| m.subject).collect();
    assert_eq!(said, ["Arrived 702"], "on: only the VIP's");
    // It still arrives, unread, in the Inbox.
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    assert!(s.list.iter().any(|m| m.subject == "Arrived 703" && !m.flags.seen));
}
