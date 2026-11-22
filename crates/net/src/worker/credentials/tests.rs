use std::sync::Arc;

use mailcraft_store::Store;

use super::super::{Auth, Pool, SyncOptions, Worker};
use super::*;

/// Found in review: the password just given was kept for signing in only when the account's lock
/// on it could be had whole, unlike everywhere else it's taken: after a thread panicked holding
/// it, the account signed in with no password, and nothing said why.
#[test]
fn a_password_given_is_kept_after_a_thread_panicked_holding_the_lock() {
    let store = Store::memory();
    let pool = Pool::new(store.clone(), Arc::new(MemoryCredentials::default()));
    let acc = AccountConfig { id: "a1".into(), ..Default::default() };
    let mut w = Worker::new(acc, Auth::shared(""), store, pool.sink.clone(), SyncOptions::default());
    w.creds = Some(Arc::new(MemoryCredentials::default()));
    w.keychain = Some(Some("pw".into()));
    let held = w.auth.clone();
    let _ = std::thread::spawn(move || {
        let _held = held.lock();
        panic!("a thread panicking with the lock held");
    })
    .join();
    assert!(w.auth.is_poisoned());
    assert!(w.settle_password());
    assert_eq!(w.auth.lock().unwrap_or_else(std::sync::PoisonError::into_inner).secret, "pw");
}

/// An account at `port` on this computer, as the fake IMAP server knows it (password `pw`);
/// nothing is sent, and its calendars were looked for today (no address looked up for them).
fn on_this_computer(store: &Store, port: u16) -> AccountConfig {
    use mailcraft_model::{Security, ServerConfig};
    crate::fakes::fake_dav::looked_for_today(store, "a1");
    let at = |port| ServerConfig { host: "127.0.0.1".into(), port, security: Security::None };
    AccountConfig {
        id: "a1".into(),
        email: "me@studio.example".into(),
        username: "me@studio.example".into(),
        imap: at(port),
        smtp: at(1),
        ..Default::default()
    }
}

fn start(pool: &mut Pool, account: &AccountConfig, password: &str) {
    let password = Some(mailcraft_model::job::Secret(password.into()));
    pool.submit(mailcraft_model::Job::Start { account: account.clone(), password });
}

fn status(e: &mailcraft_model::Event, f: impl Fn(&mailcraft_model::SyncStatus) -> bool) -> bool {
    matches!(e, mailcraft_model::Event::Status { account, status } if account == "a1" && f(status))
}

/// A password typed into Sign in again went into the keychain before the server was asked, so a
/// mistyped one, refused, had already replaced the one kept there, which may have been right (a
/// server refuses a good one at times, after connections time out).
#[test]
fn a_password_given_is_kept_once_the_server_takes_it() {
    use mailcraft_model::SyncStatus;
    let st = crate::fake::State::new();
    let port = crate::fake::serve_tcp(&st);
    let store = Store::memory();
    let account = on_this_computer(&store, port);
    let creds = Arc::new(MemoryCredentials::default());
    // The one kept before the password was changed on the server.
    creds.set(&account, "old").unwrap();
    let mut pool = Pool::new(store, creds.clone());

    start(&mut pool, &account, "typo");
    crate::waiting::until(&pool, "the password refused", |e| status(e, |s| matches!(s, SyncStatus::NeedsPassword { .. })));
    assert_eq!(creds.get(&account).as_deref(), Some("old"), "a password the server refused replaced the one kept");

    start(&mut pool, &account, "pw");
    crate::waiting::until(&pool, "online", |e| status(e, |s| matches!(s, SyncStatus::Online { .. })));
    assert_eq!(creds.get(&account).as_deref(), Some("pw"), "the password the server took isn't kept");
    pool.submit(mailcraft_model::Job::Remove { account: "a1".into() });
}

/// With none kept yet (a new account), the password given is kept at once: there's nothing it
/// could replace, and kept only once a server that can't be reached yet took it, the account
/// would ask for it again after MailCraft was closed.
#[test]
fn a_password_given_with_none_kept_is_kept_at_once() {
    use mailcraft_model::SyncStatus;
    let port = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
    let store = Store::memory();
    let account = on_this_computer(&store, port);
    let creds = Arc::new(MemoryCredentials::default());
    let mut pool = Pool::new(store, creds.clone());

    start(&mut pool, &account, "pw");
    crate::waiting::until(&pool, "no server there", |e| status(e, |s| !matches!(s, SyncStatus::Connecting)));
    assert_eq!(creds.get(&account).as_deref(), Some("pw"));
    pool.submit(mailcraft_model::Job::Remove { account: "a1".into() });
}
