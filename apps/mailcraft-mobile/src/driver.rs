//! The app's own thread: the core and the worker pool, and nothing else touching them.
//!
//! The shells' calls only queue [`Input`]s; this thread takes them one at a time, gives the
//! person's actions to the core, the jobs the core asks for to the pool, and the pool's events
//! back to the core, then puts the new view model in the [`Outbox`] when it changed.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crux_core::Core;
use mailcraft_engine::Session;
use mailcraft_engine::model::dates::Clock;
use mailcraft_engine::store::Store;
use mailcraft_mobile::{Action, Effect, Event, Mail};
use mailcraft_net::worker::Pool;
use serde_json::json;

use crate::credentials::ShellCredentials;
use crate::outbox::Outbox;

enum Input {
    Action(String),
    Zone(i32, String),
    /// The pool has events.
    Woke,
}

pub(crate) struct Driver {
    tx: Mutex<Sender<Input>>,
    outbox: Arc<Outbox>,
}

/// How often the clock is set when nothing else happens (and the pool looked at).
const BEAT: Duration = Duration::from_secs(1);

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)).unwrap_or(0)
}

impl Driver {
    pub(crate) fn start(dir: PathBuf, secrets: std::collections::HashMap<String, String>, offset: i32, zone: String) -> Result<Driver, String> {
        let (tx, rx) = mpsc::channel();
        let outbox = Arc::new(Outbox::default());
        let (ready_tx, ready_rx) = mpsc::channel();
        let (waker, out) = (tx.clone(), outbox.clone());
        std::thread::Builder::new()
            .name("mailcraft-app".into())
            .spawn(move || {
                let app = match App::open(dir, secrets, offset, zone, waker, out) {
                    Ok(a) => {
                        let _ = ready_tx.send(Ok(()));
                        a
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                app.run(rx);
            })
            .map_err(|e| format!("the app's thread didn't start: {e}"))?;
        ready_rx.recv().map_err(|_| "the app's thread ended while starting".to_string())??;
        Ok(Driver { tx: Mutex::new(tx), outbox })
    }

    fn input(&self, i: Input) -> Result<(), String> {
        self.tx.lock().unwrap_or_else(PoisonError::into_inner).send(i).map_err(|_| "the app has stopped".to_string())
    }

    pub(crate) fn send(&self, action: &str) -> Result<(), String> {
        // Checked here, so the shell hears of an action it got wrong.
        serde_json::from_str::<Action>(action).map_err(|e| format!("not an action: {e}"))?;
        self.input(Input::Action(action.to_string()))
    }

    pub(crate) fn zone(&self, offset: i32, zone: &str) -> Result<(), String> {
        self.input(Input::Zone(offset, zone.to_string()))
    }

    pub(crate) fn outbox(&self) -> &Outbox {
        &self.outbox
    }
}

/// Where each account's data file (`<address>.mailcraft`, redb) is kept: inside the app's own
/// folder, which only the app can read (a phone has no Documents folder to share them in).
const DATA_FILES: &str = "Data Files";

/// The cache as the desktop opens it: one data file per account in [`DATA_FILES`], and the
/// account list and app state in `dir`. Mail kept in folders from before (this app's first
/// builds) is brought into the data files as they open. When they can't open, the folders are
/// opened as they are, as the desktop does, so the mail is still there.
fn open_store(dir: &std::path::Path) -> Result<Store, String> {
    let packed = Store::packed(dir, dir.join(DATA_FILES)).and_then(|s| s.open_data_files().map(|()| s));
    match packed {
        Ok(s) => Ok(s),
        Err(e) => {
            log::error!("the data files didn't open: {e}");
            Store::open_alone(dir).map_err(|e| format!("the mail folder didn't open: {e}"))
        }
    }
}

struct App {
    core: Core<Mail>,
    pool: Pool,
    outbox: Arc<Outbox>,
    offset: i32,
}

impl App {
    fn open(
        dir: PathBuf,
        secrets: std::collections::HashMap<String, String>,
        offset: i32,
        zone: String,
        waker: Sender<Input>,
        outbox: Arc<Outbox>,
    ) -> Result<App, String> {
        let store = open_store(&dir)?;
        let creds = Arc::new(ShellCredentials::new(secrets, outbox.clone()));
        let pool = Pool::new(store.clone(), creds);
        let waker = Mutex::new(waker);
        pool.set_waker(Arc::new(move || {
            let _ = waker.lock().unwrap_or_else(PoisonError::into_inner).send(Input::Woke);
        }));
        let session = Session::open(Some(store), Clock::new(now(), offset));
        let mut app = App { core: Core::new(), pool, outbox, offset };
        app.event(Event::Opened(Box::new(session)));
        app.event(Event::Zone(zone));
        Ok(app)
    }

    fn run(mut self, rx: Receiver<Input>) {
        let mut beat = Instant::now();
        loop {
            match rx.recv_timeout(BEAT) {
                Ok(Input::Action(a)) => {
                    if let Ok(a) = serde_json::from_str::<Action>(&a) {
                        self.event(Event::Shell(a));
                    }
                }
                Ok(Input::Zone(offset, zone)) => {
                    self.offset = offset;
                    self.event(Event::Clock { now: now(), offset });
                    self.event(Event::Zone(zone));
                }
                Ok(Input::Woke) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            let events = self.pool.poll();
            if !events.is_empty() {
                self.event(Event::Engine(events));
            }
            if beat.elapsed() >= BEAT {
                beat = Instant::now();
                self.event(Event::Clock { now: now(), offset: self.offset });
            }
        }
    }

    /// One event through the core, and what it asked for done. An escaped panic is said, not
    /// fatal: the thread, and the mail being written, go on (the engine's commands guard
    /// themselves already; this is the last resort, as `Session::run` is).
    fn event(&mut self, e: Event) {
        let step = catch_unwind(AssertUnwindSafe(|| {
            let effects = self.core.process_event(e);
            let mut render = false;
            for effect in effects {
                match effect {
                    Effect::Render(_) => render = true,
                    Effect::Engine(mut r) => {
                        for job in std::mem::take(&mut r.operation.0) {
                            self.pool.submit(job);
                        }
                    }
                }
            }
            if render {
                match serde_json::to_string(&self.core.view()) {
                    Ok(v) => self.outbox.render(v),
                    Err(e) => log::error!("the view didn't serialise: {e}"),
                }
            }
        }));
        if step.is_err() {
            log::error!("a step of the app panicked");
            self.outbox
                .push(json!({"kind": "notice", "text": "Something went wrong in MailCraft; what you were doing may need doing again."}).to_string());
        }
    }
}
