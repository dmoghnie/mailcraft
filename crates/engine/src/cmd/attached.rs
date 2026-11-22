//! Commands: the Files view (`session/organize/attached.rs`), every file mail brought or took.

use super::*;

use crate::{AttachedFile, FILES_MOST, FileKind};

/// How many files `files.list` gives when it isn't told.
const LISTED: usize = 100;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("view.files", "Files", "Navigation", |s, _| set_module(s, crate::Module::Files)),
        CommandSpec::new("files.list", "List files", "Files", list)
            .params(r#"{"kind"?: "picture"|"document"|"pdf"|"archive"|"other", "person"?: string (part of a name or address), "name"?: string (part of the file's name), "limit"?: number} (every file received or sent that this computer holds, newest first)"#),
        CommandSpec::new("files.filter", "Show files", "Files", filter)
            .params(r#"{"kind"?: "all"|"picture"|"document"|"pdf"|"archive"|"other", "person"?: string, "name"?: string}"#),
        CommandSpec::new("files.show", "Preview file", "Files", show).params(r#"{"key": string, "index": number} (a file files.list gave)"#),
        CommandSpec::new("files.save", "Save file", "Files", save)
            .params(r#"{"key": string, "index": number, "path": string, "open"?: bool (saved to be opened by another app: never a file that runs code)}"#),
        CommandSpec::new("files.goTo", "Go to the message", "Files", go_to).params(r#"{"key": string, "index": number}"#),
    ]
}

/// A file's description for agents.
fn described(f: &AttachedFile) -> Value {
    json!({
        "key": f.key.to_id(), "index": f.index, "name": f.name, "mime": f.mime, "size": f.size, "kind": f.kind.code(),
        "person": f.person.email, "personName": f.person.name, "sent": f.sent, "date": f.date, "subject": f.subject,
    })
}

/// Every file read (each step until there's nothing left to read), then those asked for.
fn list(s: &mut Session, v: &Value) -> CmdResult {
    let mut wanted = crate::FilesFilter::default();
    if let Some(code) = p::str(v, "kind") {
        wanted.kind = Some(FileKind::from_code(code).ok_or_else(|| CmdError::Params(format!("unknown kind `{code}`")))?);
    }
    wanted.person = p::str(v, "person").unwrap_or_default().to_string();
    wanted.name = p::str(v, "name").unwrap_or_default().to_string();
    let limit = p::u64(v, "limit").map_or(LISTED, |n| usize::try_from(n).unwrap_or(FILES_MOST).min(FILES_MOST));
    while s.files_step() {}
    let found = s.files_matching(&wanted);
    Ok(json!({"total": found.len(), "files": found.iter().take(limit).map(|f| described(f)).collect::<Vec<_>>()}))
}

/// What the view shows changed: the fields given, the others kept.
fn filter(s: &mut Session, v: &Value) -> CmdResult {
    if let Some(code) = p::str(v, "kind") {
        s.files.filter.kind = match code {
            "all" => None,
            code => Some(FileKind::from_code(code).ok_or_else(|| CmdError::Params(format!("unknown kind `{code}`")))?),
        };
    }
    if let Some(person) = p::str(v, "person") {
        s.files.filter.person = person.to_string();
    }
    if let Some(name) = p::str(v, "name") {
        s.files.filter.name = name.to_string();
    }
    Ok(json!({"shown": s.files_shown().len()}))
}

/// The file named by `key` and `index`: one the view found.
fn file_param(s: &Session, v: &Value) -> Result<AttachedFile, CmdError> {
    let key = parse_key(v).ok_or_else(|| CmdError::Params("`key` is required (account:validity:uid:folder)".into()))?;
    let index = p::u64(v, "index").and_then(|i| usize::try_from(i).ok()).ok_or_else(|| CmdError::Params("`index` is required".into()))?;
    s.files_matching(&crate::FilesFilter::default())
        .into_iter()
        .find(|f| f.key == key && f.index == index)
        .cloned()
        .ok_or_else(|| CmdError::Params(format!("no file {index} of `{}` in the Files view", key.to_id())))
}

/// The file's message in the reading pane (not marked read), for its preview.
fn show(s: &mut Session, v: &Value) -> CmdResult {
    let f = file_param(s, v)?;
    s.show_file_message(&f.key);
    Ok(json!({"key": f.key.to_id(), "index": f.index, "name": f.name}))
}

fn save(s: &mut Session, v: &Value) -> CmdResult {
    let f = file_param(s, v)?;
    let path = p::req_str(v, "path")?;
    if p::bool(v, "open").unwrap_or(false) {
        compose::refuse_runnable(path)?;
    }
    let data = s.file_bytes(&f.key, f.index)?;
    std::fs::write(path, &data).map_err(|e| CmdError::Failed(format!("{path}: {e}")))?;
    compose::mark_from_internet(std::path::Path::new(path), &data);
    Ok(json!({"saved": path, "bytes": data.len()}))
}

/// Mail, the message's folder, and the message in it.
fn go_to(s: &mut Session, v: &Value) -> CmdResult {
    let f = file_param(s, v)?;
    if s.folder(&f.key.account, &f.key.folder).is_none() {
        return Err(CmdError::Failed(format!("the folder `{}` is gone", f.key.folder)));
    }
    set_module(s, crate::Module::Mail)?;
    s.open_view(View::Folder { account: f.key.account.clone(), folder: f.key.folder.clone() });
    if s.summary(&f.key).is_none() {
        return Err(CmdError::Failed("the message isn't in its folder any more".into()));
    }
    s.open_message(&f.key);
    Ok(json!({"key": f.key.to_id()}))
}
