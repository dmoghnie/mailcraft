//! A new account's sign-in, checked before it's added: its incoming server (IMAP, POP or JMAP)
//! and its outgoing one (SMTP) signed in to as the account will connect, then left. Nothing is
//! kept; what failed is said by server, for the account dialog to show.

use mailcraft_model::{AccountConfig, SignInFailure};

use crate::NetError;
use crate::stream::Trusted;

/// Sign in to `acc`'s servers with `password` (and `smtp_password` for an outgoing server with
/// a sign-in of its own): `None` when they took it, else the first that didn't, and why.
pub(crate) fn check_sign_in(acc: &AccountConfig, password: &str, smtp_password: Option<&str>) -> Option<SignInFailure> {
    if let Err(e) = incoming(acc, password) {
        let server = acc.jmap.clone().or_else(|| acc.pop.as_ref().map(|p| p.host.clone())).unwrap_or_else(|| acc.imap.host.clone());
        return Some(failure(acc, server, false, e));
    }
    // A JMAP account sends through the same server.
    if acc.jmap.is_some() {
        return None;
    }
    outgoing(acc, password, smtp_password).err().map(|e| failure(acc, acc.smtp.host.clone(), true, e))
}

fn incoming(acc: &AccountConfig, password: &str) -> crate::Result<()> {
    let user = acc.login_name();
    if let Some(url) = &acc.jmap {
        return crate::jmap::Client::connect(url, user, password, false).map(drop);
    }
    if acc.pop.is_some() {
        return crate::pop::open(acc, password)?.quit();
    }
    let mut c = crate::imap::Client::connect_trusting(&acc.imap, Trusted::of(acc))?;
    c.login_as(user, password, acc.sign_in)?;
    c.logout();
    Ok(())
}

/// The outgoing server signed in to as sending will: its own user name and password when it has
/// them, else the incoming server's.
fn outgoing(acc: &AccountConfig, password: &str, smtp_password: Option<&str>) -> crate::Result<()> {
    let own = acc.smtp_username.as_deref().map(str::trim).filter(|u| !u.is_empty());
    let (user, password) = match own {
        Some(u) => (u, smtp_password.unwrap_or(password)),
        None => (acc.login_name(), password),
    };
    let sign_in = crate::smtp::SignIn { user, password, oauth: false, method: acc.smtp_sign_in };
    crate::smtp::check_sign_in(&acc.smtp, sign_in, Trusted::of(acc))
}

