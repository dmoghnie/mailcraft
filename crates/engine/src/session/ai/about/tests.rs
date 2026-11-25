//! What's kept of what MailCraft's AI wrote about messages: the newest asked for, each message's
//! summary and translation apart.

use super::*;

fn key(uid: u32) -> MessageKey {
    MessageKey { account: "sample".into(), folder: "INBOX".into(), validity: 1, uid }
}

/// Past the most kept, the oldest asked for goes, never the one just asked for; one asked for
/// again takes its own place.
#[test]
fn the_newest_are_kept() {
    let mut about = About::default();
    let most = u32::try_from(KEPT).unwrap();
    for uid in 0..=most {
        about.keep(key(uid), AboutKind::Summary, AboutMessage::default(), u64::from(uid) + 1);
    }
    let kept = |about: &About, uid| about.by_key.contains_key(&(key(uid), AboutKind::Summary));
    assert!(!kept(&about, 0), "the oldest let go");
    assert!((1..=most).all(|uid| kept(&about, uid)), "the newest kept");
    about.keep(key(1), AboutKind::Summary, AboutMessage::default(), 100);
    assert_eq!(about.by_key.len(), KEPT);
    assert_eq!(about.by_key.get(&(key(1), AboutKind::Summary)).and_then(|w| w.asking), Some(100), "asked for again, in its place");
}

/// A message's summary and its translation are kept apart.
#[test]
fn a_summary_and_a_translation_are_kept_apart() {
    let mut about = About::default();
    about.keep(key(1), AboutKind::Summary, AboutMessage::default(), 1);
    about.keep(key(1), AboutKind::Translation, AboutMessage::default(), 2);
    assert_eq!(about.by_key.len(), 2);
    assert_eq!(about.asks.get(&2), Some(&(key(1), AboutKind::Translation)));
}
