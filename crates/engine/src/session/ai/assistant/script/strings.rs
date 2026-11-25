//! Rhai's string functions as a script gets them, where Rhai's own would mislead or do harm.
//! `trim` and `replace` change the string they're called on and give nothing, where models,
//! writing Python or JavaScript, take what they give for the new string: `let t = s.trim()` was
//! `()`, and every comparison of it false, so a script came to a wrong answer without an error.
//! These change the string and give it too. And Rhai's `replace` builds its whole string before
//! Rhai checks the string's size, so a short call (`s.replace("", s)`) could ask for a million
//! times what a script may hold, more memory than the computer has, and end MailCraft: this one
//! checks first.

use rhai::{Dynamic, Engine, EvalAltResult, ImmutableString, Position};

/// `trim` and each `replace` (a string or a character found, a string or a character put in),
/// giving the string they made.
pub(super) fn register(engine: &mut Engine, most: usize) {
    engine.register_fn("trim", |s: &mut ImmutableString| {
        let trimmed: ImmutableString = s.trim().into();
        *s = trimmed.clone();
        trimmed
    });
    engine.register_fn("replace", move |s: &mut ImmutableString, find: &str, with: &str| replace(s, find, with, most));
    engine.register_fn("replace", move |s: &mut ImmutableString, find: char, with: &str| replace(s, &find.to_string(), with, most));
    engine.register_fn("replace", move |s: &mut ImmutableString, find: &str, with: char| replace(s, find, &with.to_string(), most));
    engine.register_fn("replace", move |s: &mut ImmutableString, find: char, with: char| replace(s, &find.to_string(), &with.to_string(), most));
}

/// `find` replaced by `with` throughout `s`, as Rhai's `replace` does it, unless the string it
/// would make is longer than `most` bytes.
fn replace(s: &mut ImmutableString, find: &str, with: &str, most: usize) -> Result<ImmutableString, Box<EvalAltResult>> {
    if s.is_empty() {
        return Ok(s.clone());
    }
    let len = replaced_len(s, find, with);
    if len > most {
        let why = format!("replace would make a string of {len} bytes, more than the {most} a script may hold");
        return Err(Box::new(EvalAltResult::ErrorRuntime(Dynamic::from(why), Position::NONE)));
    }
    let replaced: ImmutableString = s.replace(find, with).into();
    *s = replaced.clone();
    Ok(replaced)
}

/// How long `s` would be with `find` replaced by `with`: an empty `find` is found before each
/// character and at the end, as `str::replace` finds it.
pub(super) fn replaced_len(s: &str, find: &str, with: &str) -> usize {
    let times = if find.is_empty() { s.chars().count().saturating_add(1) } else { s.matches(find).count() };
    let added = with.len().saturating_sub(find.len());
    s.len().saturating_add(times.saturating_mul(added))
}
