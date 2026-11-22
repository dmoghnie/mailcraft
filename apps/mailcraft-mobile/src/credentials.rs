//! Passwords for the worker pool, kept by the phone's keystore (Android Keystore, the iOS
//! Keychain), not by MailCraft's files.
//!
//! The pool asks for them from its own threads and can't wait on the shell, so they're held in
//! memory here, as the shell handed them over at [`crate::start`]; each one the pool keeps or
//! drops is also sent to the shell ([`crate::poll`]'s `secret` and `forget` messages) for the
//! keystore. Entries are named as the desktop's keychain names them: the account's id
//! (its password), `<id>|smtp` (the outgoing server's own) and `<id>|<what>` (another secret).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mailcraft_engine::model::AccountConfig;
use mailcraft_net::worker::Credentials;
use serde_json::{Value, json};

use crate::outbox::Outbox;

/// What the shell handed over: `{"entry": "password", …}`. Not an object, or a value that isn't
/// text, is refused as a whole, so a damaged keystore is said rather than half read.
pub fn parse(secrets: &str) -> Result<HashMap<String, String>, String> {
    if secrets.trim().is_empty() {
        return Ok(HashMap::new());
    }
    let v: Value = serde_json::from_str(secrets).map_err(|e| format!("the kept passwords aren't JSON: {e}"))?;
    let o = v.as_object().ok_or("the kept passwords aren't a JSON object")?;
    o.iter().map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())).ok_or_else(|| format!("the kept password for {k} isn't text"))).collect()
}

pub(crate) struct ShellCredentials {
    kept: Mutex<HashMap<String, String>>,
    outbox: Arc<Outbox>,
}

fn lock(m: &Mutex<HashMap<String, String>>) -> MutexGuard<'_, HashMap<String, String>> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ShellCredentials {
    pub(crate) fn new(kept: HashMap<String, String>, outbox: Arc<Outbox>) -> ShellCredentials {
        ShellCredentials { kept: Mutex::new(kept), outbox }
    }

    fn get_entry(&self, entry: &str) -> Option<String> {
        lock(&self.kept).get(entry).cloned()
    }

    fn set_entry(&self, entry: String, secret: &str) {
        let changed = lock(&self.kept).insert(entry.clone(), secret.to_string()).as_deref() != Some(secret);
        if changed {
            self.outbox.push(json!({"kind": "secret", "entry": entry, "value": secret}).to_string());
        }
    }

    fn forget(&self, entry: String) {
        if lock(&self.kept).remove(&entry).is_some() {
            self.outbox.push(json!({"kind": "forget", "entry": entry}).to_string());
        }
    }
}

fn smtp(a: &AccountConfig) -> String {
    format!("{}|smtp", a.id)
}

fn other(a: &AccountConfig, what: &str) -> String {
    format!("{}|{what}", a.id)
}

impl Credentials for ShellCredentials {
    fn get(&self, account: &AccountConfig) -> Option<String> {
        self.get_entry(&account.id)
    }
    fn set(&self, account: &AccountConfig, password: &str) -> Result<(), String> {
        self.set_entry(account.id.clone(), password);
        Ok(())
    }
    fn delete(&self, account: &AccountConfig) {
        self.forget(account.id.clone());
    }
    fn get_smtp(&self, account: &AccountConfig) -> Option<String> {
        self.get_entry(&smtp(account))
    }
    fn set_smtp(&self, account: &AccountConfig, password: &str) -> Result<(), String> {
        self.set_entry(smtp(account), password);
        Ok(())
    }
    fn delete_smtp(&self, account: &AccountConfig) {
        self.forget(smtp(account));
    }
    fn get_secret(&self, account: &AccountConfig, what: &str) -> Option<String> {
        self.get_entry(&other(account, what))
    }
    fn set_secret(&self, account: &AccountConfig, what: &str, secret: &str) -> Result<(), String> {
        self.set_entry(other(account, what), secret);
        Ok(())
    }
    fn delete_secret(&self, account: &AccountConfig, what: &str) {
        self.forget(other(account, what));
    }
}
