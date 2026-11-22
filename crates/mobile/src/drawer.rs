//! The navigation drawer's accounts and folders: each account with its avatar and unread count,
//! its folders as a tree (the special ones first, the others by name, each under its parent),
//! and what's collapsed hidden. Collapsing is the desktop folder pane's own (`view.collapse`,
//! kept in the prefs), so an account folded on one is folded on the other.

use super::*;
use mailcraft_engine::AccountState;
use mailcraft_engine::model::{Folder, FolderRole, is_within};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    pub email: String,
    pub name: String,
    pub avatar: avatar::Avatar,
    /// `online`, `connecting`, `offline`, `error`…
    pub state: String,
    pub error: Option<String>,
    /// Unread in its Inbox.
    pub unread: u32,
    /// Its folders are hidden in the drawer.
    pub collapsed: bool,
    /// The folders to show, in order (none when collapsed).
    pub folders: Vec<FolderView>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderView {
    pub name: String,
    pub path: String,
    /// `inbox`, `drafts`, `sent`, `archive`, `junk`, `trash` or `other`.
    pub role: String,
    pub unread: u32,
    pub total: u32,
    /// How far it's indented: 0 for the special folders and the top level.
    pub depth: usize,
    /// It has folders inside, so it can be collapsed.
    pub parent: bool,
    pub collapsed: bool,
    /// It's the folder listed now.
    pub open: bool,
}

/// The key `view.collapse` takes for an account, and for one of its folders.
pub(crate) fn account_key(id: &str) -> String {
    format!("account:{id}")
}

pub(crate) fn folder_key(account: &str, path: &str) -> String {
    format!("folder:{account}/{path}")
}

pub(crate) fn accounts(model: &Model) -> Vec<AccountView> {
    let Some(s) = model.session.as_deref() else { return Vec::new() };
    let open = model.place.folder.as_ref();
    s.accounts.iter().map(|a| account(s, a, open)).collect()
}

fn account(s: &Session, a: &AccountState, open: Option<&(String, String)>) -> AccountView {
    let c = &a.config;
    let folded = s.prefs.collapsed.contains(&account_key(&c.id));
    let status = serde_json::to_value(&a.status).unwrap_or(Value::Null);
    AccountView {
        id: c.id.clone(),
        email: c.email.clone(),
        name: c.name.clone(),
        avatar: avatar::of(&c.name, &c.email),
        state: status.get("state").and_then(Value::as_str).unwrap_or("offline").to_string(),
        error: status.get("error").or_else(|| status.get("message")).and_then(Value::as_str).map(str::to_string),
        unread: a.folder(FolderRole::Inbox).map_or(0, |f| f.unread),
        collapsed: folded,
        folders: if folded { Vec::new() } else { tree(&c.id, &s.pane_folders(a), &s.prefs.collapsed, open) },
    }
}

/// The folders in the folder pane's order (the desktop's, with the person's own order), each
/// with its depth, those inside a collapsed one left out. The special folders stand at the top,
/// never inside another, wherever the server keeps them (Gmail's are in `[Gmail]`); a folder of
/// the person's is as deep as the folders of theirs it's inside.
pub(crate) fn tree(account: &str, folders: &[&Folder], collapsed: &BTreeSet<String>, open: Option<&(String, String)>) -> Vec<FolderView> {
    let own = |f: &Folder| f.role == FolderRole::Other;
    let inside = |f: &Folder, p: &Folder| own(f) && own(p) && p.path != f.path && is_within(&f.path, &p.path, p.delimiter);
    let mut out = Vec::new();
    for f in folders {
        let ancestors: Vec<&&Folder> = folders.iter().filter(|p| inside(f, p)).collect();
        if ancestors.iter().any(|p| collapsed.contains(&folder_key(account, &p.path))) {
            continue;
        }
        let parent = folders.iter().any(|c| inside(c, f));
        out.push(FolderView {
            name: f.display_name(),
            path: f.path.clone(),
            role: f.role.code().to_string(),
            unread: f.unread,
            total: f.total,
            depth: ancestors.len(),
            parent,
            collapsed: parent && collapsed.contains(&folder_key(account, &f.path)),
            open: open.is_some_and(|(a, p)| a == account && *p == f.path),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(path: &str, role: FolderRole) -> Folder {
        Folder { account: "a1".into(), path: path.into(), delimiter: Some('/'), role, selectable: true, ..Default::default() }
    }

    #[test]
    fn special_folders_stand_at_the_top_wherever_the_server_keeps_them() {
        let f = [
            folder("INBOX", FolderRole::Inbox),
            folder("[Gmail]/Drafts", FolderRole::Drafts),
            folder("[Gmail]/Sent Mail", FolderRole::Sent),
            folder("[Gmail]/Trash", FolderRole::Trash),
            folder("Work", FolderRole::Other),
            folder("Work/2026", FolderRole::Other),
            folder("[Gmail]", FolderRole::Other),
            folder("[Gmail]/Starred", FolderRole::Other),
        ];
        let refs: Vec<&Folder> = f.iter().collect();
        let depth = |t: &[FolderView], p: &str| t.iter().find(|v| v.path == p).map(|v| v.depth);
        let t = tree("a1", &refs, &BTreeSet::new(), None);
        assert_eq!(depth(&t, "[Gmail]/Drafts"), Some(0));
        assert_eq!(depth(&t, "[Gmail]/Trash"), Some(0));
        assert_eq!(depth(&t, "Work/2026"), Some(1));
        assert_eq!(depth(&t, "[Gmail]/Starred"), Some(1));

        let folded: BTreeSet<String> = [folder_key("a1", "[Gmail]")].into();
        let t = tree("a1", &refs, &folded, None);
        assert_eq!(depth(&t, "[Gmail]/Sent Mail"), Some(0), "a special folder isn't folded away with its container");
        assert_eq!(depth(&t, "[Gmail]/Starred"), None, "the container's own folders are");
    }
}
