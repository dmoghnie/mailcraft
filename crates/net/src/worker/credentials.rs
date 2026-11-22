//! Where passwords live: the system keychain in the app, memory in the CLI and the tests.

use std::collections::HashMap;
use std::sync::Mutex;

use mailcraft_model::AccountConfig;

#[cfg(test)]
mod tests;

/// Where passwords live. The desktop app backs this with the system keychain.
pub trait Credentials: Send + Sync {
    fn get(&self, account: &AccountConfig) -> Option<String>;
    fn set(&self, account: &AccountConfig, password: &str) -> std::result::Result<(), String>;
    fn delete(&self, account: &AccountConfig);
    /// The outgoing server's own password (accounts with an `smtp_username`), kept apart.
    fn get_smtp(&self, account: &AccountConfig) -> Option<String>;
    fn set_smtp(&self, account: &AccountConfig, password: &str) -> std::result::Result<(), String>;
    fn delete_smtp(&self, account: &AccountConfig);
    /// Another secret of the account's, named by `what` (`smime`: its S/MIME certificate's
    /// private key). A store without them keeps none.
    fn get_secret(&self, _account: &AccountConfig, _what: &str) -> Option<String> {
        None
    }
    fn set_secret(&self, _account: &AccountConfig, _what: &str, _secret: &str) -> std::result::Result<(), String> {
        Err("there's no keychain to keep it in".into())
    }
    fn delete_secret(&self, _account: &AccountConfig, _what: &str) {}
}

/// Passwords held in memory only (tests, the CLI, and when no keychain is available).
#[derive(Default)]
pub struct MemoryCredentials(Mutex<HashMap<String, String>>);

impl Credentials for MemoryCredentials {
    fn get(&self, account: &AccountConfig) -> Option<String> {
        self.0.lock().ok().and_then(|m| m.get(&account.id).cloned())
    }
    fn set(&self, account: &AccountConfig, password: &str) -> std::result::Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.insert(account.id.clone(), password.to_string());
        Ok(())
    }
    fn delete(&self, account: &AccountConfig) {
        if let Ok(mut m) = self.0.lock() {
            m.remove(&account.id);
        }
    }
    // The outgoing server's password: the secret named `smtp`.
    fn get_smtp(&self, account: &AccountConfig) -> Option<String> {
        self.get_secret(account, "smtp")
    }
    fn set_smtp(&self, account: &AccountConfig, password: &str) -> std::result::Result<(), String> {
        self.set_secret(account, "smtp", password)
    }
    fn delete_smtp(&self, account: &AccountConfig) {
        self.delete_secret(account, "smtp");
    }
    fn get_secret(&self, account: &AccountConfig, what: &str) -> Option<String> {
        self.0.lock().ok().and_then(|m| m.get(&format!("{}|{what}", account.id)).cloned())
    }
    fn set_secret(&self, account: &AccountConfig, what: &str, secret: &str) -> std::result::Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.insert(format!("{}|{what}", account.id), secret.to_string());
        Ok(())
    }
    fn delete_secret(&self, account: &AccountConfig, what: &str) {
        if let Ok(mut m) = self.0.lock() {
            m.remove(&format!("{}|{what}", account.id));
        }
    }
}

/// The bytes a keychain entry keeps as hex ([`crate::hex`]: a key's secret part, a certificate)
/// back, either case; `None` for anything else.
pub(super) fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| s.get(i..i + 2).and_then(|p| u8::from_str_radix(p, 16).ok())).collect()
}

impl super::Sink {
    /// Says a password (`what`: "the password", "the outgoing server's password"…) couldn't be
    /// saved in the keychain, and why.
    pub(super) fn not_kept(&self, account: &str, what: &str, error: &str) {
        let message = format!("{what} could not be saved in the keychain: {error}");
        self.emit(mailcraft_model::job::Event::Notice { account: account.to_string(), message });
    }
}

impl super::Worker {
    /// Before the account's thread first connects, on that thread: the password just given kept
    /// in the keychain (once the server takes it, when it would replace another), or the one kept
    /// there read. The keychain may ask the user first (macOS does after an update, for a build
    /// signed ad hoc), so it isn't done on the window's thread, which would stop drawing till
    /// it's answered. `false` when the keychain has no password: the account asks for one, and
    /// the pool forgets this thread.
    pub(super) fn settle_password(&mut self) -> bool {
        let Some(given) = self.keychain.take() else { return true };
        let Some(creds) = self.creds.clone() else { return true };
        let password = match given {
            Some(p) => {
                match creds.get(&self.acc) {
                    // The one kept already: nothing to change.
                    Some(kept) if kept == p => {}
                    // Another kept: this one replaces it once the server takes it
                    // ([`Worker::password_taken`]). Kept at once, one mistyped into Sign in again
                    // replaced the one kept, which may have been right (a server refuses a good
                    // one at times, after connections time out). Not an OAuth account's refresh
                    // token: [`Worker::secret`] keeps the newer one a provider hands out as it
                    // signs in, which this would then replace.
                    Some(_) if self.acc.oauth.is_none() => self.unkept = Some(p.clone()),
                    // None kept to replace (a new account): kept now, or the account would ask
                    // again after MailCraft is closed before a server took it.
                    _ => self.keep_password(&creds, &p),
                }
                p
            }
            None => match creds.get(&self.acc) {
                Some(p) => p,
                None => {
                    self.no_password.store(true, std::sync::atomic::Ordering::Relaxed);
                    self.status(mailcraft_model::job::SyncStatus::NeedsPassword { error: String::new() });
                    return false;
                }
            },
        };
        super::Auth::held(&self.auth).secret = password;
        if let Some(start) = self.quick_start.take() {
            start();
        }
        true
    }

    /// Signed in: the password given, if it waited for the server to take it, kept now.
    pub(super) fn password_taken(&mut self) {
        let Some(p) = self.unkept.take() else { return };
        if let Some(creds) = self.creds.clone() {
            self.keep_password(&creds, &p);
        }
    }

    fn keep_password(&self, creds: &std::sync::Arc<dyn Credentials>, password: &str) {
        if let Err(e) = creds.set(&self.acc, password) {
            self.sink.not_kept(&self.acc.id, "the password", &e);
        }
    }
}
