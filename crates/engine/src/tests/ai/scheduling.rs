//! Meeting times proposed in the message being written: the person's free times in their work
//! hours, around everything in their own calendars, go to the model with what to propose; none
//! free, nothing's asked.

use super::*;

const PROOF: &str = "Your proof for the festival poster is ready";
const HOUR: i64 = 3600;

fn ready(name: &str) -> Session {
    let mut s = sample_session(name);
    s.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    local(&mut s, "Local");
    s.take_jobs();
    s
}

/// The sample's Wednesday at `h` o'clock (`NOW` is its noon), and its Thursday.
fn wednesday(h: i64) -> i64 {
    NOW - 12 * HOUR + h * HOUR
}

fn thursday(h: i64) -> i64 {
    wednesday(h) + 24 * HOUR
}

/// Free from now on, in work hours (08:00 to 17:00), around lunch, the studio's edit and
/// render sessions, the sound mix and the storyboard review: a time too short left out.
#[test]
fn free_times_are_found_around_my_calendars() {
    let s = ready("ai-times-free");
    let half = |h: f64| (h * 2.0) as i64 * HOUR / 2;
    let free = s.my_free_times(30 * 60);
    assert_eq!(
        free.iter().take(5).copied().collect::<Vec<_>>(),
        [
            (wednesday(12), wednesday(12) + half(0.5)),
            (wednesday(13) + half(0.5), wednesday(17)),
            (thursday(8), thursday(9)),
            (thursday(13), thursday(14)),
            (thursday(15) + half(0.5), thursday(17)),
        ]
    );
    let hour = s.my_free_times(HOUR);
    assert_eq!(hour.first(), Some(&(wednesday(13) + half(0.5), wednesday(17))), "half an hour is too short for an hour");
    assert!(hour.iter().all(|(a, b)| b - a >= HOUR));
}

/// Propose meeting times: the free times go with what's asked, to the model of the account it's
/// from; with none free, nothing's asked.
#[test]
fn meeting_times_are_proposed_in_a_reply() {
    let mut s = ready("ai-times");
    let k = key_of(&s, PROOF);
    s.run("list.open", &k).unwrap();
    s.run("mail.reply", &json!({})).unwrap();
    s.take_jobs();
    let v = s.run("compose.rewrite", &json!({"how": "times", "minutes": 60})).unwrap();
    assert_eq!(v["rewrite"]["how"], "times", "{v}");
    let (id, _, _, chat) = asked(&mut s).pop().expect("a question");
    let q = &chat.turns[0].text;
    assert!(q.contains("a meeting of 60 minutes") && q.contains("- Wednesday 2026-10-14, 13:30 to 17:00") && !q.contains("12:00 to 12:30"), "{q}");
    assert!(q.contains("The message it answers") && q.contains("The proof for order 4471"), "{q}");
    answer(&mut s, id, "Hello,\n\nCould we meet on Wednesday at 14:00?\n\nThanks");
    s.run("compose.useRewrite", &json!({})).unwrap();
    assert!(matches!(s.run("compose.rewrite", &json!({"how": "times", "minutes": 10})), Err(CmdError::Params(_))), "15 minutes at least");
    // Away all week: no free time to propose, and nothing's asked.
    s.run("event.create", &json!({"summary": "Away", "start": "2026-10-14T12:00", "end": "2026-10-22T12:00"})).unwrap();
    s.take_jobs();
    assert!(matches!(s.run("compose.rewrite", &json!({"how": "times"})), Err(CmdError::Failed(_))));
    assert!(asked(&mut s).is_empty());
}