/// What a refusal is said as: in MailCraft's words where the account's connection would say
/// them (Google's app passwords, Microsoft's refusal of passwords), else as it came.
fn failure(acc: &AccountConfig, server: String, outgoing: bool, e: NetError) -> SignInFailure {
    let said = e.to_string();
    let (error, certificate) = match e {
        NetError::Untrusted(shown) => (said, Some(shown)),
        NetError::Auth(m) if mailcraft_model::google_wants_app_password(&m) => (mailcraft_model::GOOGLE_APP_PASSWORD_NEEDED.to_string(), None),
        NetError::Auth(m) if mailcraft_model::microsoft_password_account(acc) => (format!("{} ({m})", mailcraft_model::MICROSOFT_NO_PASSWORDS), None),
        NetError::Auth(m) => (m, None),
        _ => (said, None),
    };
    SignInFailure { server, outgoing, error, certificate }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    use mailcraft_model::{Security, ServerConfig, SignInMethod};

    use super::*;
    use crate::http::tests::{TEST_SERVER_WAIT, accept_within};

    /// An SMTP server on this computer for one connection that takes, or refuses, any sign-in.
    /// What the client said, once it's gone.
    fn smtp(takes: bool) -> (u16, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let mut log = String::new();
            let Some(s) = accept_within(&l, TEST_SERVER_WAIT) else { return log };
            let mut w = s.try_clone().unwrap();
            let mut r = BufReader::new(s);
            let _ = w.write_all(b"220 test ESMTP\r\n");
            let mut line = String::new();
            while r.read_line(&mut line).unwrap_or(0) > 0 {
                log.push_str(&line);
                let cmd = line.trim_end().to_ascii_uppercase();
                let reply: &[u8] = match () {
                    () if cmd.starts_with("EHLO") => b"250-test\r\n250 AUTH PLAIN LOGIN\r\n",
                    () if cmd.starts_with("AUTH") && takes => b"235 ok\r\n",
                    () if cmd.starts_with("AUTH") => b"535 5.7.8 bad credentials\r\n",
                    () if cmd == "QUIT" => b"221 bye\r\n",
                    () => b"250 ok\r\n",
                };
                if w.write_all(reply).is_err() || cmd == "QUIT" {
                    break;
                }
                line.clear();
            }
            log
        });
        (port, h)
    }

    /// An account on the fake IMAP server (its password is `pw`) and an SMTP server, in the clear
    /// on this computer.
    fn account(imap: u16, smtp: u16) -> AccountConfig {
        let mut a = AccountConfig::guess("zoe@example.org");
        a.imap = ServerConfig { host: "127.0.0.1".into(), port: imap, security: Security::None };
        a.smtp = ServerConfig { host: "127.0.0.1".into(), port: smtp, security: Security::None };
        a
    }

    #[test]
    fn both_servers_taking_it_is_no_failure() {
        let st = crate::fake::State::new();
        let (port, h) = smtp(true);
        assert_eq!(check_sign_in(&account(crate::fake::serve_tcp(&st), port), "pw", None), None);
        let said = h.join().unwrap();
        assert!(said.contains("AUTH") && said.contains("QUIT"), "signed in, and left: {said}");
        assert!(!said.contains("MAIL FROM") && !said.contains("DATA"), "nothing sent: {said}");
    }

    #[test]
    fn a_refusal_names_the_server_that_refused() {
        let st = crate::fake::State::new();
        let imap = crate::fake::serve_tcp(&st);
        let f = check_sign_in(&account(imap, 1), "wrong", None).unwrap();
        assert_eq!((f.server.as_str(), f.outgoing), ("127.0.0.1", false));
        assert!(f.error.contains("wrong password"), "{}", f.error);

        let (port, h) = smtp(false);
        let f = check_sign_in(&account(imap, port), "pw", None).unwrap();
        assert!(f.outgoing && f.error.contains("bad credentials"), "{f:?}");
        h.join().unwrap();

        // Nothing there at all.
        let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let f = check_sign_in(&account(closed, 1), "pw", None).unwrap();
        assert!(!f.outgoing && !f.error.is_empty(), "{f:?}");
    }

    /// Google's refusal when it wants an app password is said in MailCraft's words, as the
    /// account's connection says it.
    #[test]
    fn googles_app_password_refusal_is_said_plainly() {
        let st = crate::fake::State::new();
        st.lock().unwrap().refuse_passwords = Some("[ALERT] Application-specific password required: https://support.google.com/mail/".into());
        let f = check_sign_in(&account(crate::fake::serve_tcp(&st), 1), "pw", None).unwrap();
        assert_eq!(f.error, mailcraft_model::GOOGLE_APP_PASSWORD_NEEDED);
    }

    /// The outgoing server's own user name and password, when it has them; none sent where it
    /// takes mail without a sign-in.
    #[test]
    fn the_outgoing_server_is_signed_in_to_as_sending_does() {
        let st = crate::fake::State::new();
        let imap = crate::fake::serve_tcp(&st);
        let (port, h) = smtp(true);
        let mut a = account(imap, port);
        a.smtp_username = Some("out".into());
        assert_eq!(check_sign_in(&a, "pw", Some("spw")), None);
        let said = h.join().unwrap();
        assert!(said.contains(&mailcraft_model::base64::encode(b"\0out\0spw")), "its own user and password: {said}");

        let (port, h) = smtp(false);
        let mut a = account(imap, port);
        a.smtp_sign_in = SignInMethod::None;
        assert_eq!(check_sign_in(&a, "pw", None), None);
        assert!(!h.join().unwrap().contains("AUTH"));
    }
}
