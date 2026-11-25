//! What a language model answered when asked what's to act on in a message: whatever it says,
//! reading it ends without a panic, and keeps no more than its caps.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mailcraft_engine::model::dates::Clock;

fuzz_target!(|data: &[u8]| {
    let Ok(answer) = std::str::from_utf8(data) else { return };
    let facts = mailcraft_engine::facts_from(answer, &Clock::new(1_780_000_000, 3_600));
    assert!(facts.events.len() <= 20 && facts.tasks.len() <= 20 && facts.amounts.len() <= 20 && facts.tracking.len() <= 20);
});
