//! Adding an account from the phone: its servers given, or looked up from the address first
//! (`account.discover`), then its sign-in checked (`account.check`) and only then
//! `account.add`, as the desktop's dialog does. The lookup and the check are jobs: each answers
//! later, asked again after each batch of events ([`advance`]). A sign-in the servers refused is
//! said, with which server and why, and can still be added ([`Action::AddAnyway`]).

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
    /// The sign-in being checked, with the params `account.add` then takes.
    Checking { email: String, params: Value },
    /// The servers refused the sign-in: these params can still be added.
    Refused(Value),
}

impl Adding {
    /// What the add-account screen shows under way: `lookingUp` or `checking`.
    pub(crate) fn step(&self) -> Option<&'static str> {
        match self {
            Adding::Looking(_) => Some("lookingUp"),
            Adding::Checking { .. } => Some("checking"),
            Adding::Idle | Adding::Refused(_) => None,
        }
    }

    pub(crate) fn refused(&self) -> bool {
        matches!(self, Adding::Refused(_))
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
        check(model, &form, p);
        return;
    }
    model.adding = Adding::Looking(form);
    advance(model);
}

/// The lookup or the check asked again (after each batch of events).
pub(crate) fn advance(model: &mut Model) {
    match &model.adding {
        Adding::Looking(form) => {
            let form = form.clone();
            looked_up(model, form);
        }
        Adding::Checking { email, params } => {
            let (email, params) = (email.clone(), params.clone());
            checked(model, &email, params);
        }
        Adding::Idle | Adding::Refused(_) => {}
    }
}

/// Add what the servers refused, as the person asked.
pub(crate) fn add_anyway(model: &mut Model) {
    if let Adding::Refused(params) = std::mem::take(&mut model.adding) {
        model.problem = None;
        add(model, params);
    }
}

/// Once the lookup has answered: the sign-in checked with what it found, or the person asked
/// for the servers.
fn looked_up(model: &mut Model, form: Form) {
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
    check(model, &form, p);
}

/// The form's account, its servers in `p`, checked: the servers asked to take its sign-in.
fn check(model: &mut Model, form: &Form, mut p: Value) {
    p["email"] = json!(form.email);
    p["password"] = json!(form.password);
    if let Some(n) = form.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        p["name"] = json!(n);
    }
    // Nulls (a lookup that left a field out) are the engine's defaults, not values.
    if let Some(o) = p.as_object_mut() {
        o.retain(|_, v| !v.is_null());
    }
    model.adding = Adding::Idle;
    if run(model, "account.check", p.clone()).is_some() {
        model.adding = Adding::Checking { email: form.email.clone(), params: p };
    }
}

/// The check's answer, once it came: added when the servers took the sign-in, else said.
fn checked(model: &mut Model, email: &str, params: Value) {
    let Some(r) = run(model, "account.checked", json!({})) else {
        model.adding = Adding::Idle;
        return;
    };
    // Another check's answer (one asked before), or still asking: wait for this one's.
    if r.get("checking").and_then(Value::as_bool) != Some(false)
        || !r.get("email").and_then(Value::as_str).is_some_and(|e| e.eq_ignore_ascii_case(email))
    {
        return;
    }
    if r.get("ok").and_then(Value::as_bool) == Some(true) {
        model.adding = Adding::Idle;
        add(model, params);
        return;
    }
    let server = r.get("server").and_then(Value::as_str).unwrap_or_default();
    let error = r.get("error").and_then(Value::as_str).unwrap_or_default();
    let which = if r.get("outgoing").and_then(Value::as_bool) == Some(true) { "outgoing" } else { "incoming" };
    model.problem = Some(format!("The {which} server {server} didn't take the sign-in: {error}"));
    model.adding = Adding::Refused(params);
}

fn add(model: &mut Model, params: Value) {
    if run(model, "account.add", params).is_some() {
        model.place = Place::default();
    }
}

/// `host:port` split, the port a number.
fn host_port(s: &str) -> Option<(String, u16)> {
    let (h, p) = s.trim().rsplit_once(':')?;
    let port = p.parse::<u16>().ok().filter(|p| *p > 0)?;
    (!h.is_empty()).then(|| (h.to_string(), port))
}
