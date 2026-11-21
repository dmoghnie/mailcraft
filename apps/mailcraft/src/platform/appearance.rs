//! Linux and the BSDs: what the desktop asks of the way apps look, which winit doesn't report
//! there, read from the freedesktop portal (`org.freedesktop.appearance`) and followed as it
//! changes: its light or dark preference (`color-scheme`: winit's X11 window has no theme, its
//! Wayland one only the one MailCraft sets), as figcraft does it, and whether to keep motion down
//! (`reduced-motion`; through a portal before 1.21, which lacks it, GNOME's own
//! `enable-animations`). Elsewhere winit reports the theme, `desktop/motion.rs` asks the system
//! for the motion, and this is never started.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// What the portal said: not known (no portal, no preference), dark or light; of motion, not
/// known, keep it down or no preference.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
const UNKNOWN: u8 = 0;
const DARK: u8 = 1;
const LIGHT: u8 = 2;
const STILL: u8 = 1;
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
const MOVES: u8 = 2;

/// The desktop's preferences, as last read.
#[derive(Clone, Default)]
pub(crate) struct SystemLook {
    dark: Arc<AtomicU8>,
    motion: Arc<AtomicU8>,
}

impl SystemLook {
    /// Dark or light, when the desktop said.
    pub(crate) fn dark(&self) -> Option<bool> {
        match self.dark.load(Ordering::Relaxed) {
            DARK => Some(true),
            LIGHT => Some(false),
            _ => None,
        }
    }

    /// Whether the desktop asks for less motion (not when it said nothing).
    pub(crate) fn reduced_motion(&self) -> bool {
        self.motion.load(Ordering::Relaxed) == STILL
    }

    /// Reads the preferences, then follows them on a thread of their own; `wake` redraws the
    /// window, idle or not, when one changes. Without a session bus or portal nothing changes.
    pub(crate) fn follow(&self, wake: impl Fn() + Send + 'static) {
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let look = self.clone();
            if let Err(e) = std::thread::Builder::new().name("mailcraft-theme".into()).spawn(move || watch(&look, &wake)) {
                log::info!("colour scheme: can't start the watcher: {e}");
            }
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let _ = wake;
    }
}

/// The portal's value: 1 prefers dark, 2 prefers light, 0 no preference (GNOME's default, which
/// apps show as light).
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn from_portal(v: u32) -> u8 {
    match v {
        1 => DARK,
        2 => LIGHT,
        _ => UNKNOWN,
    }
}

/// The portal's `reduced-motion`: 1 asks for less motion, 0 has no preference, and anything else
/// is taken as 0, as the portal's documentation says.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn motion_from_portal(v: u32) -> u8 {
    if v == 1 { STILL } else { MOVES }
}

/// GNOME's `enable-animations`: off asks for less motion.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn motion_from_gnome(on: bool) -> u8 {
    if on { MOVES } else { STILL }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn watch(look: &SystemLook, wake: &dyn Fn()) {
    use zbus::zvariant::Value;
    const PORTAL: &str = "org.freedesktop.portal.Desktop";
    const SETTINGS: &str = "org.freedesktop.portal.Settings";
    const APPEARANCE: &str = "org.freedesktop.appearance";
    const GNOME: &str = "org.gnome.desktop.interface";
    // `Read` (portals before 1.17, which lack `ReadOne`) wraps the value in one more variant.
    fn inner<'v, 'a>(v: &'v Value<'a>) -> &'v Value<'a> {
        match v {
            Value::Value(i) => i,
            v => v,
        }
    }
    let number = |v: &Value| match inner(v) {
        Value::U32(x) => *x,
        _ => 0,
    };
    let scheme = |v: &Value| from_portal(number(v));
    let motion = |v: &Value| motion_from_portal(number(v));
    let gnome = |v: &Value| match inner(v) {
        Value::Bool(on) => motion_from_gnome(*on),
        _ => UNKNOWN,
    };
    let store = |to: &AtomicU8, new: u8| {
        if to.swap(new, Ordering::Relaxed) != new {
            wake();
        }
    };
    // A portal that takes the call and never answers would hold this thread for good: each call
    // gives up after 2 s (as GridCraft's theme watcher), and the signals are followed anyway.
    let Ok(conn) = zbus::blocking::connection::Builder::session().and_then(|b| b.method_timeout(std::time::Duration::from_secs(2)).build()) else {
        log::info!("colour scheme: no session bus, so the desktop's theme isn't followed");
        return;
    };
    let read = |namespace: &str, key: &str, parse: &dyn Fn(&Value) -> u8| -> zbus::Result<u8> {
        let ask = |method: &str| -> zbus::Result<u8> {
            let reply = conn.call_method(Some(PORTAL), "/org/freedesktop/portal/desktop", Some(SETTINGS), method, &(namespace, key))?;
            let body = reply.body();
            Ok(parse(&body.deserialize::<Value>()?))
        };
        ask("ReadOne").or_else(|_| ask("Read"))
    };
    // Subscribed before the first read, so a change made while it's read isn't missed (as
    // PhotoCraft's watcher): the iterator queues what comes from here on.
    let subscribe = || -> zbus::Result<zbus::blocking::MessageIterator> {
        let rule =
            zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).sender(PORTAL)?.interface(SETTINGS)?.member("SettingChanged")?.build();
        zbus::blocking::MessageIterator::for_match_rule(rule, &conn, Some(8))
    };
    // GNOME's switch is followed only where the portal hasn't `reduced-motion`: where it has,
    // that is the desktop's answer.
    let portal_motion = std::cell::Cell::new(false);
    let changes = subscribe_then_read(subscribe, || {
        match read(APPEARANCE, "color-scheme", &scheme) {
            Ok(v) => store(&look.dark, v),
            Err(e) => log::info!("colour scheme: can't read the desktop's setting: {e}"),
        }
        let asked = read(APPEARANCE, "reduced-motion", &motion);
        portal_motion.set(asked.is_ok());
        match asked.or_else(|_| read(GNOME, "enable-animations", &gnome)) {
            Ok(v) => store(&look.motion, v),
            Err(e) => log::info!("reduced motion: can't read the desktop's setting: {e}"),
        }
    });
    let follow = || -> zbus::Result<()> {
        for msg in changes? {
            let msg = msg?;
            let body = msg.body();
            let Ok((namespace, key, value)) = body.deserialize::<(String, String, Value<'_>)>() else { continue };
            match (namespace.as_str(), key.as_str()) {
                (APPEARANCE, "color-scheme") => store(&look.dark, scheme(&value)),
                (APPEARANCE, "reduced-motion") => {
                    portal_motion.set(true);
                    store(&look.motion, motion(&value));
                }
                (GNOME, "enable-animations") if !portal_motion.get() => store(&look.motion, gnome(&value)),
                _ => {}
            }
        }
        Ok(())
    };
    if let Err(e) = follow() {
        log::info!("colour scheme: can't follow the desktop's setting: {e}");
    }
}

