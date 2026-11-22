//! Commands: message templates, kept by name to start a message from or fill one with (the
//! message's More options › Templates, Home's New mail › From a template, File › Options › Mail).

use super::*;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("template.list", "Templates", "Message › More options › Templates", list),
        CommandSpec::new("template.save", "Save as template", "Message › More options", save)
            .params(r#"{"name": string (one of that name is replaced), "recipients"?: bool (keep who it's to; default false), "files"?: bool (keep its files; default false)}"#)
            .when(has_compose),
        CommandSpec::new("template.insert", "Insert a template", "Message › More options › Templates; Home › New mail › From a template", insert).params(
            r#"{"name": string, "new"?: bool (start a new message from it; default: into the message being written), "account"?: string (a new message's), "at"?: number (the character its text goes at; default the start)}"#,
        ),
        CommandSpec::new("template.rename", "Rename template", "File › Options › Mail › Templates", rename).params(r#"{"name": string, "to": string}"#),
        CommandSpec::new("template.delete", "Delete template", "File › Options › Mail › Templates", delete).params(r#"{"name": string}"#),
    ]
}

fn list(s: &mut Session, _: &Value) -> CmdResult {
    let all: Vec<Value> = s
        .templates()
        .iter()
        .map(|t| {
            let recipients = !(t.to.is_empty() && t.cc.is_empty() && t.bcc.is_empty());
            json!({"name": t.name, "subject": t.subject, "recipients": recipients, "pictures": t.pictures.len(), "files": t.files.len()})
        })
        .collect();
    Ok(json!({ "templates": all }))
}

fn save(s: &mut Session, v: &Value) -> CmdResult {
    let name = p::req_str(v, "name")?;
    let t = s.save_template(name, p::bool(v, "recipients").unwrap_or(false), p::bool(v, "files").unwrap_or(false))?;
    Ok(json!({"saved": t.name}))
}

fn insert(s: &mut Session, v: &Value) -> CmdResult {
    let name = p::req_str(v, "name")?;
    // Checked before a new message opens for it.
    if s.template(name).is_none() {
        return Err(CmdError::Params(format!("no template called {name}")));
    }
    if p::bool(v, "new").unwrap_or(false) {
        s.new_compose(p::str(v, "account"))?;
    }
    let at = p::u64(v, "at").and_then(|a| usize::try_from(a).ok());
    let range = s.insert_template(name, at)?;
    Ok(json!({"inserted": range.start, "end": range.end}))
}

fn rename(s: &mut Session, v: &Value) -> CmdResult {
    let (name, to) = (p::req_str(v, "name")?, p::req_str(v, "to")?);
    s.rename_template(name, to)?;
    Ok(json!({"renamed": name, "to": to.trim()}))
}

fn delete(s: &mut Session, v: &Value) -> CmdResult {
    let name = p::req_str(v, "name")?;
    s.delete_template(name)?;
    Ok(json!({"deleted": name}))
}
