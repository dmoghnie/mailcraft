//! Leaving a mailing list from one of its messages, as its `List-Unsubscribe` header offers
//! (RFC 2369): by the message it asks for, sent from the account through its own outgoing server,
//! or else by the list's page, opened in the browser. MailCraft itself sends nothing to the list's
//! web server (it connects only to the user's own servers): a one-click request (RFC 8058) is the
//! page's to make. Never from Junk Email: answering junk mail tells its sender the address is read.

use mailcraft_model::mailto;
use serde::Serialize;

use super::*;

/// The most links of the header looked at, and the longest one taken.
const MAX_LINKS: usize = 8;
const MAX_LINK: usize = 2048;

/// What a list's address is asked when its link gives no subject.
const SUBJECT: &str = "unsubscribe";

/// How a list says to leave it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "by", rename_all = "camelCase")]
pub enum Leaving {
    /// A message to one address, with the subject and text its link gives.
    Mail { to: String, subject: String, body: String },
    /// The list's page for leaving it, opened in the browser.
    Page { url: String },
}

/// The way to leave a list, from its `List-Unsubscribe` links: a message, where a link gives one
/// address (one naming several would mail others), else its `https:` page.
pub fn leaving_way(links: &[String]) -> Option<Leaving> {
    let links = || links.iter().take(MAX_LINKS).map(|l| l.trim()).filter(|l| !l.is_empty() && l.len() <= MAX_LINK);
    let by_mail = links().filter_map(mailto::parse).find(|m| !m.to.trim().is_empty() && !m.to.contains(',') && m.cc.is_empty() && m.bcc.is_empty());
    if let Some(m) = by_mail {
        let subject = if m.subject.trim().is_empty() { SUBJECT.to_string() } else { m.subject };
        return Some(Leaving::Mail { to: m.to.trim().to_string(), subject, body: m.body });
    }
    links().find(|l| l.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("https://"))).map(|u| Leaving::Page { url: u.to_string() })
}

impl Session {
    /// How message `key` says to leave its list, if it does.
    pub fn leaving(&self, key: &MessageKey) -> Option<Leaving> {
        let raw = self.readable_raw(key)?;
        leaving_way(&mime::parse(&raw)?.unsubscribe_links)
    }

    /// Whether message `key` is in its account's Junk Email.
    pub fn in_junk(&self, key: &MessageKey) -> bool {
        self.account(&key.account).and_then(|a| a.folder(FolderRole::Junk)).is_some_and(|f| f.path == key.folder)
    }

    /// Leave the list message `key` (the open one when `None`) came from: its message sent from
    /// the account it came to, or its page opened in the browser ([`Leaving`]).
    pub fn unsubscribe(&mut self, key: Option<MessageKey>) -> CmdResult {
        let key = key.or_else(|| self.reading.as_ref().map(|r| r.key.clone())).ok_or_else(|| CmdError::Disabled("open a message first".into()))?;
        if self.in_junk(&key) {
            return Err(CmdError::Disabled(
                "in Junk Email, block the sender instead: answering junk mail tells its sender the address is read".into(),
            ));
        }
        let way = self.leaving(&key).ok_or_else(|| CmdError::Disabled("the message names no way to leave its list".into()))?;
        match way {
            Leaving::Page { url } => {
                if self.open_urls.len() < 8 {
                    self.open_urls.push(url.clone());
                }
                Ok(serde_json::json!({ "opened": url }))
            }
            Leaving::Mail { to, subject, body } => {
                let account = self.sending_account(key.account.clone());
                let mut c = Compose { id: self.fresh_id(), account, to: to.clone(), subject, ..Default::default() };
                c.set_text(body);
                let d = self.draft_of(&c, false)?;
                let (raw, from) = self.compose_bytes(&c, &d)?;
                let recipients = d.recipients();
                let mut answer = self.deliver(&c, raw, from, recipients)?;
                if let Some(o) = answer.as_object_mut() {
                    o.insert("to".into(), serde_json::json!(to));
                }
                Ok(answer)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_message_comes_before_the_page() {
        let way = leaving_way(&links(&["https://list.example/leave?u=1", "mailto:leave@list.example?subject=stop%20please"]));
        assert_eq!(way, Some(Leaving::Mail { to: "leave@list.example".into(), subject: "stop please".into(), body: String::new() }));
        // No subject in the link: the one lists read.
        let way = leaving_way(&links(&["mailto:leave@list.example"]));
        assert!(matches!(way, Some(Leaving::Mail { subject, .. }) if subject == SUBJECT));
    }

    #[test]
    fn a_page_only_when_its_link_is_https() {
        assert_eq!(leaving_way(&links(&["https://list.example/leave"])), Some(Leaving::Page { url: "https://list.example/leave".into() }));
        assert_eq!(leaving_way(&links(&["http://list.example/leave", "ftp://list.example", "javascript:alert(1)"])), None);
        assert_eq!(leaving_way(&[]), None);
    }

    #[test]
    fn a_link_that_would_mail_others_is_left_out() {
        for l in [
            "mailto:leave@list.example,boss@work.example",
            "mailto:leave@list.example?cc=boss@work.example",
            "mailto:leave@list.example?bcc=x@y.example",
        ] {
            assert_eq!(leaving_way(&links(&[l])), None, "{l}");
        }
        // The page then, if there's one.
        let way = leaving_way(&links(&["mailto:a@x.example,b@y.example", "https://list.example/leave"]));
        assert!(matches!(way, Some(Leaving::Page { .. })), "{way:?}");
    }
}
