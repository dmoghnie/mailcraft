//! The buttons on new-mail notifications, as the system reports them (Linux, the BSDs, macOS;
//! Windows' toasts are MailCraft's own and have none yet). Each notification waits for its
//! answer on a thread of its own (`notified.rs`), until it's clicked, a button is pressed or it's
//! closed (macOS keeps one in Notification Center, waiting, until it's cleared); the button
//! pressed is kept until the window asks (`Services::notices_pressed`).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// At most this many notifications wait for their click or their buttons at once: past these,
/// the next has no buttons and its click isn't followed (each holds a thread while it waits).
pub(crate) const WAITING_MOST: usize = 32;

/// The buttons pressed kept for the window, at most (it asks every pass).
const PRESSED_MOST: usize = 64;

static WAITING: AtomicUsize = AtomicUsize::new(0);

/// A notification waiting for its buttons, counted while it lives.
pub(crate) struct Waiting(());

impl Waiting {
    /// One more waiting; none when [`WAITING_MOST`] are already.
    pub(crate) fn new() -> Option<Waiting> {
        WAITING.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < WAITING_MOST).then_some(n + 1)).ok().map(|_| Waiting(()))
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        WAITING.fetch_sub(1, Ordering::AcqRel);
    }
}

/// The buttons pressed, each with its notification, that the window hasn't taken yet.
pub(crate) type Pressed = Arc<Mutex<Vec<(u64, String)>>>;

/// The button the system's answer names: by its id (Windows, Linux) or by its label (macOS
/// names the one picked by what it says). None for a click on the notification, its closing,
/// or anything else.
pub(crate) fn button_of<'a>(answer: &str, buttons: &'a [(String, String)]) -> Option<&'a str> {
    buttons.iter().find(|(id, label)| answer == id || answer == label).map(|(id, _)| id.as_str())
}

/// A button pressed, kept for the window.
pub(crate) fn press(pressed: &Pressed, notice: u64, id: &str) {
    let mut kept = pressed.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if kept.len() < PRESSED_MOST {
        kept.push((notice, id.to_string()));
    }
}

/// The buttons pressed since the window asked last.
pub(crate) fn take(pressed: &Pressed) -> Vec<(u64, String)> {
    std::mem::take(&mut *pressed.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_button_is_known_by_its_id_or_its_label_and_kept_for_the_window() {
        let buttons: Vec<(String, String)> =
            [("read", "Als gelesen markieren"), ("reply", "Antworten")].map(|(i, l)| (i.to_string(), l.to_string())).into();
        assert_eq!(button_of("read", &buttons), Some("read"));
        assert_eq!(button_of("Antworten", &buttons), Some("reply"), "macOS answers with the label");
        for not_one in ["default", "__closed", "", "delete", "READ"] {
            assert_eq!(button_of(not_one, &buttons), None, "{not_one}");
        }
        let pressed = Pressed::default();
        for n in 0..100 {
            press(&pressed, n, "read");
        }
        assert_eq!(take(&pressed).len(), PRESSED_MOST);
        assert!(take(&pressed).is_empty());
    }

    #[test]
    fn only_so_many_notifications_wait_at_once() {
        let waiting: Vec<Waiting> = std::iter::from_fn(Waiting::new).take(WAITING_MOST + 1).collect();
        assert_eq!(waiting.len(), WAITING_MOST);
        assert!(Waiting::new().is_none());
        drop(waiting);
        assert!(Waiting::new().is_some());
    }
}
