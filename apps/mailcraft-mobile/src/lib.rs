//! MailCraft's phone library: what the Android and iOS shells load.
//!
//! The app core ([`mailcraft_mobile::Mail`]) and `mailcraft-net`'s worker pool run on one thread
//! of their own (the [`driver`]); the shells talk to it through six calls, the same on both
//! phones ([`start`], [`send`], [`poll`], [`view`], [`zone`], [`background`]), wrapped for JNI in `android.rs`
//! and for C (Swift) in `ios.rs`. Everything crosses as JSON text:
//!
//! * in: the person's [`mailcraft_mobile::Action`]s;
//! * out, from [`poll`]: `{"kind": "render", "view": …}` (the whole [`mailcraft_mobile::ViewModel`]),
//!   `{"kind": "secret", "entry": …, "value": …}` and `{"kind": "forget", "entry": …}` (a password
//!   for the phone's keystore to keep or drop: [`credentials`]).
//!
//! No call blocks the shell's UI thread but [`poll`], which the shell calls from a thread of its
//! own. Nothing here panics; a call that can't be done answers with an error string.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::OnceLock;
use std::time::Duration;

pub mod credentials;
mod driver;
mod outbox;

#[cfg(target_os = "android")]
mod android;
#[cfg(any(target_os = "ios", test))]
mod ios;

use driver::Driver;

static DRIVER: OnceLock<Driver> = OnceLock::new();
static LOGGING: std::sync::Once = std::sync::Once::new();

/// Open the cache in `dir` (the app's own files folder) and start. `secrets` is what the
/// keystore kept, `{"entry": "password", …}` as [`poll`]'s `secret` messages named them;
/// `offset` the local offset from UTC in seconds and `zone` the time zone's name. A second call
/// does nothing (one app per process).
pub fn start(dir: &str, secrets: &str, offset: i32, zone: &str) -> Result<(), String> {
    if DRIVER.get().is_some() {
        return Ok(());
    }
    // The desktop's log (`mailcraft.log` in the mail folder, the last run's kept as the old one),
    // and every panic in it by where it happened: started first, so opening the store is logged.
    // Once a process: a second start would set the last run's log aside under the logger still
    // writing it (a start that failed, then one that didn't).
    LOGGING.call_once(|| {
        if mailcraft_net::logfile::start(Some(std::path::Path::new(dir)), false).is_some() {
            mailcraft_net::logfile::log_panics();
        }
        log::info!("MailCraft {} for phones starting", env!("CARGO_PKG_VERSION"));
        #[cfg(target_os = "android")]
        log::info!(
            "TLS: {}",
            if mailcraft_net::stream::platform_is_ready() {
                "Android's trust store asked for servers Mozilla's roots don't know"
            } else {
                "Android's trust store not set up: Mozilla's roots alone"
            }
        );
    });
    let d = Driver::start(dir.into(), credentials::parse(secrets)?, offset, zone.to_string())?;
    // Two shells' threads starting at once: the second driver is dropped (its thread ends).
    let _ = DRIVER.set(d);
    Ok(())
}

/// The person's action, as JSON.
pub fn send(action: &str) -> Result<(), String> {
    DRIVER.get().ok_or("not started")?.send(action)
}

/// The next message for the shell, waiting up to `timeout_ms` for one; empty when none came.
pub fn poll(timeout_ms: u64) -> String {
    match DRIVER.get() {
        Some(d) => d.outbox().next(Duration::from_millis(timeout_ms.min(60_000))).unwrap_or_default(),
        None => {
            std::thread::sleep(Duration::from_millis(timeout_ms.min(1_000)));
            String::new()
        }
    }
}

/// The view model now, as JSON (what the last `render` message carried).
pub fn view() -> String {
    DRIVER.get().map(|d| d.outbox().view()).unwrap_or_else(|| r#"{"screen":"folders","ready":false}"#.into())
}

/// The app went to the background (Android's `onStop`, iOS's scene going to the background):
/// the data files are sealed, as the desktop seals them as it closes.
pub fn background() -> Result<(), String> {
    DRIVER.get().ok_or("not started")?.background()
}

/// The phone's offset or time zone changed.
pub fn zone(offset: i32, zone: &str) -> Result<(), String> {
    DRIVER.get().ok_or("not started")?.zone(offset, zone)
}

#[cfg(test)]
mod tests;
