//! Engine tests: the Inbox in four groups (Primary, Transactions, Updates and Promotions),
//! sorted from each message's headers on this computer.

use mailcraft_model::MessageKey;

use super::*;
use crate::InboxGroup;

/// A message from `from` about `subject` come into the sample's Inbox, with these header lines
/// too (each ending `\r\n`).
fn arrive(s: &mut Session, from: &str, subject: &str, extra: &str) -> MessageKey {
    let key = next_key(s, "sample", "INBOX");
    let raw = format!(
        "From: {from}\r\nTo: riley@example.com\r\nSubject: {subject}\r\nDate: Mon, 16 Nov 2026 09:00:00 +0000\r\nMessage-ID: <m{}@example.org>\r\n{extra}MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHello.\r\n",
        key.uid
    );
    file_whole(s, key, raw.as_bytes())
}

/// Four groups over the sample: the session with them on, and a message of each kind, in
/// English and German, with its group and whether it was sent in bulk (each header line added
/// says so).
fn grouped(name: &str) -> (Session, Vec<(MessageKey, InboxGroup, bool)>) {
    let mut s = sample_session(name);
    let list = "List-Id: <news.example>\r\nList-Unsubscribe: <mailto:leave@news.example>\r\n";
    let mail = [
        ("Ana Lima <ana@friends.example>", "Lunch on Friday?", "", InboxGroup::Primary),
        ("Shop <orders@shop.example>", "Your order #4411 has shipped", "", InboxGroup::Transactions),
        ("Bank <noreply@bank.example>", "Ihr Kontoauszug für Oktober", "Auto-Submitted: auto-generated\r\n", InboxGroup::Transactions),
        ("Laden <hallo@laden.example>", "Ihre Bestellung ist unterwegs", list, InboxGroup::Transactions),
        ("Shop <news@shop.example>", "50% off everything this weekend", list, InboxGroup::Promotions),
        ("Laden <news@laden.example>", "Nur heute: 20 % Rabatt auf alles", list, InboxGroup::Promotions),
        ("Forum <digest@forum.example>", "Your weekly digest", "List-Id: <forum.example>\r\nPrecedence: bulk\r\n", InboxGroup::Updates),
        ("Verein <info@verein.example>", "Neuigkeiten aus dem Verein", list, InboxGroup::Updates),
        // Written to (the sample sent it the poster's order): Primary, a receipt or not.
        ("Print Shop <orders@printshop.example>", "Your order has shipped", list, InboxGroup::Primary),
    ];
    let keys = mail.iter().map(|(from, subject, extra, g)| (arrive(&mut s, from, subject, extra), *g, !extra.is_empty())).collect();
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    s.run("view.focused", &json!({"on": true})).unwrap();
    s.run("view.inboxGroups", &json!({"on": true})).unwrap();
    (s, keys)
}

/// Each tab lists the messages of its group; a VIP's are Primary. Turned off, the Inbox is
/// Focused and Other as before: a receipt not sent in bulk is Focused.
#[test]
fn mail_is_sorted_four_ways_from_its_headers_in_two_languages() {
    let (mut s, keys) = grouped("groups-sorted");
    for g in InboxGroup::ALL {
        s.run("list.focusTab", &json!({"value": g.code()})).unwrap();
        assert_eq!(s.shown_group(), g);
        for (i, (k, want, _)) in keys.iter().enumerate() {
            let listed = s.list.iter().any(|m| m.key == *k);
            assert_eq!(listed, *want == g, "message {i} in {g:?}: it's {want:?}");
        }
    }
    let digest = keys.get(6).map(|(k, ..)| k.clone()).unwrap();
    s.run("people.vip", &json!({"address": "digest@forum.example"})).unwrap();
    s.run("list.focusTab", &json!({"value": "primary"})).unwrap();
    assert!(s.list.iter().any(|m| m.key == digest), "a VIP's mail is Primary");
    s.run("people.unvip", &json!({"address": "digest@forum.example"})).unwrap();

    s.run("view.inboxGroups", &json!({"on": false})).unwrap();
    assert!(!s.focus_other);
    let shipped = keys.get(1).map(|(k, ..)| k.clone()).unwrap();
    assert!(s.list.iter().any(|m| m.key == shipped), "Focused has the receipt");
    s.run("list.focusTab", &json!({"value": "other"})).unwrap();
    for (i, (k, g, bulk)) in keys.iter().enumerate() {
        // Written to: Focused, bulk or not.
        let other = *bulk && *g != InboxGroup::Primary;
        assert_eq!(s.list.iter().any(|m| m.key == *k), other, "message {i}: Other has the bulk mail");
    }
    for bad in [json!({"value": "social"}), json!({})] {
        assert!(matches!(s.run("list.focusTab", &bad), Err(CmdError::Params(_))), "{bad}");
    }
}

