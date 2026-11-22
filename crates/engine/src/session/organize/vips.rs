//! VIPs: the few people whose mail always matters, by their address. Their mail always goes to
//! Focused, the VIP folder (a search of every mailbox) holds it, and notices of new mail can be
//! kept to theirs ([`Prefs::notify_vips_only`]).

use super::*;

/// The most VIPs kept: a few people, and the VIP folder's search names each one.
pub const VIPS_MAX: usize = 100;
/// The VIP folder, as the search it is ([`Session::open_vip_folder`]).
pub const VIP_FOLDER: &str = "VIP";
/// The longest address taken.
const ADDRESS_MAX: usize = 320;

impl Session {
    /// Whether mail from `email` is a VIP's.
    pub fn is_vip(&self, email: &str) -> bool {
        !self.prefs.vips.is_empty() && self.prefs.vips.contains(&email.trim().to_ascii_lowercase())
    }

    /// The VIPs' addresses, in order.
    pub fn vips(&self) -> Vec<String> {
        self.prefs.vips.iter().cloned().collect()
    }

    /// The address `address` names (lower case), or the first selected message's sender's.
    fn vip_address(&self, address: Option<&str>) -> Result<String, CmdError> {
        let a = match address {
            Some(a) => a.trim().to_ascii_lowercase(),
            None => {
                let m = self.selected_iter().next().ok_or_else(|| CmdError::Disabled("select a message first".into()))?;
                m.from.email.trim().to_ascii_lowercase()
            }
        };
        let fits = a.len() <= ADDRESS_MAX && !a.chars().any(|c| c.is_control() || c.is_whitespace() || c == ',');
        let (local, domain) = a.rsplit_once('@').unwrap_or_default();
        if !fits || local.is_empty() || domain.is_empty() {
            return Err(CmdError::Params(format!("not an email address: {}", a.chars().take(80).collect::<String>())));
        }
        Ok(a)
    }

    /// Make `address` (the first selected message's sender when `None`) a VIP, or (`on: false`)
    /// no longer one. The list and the VIP folder, when it's shown, follow.
    pub fn set_vip(&mut self, address: Option<&str>, on: bool) -> CmdResult {
        let a = self.vip_address(address)?;
        if on && !self.prefs.vips.contains(&a) && self.prefs.vips.len() >= VIPS_MAX {
            return Err(CmdError::Failed(format!("at most {VIPS_MAX} VIPs")));
        }
        // Shown before the change: its search, as it was.
        let shown = self.vip_folder_shown();
        let changed = if on { self.prefs.vips.insert(a.clone()) } else { self.prefs.vips.remove(&a) };
        if changed {
            // Their mail may be in the other pane of the Focused Inbox now: a list kept under a
            // search is built again rather than shown.
            self.mail_changed();
            if shown {
                self.show_vip_folder();
            } else {
                self.refresh_list();
            }
        }
        Ok(serde_json::json!({ "address": a, "vip": on, "vips": self.prefs.vips.len() }))
    }

    /// What the VIP folder searches for: mail from any VIP. `None` with no VIPs.
    pub fn vip_query(&self) -> Option<String> {
        (!self.prefs.vips.is_empty()).then(|| format!("from:{}", self.vips().join(",")))
    }

    /// Show the VIP folder: the VIPs' mail in every mailbox, newest first.
    pub fn open_vip_folder(&mut self) -> CmdResult {
        if self.prefs.vips.is_empty() {
            return Err(CmdError::Disabled("no VIPs yet: mark someone as a VIP first".into()));
        }
        let n = self.show_vip_folder();
        Ok(serde_json::json!({ "messages": n, "vips": self.prefs.vips.len() }))
    }

    /// The VIP folder's search run again (or the search ended, with no VIPs left). How many
    /// messages it holds.
    fn show_vip_folder(&mut self) -> usize {
        let query = self.vip_query().unwrap_or_default();
        self.search_as(&query, SearchScope::All, (!query.is_empty()).then(|| VIP_FOLDER.to_string()))
    }

    /// Whether the VIP folder is on screen (its search, as it is now).
    pub fn vip_folder_shown(&self) -> bool {
        self.search_folder.as_deref() == Some(VIP_FOLDER) && self.search.as_ref().map(|(q, _)| q.clone()) == self.vip_query()
    }
}
