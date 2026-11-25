//! Documents written on a thread of the store's own ([`Store::save_doc_later`]).
//!
//! Each write is flushed to the disk before it replaces the file: a chat with MailCraft's AI,
//! written whole at every step of an answer (megabytes, once it read long messages), held the
//! window's frame while it was. The caller hands over the document as it is now; the thread
//! writes the newest copy of each (older ones handed over meanwhile are skipped), and a removal
//! handed over the same way comes in its place. What reads one ([`Store::load_doc`],
//! [`Store::doc_names`]) sees it as handed over, written yet or not, and closing
//! ([`Store::seal_all`]) waits for them all. The thread starts when there's something to write
//! and ends when there's nothing left. In the browser, or when no thread can be started, a
//! document is written as it's handed over.
//!
//! A document is written either this way or at once ([`Store::save_doc`], [`Store::remove_doc`]),
//! never both: one handed over still to be written would come after one written at once.

use std::collections::BTreeMap;
use std::sync::{Condvar, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::*;

/// How long closing waits for the documents handed over, at most.
const FLUSH_WAIT: Duration = Duration::from_secs(10);

/// A document handed over: its bytes, or none to remove it; and its number, which tells it
/// from a newer copy handed over while it was written.
#[derive(Clone)]
struct Handed {
    bytes: Option<Arc<Vec<u8>>>,
    n: u64,
}

#[derive(Default)]
struct Waiting {
    docs: BTreeMap<PathBuf, Handed>,
    handed: u64,
    /// The thread is writing them.
    running: bool,
}

/// The documents handed over and the thread writing them, shared by a store's clones.
#[derive(Default)]
pub(crate) struct DocsLater {
    waiting: Mutex<Waiting>,
    /// One was written, or the thread ended.
    changed: Condvar,
    /// Held by a test to keep the thread from writing.
    #[cfg(test)]
    pub(crate) pause: Mutex<()>,
}

impl std::fmt::Debug for DocsLater {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let w = self.waiting();
        f.debug_struct("DocsLater").field("waiting", &w.docs.len()).field("running", &w.running).finish_non_exhaustive()
    }
}

impl DocsLater {
    fn waiting(&self) -> MutexGuard<'_, Waiting> {
        self.waiting.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `bytes` to be written at `path` (none: removed) by the thread, started if it isn't
    /// running; written here when there's no thread to write them.
    fn hand(self: &Arc<Self>, files: &Routed, path: PathBuf, bytes: Option<Vec<u8>>) -> Result<()> {
        if cfg!(target_arch = "wasm32") {
            return write_now(files, &path, bytes.as_deref());
        }
        let mut w = self.waiting();
        w.handed = w.handed.saturating_add(1);
        let n = w.handed;
        w.docs.insert(path, Handed { bytes: bytes.map(Arc::new), n });
        if w.running {
            return Ok(());
        }
        let (later, theirs) = (self.clone(), files.clone());
        match std::thread::Builder::new().name("mail-docs".into()).spawn(move || write_on(&later, &theirs)) {
            Ok(_) => {
                w.running = true;
                Ok(())
            }
            Err(e) => {
                log::warn!("documents are written as they change: no thread to write them ({e})");
                let docs = std::mem::take(&mut w.docs);
                drop(w);
                docs.into_iter().try_for_each(|(path, h)| write_now(files, &path, h.bytes.as_deref().map(Vec::as_slice)))
            }
        }
    }

    /// The document at `path` as handed over and not yet written: `Some(None)` when it's to be
    /// removed; `None` when nothing waits for it.
    fn handed(&self, path: &Path) -> Option<Option<Arc<Vec<u8>>>> {
        self.waiting().docs.get(path).map(|h| h.bytes.clone())
    }

    /// The names of the documents in `dir` waiting to be written, each with whether it's kept
    /// (or removed).
    fn handed_in(&self, dir: &Path) -> Vec<(String, bool)> {
        let w = self.waiting();
        let in_dir = |p: &&PathBuf| p.parent() == Some(dir);
        w.docs.iter().filter(|(p, _)| in_dir(p)).filter_map(|(p, h)| Some((p.file_name()?.to_str()?.to_string(), h.bytes.is_some()))).collect()
    }

    /// Waits until everything handed over is written, for [`FLUSH_WAIT`] at most.
    fn flush(&self) {
        let mut w = self.waiting();
        // Nothing waits, as in the browser, which writes them at once: the clock isn't read
        // there (it panics on wasm32).
        if !w.running || w.docs.is_empty() {
            return;
        }
        let start = Instant::now();
        while w.running && !w.docs.is_empty() {
            let left = FLUSH_WAIT.saturating_sub(start.elapsed());
            if left.is_zero() {
                log::warn!("{} documents weren't written before closing", w.docs.len());
                return;
            }
            w = self.changed.wait_timeout(w, left).unwrap_or_else(PoisonError::into_inner).0;
        }
    }
}

