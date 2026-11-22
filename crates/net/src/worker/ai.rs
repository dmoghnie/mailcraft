//! MailCraft's own AI in the pool: a model asked on a thread of its own, its key read from the
//! keychain there (the engine never holds it), and keys kept or forgotten.

use super::routing::spawn;
use super::*;
use mailcraft_model::{AiDone, AiWork};

#[cfg(test)]
mod tests;

/// At most this many questions wait for a model at once: past that, one is answered at once
/// with why. Each waits on a thread of its own for up to [`crate::ai::THINKING`], so a command
/// run in a loop can't start threads without end.
pub(crate) const ASKING_MOST: usize = 4;

/// One of the [`ASKING_MOST`] places of the pool's `asking`; given back when dropped, whatever
/// ends the wait.
struct Asking(Arc<AtomicUsize>);

impl Asking {
    fn take(asking: &Arc<AtomicUsize>) -> Option<Asking> {
        let taken = asking.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < ASKING_MOST).then_some(n + 1));
        taken.ok().map(|_| Asking(asking.clone()))
    }
}

impl Drop for Asking {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Pool {
    pub(super) fn ai(&self, work: AiWork) {
        match work {
            AiWork::Ask { id, endpoint, key, chat } => {
                let failed = |error: String| Event::Ai { done: AiDone::Answered { id, answer: None, error: Some(error) } };
                let Some(slot) = Asking::take(&self.asking) else {
                    return self.sink.emit(failed(format!("MailCraft is already waiting for {ASKING_MOST} answers; ask again once one comes")));
                };
                let (sink, creds) = (self.sink.clone(), self.creds.clone());
                let spawned = spawn("mail-ai", move || {
                    let _slot = slot;
                    let kept = key.as_deref().map(|name| creds.get_key(name));
                    let done = match kept {
                        Some(None) => AiDone::Answered { id, answer: None, error: Some("no key is kept for this model".into()) },
                        Some(Some(k)) => answered(id, crate::ai::ask(&endpoint, Some(&k), &chat)),
                        None => answered(id, crate::ai::ask(&endpoint, None, &chat)),
                    };
                    sink.emit(Event::Ai { done });
                });
                if let Err(e) = spawned {
                    self.sink.emit(failed(e.to_string()));
                }
            }
            AiWork::SaveKey { name, key } => {
                let error = match key {
                    Some(k) if !k.0.trim().is_empty() => self.creds.set_key(&name, k.0.trim()).err(),
                    _ => {
                        self.creds.delete_key(&name);
                        None
                    }
                };
                self.sink.emit(Event::Ai { done: AiDone::KeySaved { name, error } });
            }
        }
    }
}

fn answered(id: u64, r: Result<mailcraft_model::AiAnswer>) -> AiDone {
    match r {
        Ok(a) => AiDone::Answered { id, answer: Some(a), error: None },
        Err(e) => AiDone::Answered { id, answer: None, error: Some(e.to_string()) },
    }
}
