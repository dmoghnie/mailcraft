//! Rhai's string functions as a script gets them, where Rhai's own would do harm: `replace`
//! builds its whole string before Rhai checks the string's size, so a short call
//! (`s.replace("", s)`) could ask for a million times what a script may hold, more memory than
//! the computer has, and end MailCraft.

use rhai::{Dynamic, Engine, EvalAltResult, ImmutableString, Position};

/// Rhai's `replace` with a string to put in, checked before it builds: the others put in a
/// character, at most four times as long as what it replaces, which Rhai's own check catches.
pub(super) fn register(engine: &mut Engine, most: usize) {
    engine.register_fn("replace", move |s: &mut ImmutableString, find: &str, with: &str| replace(s, find, with, most));
    engine.register_fn("replace", move |s: &mut ImmutableString, find: char, with: &str| replace(s, &find.to_string(), with, most));
}

/// `find` replaced by `with` throughout `s`, as Rhai's `replace` does it, unless the string it
/// would make is longer than `most` bytes.
fn replace(s: &mut ImmutableString, find: &str, with: &str, most: usize) -> Result<(), Box<EvalAltResult>> {
    if s.is_empty() {
        return Ok(());
    }
    let len = replaced_len(s, find, with);
    if len > most {
        let why = format!("replace would make a string of {len} bytes, more than the {most} a script may hold");
        return Err(Box::new(EvalAltResult::ErrorRuntime(Dynamic::from(why), Position::NONE)));
    }
    *s = s.replace(find, with).into();
    Ok(())
}

/// How long `s` would be with `find` replaced by `with`: an empty `find` is found before each
/// character and at the end, as `str::replace` finds it.
pub(super) fn replaced_len(s: &str, find: &str, with: &str) -> usize {
    let times = if find.is_empty() { s.chars().count().saturating_add(1) } else { s.matches(find).count() };
    let added = with.len().saturating_sub(find.len());
    s.len().saturating_add(times.saturating_mul(added))
}
