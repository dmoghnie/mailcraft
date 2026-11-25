//! What a chat sends its model: the same each time but for what's new, so a server can reuse
//! what it read of it before.

use super::*;

/// The system prompt stays the same as the minutes pass; the time goes with what the person
/// says, each turn with its own.
#[test]
fn the_system_prompt_stays_the_same_as_time_passes() {
    let mut s = ready("sent-time");
    let id = send(&mut s, None, "What came today?");
    let (q, first) = question(&mut s);
    calls(&mut s, q, "Nothing yet.", &[]);
    let at_first = s.clock.now;
    s.clock.now += 90;
    send(&mut s, Some(&id), "And now?");
    let (_, second) = question(&mut s);
    assert_eq!(first.system, second.system);
    let when = |at| format!("(Sent {})\n\n", mailcraft_model::dates::full_time(at, &s.clock));
    assert_eq!(first.turns[0].text, format!("{}What came today?", when(at_first)));
    assert_eq!(second.turns[0].text, first.turns[0].text, "a turn is sent as it was");
    assert_eq!(second.turns[2].text, format!("{}And now?", when(s.clock.now)));
}
