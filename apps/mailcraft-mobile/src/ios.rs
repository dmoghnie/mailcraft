//! The Swift shell's C functions (`ios/MailCraftCore/include/mailcraftcore.h` declares them).
//!
//! Text goes in as NUL-terminated UTF-8 and comes back as a string this library made, which the
//! caller hands to [`mailcraft_free`] once read. A null or non-UTF-8 argument is read as empty
//! (which the call then refuses), never followed. Errors come back as their text, empty when all
//! went well.
//!
//! Built into the tests on every system too (`cfg(test)`), so the pointer handling is checked
//! where it can be run, not only on an iPhone.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char};

/// The text at `p`, empty for null or text that isn't UTF-8.
fn text(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: the caller passes a pointer to a NUL-terminated string that stays valid for this
    // call (the header says so); null was refused above. It is only read, up to its NUL.
    unsafe { CStr::from_ptr(p) }.to_str().map(str::to_string).unwrap_or_default()
}

/// `s` handed to the caller, to give back to [`mailcraft_free`]. A NUL inside (JSON never has
/// one unescaped) cuts it there rather than failing.
fn out(s: String) -> *mut c_char {
    let s = match CString::new(s) {
        Ok(c) => c,
        Err(e) => {
            let n = e.nul_position();
            let mut v = e.into_vec();
            v.truncate(n);
            CString::new(v).unwrap_or_default()
        }
    };
    s.into_raw()
}

fn said(r: Result<(), String>) -> *mut c_char {
    out(r.err().unwrap_or_default())
}

/// SAFETY (each export): the name is exported once, and the C signature in the header matches.
#[unsafe(no_mangle)]
pub extern "C" fn mailcraft_start(dir: *const c_char, secrets: *const c_char, offset: i32, zone: *const c_char) -> *mut c_char {
    said(crate::start(&text(dir), &text(secrets), offset, &text(zone)))
}

#[unsafe(no_mangle)]
pub extern "C" fn mailcraft_send(action: *const c_char) -> *mut c_char {
    said(crate::send(&text(action)))
}

#[unsafe(no_mangle)]
pub extern "C" fn mailcraft_poll(timeout_ms: u64) -> *mut c_char {
    out(crate::poll(timeout_ms))
}

#[unsafe(no_mangle)]
pub extern "C" fn mailcraft_view() -> *mut c_char {
    out(crate::view())
}

#[unsafe(no_mangle)]
pub extern "C" fn mailcraft_zone(offset: i32, zone: *const c_char) -> *mut c_char {
    said(crate::zone(offset, &text(zone)))
}

/// A string one of these functions returned, given back. Null is ignored.
#[unsafe(no_mangle)]
pub extern "C" fn mailcraft_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    // SAFETY: `s` came from `CString::into_raw` in `out` (the header says only those are given
    // back, each once), so it is retaken as the CString it was and dropped.
    drop(unsafe { CString::from_raw(s) });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn back(p: *mut c_char) -> String {
        let s = text(p);
        mailcraft_free(p);
        s
    }

    #[test]
    fn null_and_bad_text_are_read_as_empty() {
        assert_eq!(text(std::ptr::null()), "");
        let bad = [0xffu8, 0xfe, 0];
        assert_eq!(text(bad.as_ptr().cast()), "");
        let good = CString::new("é ok").unwrap();
        assert_eq!(text(good.as_ptr()), "é ok");
        mailcraft_free(std::ptr::null_mut());
    }

    #[test]
    fn a_nul_inside_cuts_the_text_there() {
        assert_eq!(back(out("ab\0cd".into())), "ab");
    }
}
