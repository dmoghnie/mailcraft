//! Clean-up suggestions: worked out from the Inbox as it's kept here, nothing asked of a model
//! or a server: a list never read whose newest message names a way out, a bulk sender whose mail
//! piles up, and read mail left for a month; never a VIP's mail or a person's. Archiving the old
//! mail moves only what's read and left alone, as one Undo.

use super::*;
use mailcraft_model::{EmailAddress, MessageKey, PINNED, Summary};

const DAY: i64 = 86_400;

/// A message from `from` arriving in the sample Inbox as uid `uid`, `days` old, unread, made
/// as `like` says; with `raw` as its source, kept here.
fn arrives(s: &mut Session, uid: u32, from: (&str, &str), days: i64, like: impl Fn(&mut Summary), raw: Option<&[u8]>) -> MessageKey {
    let store = s.store.clone().unwrap();
    let mut m = Summary::clone(&store.load_folder("sample", "INBOX").unwrap().messages[0]);
    m.key.uid = uid;
    m.subject = format!("Message {uid}");
    m.from = EmailAddress::new(from.0, from.1);
    m.message_id = id(uid);
    (m.flags.seen, m.flags.answered, m.flags.flagged, m.bulk, m.date) = (false, false, false, false, NOW - days * DAY);
    m.flags.keywords.clear();
    like(&mut m);
    file_in(s, m, raw)
}

fn id(uid: u32) -> String {
    format!("<cleanup-{uid}@x.example>")
}

/// An issue of a list that says how to leave it.
fn issue(uid: u32) -> Vec<u8> {
    format!("From: The Weekly Paper <news@paper.example>\r\nTo: riley@studio.example\r\nSubject: Issue {uid}\r\nMessage-ID: {}\r\nList-Unsubscribe: <mailto:leave@paper.example>\r\n\r\nThe news.\r\n", id(uid)).into_bytes()
}

fn bulk(m: &mut Summary) {
    m.bulk = true;
}

fn read(m: &mut Summary) {
    m.flags.seen = true;
}

const PAPER: (&str, &str) = ("The Weekly Paper", "news@paper.example");
const SHOP: (&str, &str) = ("Deals", "deals@shop.example");
const CLUB: (&str, &str) = ("Club Digest", "digest@club.example");
const VIP: (&str, &str) = ("Studio News", "news@studio.example");
const ZOE: (&str, &str) = ("Zoe Park", "zoe@studio.example");

fn senders(s: &mut Session) -> Vec<Value> {
    s.run("mail.cleanupSuggestions", &json!({})).unwrap()["senders"].as_array().cloned().unwrap_or_default()
}

#[test]
fn lists_never_read_and_bulk_mail_that_piles_up_are_suggested() {
    let mut s = sample_session("cleanup-senders");
    let paper: Vec<MessageKey> = (900..903).map(|uid| arrives(&mut s, uid, PAPER, i64::from(903 - uid), bulk, Some(&issue(uid)))).collect();
    for uid in 910..916 {
        arrives(&mut s, uid, SHOP, i64::from(916 - uid), |m| (m.bulk, m.flags.seen) = (true, uid < 912), None);
    }
    // A list answered once isn't left; three messages don't pile up.
    for uid in 920..923 {
        arrives(&mut s, uid, CLUB, 1, |m| (m.bulk, m.flags.answered) = (true, uid == 920), Some(&issue(uid)));
    }
    s.run("people.vip", &json!({"address": VIP.1})).unwrap();
    for uid in 930..936 {
        arrives(&mut s, uid, VIP, 1, bulk, None);
        arrives(&mut s, uid + 10, ZOE, 1, |_| {}, None);
    }
    let found = senders(&mut s);
    let at = |addr: &str| found.iter().position(|c| c["sender"] == addr);
    let paper_said = &found[at(PAPER.1).expect("the paper")];
    assert_eq!(
        (paper_said["leave"].as_bool(), paper_said["sweep"].as_bool(), paper_said["count"].as_u64(), paper_said["unread"].as_u64()),
        (Some(true), Some(false), Some(3), Some(3))
    );
    assert_eq!((paper_said["newest"].as_str(), paper_said["name"].as_str()), (Some(paper[2].to_id().as_str()), Some("The Weekly Paper")));
    let shop = found[at(SHOP.1).expect("the shop")].clone();
    assert_eq!(
        (shop["leave"].as_bool(), shop["sweep"].as_bool(), shop["count"].as_u64(), shop["unread"].as_u64()),
        (Some(false), Some(true), Some(6), Some(4))
    );
    assert!(at(SHOP.1) < at(PAPER.1), "the most mail first");
    for never in [CLUB.1, VIP.1, ZOE.1] {
        assert!(at(never).is_none(), "{never}: {found:?}");
    }

    // Swept but for its newest, the shop's mail no longer piles up.
    s.run("mail.sweepPile", &json!({"key": shop["newest"]})).unwrap();
    assert!(at_none(&senders(&mut s), SHOP.1));
}

fn at_none(found: &[Value], addr: &str) -> bool {
    found.iter().all(|c| c["sender"] != addr)
}

#[test]
fn read_mail_left_for_a_month_is_archived_as_one_undo() {
    let mut s = sample_session("cleanup-old");
    let before = s.run("mail.cleanupSuggestions", &json!({})).unwrap()["old"][0]["count"].as_u64().unwrap_or(0);
    arrives(&mut s, 950, ZOE, 40, read, None);
    let kept = [
        arrives(&mut s, 951, ZOE, 40, |m| (m.flags.seen, m.flags.flagged) = (true, true), None),
        arrives(&mut s, 952, ZOE, 40, |m| (m.flags.seen, m.flags.keywords) = (true, vec![PINNED.to_string()]), None),
        arrives(&mut s, 953, ZOE, 40, |_| {}, None),
        arrives(&mut s, 954, ZOE, 3, read, None),
    ];
    let found = s.run("mail.cleanupSuggestions", &json!({})).unwrap();
    assert_eq!((found["old"][0]["account"].as_str(), found["old"][0]["count"].as_u64()), (Some("sample"), Some(before + 1)), "{found}");

    let moved = s.run("mail.archiveOld", &json!({"account": "sample"})).unwrap();
    assert_eq!(moved["moved"].as_u64(), Some(before + 1));
    let ids = |s: &mut Session, folder: &str| s.folder_state("sample", folder).messages.iter().map(|m| m.message_id.clone()).collect::<Vec<_>>();
    let inbox = ids(&mut s, "INBOX");
    assert!(!inbox.contains(&id(950)), "archived");
    for k in &kept {
        assert!(inbox.contains(&id(k.uid)), "{k:?} stays");
    }
    assert!(ids(&mut s, "Archive").contains(&id(950)), "in Archive");
    assert!(s.run("mail.cleanupSuggestions", &json!({})).unwrap()["old"].as_array().is_some_and(Vec::is_empty), "nothing old is left");
    s.run("edit.undo", &json!({})).unwrap();
    assert!(ids(&mut s, "INBOX").contains(&id(950)), "back, as one Undo");

    // An account that isn't there, or none named, is refused.
    for bad in [json!({"account": "nope"}), json!({}), json!({"account": 5})] {
        assert!(matches!(s.run("mail.archiveOld", &bad), Err(CmdError::Params(_))), "{bad}");
    }
}
