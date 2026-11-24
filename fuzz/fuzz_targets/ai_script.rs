//! A script MailCraft's AI wrote, run as the assistant runs one: the input's text up to its
//! first NUL is the script, and the JSON values after it answer its calls in turn. Whatever it
//! does, the run ends (bounded in operations, calls, depth and size) without a panic or a crash.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (source, answers) = match data.iter().position(|b| *b == 0) {
        Some(at) => (data.get(..at).unwrap_or_default(), data.get(at + 1..).unwrap_or_default()),
        None => (data, &[][..]),
    };
    let Ok(source) = std::str::from_utf8(source) else { return };
    let _ = mailcraft_engine::run_script_against(source, answers);
});