/// The thread: each document waiting written, one at a time, until none waits.
fn write_on(later: &DocsLater, files: &Routed) {
    loop {
        let next = {
            let mut w = later.waiting();
            match w.docs.iter().next().map(|(p, h)| (p.clone(), h.clone())) {
                Some(next) => next,
                None => {
                    w.running = false;
                    later.changed.notify_all();
                    return;
                }
            }
        };
        #[cfg(test)]
        let held = later.pause.lock().unwrap_or_else(PoisonError::into_inner);
        let (path, h) = next;
        if let Err(e) = write_now(files, &path, h.bytes.as_deref().map(Vec::as_slice)) {
            log::warn!("a document couldn't be kept: {e}");
        }
        #[cfg(test)]
        drop(held);
        let mut w = later.waiting();
        // A newer copy handed over meanwhile is written next.
        if w.docs.get(&path).is_some_and(|x| x.n == h.n) {
            w.docs.remove(&path);
        }
        later.changed.notify_all();
    }
}

/// `bytes` written at `path`, or the file removed (one not there is no matter).
fn write_now(files: &Routed, path: &Path, bytes: Option<&[u8]>) -> Result<()> {
    match bytes {
        Some(b) => files.write(path, b),
        None => match files.remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_err(path)(e)),
            _ => Ok(()),
        },
    }
}

impl Store {
    /// `value` kept as document `name`, as [`Store::save_doc`] keeps it, but written on the
    /// store's thread: the caller doesn't wait for the disk. Read meanwhile, it's as handed over.
    pub fn save_doc_later<T: Serialize + ?Sized>(&self, name: &str, value: &T) -> Result<()> {
        let path = self.root.join(safe_doc(name));
        let bytes = to_json(&path, value)?;
        self.docs.hand(&self.files, path, Some(bytes))
    }

    /// Document `name` removed on the store's thread, after any copy of it handed over before.
    pub fn remove_doc_later(&self, name: &str) -> Result<()> {
        self.docs.hand(&self.files, self.root.join(safe_doc(name)), None)
    }

    /// Waits until the documents handed over ([`Store::save_doc_later`]) are written.
    pub fn flush_docs(&self) {
        self.docs.flush();
    }

    /// Document `name` as handed over and still to be written: `Some(None)` when it's to be
    /// removed.
    pub(crate) fn doc_handed(&self, name: &str) -> Option<Option<Arc<Vec<u8>>>> {
        self.docs.handed(&self.root.join(safe_doc(name)))
    }

    /// The documents at the top of the data folder still to be written, each with whether it's
    /// kept (or removed).
    pub(crate) fn docs_handed(&self) -> Vec<(String, bool)> {
        self.docs.handed_in(&self.root)
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::temp_store;

    /// Handed over, a document is read back as it was handed, before it's written and after;
    /// the newest copy is the one written; removed, it's gone, listed or read.
    #[test]
    fn a_document_handed_over_reads_as_handed_and_the_newest_is_written() {
        let store = temp_store("docs-later");
        let file = store.root().join("chat-1.json");
        {
            let _held = store.docs.pause.lock().unwrap();
            store.save_doc_later("chat-1.json", &json_of("first")).unwrap();
            store.save_doc_later("chat-1.json", &json_of("second")).unwrap();
            assert!(!file.exists(), "held: not written yet");
            assert_eq!(store.load_doc::<serde_json::Value>("chat-1.json").unwrap(), json_of("second"));
            assert_eq!(store.doc_names("chat-"), ["chat-1.json"]);
        }
        store.flush_docs();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&std::fs::read(&file).unwrap()).unwrap(), json_of("second"));
        {
            let _held = store.docs.pause.lock().unwrap();
            store.remove_doc_later("chat-1.json").unwrap();
            assert!(file.exists(), "held: not removed yet");
            assert_eq!(store.load_doc::<serde_json::Value>("chat-1.json").unwrap(), serde_json::Value::Null);
            assert!(store.doc_names("chat-").is_empty());
        }
        store.flush_docs();
        assert!(!file.exists());
    }

    /// Written, then removed, then written again while held: what was handed last is what's
    /// there.
    #[test]
    fn what_was_handed_over_last_is_what_is_kept() {
        let store = temp_store("docs-later-order");
        {
            let _held = store.docs.pause.lock().unwrap();
            store.save_doc_later("a.json", &json_of("one")).unwrap();
            store.remove_doc_later("a.json").unwrap();
            store.save_doc_later("a.json", &json_of("two")).unwrap();
            store.remove_doc_later("b.json").unwrap();
        }
        store.flush_docs();
        assert_eq!(store.load_doc::<serde_json::Value>("a.json").unwrap(), json_of("two"));
        assert!(!store.root().join("b.json").exists());
    }

    fn json_of(s: &str) -> serde_json::Value {
        serde_json::json!({"said": s})
    }
}