/// A sender put in another group stays there, for mail that comes later too, and after a
/// restart (the settings saved and read again); put in Focused or Other, it gives way.
#[test]
fn a_sender_moved_stays_in_its_group_across_a_restart() {
    let (mut s, _) = grouped("groups-moved");
    s.run("list.focusTab", &json!({"value": "updates"})).unwrap();
    let k = s.list.iter().find(|m| m.from.email == "info@verein.example").map(|m| m.key.clone()).unwrap();
    s.run("list.select", &json!({"key": k.to_id()})).unwrap();
    for bad in [json!({"group": "social"}), json!({"group": 2}), json!({})] {
        assert!(matches!(s.run("mail.moveToGroup", &bad), Err(CmdError::Params(_))), "{bad}");
    }
    s.run("mail.moveToGroup", &json!({"group": "promotions"})).unwrap();
    assert!(s.list.iter().all(|m| m.key != k), "gone from Updates");
    let later = arrive(&mut s, "Verein <info@verein.example>", "Termine im Dezember", "List-Id: <verein.example>\r\n");
    s.run("list.focusTab", &json!({"value": "promotions"})).unwrap();
    assert!(s.list.iter().any(|m| m.key == k) && s.list.iter().any(|m| m.key == later), "both in Promotions");

    let saved = serde_json::to_string(&s.prefs).unwrap();
    let store = s.store.clone().unwrap();
    drop(s);
    let mut again = Session::open(Some(store), mailcraft_model::dates::Clock::new(NOW, 0));
    again.prefs = serde_json::from_str(&saved).unwrap();
    again.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    again.run("list.focusTab", &json!({"value": "promotions"})).unwrap();
    assert!(again.list.iter().any(|m| m.key == later), "still in Promotions after a restart");
    // Without the four groups: in Other.
    let m = again.summary(&later).cloned().unwrap();
    assert!(!again.is_focused(&m));
    // Put in Focused: Primary too, its group's choice gone.
    again.run("list.select", &json!({"key": later.to_id()})).unwrap();
    again.run("mail.moveToFocused", &json!({})).unwrap();
    assert_eq!(again.inbox_group(&m), InboxGroup::Primary);
}

/// Asked for, a group other than Primary shows each sender's mail stacked into one row, counted;
/// Primary and the list without it stay one row a message.
#[test]
fn each_senders_mail_is_stacked_in_a_group_when_asked() {
    let (mut s, _) = grouped("groups-stacked");
    for subject in ["Termine im Dezember", "Mitgliederversammlung"] {
        arrive(&mut s, "Verein <info@verein.example>", subject, "List-Id: <verein.example>\r\n");
    }
    s.run("list.focusTab", &json!({"value": "updates"})).unwrap();
    let messages = s.list.len();
    let rows = |s: &Session| s.rows().iter().filter_map(|r| if let Row::Message { count, .. } = r { Some(*count) } else { None }).collect::<Vec<_>>();
    assert_eq!(rows(&s).len(), messages, "a row a message");
    s.run("view.groupDigest", &json!({"on": true})).unwrap();
    let mut senders: Vec<String> = s.list.iter().map(|m| m.from.email.to_ascii_lowercase()).collect();
    senders.sort();
    senders.dedup();
    let stacked = rows(&s);
    assert_eq!(stacked.len(), senders.len(), "a row a sender");
    assert_eq!(stacked.iter().sum::<usize>(), messages);
    assert!(stacked.contains(&3), "the club's three: {stacked:?}");
    s.run("list.focusTab", &json!({"value": "primary"})).unwrap();
    assert!(rows(&s).iter().all(|&c| c == 1), "Primary isn't stacked");
}
