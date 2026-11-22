//! Adding an account from the phone: its servers given, or looked up from the address first
//! (`account.discover`, which answers later: the lookup is a job), then `account.add`.

use super::*;

/// The add-account form as sent.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Form {
    pub email: String,
    pub password: String,
    pub name: Option<String>,
    /// `host:port`, both or neither: given, nothing is looked up.
    pub imap: Option<String>,
    pub smtp: Option<String>,
}

/// Where adding is.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum Adding {
    #[default]
    Idle,
    /// The servers being looked up for this form.
    Looking(Form),
}

impl Adding {
    pub(crate) fn busy(&self) -> bool {
        matches!(self, Adding::Looking(_))
    }
}

pub(crate) fn start(model: &mut Model, form: Form) {
    model.problem = None;
    let email = form.email.trim().to_string();
    if email.is_empty() || form.password.is_empty() {
        model.problem = Some("Type the address and its password.".into());
        return;
    }
    let form = Form { email, ..form };
    if form.imap.is_some() || form.smtp.is_some() {
        let (Some(imap), Some(smtp)) = (&form.imap, &form.smtp) else {
            model.problem = Some("Give both servers, incoming and outgoing, or neither to look them up.".into());
            return;
        };
        let (Some(i), Some(o)) = (host_port(imap), host_port(smtp)) else {
            model.problem = Some("A server is written host:port, as mail.example.com:993.".into());
            return;
        };
        let p = json!({"imapHost": i.0, "imapPort": i.1, "smtpHost": o.0, "smtpPort": o.1});
        add(model, &form, p);
        return;
    }
    model.adding = Adding::Looking(form);
    advance(model);
}

/// The lookup asked again (after each batch of events): once it has answered, the account is
/// added with what it found, or the person is asked for the servers.
pub(crate) fn advance(model: &mut Model) {
    let Adding::Looking(form) = &model.adding else { return };
    let form = form.clone();
    let Some(r) = run(model, "account.discover", json!({"email": form.email})) else {
        model.adding = Adding::Idle;
        return;
    };
    if r.get("pending").and_then(Value::as_bool) == Some(true) {
        return;
    }
    model.adding = Adding::Idle;
    if r.get("found").and_then(Value::as_bool) != Some(true) {
        model.problem = Some("No settings are published for this address: type its incoming (IMAP) and outgoing (SMTP) servers.".into());
        return;
    }
    let server = |k: &str| r.get(k).filter(|v| v.is_object()).cloned().unwrap_or(Value::Null);
    let (imap, smtp) = (server("imap"), server("smtp"));
    let mut p = json!({
        "imapHost": imap["host"], "imapPort": imap["port"], "imapSecurity": imap["security"],
        "smtpHost": smtp["host"], "smtpPort": smtp["port"], "smtpSecurity": smtp["security"],
    });
    if let Some(u) = r.get("username").and_then(Value::as_str).filter(|u| !u.is_empty()) {
        p["username"] = json!(u);
    }
    add(model, &form, p);
}

fn add(model: &mut Model, form: &Form, mut p: Value) {
    p["email"] = json!(form.email);
    p["password"] = json!(form.password);
    if let Some(n) = form.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        p["name"] = json!(n);
    }
    // Nulls (a lookup that left a field out) are the engine's defaults, not values.
    if let Some(o) = p.as_object_mut() {
        o.retain(|_, v| !v.is_null());
    }
    if run(model, "account.add", p).is_some() {
        model.place = Place::default();
    }
}

/// `host:port` split, the port a number.
fn host_port(s: &str) -> Option<(String, u16)> {
    let (h, p) = s.trim().rsplit_once(':')?;
    let port = p.parse::<u16>().ok().filter(|p| *p > 0)?;
    (!h.is_empty()).then(|| (h.to_string(), port))
}
