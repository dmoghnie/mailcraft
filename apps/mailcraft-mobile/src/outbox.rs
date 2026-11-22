//! What waits for the shell: messages in order, but only the newest render (the view model is
//! whole each time, so an older one is never worth drawing).

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// At most this many messages wait: a shell that stopped polling doesn't grow the queue forever.
/// Renders don't count (one at most); secrets are few.
const MAX: usize = 1_000;

#[derive(Default)]
pub(crate) struct Outbox {
    state: Mutex<State>,
    ready: Condvar,
}

#[derive(Default)]
struct State {
    queue: VecDeque<String>,
    /// The view model not yet taken, when there's a newer one than the shell drew.
    render: Option<String>,
    /// The newest view model, for [`Outbox::view`].
    view: String,
}

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Outbox {
    /// A new view model, unless it's the one drawn already (the clock's beat each second
    /// mostly changes nothing).
    pub(crate) fn render(&self, view: String) {
        let mut s = lock(&self.state);
        if s.view == view {
            return;
        }
        s.render = Some(format!(r#"{{"kind":"render","view":{view}}}"#));
        s.view = view;
        self.ready.notify_all();
    }

    pub(crate) fn push(&self, message: String) {
        let mut s = lock(&self.state);
        if s.queue.len() >= MAX {
            s.queue.pop_front();
        }
        s.queue.push_back(message);
        self.ready.notify_all();
    }

    /// The next message, the queued ones first (a password kept before the view that shows its
    /// account), waiting up to `timeout`.
    pub(crate) fn next(&self, timeout: Duration) -> Option<String> {
        let end = Instant::now() + timeout;
        let mut s = lock(&self.state);
        loop {
            if let Some(m) = s.queue.pop_front() {
                return Some(m);
            }
            if let Some(r) = s.render.take() {
                return Some(r);
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            s = self.ready.wait_timeout(s, left).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
    }

    pub(crate) fn view(&self) -> String {
        let s = lock(&self.state);
        if s.view.is_empty() { r#"{"screen":"folders","ready":false}"#.into() } else { s.view.clone() }
    }
}
