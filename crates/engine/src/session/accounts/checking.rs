//! A new account's sign-in, checked before it's added (`account.check`): the network layer signs
//! in to its servers as it will connect and leaves again; the answer is kept here for the account
//! dialog (and `account.checked`). Nothing of the account is kept until it's added.

use mailcraft_model::SignInFailure;
use serde_json::json;

use super::*;

/// The last sign-in asked to be checked: its number, the address, and its answer once it came
/// (`Some(None)`: the servers took it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInCheck {
    pub id: u64,
    pub email: String,
    pub answer: Option<Option<SignInFailure>>,
}

impl Session {
    /// Check the sign-in of an account as it would be added, without adding it: refused as
    /// adding it would be, then its servers signed in to ([`Job::CheckSignIn`]). Only an account
    /// with a mail server that signs in with a password is checked (IMAP, POP, JMAP).
    pub fn check_sign_in(&mut self, mut config: AccountConfig, password: String, smtp_password: Option<String>) -> CmdResult {
        self.admissible(&mut config)?;
        if config.is_local() || config.oauth.is_some() || config.graph.is_some() || config.ews.is_some() {
            return Err(CmdError::Params("only IMAP, POP and JMAP accounts that sign in with a password are checked before they're added".into()));
        }
        if password.is_empty() {
            return Err(CmdError::Params("`password` is required".into()));
        }
        // A number of its own: the answer to one asked before isn't taken for this one's.
        let id = self.sign_in_check.as_ref().map_or(1, |c| c.id.wrapping_add(1));
        let email = config.email.clone();
        self.sign_in_check = Some(SignInCheck { id, email: email.clone(), answer: None });
        self.jobs.push(Job::CheckSignIn { id, account: config, password: Secret(password), smtp_password: smtp_password.map(Secret) });
        Ok(json!({"checking": true, "email": email}))
    }

    /// A sign-in checked ([`Event::SignInChecked`]): kept when it's the last one asked.
    pub(crate) fn sign_in_checked(&mut self, id: u64, failure: Option<SignInFailure>) {
        if let Some(c) = self.sign_in_check.as_mut().filter(|c| c.id == id) {
            c.answer = Some(failure);
        }
    }

    /// The last sign-in checked, as `account.checked` says it: still checking, taken, or which
    /// server refused it and why.
    pub fn sign_in_check_json(&self) -> Value {
        let Some(c) = &self.sign_in_check else { return json!({"checking": false}) };
        match &c.answer {
            None => json!({"checking": true, "email": c.email}),
            Some(None) => json!({"checking": false, "email": c.email, "ok": true}),
            Some(Some(f)) => json!({
                "checking": false, "email": c.email, "ok": false, "server": f.server, "outgoing": f.outgoing,
                "error": f.error, "untrustedCertificate": f.certificate.as_ref().map(|c| c.fingerprint.clone()),
            }),
        }
    }
}