/// Subscribe to the changes, then read the setting as it is: in this order, a change made while
/// it's read comes as a change, where the other way round it was lost.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn subscribe_then_read<S>(subscribe: impl FnOnce() -> S, read: impl FnOnce()) -> S {
    let changes = subscribe();
    read();
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The portal's three answers, and anything else it might say, as MailCraft takes them.
    #[test]
    fn the_portal_values() {
        let s = SystemLook::default();
        assert_eq!(s.dark(), None, "nothing said yet");
        for (v, want) in [(1, Some(true)), (2, Some(false)), (0, None), (7, None)] {
            s.dark.store(from_portal(v), Ordering::Relaxed);
            assert_eq!(s.dark(), want, "{v}");
        }
    }

    /// Less motion when the portal's `reduced-motion` is 1 or GNOME's animations are off; not
    /// when nothing was said, the portal has no preference or says what it doesn't define.
    #[test]
    fn the_portal_s_motion() {
        let s = SystemLook::default();
        assert!(!s.reduced_motion(), "nothing said yet");
        for (v, want) in [(1, true), (0, false), (2, false), (7, false)] {
            s.motion.store(motion_from_portal(v), Ordering::Relaxed);
            assert_eq!(s.reduced_motion(), want, "{v}");
        }
        for (on, want) in [(false, true), (true, false)] {
            s.motion.store(motion_from_gnome(on), Ordering::Relaxed);
            assert_eq!(s.reduced_motion(), want, "animations {on}");
        }
    }

    /// The desktop switches to dark while MailCraft reads it as light: the switch comes as a
    /// change, so dark is where it ends.
    #[test]
    fn a_switch_while_starting_is_kept() {
        let s = SystemLook::default();
        let subscribed = std::cell::Cell::new(false);
        let queued = std::cell::Cell::new(None);
        subscribe_then_read(
            || subscribed.set(true),
            || {
                s.dark.store(from_portal(2), Ordering::Relaxed);
                // The desktop changes as the answer comes back: told only to a subscriber.
                if subscribed.get() {
                    queued.set(Some(1));
                }
            },
        );
        if let Some(v) = queued.get() {
            s.dark.store(from_portal(v), Ordering::Relaxed);
        }
        assert_eq!(s.dark(), Some(true));
    }

    /// Against a desktop portal on a real session bus, run by hand (`MAILCRAFT_PORTAL=1`, with
    /// one that says dark and less motion, then changes to light and motion): read at once, the
    /// changes followed and the window woken for them.
    #[test]
    #[ignore = "needs a desktop portal on a session bus: MAILCRAFT_PORTAL=1"]
    fn follows_a_desktop_portal() {
        if std::env::var_os("MAILCRAFT_PORTAL").is_none() {
            eprintln!("MAILCRAFT_PORTAL isn't set: skipped");
            return;
        }
        let s = SystemLook::default();
        let (woken, wakes) = std::sync::mpsc::channel();
        s.follow(move || {
            let _ = woken.send(());
        });
        let until = |want: (Option<bool>, bool)| {
            let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while (s.dark(), s.reduced_motion()) != want {
                assert!(std::time::Instant::now() < end, "never {want:?}, still {:?}", (s.dark(), s.reduced_motion()));
                let _ = wakes.recv_timeout(std::time::Duration::from_millis(100));
            }
        };
        until((Some(true), true));
        until((Some(false), false));
    }
}
