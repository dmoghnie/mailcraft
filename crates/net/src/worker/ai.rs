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
/// How often a streamed answer's growth is sent on: often enough to read as it's written, not
/// an event for every word.
const PART_EVERY: std::time::Duration = std::time::Duration::from_millis(80);

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
            AiWork::Ask { id, endpoint, key, chat, stream } => {
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
                        Some(k) if stream => answered(id, streamed(&sink, id, &endpoint, k.as_deref(), &chat)),
                        Some(k) => answered(id, crate::ai::ask(&endpoint, k.as_deref(), &chat)),
                        None if stream => answered(id, streamed(&sink, id, &endpoint, None, &chat)),
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

/// Ask with the answer streamed: what more came of it sent on as [`AiDone::Part`]s, every
/// [`PART_EVERY`] at most, and what came last with the answer itself.
fn streamed(
    sink: &Sink,
    id: u64,
    endpoint: &mailcraft_model::AiEndpoint,
    key: Option<&str>,
    chat: &mailcraft_model::AiChat,
) -> Result<mailcraft_model::AiAnswer> {
    let (mut text, mut thinking) = (String::new(), String::new());
    let mut sent = std::time::Instant::now();
    let mut on = |said: &str, thought: &str| {
        text.push_str(said);
        thinking.push_str(thought);
        if sent.elapsed() >= PART_EVERY {
            let (text, thinking) = (std::mem::take(&mut text), std::mem::take(&mut thinking));
            sink.emit(Event::Ai { done: AiDone::Part { id, text, thinking } });
            sent = std::time::Instant::now();
        }
    };
    crate::ai::ask_streamed(endpoint, key, chat, &mut on)
}

fn answered(id: u64, r: Result<mailcraft_model::AiAnswer>) -> AiDone {
    match r {
        Ok(a) => AiDone::Answered { id, answer: Some(a), error: None },
        Err(e) => AiDone::Answered { id, answer: None, error: Some(e.to_string()) },
    }
}
