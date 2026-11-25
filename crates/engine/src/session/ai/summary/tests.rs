//! What summaries are kept: the newest asked for.

use super::*;

fn key(uid: u32) -> MessageKey {
    MessageKey { account: "sample".into(), folder: "INBOX".into(), validity: 1, uid }
}

/// Past the most kept, the oldest asked for goes, never the one just asked for; one asked for
/// again takes its own place.
#[test]
fn the_newest_summaries_are_kept() {
    let mut s = Summaries::default();
    let most = u32::try_from(KEPT).unwrap();
    for uid in 0..=most {
        s.keep(key(uid), ThreadSummary::default(), u64::from(uid) + 1);
    }
    assert!(!s.by_key.contains_key(&key(0)), "the oldest let go");
    assert!((1..=most).all(|uid| s.by_key.contains_key(&key(uid))), "the newest kept");
    s.keep(key(1), ThreadSummary::default(), 100);
    assert_eq!(s.by_key.len(), KEPT);
    assert_eq!(s.by_key.get(&key(1)).and_then(|t| t.asking), Some(100), "asked for again, in its place");
}
