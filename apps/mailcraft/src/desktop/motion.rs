//! Whether the system asks apps to keep motion down: on a Mac, System Settings › Accessibility ›
//! Display › Reduce motion, asked as the window draws, once in a while. Elsewhere it isn't read
//! yet (Windows' Animation effects, the desktop portal's `reduced-motion`), and only Play
//! animations turns motion off there.

use std::cell::Cell;
use std::time::{Duration, Instant};

/// How long an answer is used before the system is asked again.
const FRESH: Duration = Duration::from_secs(2);

/// The system's answer, as last asked.
#[derive(Default)]
pub(crate) struct SystemMotion(Cell<Option<(Instant, bool)>>);

impl SystemMotion {
    /// Whether the system asks for less motion now.
    pub(crate) fn reduced(&self) -> bool {
        self.reduced_at(Instant::now(), ask)
    }

    /// [`SystemMotion::reduced`] at `now`, asking the system with `ask` when the last answer is
    /// stale.
    fn reduced_at(&self, now: Instant, ask: impl FnOnce() -> bool) -> bool {
        match self.0.get() {
            Some((at, reduced)) if now.saturating_duration_since(at) < FRESH => reduced,
            _ => {
                let reduced = ask();
                self.0.set(Some((now, reduced)));
                reduced
            }
        }
    }
}

/// The system's own answer.
fn ask() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The system is asked once in a while, not on every frame, and its answer is followed: a
    /// change shows within [`FRESH`].
    #[test]
    fn the_system_is_asked_once_in_a_while() {
        let m = SystemMotion::default();
        let start = Instant::now();
        let asked = Cell::new(0);
        let ask = |answer: bool| {
            asked.set(asked.get() + 1);
            answer
        };
        assert!(m.reduced_at(start, || ask(true)));
        assert!(m.reduced_at(start + Duration::from_millis(500), || ask(false)), "the answer kept");
        assert_eq!(asked.get(), 1);
        assert!(!m.reduced_at(start + FRESH, || ask(false)), "asked again");
        assert_eq!(asked.get(), 2);
    }

    /// The system answers (on a Mac, as the setting is).
    #[test]
    fn the_system_answers() {
        let _ = SystemMotion::default().reduced();
    }
}
