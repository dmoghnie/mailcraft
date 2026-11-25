//! A question asked once more when its server was busy (429), failed on its side (5xx), or
//! didn't answer in time or cut the connection: those often pass in a moment, and a task of
//! many steps shouldn't end on one. Once is enough: a server still failing after that is told
//! to the person, who can ask again.

use std::io;
use std::time::{Duration, Instant};

use super::{STOPPED, Stop};
use crate::{NetError, Result};

/// How long a question waits before it's asked once more: long enough for a moment's load to
/// pass, short enough to wait for. The tests' is short, the waiting itself the same.
#[cfg(not(test))]
const AGAIN_AFTER: Duration = Duration::from_secs(5);
#[cfg(test)]
const AGAIN_AFTER: Duration = Duration::from_millis(40);
/// How often the wait looks whether the question was stopped.
const LOOK_EVERY: Duration = Duration::from_millis(100);

/// What `ask` came to, asked once more after [`AGAIN_AFTER`] when it failed in a way that may
/// pass ([`may_pass`]) and none of its answer came yet (`came`): an answer already shown in part
/// isn't begun again. Stopped while it waits, it's stopped.
pub(super) fn twice<T>(stop: &Stop, came: &dyn Fn() -> bool, mut ask: impl FnMut() -> Result<T>) -> Result<T> {
    match ask() {
        Err(e) if may_pass(&e) && !came() && !stop.stopped() => {
            log::info!("a model's server was busy, failed or timed out: asked once more");
            wait(stop, AGAIN_AFTER)?;
            ask()
        }
        r => r,
    }
}

/// Whether `e` may pass by itself: the server busy or failing ([`NetError::Later`]), or the
/// connection timed out or was cut. Not a refusal (a key, a model, a question it won't take),
/// nor a server that isn't there.
pub(super) fn may_pass(e: &NetError) -> bool {
    match e {
        NetError::Later(_) => true,
        NetError::Io(e) => {
            matches!(
                e.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock | io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
            )
        }
        _ => false,
    }
}

/// `d` waited, unless `stop` stops the question first.
fn wait(stop: &Stop, d: Duration) -> Result<()> {
    let start = Instant::now();
    loop {
        if stop.stopped() {
            return Err(NetError::Refused(STOPPED.into()));
        }
        let left = d.saturating_sub(start.elapsed());
        if left.is_zero() {
            return Ok(());
        }
        std::thread::sleep(left.min(LOOK_EVERY));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn busy() -> NetError {
        NetError::Later("the model's server: HTTP 503".into())
    }

    /// Asked with `came` and failing with `e` each time: how many times it was asked.
    fn asked(stop: &Stop, came: bool, e: fn() -> NetError) -> (usize, Result<()>) {
        let n = Cell::new(0);
        let r = twice(stop, &|| came, || {
            n.set(n.get() + 1);
            Err(e())
        });
        (n.get(), r)
    }

    /// Busy, it's asked once more, and only once; refused, it isn't.
    #[test]
    fn a_busy_server_is_asked_once_more_a_refusal_never() {
        let stop = Stop::default();
        assert_eq!(asked(&stop, false, busy).0, 2);
        assert_eq!(asked(&stop, false, || NetError::No("the model's server: the key was refused".into())).0, 1);
        let n = Cell::new(0);
        let r = twice(&stop, &|| false, || {
            n.set(n.get() + 1);
            if n.get() == 1 { Err(busy()) } else { Ok("answered") }
        });
        assert_eq!((n.get(), r.ok()), (2, Some("answered")));
    }

    /// An answer that came in part, or a question stopped, isn't asked again.
    #[test]
    fn an_answer_begun_or_a_question_stopped_isnt_asked_again() {
        assert_eq!(asked(&Stop::default(), true, busy).0, 1);
        let stop = Stop::default();
        stop.stop();
        assert_eq!(asked(&stop, false, busy).0, 1);
    }

    /// Stopped while it waits, the wait ends at once, and says it was stopped.
    #[test]
    fn stopped_while_it_waits_the_wait_ends() {
        let stop = Stop::default();
        let by = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            by.stop();
        });
        let start = Instant::now();
        let r = wait(&stop, Duration::from_secs(60));
        assert!(r.unwrap_err().to_string().contains(STOPPED));
        assert!(start.elapsed() < Duration::from_secs(5), "{:?}", start.elapsed());
    }

    /// What may pass: busy, timed out, cut; not a server that isn't there, nor one that refused.
    #[test]
    fn what_may_pass() {
        let io = |k: io::ErrorKind| NetError::Io(io::Error::from(k));
        for k in [io::ErrorKind::TimedOut, io::ErrorKind::WouldBlock, io::ErrorKind::ConnectionReset, io::ErrorKind::ConnectionAborted] {
            assert!(may_pass(&io(k)), "{k:?}");
        }
        assert!(may_pass(&busy()));
        assert!(!may_pass(&io(io::ErrorKind::ConnectionRefused)));
        assert!(!may_pass(&NetError::No("the model's server: no such model or address".into())));
        assert!(!may_pass(&NetError::Protocol("cut short".into())));
    }
}
