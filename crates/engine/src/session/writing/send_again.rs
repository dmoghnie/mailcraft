//! A message the user sent, sent again (Apple Mail's Send Again, Outlook's Resend): opened as a
//! new one with the same recipients, subject, text and files, to change or send as it is.

use super::*;
use crate::session::security::is_sealed;

impl Session {
    /// Whether message `key` came from one of the user's addresses.
    pub fn sent_by_me(&self, key: &MessageKey) -> bool {
        let Some(from) = self.summary(key).map(|m| m.from.email.trim().to_ascii_lowercase()) else { return false };
        !from.is_empty() && self.my_addresses().contains(&from)
    }

    /// Message `key` (the open one when `None`), one the user sent, as a new message being
    /// written from the account it went from: nothing goes until it's sent.
    pub fn send_again(&mut self, key: Option<MessageKey>) -> CmdResult {
        let key = key.or_else(|| self.reading.as_ref().map(|r| r.key.clone())).ok_or_else(|| CmdError::Disabled("open a message first".into()))?;
        if !self.sent_by_me(&key) {
            return Err(CmdError::Disabled("only a message you sent can be sent again".into()));
        }
        let raw = self.readable_raw(&key).ok_or_else(|| CmdError::Disabled("the message is still downloading".into()))?;
        if is_sealed(&raw) {
            return Err(CmdError::Disabled("the message is encrypted: it can be sent again once it has been decrypted".into()));
        }
        let account = self.sending_account(key.account.clone());
        let mut c = self.compose_from_saved(&account, &key, &raw)?;
        // A new message: no copy of it in Drafts yet, and no message it answers to mark again.
        c.draft = None;
        c.draft_id.clear();
        c.source = None;
        c.mode = None;
        c.window = self.prefs.new_mail_window;
        let id = c.id;
        self.open_compose(c);
        self.made_now(id);
        Ok(serde_json::json!({ "compose": id }))
    }
}
